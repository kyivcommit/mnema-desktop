//! The commands the webview may call.
//!
//! Every one of them translates and delegates. A command that computes is a
//! defect: the core crates are where behaviour lives, and behaviour that lives
//! here can only be reached through a window.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

use mnema_core::Coordinate;
use mnema_index::QueryRule;
use mnema_search::{Arms, ContentArm, FusionRule, Missing, Provider, TextArm};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::error::Error;
use crate::job::{self, JobEvent};
use crate::state::AppState;

/// How many lexical hits a search returns.
///
/// A placeholder with a number on it. What a search should return, and how the
/// lexical and dense arms are fused into it, is the search/RAG spec's decision;
/// this is here so the walking skeleton has an end.
const SEARCH_LIMIT: i64 = 20;

/// The query rule `search` asks the text arm with, until a live sweep over a
/// measured gold set replaces this placeholder with the winner.
const SEARCH_QUERY_RULE: QueryRule = QueryRule::AllTerms;

/// The fusion rule `search` combines both arms with. Same placeholder status
/// as [`SEARCH_QUERY_RULE`] above.
const SEARCH_FUSION_RULE: FusionRule = FusionRule::Rrf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub path: String,
    pub schema_version: i64,
}

/// `(async)` on a synchronous function, and both halves of that are deliberate.
///
/// Without it a command is `ExecutionContext::Blocking` and runs inline on the
/// main thread. This one creates a directory and applies migrations; the next
/// one takes the index mutex, which a running indexing job also wants. Five
/// seconds of that on the main thread is precisely the frozen window that
/// `BUSY_TIMEOUT` is set short to avoid — the timeout would be doing nothing but
/// choosing how long the application appears dead.
///
/// The function stays synchronous rather than becoming `async fn` because there
/// is then no await for a `std` mutex guard to be held across. Tauri wraps the
/// body in a task either way (`respond_async_serialized`), so it leaves the main
/// thread all the same.
///
/// What this does NOT fix: the body still occupies a worker of a pool sized to
/// the core count. Giving the indexing job its own connection, so a search never
/// queues behind the writer at all, is the indexing spec's decision.
#[tauri::command(async)]
pub fn open_index(state: State<'_, AppState>) -> Result<IndexInfo, Error> {
    let (path, schema_version) = state.open_index()?;
    Ok(IndexInfo {
        path: path.display().to_string(),
        schema_version,
    })
}

/// Off the main thread for the reason given on [`open_index`].
#[tauri::command(async)]
pub fn add_watched_folder(state: State<'_, AppState>, path: String) -> Result<i64, Error> {
    state.with_index(|db| db.insert_watched_root(&path))
}

/// Off the main thread for the reason given on [`open_index`].
///
/// `Db::delete_watched_root` already closes §7.1.1's cascade gap — a
/// document whose last path went with its root goes too, vectors included —
/// but nothing before this command could reach it from outside a Rust test.
/// `removing_a_watched_folder_takes_its_documents_with_it`
/// (`tests/commands.rs`) is the first thing that exercises the fix through
/// the seam it was written for: add a folder, walk it, remove it, and check
/// that `search` no longer answers for it.
#[tauri::command(async)]
pub fn remove_watched_folder(state: State<'_, AppState>, root_id: i64) -> Result<u64, Error> {
    state.with_index(|db| db.delete_watched_root(root_id))
}

/// The window needs a citation, not a chunk id. `mnema-index` already
/// re-exports `Citation` and it is `Serialize` (`write.rs:11`), so this
/// crosses the seam without touching the dependency graph — the seam was
/// simply never crossed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub chunk_id: i64,
    pub text: String,
    pub relative_path: Option<String>,
    pub section_title: Option<String>,
    pub coordinate: Coordinate,
}

/// The text arm's outcome, without the chunk ids `hits` already carries.
///
/// Not [`mnema_search::TextArm`] reused: this needs `Serialize` and a
/// camelCase `kind`, and a count keeps this and `hits` from being two lists
/// that could disagree about what the arm found.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TextArmReport {
    Off,
    Answered { matched: usize },
}

impl From<TextArm> for TextArmReport {
    fn from(arm: TextArm) -> Self {
        match arm {
            TextArm::Off => Self::Off,
            TextArm::Answered { chunks } => Self::Answered {
                matched: chunks.len(),
            },
        }
    }
}

/// The content arm's outcome, in the same shape as [`TextArmReport`].
///
/// [`Missing`]'s two values become their own variants rather than staying
/// nested under one `NotConfigured`: each is fixed in a different place, and
/// `render.js` names a sentence per `kind`, not per nested field.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ContentArmReport {
    Off,
    NoKey,
    NoModel,
    Failed {
        reason: String,
    },
    Answered {
        matched: usize,
        embedded: i64,
        total: i64,
    },
}

impl From<ContentArm> for ContentArmReport {
    fn from(arm: ContentArm) -> Self {
        match arm {
            ContentArm::Off => Self::Off,
            ContentArm::NotConfigured(Missing::NoKey) => Self::NoKey,
            ContentArm::NotConfigured(Missing::NoModel) => Self::NoModel,
            ContentArm::Failed { reason } => Self::Failed { reason },
            ContentArm::Answered {
                chunks,
                embedded,
                total,
            } => Self::Answered {
                matched: chunks.len(),
                embedded,
                total,
            },
        }
    }
}

/// What a search answers with: the citations to draw, and what each arm did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAnswer {
    pub hits: Vec<Hit>,
    pub text: TextArmReport,
    pub content: ContentArmReport,
}

/// `meta`'s own rule (D106): absent, or anything but the literal `"off"`,
/// leaves the arm on.
fn arm_is_on(value: Option<String>) -> bool {
    value.as_deref() != Some("off")
}

/// Off the main thread for the reason given on [`open_index`].
///
/// Both arms, fused by `mnema_search::search`, in place of the lexical arm
/// alone D29 left this command with. The arms come from `meta`, never a
/// parameter — the window already saved a choice through
/// [`set_search_arms`].
///
/// Only [`Error::NoKey`] turns into no provider; every other failure of
/// [`crate::models::key`] stops the search, like every other command that
/// needs the credential store.
#[tauri::command(async)]
pub fn search(state: State<'_, AppState>, query: String) -> Result<SearchAnswer, Error> {
    let provider = match crate::models::key(&state) {
        Ok(key) => Some(Provider {
            base: state.provider_base().to_string(),
            key,
        }),
        Err(Error::NoKey) => None,
        Err(e) => return Err(e),
    };

    state.with_index(|db| {
        let arms = Arms {
            text: arm_is_on(db.meta_get(mnema_index::META_SEARCH_TEXT_ARM)?),
            content: arm_is_on(db.meta_get(mnema_index::META_SEARCH_CONTENT_ARM)?),
        };
        let found = mnema_search::search(
            db,
            provider,
            &query,
            arms,
            SEARCH_QUERY_RULE,
            SEARCH_FUSION_RULE,
            SEARCH_LIMIT,
        )?;

        // A chunk that vanished between the fuse and this read is not an
        // error: a walk running alongside a search is the ordinary case that
        // motivated the job holding its own connection at all (see
        // `AppState::open_job_index`). Only `citation`'s `None` is read this
        // way — the `?` right before it still stops the whole search on any
        // other failure.
        let mut hits = Vec::new();
        for chunk_id in found.chunks {
            if let Some(c) = db.citation(chunk_id)? {
                hits.push(Hit {
                    chunk_id,
                    text: c.text,
                    relative_path: c.relative_path,
                    section_title: c.section_title,
                    coordinate: c.coordinate,
                });
            }
        }

        Ok(SearchAnswer {
            hits,
            text: found.text.into(),
            content: found.content.into(),
        })
    })
}

/// The one way the window changes a toggle. `search` reads the same two rows,
/// so the arm a person ticked and the arm that ran cannot disagree.
#[tauri::command(async)]
pub fn set_search_arms(state: State<'_, AppState>, text: bool, content: bool) -> Result<(), Error> {
    state.with_index(|db| {
        db.meta_set(
            mnema_index::META_SEARCH_TEXT_ARM,
            if text { "on" } else { "off" },
        )?;
        db.meta_set(
            mnema_index::META_SEARCH_CONTENT_ARM,
            if content { "on" } else { "off" },
        )
    })
}

/// Off the main thread for the reason given on [`open_index`].
#[tauri::command(async)]
pub fn skips(
    state: State<'_, AppState>,
    root_id: i64,
) -> Result<Vec<mnema_index::SkippedFile>, Error> {
    state.with_index(|db| db.skips_for_root(root_id))
}

/// Demonstrates the progress path end to end without doing real work.
///
/// A channel, not an event: Tauri's own documentation says events are unsuited
/// to throughput and may arrive out of order, and progress that jumps backwards
/// reads as a broken application. Ordering within a channel is Tauri's own
/// guarantee — each send is stamped with an index and the JavaScript side
/// buffers anything that arrives early.
///
/// Left blocking, unlike the two above: claiming the slot is one compare-exchange
/// and spawning a thread is not work. Nothing here touches the database.
#[tauri::command]
pub fn start_probe_job(
    state: State<'_, AppState>,
    on_progress: Channel<JobEvent>,
) -> Result<(), Error> {
    let slot = state.claim_job()?;

    // A dedicated OS thread, not a task on the async pool: that pool is sized to
    // the core count and also serves every other command, and a real indexing
    // job runs for hours. It is one thread and stays one — PDF extraction is
    // serialised within the process (D35), so widening this would buy nothing
    // and would contend for the single writer as well.
    std::thread::spawn(move || {
        // The last count the window was actually shown. Read after the catch, so
        // a job that dies mid-way can still say where it got to.
        let reported = AtomicU64::new(0);

        // `catch_unwind`, not line order. The slot is freed by `JobSlot::drop`
        // however this thread ends, but the ending message used to sit after the
        // call and an unwind stepped straight over it: the page never heard that
        // the job was over, `setRunning(false)` never ran, and Start stayed
        // disabled for the life of the window — a state the user cannot leave,
        // because a reloaded page has nobody to ask.
        //
        // The probe cannot panic. Indexing, which inherits this shape, calls
        // pdfium through FFI.
        //
        // AssertUnwindSafe: everything touched after the catch is an atomic
        // counter and a channel send, neither of which an unwind can leave
        // half-updated. That is the property `UnwindSafe` checks and cannot infer
        // through the `dyn Fn` inside a `Channel`.
        let caught = catch_unwind(AssertUnwindSafe(|| {
            job::run_probe(
                job::PROBE_UNITS,
                job::PROBE_UNIT,
                job::REPORT_INTERVAL,
                slot.cancel_flag(),
                |progress| {
                    let done = progress.done;
                    // A failed send means the webview is gone — reloaded, or
                    // closed while the job runs. The job deliberately continues:
                    // the work is the point, the drawing of it is not.
                    //
                    // Recorded only after the send returns, and only if it
                    // succeeded. `Ended::failed` promises the last count the
                    // window was *shown*; storing before the send would record a
                    // number the window never received, and a panic during the
                    // send — the likeliest place for one, since that is where
                    // this thread calls out — would report exactly that number.
                    if on_progress.send(JobEvent::Progress(progress)).is_ok() {
                        reported.store(done, Ordering::Relaxed);
                    }
                },
            )
        }));

        let ending = match caught {
            Ok(outcome) => job::Ended::of(outcome, job::PROBE_UNITS),
            // The probe cannot panic (see the doc comment on `caught` above),
            // but the type still has to account for the possibility —
            // `job::panic_message` reads whatever text the payload carries
            // the same way `walk_job.rs`'s own panic arm does, rather than
            // leaving this one `Ended` variant without one.
            Err(panic) => job::Ended::failed(
                reported.load(Ordering::Relaxed),
                job::PROBE_UNITS,
                job::panic_message(&*panic),
            ),
        };
        let _ = on_progress.send(JobEvent::Ended(ending));
        // `slot` is dropped here and the job slot is free again, whether the loop
        // finished, was cancelled, or panicked.
    });

    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub running: bool,
}

/// What the window asks on load.
///
/// A page that reloads mid-job has no channel any more — the one the job sends
/// on belongs to the page that started it. Without this it cannot tell a running
/// job from an idle one, and would have to draw a guess: either an idle window
/// over a job that is still writing, or a Start button it will not re-enable.
///
/// Blocking, like `cancel_job`, and for the same reason: one atomic load, and it
/// must not queue behind a search.
#[tauri::command]
pub fn job_status(state: State<'_, AppState>) -> JobStatus {
    JobStatus {
        running: state.job_is_running(),
    }
}

/// Left blocking: one atomic store, and it must not queue behind a search.
/// Cancelling has to answer even when the async pool is fully occupied — losing
/// the ability to stop a job is the one failure the user cannot work around.
#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) {
    state.cancel_job();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every discriminant the window sees has its camel-case spelling pinned
    /// — the same guard `models.rs` carries, for the same reason:
    /// `render.js` matches on these strings and a rename here becomes a
    /// missing table key there.
    #[test]
    fn every_search_discriminant_the_window_sees_has_its_camel_case_spelling_pinned() {
        let spellings: Vec<String> = [
            ContentArmReport::Off,
            ContentArmReport::NoKey,
            ContentArmReport::NoModel,
            ContentArmReport::Failed {
                reason: String::new(),
            },
            ContentArmReport::Answered {
                matched: 0,
                embedded: 0,
                total: 0,
            },
        ]
        .iter()
        .map(|v| {
            serde_json::to_value(v).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(spellings, ["off", "noKey", "noModel", "failed", "answered"]);
    }
}
