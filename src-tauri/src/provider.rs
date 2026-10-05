//! Which provider answers — OpenRouter or the local Mnema process — behind ONE
//! endpoint. Every call that leaves for a model asks [`endpoint`] (through
//! `AppState::endpoint` or a job's captured [`Provider`]), so the two can never
//! be chosen differently at two sites.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
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
    sidecar: Mutex<Option<Sidecar>>,
    cancel: [AtomicBool; 2],
}

impl Local {
    pub fn new(store: Store, binary: PathBuf) -> Self {
        Self {
            store,
            binary,
            env: Vec::new(),
            sidecar: Mutex::new(None),
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
            *sidecar = Some(Sidecar::start_with_env(
                &self.binary,
                &self.store.dir(ModelId::Embed),
                &self.store.dir(ModelId::Chat),
                &env,
            )?);
        }
        Ok(sidecar.as_ref().expect("started above").endpoint()?)
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

#[tauri::command(async)]
pub fn set_provider_choice(
    state: tauri::State<'_, crate::state::AppState>,
    choice: ProviderChoice,
    existing_vectors: crate::models::ExistingVectors,
) -> Result<ProviderChoice, Error> {
    change(&state, choice, existing_vectors)
}

/// [`set_provider_choice`]'s body, reachable without a `State`.
pub fn change(
    state: &crate::state::AppState,
    choice: ProviderChoice,
    existing_vectors: crate::models::ExistingVectors,
) -> Result<ProviderChoice, Error> {
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
        let adopted = state.with_index(|db| {
            Ok(crate::models::adopt_retiring_whatever_blocks(
                db,
                LOCAL_EMBED_MODEL,
                LOCAL_EMBED_DIM,
                state.credential_ref(),
                &mnema_chunk::chunker_hash(),
                existing_vectors,
            ))
        })?;
        // `set_embedding_model`'s rule: a kept resumable report counts against
        // the active space, so any exit that may have moved it gives that up.
        match adopted {
            Ok(_) => slot.forget_restore(),
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
    Ok(state.provider_choice())
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
    let local = state.local();
    local.stop();
    local.store.remove(id.into())?;
    Ok(())
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
