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

/// How many hits `search` returns, after both arms are fused into one list.
///
/// A placeholder with a number on it — what a search should return is still
/// unsettled. How the two arms are fused is no longer this constant's story;
/// see `SEARCH_QUERY_RULE` and `SEARCH_FUSION_RULE` below.
const SEARCH_LIMIT: i64 = 20;

/// The query rule `search` asks the text arm with — the live sweep's winner,
/// tied with `AllTerms` once fused but ahead of it text-only alone. See D108.
const SEARCH_QUERY_RULE: QueryRule = QueryRule::TermsInIndex;

/// The fusion rule `search` combines both arms with — tied with every other
/// rule under the winning query rule, kept as the principled default. D109.
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
///
/// `pub(crate)`: `models::read_settings` reads the same two meta rows to
/// answer the window's own question ("what did I save?"), and reusing this
/// is what keeps that answer and `search`'s from ever disagreeing.
pub(crate) fn arm_is_on(value: Option<String>) -> bool {
    value.as_deref() != Some("off")
}

/// Resolves the content arm's input before any read snapshot opens: the
/// model comes from one short `with_index` lock, then [`embed_query`] runs
/// with no lock held at all — the network call [`search`]'s own doc says
/// must never share a scope with `read_snapshot`, and why I5 (a network
/// call inside the index mutex) closes as a side effect of this split.
///
/// `Ok((None, Some(_)))` is a terminal report `search` must not try to
/// improve on — `content_failure` already covers a broken credential
/// store; this also covers no model and a failed embed. `Ok((Some(_),
/// None))` is ready for `search` to answer with.
///
/// **Two kinds of failure, told apart.** `active_space`/`space_model`
/// failing inside the closure — e.g. `NoSuchSpace` from a dangling
/// `meta.active_space` (`models.rs:243-258`) — become
/// `ContentArmReport::Failed` rather than escaping through `with_index`'s
/// own `?`. That outer `?` is for `IndexNotOpen`/`StatePoisoned`, and is
/// unreachable from `search`: its own earlier `with_index` call (the arm
/// toggles, `bridge.rs:273`) rejects on either first. Pinned by
/// `an_index_failure_inside_the_content_arm_stays_local_to_it`.
fn resolve_content_query(
    state: &State<'_, AppState>,
    provider: &Option<Provider>,
    query: &str,
    content_on: bool,
    content_failure: Option<String>,
) -> Result<(Option<mnema_search::ContentQuery>, Option<ContentArmReport>), Error> {
    if let Some(reason) = content_failure {
        return Ok((None, Some(ContentArmReport::Failed { reason })));
    }
    if !content_on {
        return Ok((None, None));
    }
    let Some(provider) = provider else {
        return Ok((None, Some(ContentArmReport::NoKey)));
    };
    let resolved: Result<Option<(i64, String)>, mnema_index::Error> = state.with_index(|db| {
        Ok(match db.active_space() {
            Ok(Some(space_id)) => db
                .space_model(space_id)
                .map(|(model, _width)| Some((space_id, model))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        })
    })?;
    let (space_id, model) = match resolved {
        Ok(Some(pair)) => pair,
        Ok(None) => return Ok((None, Some(ContentArmReport::NoModel))),
        Err(e) => {
            return Ok((
                None,
                Some(ContentArmReport::Failed {
                    reason: e.to_string(),
                }),
            ));
        }
    };
    match mnema_search::embed_query(provider, &model, query) {
        Ok(vector) => Ok((Some(mnema_search::ContentQuery { space_id, vector }), None)),
        Err(reason) => Ok((None, Some(ContentArmReport::Failed { reason }))),
    }
}

/// Off the main thread for the reason given on [`open_index`].
///
/// Both arms, fused by `mnema_search::search`, in place of the lexical arm
/// alone D29 left this command with. The arms come from `meta`, never a
/// parameter — the window already saved a choice through
/// [`set_search_arms`].
///
/// The key is asked for only when `arms.content` is on — pinned by
/// `a_text_only_search_does_not_touch_a_credential_store_it_does_not_need`
/// — and only [`Error::NoKey`] then turns into no provider.
///
/// Everything from the lexical arm through fusion through citation
/// resolution runs inside one [`mnema_index::Db::read_snapshot`], so a
/// rebuild committing on the job's own connection mid-search cannot hand a
/// reused chunk id's citation to the wrong text. [`resolve_content_query`]
/// runs first and resolves what the content arm needs — including any
/// reason it has nothing to run on — before that snapshot ever opens, so
/// the snapshot holds no network call and [`ContentArmReport::NoKey`],
/// [`ContentArmReport::NoModel`] and a broken credential store all reach
/// the answer the same way `content_failure` always did: as an override
/// applied after the snapshot closes, in place of whatever
/// `mnema_search::search` answered on its own. Pinned by
/// `a_broken_credential_store_does_not_take_the_text_arm_down_with_it`.
#[tauri::command(async)]
pub fn search(state: State<'_, AppState>, query: String) -> Result<SearchAnswer, Error> {
    let arms = state.with_index(|db| {
        Ok(Arms {
            text: arm_is_on(db.meta_get(mnema_index::META_SEARCH_TEXT_ARM)?),
            content: arm_is_on(db.meta_get(mnema_index::META_SEARCH_CONTENT_ARM)?),
        })
    })?;

    let (provider, content_failure) = if arms.content {
        match crate::models::key(&state) {
            Ok(key) => (
                Some(Provider {
                    base: state.provider_base().to_string(),
                    key,
                }),
                None,
            ),
            Err(Error::NoKey) => (None, None),
            Err(e) => (None, Some(e.to_string())),
        }
    } else {
        (None, None)
    };

    let (content_query, content_override) =
        resolve_content_query(&state, &provider, &query, arms.content, content_failure)?;

    state.with_index(|db| {
        db.read_snapshot(|db| {
            let found = mnema_search::search(
                db,
                content_query,
                &query,
                arms.text,
                SEARCH_QUERY_RULE,
                SEARCH_FUSION_RULE,
                SEARCH_LIMIT,
            )?;

            // A chunk that vanished between the fuse and this read is not an
            // error: a walk running alongside a search is the ordinary case
            // that motivated the job holding its own connection at all (see
            // `AppState::open_job_index`). Only `citation`'s `None` is read
            // this way — the `?` right before it still stops the whole
            // search on any other failure.
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

            let content = content_override.unwrap_or_else(|| found.content.into());

            Ok(SearchAnswer {
                hits,
                text: found.text.into(),
                content,
            })
        })
    })
}

/// The one way the window changes a toggle. `search` reads the same two rows,
/// so the arm a person ticked and the arm that ran cannot disagree.
///
/// The two rows are written by [`mnema_index::Db::meta_set_many`], in one
/// transaction, so a failure between them cannot leave one arm's saved
/// choice disagreeing with the other's.
#[tauri::command(async)]
pub fn set_search_arms(state: State<'_, AppState>, text: bool, content: bool) -> Result<(), Error> {
    if !text && !content {
        return Err(Error::NoSearchArm);
    }
    state.with_index(|db| {
        db.meta_set_many(&[
            (
                mnema_index::META_SEARCH_TEXT_ARM,
                if text { "on" } else { "off" },
            ),
            (
                mnema_index::META_SEARCH_CONTENT_ARM,
                if content { "on" } else { "off" },
            ),
        ])
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

    /// `matched`, `embedded` and `total` carry the arm's own numbers, not a
    /// placeholder: replacing `chunks.len()` with `0` in either `From` impl
    /// above must fail this, without needing a live provider to reach the
    /// content arm's `Answered` case at all.
    #[test]
    fn an_answered_arm_report_carries_the_real_numbers_not_a_placeholder() {
        assert!(matches!(
            TextArmReport::from(TextArm::Answered {
                chunks: vec![10, 20, 30],
            }),
            TextArmReport::Answered { matched: 3 }
        ));
        assert!(matches!(
            ContentArmReport::from(ContentArm::Answered {
                chunks: vec![10, 20],
                embedded: 7,
                total: 12,
            }),
            ContentArmReport::Answered {
                matched: 2,
                embedded: 7,
                total: 12,
            }
        ));
    }
}
