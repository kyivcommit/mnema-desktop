//! Which provider answers — OpenRouter or the local Mnema process — behind ONE
//! endpoint. Every call that leaves for a model asks [`endpoint`] (through
//! `AppState::endpoint` or a job's captured [`Provider`]), so the two can never
//! be chosen differently at two sites.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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
/// first [`endpoint`] that finds both models `Ready`.
pub struct Local {
    pub store: Store,
    binary: PathBuf,
    env: Vec<(String, String)>,
    sidecar: Mutex<Option<Arc<Sidecar>>>,
    /// A load started by [`on_show`] is under way: [`Local::endpoint`] waits
    /// for it, so a question asked the instant the launcher opens meets
    /// loaded models and not a half-loaded process.
    loading: Mutex<bool>,
    loaded: Condvar,
    /// The launcher went cold and has not been shown since. A scan that ends
    /// now finishes the unloading [`on_cold`] left half done.
    cold: AtomicBool,
    /// How many times the launcher has been shown: a cold clock started before
    /// the last show is void.
    shown: AtomicU64,
    /// The launcher is up (or was, not yet cold): the models should be in
    /// memory. A process that restarted holds none, so [`Local::endpoint`]
    /// loads them again whenever this is set and `loaded_pid` is not the
    /// running process.
    want: AtomicBool,
    loaded_pid: Mutex<u32>,
    cancel: [AtomicBool; 2],
}

impl Local {
    pub fn new(store: Store, binary: PathBuf) -> Self {
        Self {
            store,
            binary,
            env: Vec::new(),
            sidecar: Mutex::new(None),
            loading: Mutex::new(false),
            loaded: Condvar::new(),
            cold: AtomicBool::new(false),
            shown: AtomicU64::new(0),
            want: AtomicBool::new(false),
            loaded_pid: Mutex::new(0),
            cancel: [AtomicBool::new(false), AtomicBool::new(false)],
        }
    }

    /// Extra environment for the process (tests steer the fake with it).
    pub fn with_env(mut self, env: &[(&str, &str)]) -> Self {
        self.env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    pub fn models_ready(&self) -> bool {
        [ModelId::Embed, ModelId::Chat]
            .into_iter()
            .all(|id| self.store.state(id) == ModelState::Ready)
    }

    /// The running process's endpoint, starting it on first use. Held under
    /// the lock for the start so two callers cannot start two processes.
    fn endpoint(&self) -> Result<Endpoint, Error> {
        {
            let mut loading = self
                .loading
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while *loading {
                loading = self
                    .loaded
                    .wait(loading)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
        self.endpoint_now()
    }

    fn set_loading(&self, on: bool) {
        *self
            .loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = on;
        self.loaded.notify_all();
    }

    /// [`Local::endpoint`] without waiting for a load: the load's own thread
    /// asks through this, or it would wait for itself.
    fn endpoint_now(&self) -> Result<Endpoint, Error> {
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
            // Held across the load: two callers cannot load the same process
            // twice, and the second finds it already done.
            let mut loaded = self
                .loaded_pid
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *loaded != sidecar.pid() {
                sidecar.load()?;
                *loaded = sidecar.pid();
            }
        }
        Ok(sidecar.endpoint()?)
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
    fn forget_loaded(&self) {
        *self
            .loaded_pid
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 0;
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

    /// Stops the process (if any): the next [`endpoint`] starts a fresh one
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

/// Everything [`endpoint`] reads, owned, so a job thread can carry it without
/// `AppState`.
#[derive(Clone)]
pub struct Provider {
    pub data_dir: PathBuf,
    pub openrouter_base: String,
    pub credential_ref: String,
    pub local: Arc<Local>,
}

impl Provider {
    /// The choice as the preferences file states it right now.
    pub fn choice(&self) -> ProviderChoice {
        crate::prefs::provider_choice(&self.data_dir)
    }

    /// Where to send model requests, and the secret for them.
    ///
    /// OpenRouter: its base for both `base` and `query_base`, and the stored key
    /// (`Error::NoKey` when none). Mnema: the local process, or
    /// `Error::ProviderNotReady` while either model is not downloaded.
    pub fn endpoint(&self) -> Result<Endpoint, Error> {
        self.endpoint_as(self.choice())
    }

    /// [`Provider::endpoint`] for a choice the caller already read — so a
    /// command that branches on the choice and then asks for the endpoint
    /// cannot read the preferences file twice and get two answers.
    pub fn endpoint_as(&self, choice: ProviderChoice) -> Result<Endpoint, Error> {
        endpoint(
            choice,
            &self.openrouter_base,
            &self.credential_ref,
            &self.local,
        )
    }
}

pub fn endpoint(
    choice: ProviderChoice,
    openrouter_base: &str,
    credential_ref: &str,
    local: &Local,
) -> Result<Endpoint, Error> {
    match choice {
        ProviderChoice::OpenRouter => {
            let token = mnema_secrets::load(credential_ref)?.ok_or(Error::NoKey)?;
            Ok(Endpoint {
                base: openrouter_base.to_string(),
                query_base: openrouter_base.to_string(),
                token,
            })
        }
        ProviderChoice::Mnema => {
            if !local.models_ready() {
                return Err(Error::ProviderNotReady);
            }
            local.endpoint()
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
pub fn provider_choice(state: tauri::State<'_, crate::state::AppState>) -> ProviderChoice {
    state.provider_choice()
}

/// What [`set_provider_choice`] did: the choice now in force, and the spaces a
/// confirmed `Discard` destroyed on the way (empty otherwise) — the same fact
/// `set_embedding_model` puts on the wire as `AdoptedModel::retired`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSwitch {
    pub choice: ProviderChoice,
    pub retired: Vec<crate::models::RetiredSpace>,
}

#[tauri::command(async)]
pub fn set_provider_choice(
    state: tauri::State<'_, crate::state::AppState>,
    choice: ProviderChoice,
    existing_vectors: crate::models::ExistingVectors,
) -> Result<ProviderSwitch, Error> {
    change(&state, choice, existing_vectors)
}

/// [`set_provider_choice`]'s body, reachable without a `State`.
pub fn change(
    state: &crate::state::AppState,
    choice: ProviderChoice,
    existing_vectors: crate::models::ExistingVectors,
) -> Result<ProviderSwitch, Error> {
    let mut retired = Vec::new();
    // The job slot, for the reason `models::set_embedding_model` takes it: a
    // scan resolves its endpoint once, on its own thread, after its claim
    // (`scan_job::ScanDeps::production`). Refused while one runs, the choice
    // cannot move under a run that is already embedding through the other
    // provider; claimed, no run can start until the choice is written.
    let mut slot = state.claim_job(
        crate::scan_state::Phase::Other {
            job: crate::scan_state::OtherJob::ModelAdoption,
        },
        false,
    )?;
    if choice == ProviderChoice::Mnema {
        // The local process embeds with bge-m3 alone. From OpenRouter's
        // bge-m3 this finds the same space and moves nothing; from any other
        // model with vectors it is refused under `Keep` — the ordinary model
        // change's own refusal, which the window answers with its count-based
        // confirmation — and retires the old space under `Discard` (owner's
        // ruling on F1, option B). Refused, the choice is not written.
        let (before, adopted) = state.with_index(|db| {
            let before = db.active_space()?;
            Ok((
                before,
                crate::models::adopt_retiring_whatever_blocks(
                    db,
                    LOCAL_EMBED_MODEL,
                    LOCAL_EMBED_DIM,
                    state.credential_ref(),
                    &mnema_chunk::chunker_hash(),
                    existing_vectors,
                ),
            ))
        })?;
        // `set_embedding_model`'s rule: a kept resumable report counts against
        // the active space, so an exit that moved it gives the report up. Only
        // one that moved it: from OpenRouter's bge-m3 the space stays, and so
        // does «Продовжити» with its partial-reading warning.
        match adopted {
            Ok((space, gone)) => {
                if before != Some(space.space_id) {
                    slot.forget_restore();
                }
                retired = gone;
            }
            Err(e) => {
                if matches!(e, Error::RetiredThenFailed { .. }) {
                    slot.forget_restore();
                }
                return Err(e);
            }
        }
    }
    crate::prefs::write_key(
        state.data_dir(),
        PREFS_KEY,
        serde_json::to_value(choice).expect("a unit variant serialises"),
    )?;
    state.forget_provider_status();
    if choice != ProviderChoice::Mnema {
        // Nothing of the local provider should stay in memory under OpenRouter.
        state.local().stop();
    }
    Ok(ProviderSwitch {
        choice: state.provider_choice(),
        retired,
    })
}

#[tauri::command]
pub fn mnema_available() -> bool {
    mnema_local::available()
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
    // The job slot, as `change` takes it: a scan embedding through the process
    // would otherwise end Failed with "connection refused" for a removal the
    // person made. Drawn as a model change (`ModelAdoption`) — it is one; the
    // kept resumable ending is restored on release, the space does not move.
    let _slot = state.claim_job(
        crate::scan_state::Phase::Other {
            job: crate::scan_state::OtherJob::ModelAdoption,
        },
        false,
    )?;
    let local = state.local();
    local.stop();
    local.store.remove(id.into())?;
    Ok(())
}

/// The launcher went cold (hidden long enough): give the memory back.
pub fn on_cold(state: &crate::state::AppState) {
    let local = state.local();
    local.cold.store(true, Ordering::SeqCst);
    local.want.store(false, Ordering::SeqCst);
    if state.provider_choice() != ProviderChoice::Mnema {
        return;
    }
    if let Some(sidecar) = local.started() {
        local.forget_loaded();
        // Best effort: a process that will not answer holds no memory worth
        // waiting for, and the next show asks again.
        // bge-m3 stays while a scan runs: it is what the scan embeds with.
        let _ = sidecar.unload(state.job_is_running().then_some(ModelId::Chat));
    }
}

/// The launcher is being shown: load what it will need, off the caller's
/// thread. The handle is for tests; production drops it.
pub fn on_show(state: &crate::state::AppState) -> std::thread::JoinHandle<()> {
    let provider = state.provider();
    provider.local.cold.store(false, Ordering::SeqCst);
    provider.local.shown.fetch_add(1, Ordering::SeqCst);
    provider.local.want.store(true, Ordering::SeqCst);
    if provider.choice() != ProviderChoice::Mnema || !provider.local.models_ready() {
        return std::thread::spawn(|| {});
    }
    // Set before the thread exists, so a question asked the instant `on_show`
    // returns already finds the load pending.
    provider.local.set_loading(true);
    std::thread::spawn(move || {
        // `want` is set, so this starts the process and loads it.
        let _ = provider.local.endpoint_now();
        provider.local.set_loading(false);
    })
}

/// Starts the clock that makes the launcher cold: after `after`, if it has not
/// been hidden again or shown since, [`on_cold`]. Called at every hide. Its own
/// timer, not [`crate::go_cold_if_idle`], which only runs at a show — too late,
/// the person is back by then.
pub fn start_cold_timer<R: tauri::Runtime>(app: &tauri::AppHandle<R>, after: Duration) {
    use tauri::Manager as _;
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let hidden = app.state::<crate::launcher_layout::HiddenAt>().get();
    let shown = state.local().shown.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let state = app.state::<crate::state::AppState>();
        // A later hide has its own clock; a show since this hide ends it.
        if app.state::<crate::launcher_layout::HiddenAt>().get() == hidden
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
        if self.choice() != ProviderChoice::Mnema || !self.local.cold.load(Ordering::SeqCst) {
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

    fn state_choosing(dir: &std::path::Path, choice: ProviderChoice) -> AppState {
        crate::prefs::write_key(dir, PREFS_KEY, serde_json::to_value(choice).unwrap()).unwrap();
        AppState::new(
            dir.to_path_buf(),
            "w".into(),
            "http://127.0.0.1:1".into(),
            String::new(),
        )
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
        let endpoint = state.endpoint();
        assert!(
            matches!(endpoint, Err(Error::ProviderNotReady)),
            "{endpoint:?}"
        );
        assert_eq!(
            crate::provider_status::status(&state),
            ProviderStatus::NotConfigured {
                missing: Missing::LocalModels
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
        let endpoint = state.endpoint();
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
        use mnema_local::{FileSpec, Manifest, ModelSpec};
        crate::prefs::write_key(dir, PREFS_KEY, serde_json::to_value(choice).unwrap()).unwrap();
        let state = AppState::new(
            dir.to_path_buf(),
            "w".into(),
            "http://127.0.0.1:1".into(),
            String::new(),
        );
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
        state.install_local(Local::new(store, fake_mlx()).with_env(&all));
        (state, log)
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
        state.endpoint().expect("the process starts");
        on_cold(&state);
        assert_eq!(
            count(&log, "start POST /mnema/unload"),
            1,
            "{}",
            logged(&log)
        );
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
        let ep = state.endpoint().expect("the question's endpoint");
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
        state.endpoint().expect("the process starts");
        // The slot a scan holds; any job that holds it counts, as for every
        // other "a job is running" question.
        let _scan = state
            .claim_job(
                crate::scan_state::Phase::Other {
                    job: crate::scan_state::OtherJob::Probe,
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
                crate::scan_state::Phase::Other {
                    job: crate::scan_state::OtherJob::Probe,
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
        state.endpoint().expect("the process starts");
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
        state.endpoint().expect("the process starts");
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
    fn a_restarted_process_is_loaded_again_before_the_next_question() {
        let dir = tempfile::tempdir().unwrap();
        // Dies after its second authorised request: the show's load, then one chat.
        let (state, log) = app(
            dir.path(),
            ProviderChoice::Mnema,
            &[("FAKE_MLX_DIE_AFTER", "2")],
        );
        on_show(&state).join().unwrap();
        let ep = state.endpoint().expect("the first process answers");
        post(&ep, "/v1/chat/completions", "{}");
        std::thread::sleep(Duration::from_millis(300));
        state.endpoint().expect("the supervisor's restart");
        let text = logged(&log);
        assert_eq!(count_prefix(&log, "spawn"), 2, "{text}");
        assert_eq!(
            count(&log, "start POST /mnema/load"),
            2,
            "the new process holds no models until it is told to load:\n{text}"
        );
    }

    #[test]
    fn leaving_mnema_stops_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state.endpoint().expect("the process starts");
        assert!(state.local().started().is_some());
        let switched = change(
            &state,
            ProviderChoice::OpenRouter,
            crate::models::ExistingVectors::Keep,
        )
        .expect("switching to OpenRouter");
        assert_eq!(switched.choice, ProviderChoice::OpenRouter);
        assert!(
            state.local().started().is_none(),
            "no process is left running under OpenRouter"
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
            let ep = state.endpoint().expect("an endpoint");
            post(&ep, "/v1/models", "");
            std::thread::sleep(Duration::from_millis(300));
        };
        die(&state); // process 1
        die(&state); // its one restart
        assert!(
            matches!(state.endpoint(), Err(Error::Local(_))),
            "dead twice, and said so"
        );
        // A status poll finds it dead and never spawns.
        assert!(matches!(state.local().running(), Some(Err(_)) | None));
        assert_eq!(count_prefix(&log, "spawn"), 2, "{}", logged(&log));
        // The next explicit use starts a fresh one, with a fresh restart budget.
        let fresh = state.endpoint();
        assert!(fresh.is_ok(), "{fresh:?}");
        assert_eq!(count_prefix(&log, "spawn"), 3, "{}", logged(&log));
    }

    fn unloads_after(
        hide_then: impl FnOnce(&AppState, &tauri::AppHandle<tauri::test::MockRuntime>),
    ) -> usize {
        use tauri::Manager as _;
        let dir = tempfile::tempdir().unwrap();
        let (state, log) = app(dir.path(), ProviderChoice::Mnema, &[]);
        state.endpoint().expect("the process starts");
        let mock = tauri::test::mock_app();
        mock.manage(crate::launcher_layout::HiddenAt::default());
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
            app.state::<crate::launcher_layout::HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(100));
        };
        assert_eq!(unloads_after(hide), 1, "hidden, and left alone");

        // Shown again before it fires.
        let shown = |state: &AppState, app: &tauri::AppHandle<tauri::test::MockRuntime>| {
            app.state::<crate::launcher_layout::HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(400));
            std::thread::sleep(Duration::from_millis(50));
            on_show(state).join().unwrap();
        };
        assert_eq!(unloads_after(shown), 0, "shown before the clock ran out");

        // Hidden again (blur after a hide): the older clock is not the one that counts.
        let rehidden = |_: &AppState, app: &tauri::AppHandle<tauri::test::MockRuntime>| {
            app.state::<crate::launcher_layout::HiddenAt>().mark();
            start_cold_timer(app, Duration::from_millis(400));
            std::thread::sleep(Duration::from_millis(50));
            app.state::<crate::launcher_layout::HiddenAt>().mark();
        };
        assert_eq!(unloads_after(rehidden), 0, "a later hide owns the clock");
    }
}
