//! Which provider answers each role — OpenRouter or the local Mnema process.
//! Every call that leaves for a model asks [`Provider::endpoint_for`] with its
//! role's provider (through `AppState::endpoint_for` or a job's captured
//! [`Provider`]), so one role's provider is never chosen differently at two
//! sites.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use mnema_local::{ModelId, ModelState, Sidecar, Store};
use serde::{Deserialize, Serialize};

pub use mnema_local::Endpoint;

use crate::error::Error;

/// Which provider the person chose. Persisted in the preferences file under
/// [`PREFS_KEY`]; absent or unreadable means OpenRouter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderChoice {
    #[default]
    OpenRouter,
    Mnema,
}

pub const PREFS_KEY: &str = "provider";

/// What a provider is used for. Each role picks its own provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Embed,
    Chat,
}

/// The provider chosen for each role; `None` = nothing configured for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Roles {
    pub embed: Option<ProviderChoice>,
    pub chat: Option<ProviderChoice>,
}

/// Which providers are set up and able to serve which role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Configured {
    pub open_router: bool,
    pub mnema_embed: bool,
    pub mnema_chat: bool,
}

impl Configured {
    pub fn serves(&self, p: ProviderChoice, r: Role) -> bool {
        match (p, r) {
            (ProviderChoice::OpenRouter, _) => self.open_router,
            (ProviderChoice::Mnema, Role::Embed) => self.mnema_embed,
            (ProviderChoice::Mnema, Role::Chat) => self.mnema_chat,
        }
    }

    pub fn any(&self, p: ProviderChoice) -> bool {
        self.serves(p, Role::Embed) || self.serves(p, Role::Chat)
    }
}

/// How long a local answer may take. A weak Mac writes slower than the cloud's
/// `INTERACTIVE_TIMEOUT` allows.
pub const LOCAL_CHAT_TIMEOUT: Duration = Duration::from_secs(60);
/// The chat model's name on the local wire — never the stored OpenRouter name.
pub const LOCAL_CHAT_MODEL: &str = "gemma-4-e2b-it";
/// The one embedding model the local process serves, shared with OpenRouter's
/// `baai/bge-m3` so switching providers keeps one space.
pub const LOCAL_EMBED_MODEL: &str = "baai/bge-m3";
/// The width of [`LOCAL_EMBED_MODEL`]. Stated rather than measured: the
/// switch adopts the space before the local models may even be downloaded, and
/// the model is pinned by sha256 (`mnema_local::Manifest::pinned`), so its
/// width is a fact of the pinned file, not a guess about a catalogue.
pub const LOCAL_EMBED_DIM: i64 = 1024;
/// Where the model files come from.
pub const HUB: &str = "https://huggingface.co";

impl ProviderChoice {
    /// Texts per embedding request during a scan.
    pub fn scan_batch(self) -> usize {
        match self {
            Self::OpenRouter => crate::embed_job::BATCH,
            Self::Mnema => 16,
        }
    }

    /// Embedding requests in flight at once during a scan.
    pub fn scan_workers(self) -> usize {
        match self {
            Self::OpenRouter => crate::embed_job::WORKERS,
            Self::Mnema => 1,
        }
    }

    pub fn chat_timeout(self) -> Duration {
        match self {
            Self::OpenRouter => mnema_provider::INTERACTIVE_TIMEOUT,
            Self::Mnema => LOCAL_CHAT_TIMEOUT,
        }
    }
}

/// The local models and the process that serves them. Started lazily, on the
/// first use by a role on Mnema whose model is `Ready`.
pub struct Local {
    pub store: Store,
    binary: PathBuf,
    env: Vec<(String, String)>,
    sidecar: Mutex<Option<Arc<Sidecar>>>,
    /// The launcher went cold and has not been shown since. A scan that ends
    /// now finishes the unloading [`on_cold`] left half done.
    cold: AtomicBool,
    /// How many times the launcher has been shown: a cold clock started before
    /// the last show is void.
    shown: AtomicU64,
    /// The launcher is up (or was, not yet cold): the models should be in
    /// memory. A process that restarted holds none, so [`Local::endpoint`]
    /// loads them again whenever this is set and [`Loads::loaded`] is not the
    /// running process.
    want: AtomicBool,
    loads: Arc<(Mutex<Loads>, Condvar)>,
    cancel: [AtomicBool; 2],
    /// Whether this machine can run the process at all ([`mnema_local::available`]
    /// unless set otherwise). The one answer both the stored choice and the
    /// window's radios read, so a test can stand on any host as either kind.
    available: bool,
    /// How long a question waits for a load before it answers with the
    /// passages alone ([`LOCAL_CHAT_TIMEOUT`] unless set otherwise).
    question_wait: Duration,
    /// What loads the models into the process: [`Sidecar::load`], unless a
    /// test makes the load misbehave in a way the fake cannot.
    load: fn(&Sidecar, &[ModelId]) -> Result<(), mnema_local::Error>,
    /// The models a load asks for: those of the roles on Mnema ([`on_show`]
    /// and every role change set it); both until then. A load asks only for
    /// the ones that are downloaded.
    wanted: Mutex<Vec<ModelId>>,
}

impl Local {
    pub fn new(store: Store, binary: PathBuf) -> Self {
        Self {
            store,
            binary,
            env: Vec::new(),
            sidecar: Mutex::new(None),
            cold: AtomicBool::new(false),
            shown: AtomicU64::new(0),
            want: AtomicBool::new(false),
            loads: Arc::default(),
            cancel: [AtomicBool::new(false), AtomicBool::new(false)],
            available: mnema_local::available(),
            question_wait: LOCAL_CHAT_TIMEOUT,
            load: Sidecar::load,
            wanted: Mutex::new(vec![ModelId::Embed, ModelId::Chat]),
        }
    }

    #[cfg(test)]
    fn with_load(
        mut self,
        load: fn(&Sidecar, &[ModelId]) -> Result<(), mnema_local::Error>,
    ) -> Self {
        self.load = load;
        self
    }

    /// Overrides [`LOCAL_CHAT_TIMEOUT`] as a question's wait for a load, so a
    /// test of a hung load does not pay a minute.
    pub fn with_question_wait(mut self, wait: Duration) -> Self {
        self.question_wait = wait;
        self
    }

    /// Overrides the host check: a hook for tests and embedders, whose fake or
    /// substitute process runs on any host.
    pub fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    pub fn available(&self) -> bool {
        self.available
    }

    /// Extra environment for the process (tests steer the fake with it).
    pub fn with_env(mut self, env: &[(&str, &str)]) -> Self {
        self.env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    pub fn ready(&self, id: ModelId) -> bool {
        self.store.state(id) == ModelState::Ready
    }

    /// [`Local::endpoint_within`] with no limit on the wait for a load: what
    /// everything but a question uses (a scan, the show's own load).
    fn endpoint(&self) -> Result<Endpoint, Error> {
        self.endpoint_within(None)
    }

    /// The running process's endpoint, starting it on first use, and loaded
    /// when the launcher is up. [`on_show`] sets `want` before its thread
    /// exists, and a caller that finds a load in flight waits for it (see
    /// [`Local::ensure_loaded`]), so a question asked the instant the launcher
    /// opens either waits for that load or starts it itself: never a chat
    /// before the models are in.
    fn endpoint_within(&self, wait: Option<Duration>) -> Result<Endpoint, Error> {
        let sidecar = self.process()?;
        // The supervisor's restart happens inside this call, so what follows
        // sees the process that will answer.
        if let Err(e) = sidecar.endpoint() {
            // Dead with no restart left: this use hears why, and the next one
            // starts a fresh process (and a fresh restart budget) instead of
            // the app staying without models until it is quit. Only explicit
            // uses come through here; a status poll goes through `running`.
            if matches!(e, mnema_local::Error::Crashed { .. }) {
                let mut slot = self
                    .sidecar
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sidecar)) {
                    *slot = None;
                }
            }
            return Err(e.into());
        }
        if self.want.load(Ordering::SeqCst) {
            self.ensure_loaded(&sidecar, wait)?;
        }
        Ok(sidecar.endpoint()?)
    }

    /// Loads the models into `sidecar` unless they are in. One load at a time,
    /// on a thread of its own: a second caller waits on the load in flight
    /// rather than starting another, and a caller with a `wait` can stop
    /// waiting. `wait` runs from the moment the load began, not from the call,
    /// so once a hung load has cost one question its wait, the next question
    /// hears [`Error::LocalLoading`] at once instead of waiting it out again.
    /// The load itself goes on to [`Sidecar::load`]'s own limit (a cold disk is
    /// slow); when it ends, the next caller finds the models in, or starts a
    /// fresh load if it failed. `None` waits for the end.
    fn ensure_loaded(&self, sidecar: &Arc<Sidecar>, wait: Option<Duration>) -> Result<(), Error> {
        // Read before the lock: during a restart `pid` waits on the process's
        // own lock, and no caller may wait on that while holding this one.
        let pid = sidecar.pid();
        let (lock, done) = &*self.loads;
        let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if st.loaded == pid {
                return Ok(());
            }
            let began = match st.running {
                Some(began) => began,
                None => {
                    // Only what is downloaded: the process fails a whole load
                    // that names a model whose files are gone.
                    let which: Vec<ModelId> = self
                        .wanted
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .iter()
                        .copied()
                        .filter(|id| self.ready(*id))
                        .collect();
                    // An empty load asks for both; nothing to load is no load
                    // (the process loads on demand).
                    if which.is_empty() {
                        return Ok(());
                    }
                    let attempt = st.attempt + 1;
                    let began = Instant::now();
                    let (loads, sidecar, load) =
                        (Arc::clone(&self.loads), Arc::clone(sidecar), self.load);
                    // The end is recorded by a guard, so a load that panics
                    // still ends: otherwise every caller with no limit would
                    // wait on it for ever, and no load could start again.
                    std::thread::Builder::new()
                        .name("mnema-load".into())
                        .spawn(move || {
                            let mut end = LoadEnd {
                                loads,
                                attempt,
                                pid,
                                result: None,
                            };
                            end.result = Some(load(&sidecar, &which));
                        })
                        .map_err(|e| {
                            mnema_local::Error::Io(format!("cannot start the model load: {e}"))
                        })?;
                    // Still under the lock, so the thread cannot record its
                    // end before the load is marked in flight.
                    st.attempt = attempt;
                    st.running = Some(began);
                    began
                }
            };
            let mine = st.attempt;
            while st.last.as_ref().is_none_or(|(n, _)| *n < mine) {
                st = match wait {
                    None => done.wait(st).unwrap_or_else(PoisonError::into_inner),
                    Some(wait) => {
                        let left = (began + wait).saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return Err(Error::LocalLoading);
                        }
                        done.wait_timeout(st, left)
                            .unwrap_or_else(PoisonError::into_inner)
                            .0
                    }
                };
            }
            if let Some((n, Err(e))) = &st.last
                && *n == mine
            {
                return Err(e.clone().into());
            }
        }
    }

    /// The process, started on first use. Held under the lock for the start so
    /// two callers cannot start two processes.
    fn process(&self) -> Result<Arc<Sidecar>, Error> {
        let mut sidecar = self
            .sidecar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sidecar.is_none() {
            let env: Vec<(&str, &str)> = self
                .env
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            *sidecar = Some(Arc::new(Sidecar::start_with_env(
                &self.binary,
                &self.store.dir(ModelId::Embed),
                &self.store.dir(ModelId::Chat),
                &env,
            )?));
        }
        Ok(Arc::clone(sidecar.as_ref().expect("started above")))
    }

    /// The models are no longer in memory (or no longer all of them).
    /// Waits out a load in flight first, as the lock held across the load
    /// once made it: a load ending after this would mark as loaded what the
    /// caller is about to unload.
    fn forget_loaded(&self) {
        let (lock, done) = &*self.loads;
        let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while st.running.is_some() {
            st = done.wait(st).unwrap_or_else(PoisonError::into_inner);
        }
        st.loaded = 0;
    }

    /// Whether a process this provider already started still answers —
    /// `None` when none was started. Never starts one: a status poll must not
    /// be what brings the process up. An existing process that died gets the
    /// supervisor's one restart, as any request to it would.
    pub(crate) fn running(&self) -> Option<Result<(), Error>> {
        let sidecar = self
            .sidecar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sidecar
            .as_ref()
            .map(|s| s.endpoint().map(|_| ()).map_err(Error::from))
    }

    /// The process this provider already started, if any. Never starts one.
    fn started(&self) -> Option<Arc<Sidecar>> {
        self.sidecar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn cancel_flag(&self, id: ModelId) -> &AtomicBool {
        &self.cancel[id as usize]
    }

    /// Stops the process (if any): the next [`Provider::endpoint_for`] starts a fresh one
    /// over whatever files are there by then.
    pub(crate) fn stop(&self) {
        let old = self
            .sidecar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(old);
    }
}

/// The models' load into the process, shared by every caller that needs it.
#[derive(Default)]
struct Loads {
    /// The process whose models are in memory; 0 for none.
    loaded: u32,
    /// When the load in flight began; `None` while none is.
    running: Option<Instant>,
    /// Numbers the loads, so a waiter tells its own load's end from a later one's.
    attempt: u64,
    /// How the last load to end ended, and its number.
    last: Option<(u64, Result<(), mnema_local::Error>)>,
}

/// Records the end of a load when its thread ends, however it ends: a panic
/// leaves no `result`, and is recorded as a failed load.
struct LoadEnd {
    loads: Arc<(Mutex<Loads>, Condvar)>,
    attempt: u64,
    pid: u32,
    result: Option<Result<(), mnema_local::Error>>,
}

impl Drop for LoadEnd {
    fn drop(&mut self) {
        let result = self.result.take().unwrap_or_else(|| {
            Err(mnema_local::Error::Io(
                "the model load stopped unexpectedly".into(),
            ))
        });
        let (lock, done) = &*self.loads;
        let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if result.is_ok() {
            st.loaded = self.pid;
        }
        st.running = None;
        st.last = Some((self.attempt, result));
        done.notify_all();
    }
}

/// Everything [`Provider::endpoint_for`] reads, owned, so a job thread can carry it without
/// `AppState`.
#[derive(Clone)]
pub struct Provider {
    pub data_dir: PathBuf,
    pub openrouter_base: String,
    pub credential_ref: String,
    pub local: Arc<Local>,
}

impl Provider {
    /// The provider of each role, as the preferences file states it, with the
    /// migration from the single key read against what is set up right now.
    pub fn roles(&self) -> Roles {
        // Built only when a migration needs it: no keychain read otherwise.
        crate::prefs::roles(&self.data_dir, || self.configured())
    }

    /// Which providers can serve which role right now. Reads the keychain.
    pub fn configured(&self) -> Configured {
        // An unreadable store is not "no key": it must not move a role away.
        self.configured_given(
            mnema_secrets::load(&self.credential_ref).map_or(true, |k| k.is_some()),
        )
    }

    /// [`Provider::configured`] with OpenRouter's answer already measured.
    pub fn configured_given(&self, open_router: bool) -> Configured {
        let up = self.local.available();
        Configured {
            open_router,
            mnema_embed: up && self.local.ready(ModelId::Embed),
            mnema_chat: up && self.local.ready(ModelId::Chat),
        }
    }

    /// The models of the roles on Mnema that are downloaded.
    fn mnema_models(&self) -> Vec<ModelId> {
        let r = self.roles();
        [(r.embed, ModelId::Embed), (r.chat, ModelId::Chat)]
            .into_iter()
            .filter(|(p, id)| *p == Some(ProviderChoice::Mnema) && self.local.ready(*id))
            .map(|(_, id)| id)
            .collect()
    }

    /// Where to send a role's requests, and the secret for them. `p` is the
    /// role's provider and `id` its local model.
    ///
    /// No provider: `Error::RoleEmpty`, never OpenRouter in its place, so data
    /// a person keeps local never leaves the machine. OpenRouter: see
    /// [`Provider::open_router`]. Mnema: the local process, or
    /// `Error::ProviderNotReady` while this role's model is not downloaded.
    pub fn endpoint_for(&self, p: Option<ProviderChoice>, id: ModelId) -> Result<Endpoint, Error> {
        self.resolve(p, id, None)
    }

    /// [`Provider::endpoint_for`] for a question: under Mnema it waits for a
    /// load no longer than [`LOCAL_CHAT_TIMEOUT`], then hears
    /// [`Error::LocalLoading`] and answers with the passages.
    pub fn question_endpoint_for(
        &self,
        p: Option<ProviderChoice>,
        id: ModelId,
    ) -> Result<Endpoint, Error> {
        self.resolve(p, id, Some(self.local.question_wait))
    }

    /// OpenRouter whatever the roles say: its base for both `base` and
    /// `query_base`, and the stored key (`Error::NoKey` when none). For the
    /// probes of OpenRouter itself (a key check, a model's width).
    pub fn open_router(&self) -> Result<Endpoint, Error> {
        let token = mnema_secrets::load(&self.credential_ref)?.ok_or(Error::NoKey)?;
        Ok(Endpoint {
            base: self.openrouter_base.clone(),
            query_base: self.openrouter_base.clone(),
            token,
        })
    }

    fn resolve(
        &self,
        p: Option<ProviderChoice>,
        id: ModelId,
        load_wait: Option<Duration>,
    ) -> Result<Endpoint, Error> {
        match p.ok_or(Error::RoleEmpty)? {
            ProviderChoice::OpenRouter => self.open_router(),
            ProviderChoice::Mnema => {
                // A host that cannot run the process has no model ready to it.
                if !(self.local.available() && self.local.ready(id)) {
                    return Err(Error::ProviderNotReady);
                }
                self.local.endpoint_within(load_wait)
            }
        }
    }
}

/// A local model as the window names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalModel {
    Embed,
    Chat,
}

impl From<LocalModel> for ModelId {
    fn from(m: LocalModel) -> Self {
        match m {
            LocalModel::Embed => ModelId::Embed,
            LocalModel::Chat => ModelId::Chat,
        }
    }
}

/// [`ModelState`] on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LocalModelState {
    Absent,
    Downloading { done: u64, total: u64 },
    Ready,
    Failed { message: String },
}

impl From<ModelState> for LocalModelState {
    fn from(s: ModelState) -> Self {
        match s {
            ModelState::Absent => Self::Absent,
            ModelState::Downloading { done, total } => Self::Downloading { done, total },
            ModelState::Ready => Self::Ready,
            ModelState::Failed(message) => Self::Failed { message },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalModelRow {
    pub id: LocalModel,
    pub state: LocalModelState,
}

/// The event a download reports through.
pub const PROGRESS_EVENT: &str = "local-model-progress";

#[derive(Debug, Clone, Serialize)]
struct Progress {
    id: LocalModel,
    done: u64,
    total: u64,
}

/// Why a download did not finish. Tagged, unlike [`Error`], because "not
/// enough room" carries two numbers the window words itself.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadError {
    NoSpace { needed: u64, free: u64 },
    Cancelled,
    Failed { message: String },
}

impl From<mnema_local::Error> for DownloadError {
    fn from(e: mnema_local::Error) -> Self {
        match e {
            mnema_local::Error::NoSpace { needed, free } => Self::NoSpace { needed, free },
            mnema_local::Error::Cancelled => Self::Cancelled,
            other => Self::Failed {
                message: other.to_string(),
            },
        }
    }
}

#[tauri::command]
pub fn mnema_available(state: tauri::State<'_, crate::state::AppState>) -> bool {
    state.mnema_available()
}

#[tauri::command(async)]
pub fn local_models(state: tauri::State<'_, crate::state::AppState>) -> Vec<LocalModelRow> {
    let local = state.local();
    [LocalModel::Embed, LocalModel::Chat]
        .into_iter()
        .map(|id| LocalModelRow {
            id,
            state: local.store.state(id.into()).into(),
        })
        .collect()
}

/// Downloads one model, reporting on [`PROGRESS_EVENT`]. Blocks its own
/// (async-pool) thread for the length of the download.
#[tauri::command(async)]
pub fn download_model<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    id: LocalModel,
) -> Result<(), DownloadError> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter as _;
    let local = state.local();
    let flag = local.cancel_flag(id.into());
    flag.store(false, Ordering::SeqCst);
    let progress = |done: u64, total: u64| {
        let _ = app.emit(PROGRESS_EVENT, Progress { id, done, total });
    };
    local.store.download(id.into(), &progress, flag)?;
    on_configured(&state, ProviderChoice::Mnema, Some(id.into()));
    Ok(())
}

#[tauri::command]
pub fn cancel_download(state: tauri::State<'_, crate::state::AppState>, id: LocalModel) {
    state
        .local()
        .cancel_flag(id.into())
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Deletes one model's files. The running process (if any) is stopped first:
/// it has the files open, and the next request starts a fresh one — or finds
/// the model missing.
#[tauri::command(async)]
pub fn remove_model(
    state: tauri::State<'_, crate::state::AppState>,
    id: LocalModel,
) -> Result<(), Error> {
    remove(&state, id)
}

/// [`remove_model`]'s body, reachable without a `State`.
pub fn remove(state: &crate::state::AppState, id: LocalModel) -> Result<(), Error> {
    // The job slot, as every role writer takes it: a scan embedding through
    // the process would otherwise end Failed with "connection refused" for a
    // removal the person made. Drawn as a model change (`ModelAdoption`) — it
    // is one; the kept resumable ending is restored on release, the space does
    // not move.
    let _slot = adoption_slot(state)?;
    // The role first: a removal that then fails leaves a choice to make again,
    // never a role on a model that is gone.
    on_unconfigured(state, ProviderChoice::Mnema, Some(id.into()))?;
    let local = state.local();
    local.stop();
    local.store.remove(id.into())?;
    Ok(())
}

/// The slot every role write is made under: the scan re-reads the embed role
/// inside its run and relies on it staying put.
pub(crate) fn adoption_slot(
    state: &crate::state::AppState,
) -> Result<crate::state::JobSlot, Error> {
    state.claim_job(
        crate::scan_state::Phase::Other {
            job: crate::scan_state::OtherJob::ModelAdoption,
        },
        false,
    )
}

/// `p` just became configured (a key stored, a local model downloaded): every
/// EMPTY role `p` serves gets a model, and a role already on `p` whose model is
/// missing gets one too. A chosen role is never touched. Best effort: a failed
/// write leaves the role empty; a busy slot defers it to [`on_slot_change`].
///
/// `which` limits Mnema to the role of the model that arrived — Mnema is
/// configured per model, so downloading bge-m3 says nothing new about chat.
pub fn on_configured(state: &crate::state::AppState, p: ProviderChoice, which: Option<ModelId>) {
    fill_roles(state, p, true, which);
}

/// [`on_configured`]'s substitution. `empty_too: false` fills only roles
/// already on `p` whose model is missing (a key stored while the index was
/// shut): a boot never moves an empty role — one emptied by removing a local
/// model would otherwise go to OpenRouter at every launch.
pub(crate) fn fill_roles(
    state: &crate::state::AppState,
    p: ProviderChoice,
    empty_too: bool,
    which: Option<ModelId>,
) {
    let Ok(_slot) = adoption_slot(state) else {
        // A scan holds the slot: applied when it is released.
        state.queue_fill(Fill {
            p,
            empty_too,
            which,
        });
        return;
    };
    let before = state.roles();
    let configured = state.provider().configured();
    for (role, id, now) in [
        (Role::Chat, ModelId::Chat, before.chat),
        (Role::Embed, ModelId::Embed, before.embed),
    ] {
        if !configured.serves(p, role) || which.is_some_and(|w| w != id) {
            continue;
        }
        let fill = match now {
            None => empty_too,
            Some(q) => q == p && model_missing(state, role),
        };
        if fill {
            match role {
                Role::Chat => fill_chat(state, p),
                Role::Embed => fill_embed(state, p),
            }
        }
    }
    after_role_change(state, before);
}

/// A [`fill_roles`] call a busy slot deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    p: ProviderChoice,
    empty_too: bool,
    which: Option<ModelId>,
}

/// The job slot changed hands (the job observer): apply the substitutions a
/// busy slot deferred. One that still finds the slot taken queues itself again.
pub fn on_slot_change(state: &crate::state::AppState) {
    for f in state.take_fills() {
        fill_roles(state, f.p, f.empty_too, f.which);
    }
}

/// The role is on a provider but the index has no model for it.
fn model_missing(state: &crate::state::AppState, role: Role) -> bool {
    match role {
        Role::Chat => matches!(
            state.with_index(|db| db.meta_get(mnema_index::META_CHAT_MODEL)),
            Ok(None)
        ),
        Role::Embed => matches!(state.with_index(|db| db.active_space()), Ok(None)),
    }
}

/// Chat: `p`'s default model, then the role. The model write is best effort:
/// Mnema's chat never reads it, and a shut index is filled at the next boot.
fn fill_chat(state: &crate::state::AppState, p: ProviderChoice) {
    // Gemma takes no tools: with the switch on it is no valid default, and the
    // role is left as it is (never moved to the cloud either).
    if p == ProviderChoice::Mnema && crate::prefs::chat_tools(state.data_dir()) {
        return;
    }
    let model = match p {
        ProviderChoice::OpenRouter => crate::models::DEFAULT_MODELS.chat,
        ProviderChoice::Mnema => LOCAL_CHAT_MODEL,
    };
    let _ = state.with_index(|db| db.meta_set(mnema_index::META_CHAT_MODEL, model));
    let _ = crate::prefs::write_role(state.data_dir(), Role::Chat, Some(p));
}

/// Embedding: the active space's model if `p` offers it, else nothing; no
/// space — `p`'s default, adopted without retiring anything. Never `Discard`:
/// an automatic act deletes no vectors.
fn fill_embed(state: &crate::state::AppState, p: ProviderChoice) {
    let space = state.with_index(|db| match db.active_space()? {
        Some(id) => Ok(Some(db.space_model(id)?.0)),
        None => Ok(None),
    });
    let ok = match space {
        // Every space was built from an OpenRouter model or from bge-m3, which
        // OpenRouter offers too; Mnema offers bge-m3 alone.
        Ok(Some(model)) => p == ProviderChoice::OpenRouter || model == LOCAL_EMBED_MODEL,
        Ok(None) => adopt_default_embedding(state, p).is_ok(),
        // A shut index: OpenRouter's model is filled at the next boot; Mnema's
        // cannot be checked against a space nobody can read.
        Err(_) => p == ProviderChoice::OpenRouter,
    };
    if ok {
        let _ = crate::prefs::write_role(state.data_dir(), Role::Embed, Some(p));
    }
}

/// Adopts `p`'s default embedding model into an index with no active space.
/// OpenRouter's width is the provider's answer; Mnema's is the pinned file's.
fn adopt_default_embedding(state: &crate::state::AppState, p: ProviderChoice) -> Result<(), Error> {
    let (model, dim) = match p {
        ProviderChoice::OpenRouter => {
            let endpoint = state.open_router_endpoint()?;
            let model = crate::models::DEFAULT_MODELS.embedding;
            let check =
                mnema_provider::check_embedding_model(&endpoint.base, &endpoint.token, model)?;
            (model, check.dim as i64)
        }
        ProviderChoice::Mnema => (LOCAL_EMBED_MODEL, LOCAL_EMBED_DIM),
    };
    state.with_index(|db| {
        db.adopt_embedding_model(
            model,
            dim,
            state.credential_ref(),
            &mnema_chunk::chunker_hash(),
        )
    })?;
    Ok(())
}

/// `p` stopped being configured: every role on it is emptied (`which` limits
/// Mnema to that model's role). Only the choice: the space, its vectors and
/// the chat model stay. The caller holds the job slot.
pub fn on_unconfigured(
    state: &crate::state::AppState,
    p: ProviderChoice,
    which: Option<ModelId>,
) -> Result<(), Error> {
    let before = state.roles();
    for (role, id, now) in [
        (Role::Embed, ModelId::Embed, before.embed),
        (Role::Chat, ModelId::Chat, before.chat),
    ] {
        if now == Some(p) && which.is_none_or(|w| w == id) {
            crate::prefs::write_role(state.data_dir(), role, None)?;
        }
    }
    after_role_change(state, before);
    Ok(())
}

/// After a role write: no role left on Mnema stops the process; a role that
/// left Mnema has its model unloaded from a process that keeps running.
pub(crate) fn after_role_change(state: &crate::state::AppState, before: Roles) {
    state.forget_provider_status();
    let after = state.roles();
    let local = state.local();
    let mnema = Some(ProviderChoice::Mnema);
    let which: Vec<ModelId> = [(after.embed, ModelId::Embed), (after.chat, ModelId::Chat)]
        .into_iter()
        .filter(|(p, _)| *p == mnema)
        .map(|(_, id)| id)
        .collect();
    // Empty keeps the last set, as at a show: nothing on Mnema loads nothing.
    if !which.is_empty() {
        *local.wanted.lock().unwrap_or_else(PoisonError::into_inner) = which;
    }
    if after.embed != mnema && after.chat != mnema {
        local.stop();
        return;
    }
    if let Some(sidecar) = local.started() {
        for (was, now, id) in [
            (before.embed, after.embed, ModelId::Embed),
            (before.chat, after.chat, ModelId::Chat),
        ] {
            if was == mnema && now != mnema {
                // Best effort: a process that will not answer holds nothing.
                let _ = sidecar.unload(Some(id));
            }
        }
    }
}

/// The launcher went cold (hidden long enough): give the memory back.
pub fn on_cold(state: &crate::state::AppState) {
    let local = state.local();
    local.cold.store(true, Ordering::SeqCst);
    local.want.store(false, Ordering::SeqCst);
    if let Some(sidecar) = local.started() {
        local.forget_loaded();
        // Best effort: a process that will not answer holds no memory worth
        // waiting for, and the next show asks again.
        // bge-m3 stays while a scan runs on Mnema: it is what the scan embeds with.
        let keep_embed = scan_running(state) && state.roles().embed == Some(ProviderChoice::Mnema);
        let _ = sidecar.unload(keep_embed.then_some(ModelId::Chat));
    }
}

/// The launcher is being shown: load what it will need, off the caller's
/// thread. The handle is for tests; production drops it.
pub fn on_show(state: &crate::state::AppState) -> std::thread::JoinHandle<()> {
    let provider = state.provider();
    provider.local.cold.store(false, Ordering::SeqCst);
    provider.local.shown.fetch_add(1, Ordering::SeqCst);
    let which = provider.mnema_models();
    // An empty set sets nothing and loads nothing: with no role on Mnema no
    // `endpoint` reaches the process, so `want` alone does no harm.
    if !which.is_empty() {
        *provider
            .local
            .wanted
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = which.clone();
    }
    // After `wanted`: a caller that sees `want` must see the set it loads.
    provider.local.want.store(true, Ordering::SeqCst);
    if which.is_empty() {
        return std::thread::spawn(|| {});
    }
    std::thread::spawn(move || {
        // `want` is set, so this starts the process and loads it.
        let _ = provider.local.endpoint();
    })
}

/// Whether a scan (reading or embedding) holds the job slot. Not any job: only
/// a scan thread calls `scan_end`, so keeping bge-m3 for a removal or an
/// adoption would keep it until the next cold cycle.
fn scan_running(state: &crate::state::AppState) -> bool {
    use crate::scan_state::{Phase, ScanSnapshot};
    matches!(
        state.scan_state().snapshot,
        ScanSnapshot::Running {
            phase: Phase::Reading { .. } | Phase::Embedding { .. },
            ..
        }
    )
}

/// When the launcher window last lost focus (a close hides it and marks again);
/// `None` until the first. Managed; written by `hide_launcher` and the
/// `Focused(false)` arm in `lib.rs`, read by [`start_cold_timer`]. A wall-clock
/// time: `Instant` does not advance while the machine sleeps, and sleep is time
/// in the background too. The mark is written only by that hide path — an
/// answer landing must never write it, or it voids the clock armed in the
/// background and arms none, and the models are never unloaded.
#[derive(Default)]
pub struct HiddenAt(Mutex<Option<SystemTime>>);

impl HiddenAt {
    pub fn mark(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(SystemTime::now());
    }
    pub fn get(&self) -> Option<SystemTime> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Starts the clock that unloads the local models: after `after` out of focus,
/// if the window has not lost focus again or been shown since, [`on_cold`].
/// Called at every focus loss and at a close. Its own timer, and the only one —
/// the window is a normal one now, so nothing is "forgotten" at a show (§1).
pub fn start_cold_timer<R: tauri::Runtime>(app: &tauri::AppHandle<R>, after: Duration) {
    use tauri::Manager as _;
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let hidden = app.state::<HiddenAt>().get();
    let shown = state.local().shown.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let state = app.state::<crate::state::AppState>();
        // A later focus loss has its own clock; a show since this one ends it.
        if app.state::<HiddenAt>().get() == hidden
            && state.local().shown.load(Ordering::SeqCst) == shown
        {
            on_cold(&state);
        }
    });
}

/// A scan ended: if the launcher went cold meanwhile, drop bge-m3 too.
pub fn on_scan_end(state: &crate::state::AppState) {
    state.provider().scan_end();
}

impl Provider {
    /// [`on_scan_end`] for a job thread, which carries a `Provider` and not
    /// `AppState`.
    pub fn scan_end(&self) {
        let r = self.roles();
        if r.embed != Some(ProviderChoice::Mnema) || !self.local.cold.load(Ordering::SeqCst) {
            return;
        }
        if let Some(sidecar) = self.local.started() {
            self.local.forget_loaded();
            let _ = sidecar.unload(Some(ModelId::Embed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_status::{Missing, ProviderStatus};
    use crate::state::AppState;

    /// An app state choosing `choice` on a host taken as able to run Mnema,
    /// whatever host the test runs on: a Linux CI run otherwise reads the
    /// stored choice as OpenRouter.
    fn state_choosing(dir: &std::path::Path, choice: ProviderChoice) -> AppState {
        for role in [Role::Embed, Role::Chat] {
            crate::prefs::write_role(dir, role, Some(choice)).unwrap();
        }
        let state = AppState::new(
            dir.to_path_buf(),
            "w".into(),
            "http://127.0.0.1:1".into(),
            String::new(),
        );
        state.install_local(
            Local::new(
                Store::new(dir.join("models"), HUB.to_string()),
                PathBuf::new(),
            )
            .with_available(true),
        );
        state
    }

    #[test]
    fn cancelling_one_download_leaves_the_other_running() {
        use std::sync::atomic::Ordering::SeqCst;
        let dir = tempfile::tempdir().unwrap();
        let local = Local::new(
            Store::new(dir.path().to_path_buf(), HUB.to_string()),
            PathBuf::from("unused"),
        );
        local
            .cancel_flag(LocalModel::Chat.into())
            .store(true, SeqCst);
        assert!(local.cancel_flag(LocalModel::Chat.into()).load(SeqCst));
        assert!(
            !local.cancel_flag(LocalModel::Embed.into()).load(SeqCst),
            "cancelling Chat cancelled Embed"
        );
        // And the other way round, since one shared flag would pass either half alone.
        local
            .cancel_flag(LocalModel::Chat.into())
            .store(false, SeqCst);
        local
            .cancel_flag(LocalModel::Embed.into())
            .store(true, SeqCst);
        assert!(!local.cancel_flag(LocalModel::Chat.into()).load(SeqCst));
    }

    #[test]
    fn mnema_without_ready_models_asks_for_the_models() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_choosing(dir.path(), ProviderChoice::Mnema);
        let endpoint = state.endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed);
        assert!(
            matches!(endpoint, Err(Error::ProviderNotReady)),
            "{endpoint:?}"
        );
        assert_eq!(
            crate::provider_status::status(&state),
            crate::provider_status::RoleStatuses {
                embed: ProviderStatus::NotConfigured {
                    missing: Missing::LocalModels
                },
                chat: ProviderStatus::NotConfigured {
                    missing: Missing::LocalModels
                },
            }
        );
    }

    #[test]
    fn openrouter_endpoint_is_unchanged() {
        mnema_secrets::test_store::register();
        let dir = tempfile::tempdir().unwrap();
        crate::prefs::write_key(
            dir.path(),
            PREFS_KEY,
            serde_json::to_value(ProviderChoice::OpenRouter).unwrap(),
        )
        .unwrap();
        let reference = format!("mnema-desktop-provider-test-{}", dir.path().display());
        mnema_secrets::store(&reference, "sk-test-synthetic").unwrap();
        let state = AppState::new(
            dir.path().to_path_buf(),
            "w".into(),
            mnema_provider::OPENROUTER_BASE.into(),
            reference,
        );
        let endpoint = state.endpoint_for(state.roles().chat, ModelId::Chat);
        assert!(
            matches!(&endpoint, Ok(e) if e.base == mnema_provider::OPENROUTER_BASE
                && e.query_base == mnema_provider::OPENROUTER_BASE
                && e.token == "sk-test-synthetic"),
            "{endpoint:?}"
        );
    }

    #[test]
    fn the_voice_remembers_the_previous_language() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_choosing(dir.path(), ProviderChoice::Mnema);
        let mnema = ProviderChoice::Mnema;
        for system in [Some("en-US"), Some("de-DE"), None] {
            assert_eq!(
                state.voice_with(mnema, "Як налаштувати резервну копію?", system),
                mnema_rag::Voice::Mnema(mnema_rag::UK)
            );
            assert_eq!(
                state.voice_with(mnema, "1.2.3?", system),
                mnema_rag::Voice::Mnema(mnema_rag::UK),
                "a question with no language of its own takes the previous one, system {system:?}"
            );
        }
    }
}

#[cfg(test)]
mod lifecycle {
    use super::*;
    use crate::state::AppState;
    use std::io::{Read, Write};
    use std::path::Path;

    fn fake_mlx() -> PathBuf {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        let path = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps")
            .join("mnema-local-fake-mlx");
        assert!(
            path.exists(),
            "{} is missing: cargo build -p mnema-local --bin mnema-local-fake-mlx",
            path.display()
        );
        path
    }

    /// An app state choosing `choice`, whose local models are `Ready` and whose
    /// process is the fake, logging to the returned file.
    fn app(dir: &Path, choice: ProviderChoice, env: &[(&str, &str)]) -> (AppState, PathBuf) {
        crate::prefs::write_key(dir, PREFS_KEY, serde_json::to_value(choice).unwrap()).unwrap();
        let state = AppState::new(
            dir.to_path_buf(),
            "w".into(),
            "http://127.0.0.1:1".into(),
            String::new(),
        );
        let (local, log) = local_at(dir, env);
        state.install_local(local);
        (state, log)
    }

    /// A [`Local`] over the fake with both models `Ready`, logging to the
    /// returned file.
    fn local_at(dir: &Path, env: &[(&str, &str)]) -> (Local, PathBuf) {
        use mnema_local::{FileSpec, Manifest, ModelSpec};
        let spec = |name: &str| ModelSpec {
            repo: format!("test/{name}"),
            commit: "0".into(),
            files: vec![FileSpec::new("weights", 1, "")],
        };
        let store =
            Store::new(dir.join("models"), "http://127.0.0.1:1".into()).with_manifest(Manifest {
                embed: spec("embed"),
                chat: spec("chat"),
            });
        for id in [ModelId::Embed, ModelId::Chat] {
            std::fs::create_dir_all(store.dir(id)).unwrap();
            std::fs::write(store.dir(id).join("weights"), b"x").unwrap();
        }
        let log = dir.join("fake-mlx.log");
        let log_env = log.display().to_string();
        let mut all = vec![("FAKE_MLX_LOG", log_env.as_str())];
        all.extend_from_slice(env);
        let local = Local::new(store, fake_mlx())
            .with_env(&all)
            .with_available(true);
        (local, log)
    }

    /// Panics on its first call, then loads as the real one does.
    fn a_load_that_panics_once(
        sidecar: &Sidecar,
        which: &[ModelId],
    ) -> Result<(), mnema_local::Error> {
        static CALLS: AtomicU64 = AtomicU64::new(0);
        if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("a load that panics (test)");
        }
        sidecar.load(which)
    }

    /// A load thread that panics must not leave its load "in flight" for ever:
    /// the caller waiting on it hears an error, and the next use loads again.
    #[test]
    fn a_load_that_panics_wakes_its_waiter_and_the_next_use_loads_again() {
        let dir = tempfile::tempdir().unwrap();
        let (local, log) = local_at(dir.path(), &[]);
        let local = Arc::new(local.with_load(a_load_that_panics_once));
        local.want.store(true, Ordering::SeqCst);

        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = Arc::clone(&local);
        std::thread::spawn(move || {
            let _ = tx.send(waiter.endpoint().map(|_| ()));
        });
        let first = rx.recv_timeout(Duration::from_secs(5));
        assert!(
            matches!(first, Ok(Err(_))),
            "the waiter on a panicked load must hear an error, got {first:?}"
        );

        let (tx, rx) = std::sync::mpsc::channel();
        let next = Arc::clone(&local);
        std::thread::spawn(move || {
            let _ = tx.send(next.endpoint().map(|_| ()));
        });
        let second = rx.recv_timeout(Duration::from_secs(5));
        assert!(
            matches!(second, Ok(Ok(()))),
            "the next use loads again, got {second:?}"
        );
        assert_eq!(count(&log, "start POST /mnema/load"), 1, "{}", logged(&log));
    }

    /// Polls `what` for up to 5 s. The fake logs `spawn` after it prints
    /// `PORT`, so a count read the instant `endpoint()` returns can be one short.
    fn eventually(what: impl Fn() -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if what() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        what()
    }

    /// Waits until the process the state's local provider holds has exited.
    fn wait_dead(state: &AppState) {
        let pid = state.local().started().expect("a process").pid();
        assert!(
            // A child nobody has reaped yet is a zombie (`Z`), and is dead.
            eventually(|| {
                let out = std::process::Command::new("ps")
                    .args(["-o", "stat=", "-p", &pid.to_string()])
                    .output()
                    .expect("ps runs");
                !out.status.success()
                    || String::from_utf8_lossy(&out.stdout)
                        .trim_start()
                        .starts_with('Z')
            }),
            "process {pid} is still alive"
        );
    }

    fn logged(log: &Path) -> String {
        std::fs::read_to_string(log).unwrap_or_default()
    }

    fn count(log: &Path, needle: &str) -> usize {
        logged(log).lines().filter(|l| *l == needle).count()
    }

    #[test]
    fn going_cold_unloads_under_mnema() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        on_cold(&state);
        assert_eq!(
            count(&log, "start POST /mnema/unload"),
            1,
            "{}",
            logged(&log)
        );
    }

    fn set_roles(dir: &Path, embed: ProviderChoice, chat: ProviderChoice) {
        crate::prefs::write_role(dir, Role::Embed, Some(embed)).unwrap();
        crate::prefs::write_role(dir, Role::Chat, Some(chat)).unwrap();
    }

    #[test]
    fn a_chat_only_mnema_role_loads_only_chat() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::OpenRouter,
            ProviderChoice::Mnema,
        );
        on_show(&state).join().unwrap();
        let text = logged(&log);
        assert!(
            text.lines()
                .any(|l| l == r#"body /mnema/load {"models":["chat"]}"#),
            "{text}"
        );
    }

    #[test]
    fn an_embed_only_role_starts_without_gemma_files() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::Mnema,
            ProviderChoice::OpenRouter,
        );
        std::fs::remove_dir_all(state.local().store.dir(ModelId::Chat)).unwrap();
        on_show(&state).join().unwrap();
        let text = logged(&log);
        assert!(
            text.lines()
                .any(|l| l == r#"body /mnema/load {"models":["embed"]}"#),
            "{text}"
        );
    }

    /// `wanted` still at its default (both) while Gemma is not downloaded: the
    /// load asks for bge-m3 alone, or the process fails every load.
    #[test]
    fn a_stale_wanted_set_loads_only_downloaded_models() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(dir.path(), ProviderChoice::Mnema, ProviderChoice::Mnema);
        std::fs::remove_dir_all(state.local().store.dir(ModelId::Chat)).unwrap();
        state.local().want.store(true, Ordering::SeqCst);
        let got = state.endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed);
        let text = logged(&log);
        assert!(got.is_ok(), "the embed endpoint loads: {got:?}\n{text}");
        assert!(
            text.lines()
                .any(|l| l == r#"body /mnema/load {"models":["embed"]}"#),
            "{text}"
        );
    }

    /// A role change sets the load set to the roles on Mnema, so a chat role
    /// moved to OpenRouter no longer loads Gemma.
    #[test]
    fn a_role_change_resets_the_load_set() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(dir.path(), ProviderChoice::Mnema, ProviderChoice::Mnema);
        let before = state.roles();
        set_roles(
            dir.path(),
            ProviderChoice::Mnema,
            ProviderChoice::OpenRouter,
        );
        after_role_change(&state, before);
        assert_eq!(*state.local().wanted.lock().unwrap(), vec![ModelId::Embed]);
    }

    #[test]
    fn no_role_on_mnema_neither_starts_nor_unloads() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::OpenRouter,
            ProviderChoice::OpenRouter,
        );
        on_show(&state).join().unwrap();
        on_cold(&state);
        assert_eq!(logged(&log), "");
        assert!(state.local().started().is_none());
    }

    #[test]
    fn going_cold_does_nothing_under_openrouter() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::OpenRouter, &[]);
        on_cold(&state);
        assert!(state.local().started().is_none(), "no process was started");
        assert_eq!(logged(&log), "", "the fake was never run");
    }

    #[test]
    fn showing_loads_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(
            dir.path(),
            ProviderChoice::Mnema,
            &[("FAKE_MLX_LOAD_MS", "2000")],
        );
        let t = std::time::Instant::now();
        let loading = on_show(&state);
        assert!(
            t.elapsed() < Duration::from_millis(50),
            "on_show took {:?}",
            t.elapsed()
        );
        loading.join().unwrap();
        assert_eq!(count(&log, "start POST /mnema/load"), 1, "{}", logged(&log));
    }

    /// One authorised POST to the process, over a bare socket (this crate has
    /// no HTTP client of its own). Returns the whole response.
    fn post(ep: &Endpoint, path: &str, body: &str) -> String {
        let addr = ep
            .base
            .trim_start_matches("http://")
            .trim_end_matches("/v1")
            .to_string();
        let mut s = std::net::TcpStream::connect(&addr).expect("the process listens");
        write!(
            s,
            "POST {path} HTTP/1.1\r\nhost: {addr}\r\nauthorization: Bearer {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            ep.token,
            body.len()
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn a_question_during_load_waits_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(
            dir.path(),
            ProviderChoice::Mnema,
            &[("FAKE_MLX_LOAD_MS", "600")],
        );
        let loading = on_show(&state);
        let ep = state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the question's endpoint");
        let answer = post(&ep, "/v1/chat/completions", "{}");
        loading.join().unwrap();
        let text = logged(&log);
        let at = |needle: &str| text.lines().position(|l| l == needle);
        let (loaded, asked) = (
            at("end POST /mnema/load"),
            at("start POST /v1/chat/completions"),
        );
        assert!(
            matches!((loaded, asked), (Some(l), Some(a)) if l < a),
            "the load must finish before the chat starts:\n{text}"
        );
        assert!(answer.contains(r#""content":"ok""#), "{answer}");
    }

    #[test]
    fn openrouter_never_starts_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::OpenRouter, &[]);
        on_show(&state).join().unwrap();
        on_cold(&state);
        assert_eq!(count_prefix(&log, "spawn"), 0, "{}", logged(&log));
        assert!(state.local().started().is_none());
    }

    fn count_prefix(log: &Path, prefix: &str) -> usize {
        logged(log)
            .lines()
            .filter(|l| l.starts_with(prefix))
            .count()
    }

    #[test]
    fn going_cold_during_a_scan_keeps_bge_m3() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        // The slot a scan holds; any job that holds it counts, as for every
        // other "a job is running" question.
        let _scan = state
            .claim_job(
                crate::scan_state::Phase::Embedding {
                    counts: crate::job::Progress::default(),
                },
                false,
            )
            .unwrap();
        on_cold(&state);
        let text = logged(&log);
        assert_eq!(count(&log, "start POST /mnema/unload"), 1, "{text}");
        assert!(
            text.lines()
                .any(|l| l == r#"body /mnema/unload {"models":["chat"]}"#),
            "only the chat model goes while a scan embeds:\n{text}"
        );
    }

    fn scan_slot(state: &AppState) -> crate::state::JobSlot {
        state
            .claim_job(
                crate::scan_state::Phase::Embedding {
                    counts: crate::job::Progress::default(),
                },
                false,
            )
            .unwrap()
    }

    const EMBED_GONE: &str = r#"body /mnema/unload {"models":["embed"]}"#;

    #[test]
    fn the_scan_ending_while_cold_unloads_bge_m3() {
        // Cold while the scan ran: bge-m3 goes when it ends.
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let scan = scan_slot(&state);
        on_cold(&state);
        drop(scan);
        on_scan_end(&state);
        assert!(
            logged(&log).lines().any(|l| l == EMBED_GONE),
            "{}",
            logged(&log)
        );

        // Shown again meanwhile: the launcher is not cold, bge-m3 stays.
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let scan = scan_slot(&state);
        on_cold(&state);
        on_show(&state).join().unwrap();
        drop(scan);
        on_scan_end(&state);
        assert!(
            !logged(&log).lines().any(|l| l == EMBED_GONE),
            "{}",
            logged(&log)
        );
    }

    #[test]
    fn going_cold_during_a_scan_with_embed_on_openrouter_unloads_both() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::OpenRouter,
            ProviderChoice::Mnema,
        );
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let _scan = scan_slot(&state);
        on_cold(&state);
        let text = logged(&log);
        assert!(
            text.lines().any(|l| l == "body /mnema/unload "),
            "the scan embeds elsewhere, so nothing is kept:\n{text}"
        );
    }

    #[test]
    fn the_scan_end_unload_follows_the_embed_role() {
        // Embed on Mnema, chat elsewhere: bge-m3 goes when the cold scan ends.
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::Mnema,
            ProviderChoice::OpenRouter,
        );
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let scan = scan_slot(&state);
        on_cold(&state);
        drop(scan);
        on_scan_end(&state);
        assert!(
            logged(&log).lines().any(|l| l == EMBED_GONE),
            "{}",
            logged(&log)
        );

        // Chat on Mnema, embed elsewhere: bge-m3 was never this process's.
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        set_roles(
            dir.path(),
            ProviderChoice::OpenRouter,
            ProviderChoice::Mnema,
        );
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let scan = scan_slot(&state);
        on_cold(&state);
        drop(scan);
        on_scan_end(&state);
        assert!(
            !logged(&log).lines().any(|l| l == EMBED_GONE),
            "{}",
            logged(&log)
        );
    }

    #[test]
    fn each_role_asks_only_for_its_own_model() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        std::fs::remove_dir_all(state.local().store.dir(ModelId::Chat)).unwrap();
        let mnema = Some(ProviderChoice::Mnema);
        assert!(matches!(
            state.endpoint_for(mnema, ModelId::Chat),
            Err(Error::ProviderNotReady)
        ));
        assert!(matches!(
            state.endpoint_for(None, ModelId::Embed),
            Err(Error::RoleEmpty)
        ));
        assert_eq!(count_prefix(&log, "spawn"), 0);
        state
            .endpoint_for(mnema, ModelId::Embed)
            .expect("the embed model alone is enough for the embed role");
    }

    #[test]
    fn a_host_that_cannot_run_mnema_has_no_mnema_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        let (local, _) = local_at(dir.path(), &[]);
        state.install_local(local.with_available(false));
        let endpoint = state.endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed);
        assert!(
            matches!(endpoint, Err(Error::ProviderNotReady)),
            "{endpoint:?}"
        );
        assert!(!state.mnema_available());
        assert_eq!(count_prefix(&log, "spawn"), 0);
    }

    #[test]
    fn a_restarted_process_is_loaded_again_before_the_next_question() {
        let dir = tempfile::tempdir().unwrap();
        // Dies after its second authorised request: the show's load, then one chat.
        let (state, log) = app(
            dir.path(),
            ProviderChoice::Mnema,
            &[("FAKE_MLX_DIE_AFTER", "2")],
        );
        on_show(&state).join().unwrap();
        let ep = state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the first process answers");
        post(&ep, "/v1/chat/completions", "{}");
        wait_dead(&state);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the supervisor's restart");
        let text = logged(&log);
        assert!(eventually(|| count_prefix(&log, "spawn") == 2), "{text}");
        assert_eq!(
            count(&log, "start POST /mnema/load"),
            2,
            "the new process holds no models until it is told to load:\n{text}"
        );
    }

    /// [`app`] with both roles on Mnema, an open index, and the process up.
    fn started_on_mnema(dir: &Path) -> (AppState, PathBuf) {
        let (state, log) = app(dir, ProviderChoice::Mnema, &[]);
        set_roles(dir, ProviderChoice::Mnema, ProviderChoice::Mnema);
        state.open_index().expect("an index");
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        (state, log)
    }

    #[test]
    fn moving_chat_off_mnema_unloads_gemma() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = started_on_mnema(dir.path());
        crate::models::choose_chat_model(&state, "vendor/chat".into(), ProviderChoice::OpenRouter)
            .expect("chat moves to OpenRouter");
        assert_eq!(
            count(&log, r#"body /mnema/unload {"models":["chat"]}"#),
            1,
            "{}",
            logged(&log)
        );
        assert!(
            state.local().started().is_some(),
            "embedding is still on Mnema: the process stays"
        );
    }

    #[test]
    fn no_mnema_role_stops_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _log) = started_on_mnema(dir.path());
        crate::prefs::write_role(dir.path(), Role::Embed, Some(ProviderChoice::OpenRouter))
            .unwrap();
        crate::models::choose_chat_model(&state, "vendor/chat".into(), ProviderChoice::OpenRouter)
            .expect("chat moves to OpenRouter");
        assert!(
            state.local().started().is_none(),
            "no process is left running with no role on Mnema"
        );
    }

    #[test]
    fn a_process_that_gave_up_is_replaced_by_the_next_explicit_use() {
        let dir = tempfile::tempdir().unwrap();
        // Every process dies after its first authorised request.
        let (state, log) = app(
            dir.path(),
            ProviderChoice::Mnema,
            &[("FAKE_MLX_DIE_AFTER", "1")],
        );
        let die = |state: &AppState| {
            let ep = state
                .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
                .expect("an endpoint");
            post(&ep, "/v1/models", "");
            wait_dead(state);
        };
        die(&state); // process 1
        die(&state); // its one restart
        assert!(
            matches!(
                state.endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed),
                Err(Error::Local(_))
            ),
            "dead twice, and said so"
        );
        // A status poll finds it dead and never spawns.
        assert!(matches!(state.local().running(), Some(Err(_)) | None));
        assert!(
            eventually(|| count_prefix(&log, "spawn") == 2),
            "{}",
            logged(&log)
        );
        // The next explicit use starts a fresh one, with a fresh restart budget.
        let fresh = state.endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed);
        assert!(fresh.is_ok(), "{fresh:?}");
        assert!(
            eventually(|| count_prefix(&log, "spawn") == 3),
            "{}",
            logged(&log)
        );
    }

    fn unloads_after(
        hide_then: impl FnOnce(&AppState, &tauri::AppHandle<tauri::test::MockRuntime>),
    ) -> usize {
        use tauri::Manager as _;
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        let mock = tauri::test::mock_app();
        mock.manage(HiddenAt::default());
        mock.manage(state);
        let handle = mock.handle().clone();
        let state = handle.state::<AppState>();
        hide_then(&state, &handle);
        std::thread::sleep(Duration::from_millis(900));
        count(&log, "start POST /mnema/unload")
    }

    #[test]
    fn the_clock_makes_a_hidden_launcher_cold() {
        use tauri::Manager as _;
        let hide = |_: &AppState, app: &tauri::AppHandle<tauri::test::MockRuntime>| {
            app.state::<HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(100));
        };
        assert_eq!(unloads_after(hide), 1, "hidden, and left alone");

        // Shown again before it fires.
        let shown = |state: &AppState, app: &tauri::AppHandle<tauri::test::MockRuntime>| {
            app.state::<HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(400));
            on_show(state).join().unwrap();
        };
        assert_eq!(unloads_after(shown), 0, "shown before the clock ran out");

        // Hidden again (blur after a hide): the older clock is not the one that counts.
        let rehidden = |_: &AppState, app: &tauri::AppHandle<tauri::test::MockRuntime>| {
            app.state::<HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(400));
            // SystemTime must have moved for the second mark to differ.
            std::thread::sleep(Duration::from_millis(20));
            app.state::<HiddenAt>().mark();
        };
        assert_eq!(unloads_after(rehidden), 0, "a later hide owns the clock");
    }

    #[test]
    fn going_cold_during_a_job_that_is_not_a_scan_unloads_everything() {
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state
            .endpoint_for(Some(ProviderChoice::Mnema), ModelId::Embed)
            .expect("the process starts");
        // Only a scan thread calls `scan_end`; keeping bge-m3 for this job would
        // keep it resident for good.
        let _adoption = state
            .claim_job(
                crate::scan_state::Phase::Other {
                    job: crate::scan_state::OtherJob::ModelAdoption,
                },
                false,
            )
            .unwrap();
        on_cold(&state);
        let text = logged(&log);
        assert!(text.lines().any(|l| l == "body /mnema/unload "), "{text}");
    }
}
