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

/// One stored exclusion rule, plus whether the path it names is still on
/// disk.
///
/// **`exists_on_disk` promises "this path is still there", not "this path is
/// a directory".** A prefix naming a FILE excludes that file — there is no
/// is-a-directory check at write time (see [`exclude_subfolder`]) — so
/// gating this field on `is_dir()` would label a working rule stale and a
/// stale-rule control would offer to remove it (review round 1, Minor 1: the
/// brief's original `is_dir()` mandate was right about subtrees and wrong
/// about the effect — a file prefix excludes the named file just fine).
///
/// **`exists_on_disk` is answered here, by the backend, and this is not a
/// convenience (task-2 brief, review round 2, P1).** The window cannot
/// answer it by comparing this list against a one-level folder listing
/// (`list_tree`'s subfolders): a stored prefix may name a NESTED folder —
/// `validate_prefix` accepts a `/`-joined sequence of one or more components
/// (`mnema-walk/src/rules.rs:336-360`) — while a one-level listing only ever
/// answers for the folders directly under the root. `Work/private` would
/// find no match among `["Work", "Photos"]` there, read as a rule whose
/// folder is gone, and be offered for removal — un-excluding a folder that
/// is still full and, under D29, still headed to a third-party provider on
/// the next scan. One filesystem lookup per stored prefix, on the side that
/// has the filesystem, is the whole fix — see [`prefix_exists_on_disk`] for
/// why that lookup is not a bare `symlink_metadata` on the joined path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredExclusion {
    pub prefix: String,
    pub exists_on_disk: bool,
}

/// Resolves `prefix` against real directory entries under `root`, one
/// component at a time, comparing names with byte equality — not by handing
/// the joined path to the filesystem's own lookup, which is case-INSENSITIVE
/// on APFS (macOS, the default) and on Windows, while `ignore`'s override
/// matcher is case-sensitive (`WalkRules::builder` never calls
/// `case_insensitive`, `mnema-walk/src/rules.rs:224-300`). A stored prefix
/// `private` against a folder actually spelled `Private` would otherwise
/// report `existsOnDisk: true` while excluding nothing — a dead rule reading
/// as live (review round 1, Important 2, measured: `WalkRules::new(true,
/// true, vec!["private"])` over a fixture holding `Private/secret.txt`
/// leaves `Private/secret.txt` in the walk's own `found`, i.e. the rule
/// excludes nothing, while the naive `symlink_metadata` check answered
/// `true`).
///
/// `symlink_metadata`, not `metadata`, at the last step: a symlink is not
/// followed to decide existence, matching `WalkRules::builder`'s own
/// `follow_links(false)`. Existence alone, not `is_dir()` — see
/// [`StoredExclusion`]'s own doc comment for why the directory check was
/// dropped.
fn prefix_exists_on_disk(root: &std::path::Path, prefix: &str) -> bool {
    let mut current = root.to_path_buf();
    for component in prefix.split('/') {
        let found = std::fs::read_dir(&current).ok().and_then(|entries| {
            entries
                .flatten()
                .find(|entry| entry.file_name() == std::ffi::OsStr::new(component))
        });
        match found {
            Some(entry) => current = entry.path(),
            None => return false,
        }
    }
    std::fs::symlink_metadata(&current).is_ok()
}

/// Off the main thread for the reason given on [`open_index`].
///
/// **The root itself is stat'd once, before the loop, and an unreachable
/// root refuses the whole call rather than answering per prefix (review
/// round 1, Important 1).** An external drive unmounted, a network volume
/// down, or the folder moved all make `prefix_exists_on_disk` fail for
/// every candidate — the same way `std::fs::read_dir` fails for a root that
/// is not there. Left unguarded, that reads as "every rule's folder is
/// gone", and a stale-rule control would offer to remove all of a root's
/// exclusions in one screen; accepting that offer sends every previously
/// protected folder's contents to the provider on the next scan under D29.
/// Refusing is the conservative direction — the product already renders a
/// job refusal this way (`EndReason::RootUnavailable`, `job.rs:79-87`), so
/// this is the same shape applied one command earlier.
#[tauri::command(async)]
pub fn list_exclusions(
    state: State<'_, AppState>,
    root_id: i64,
) -> Result<Vec<StoredExclusion>, Error> {
    let root = state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    let root_path = std::path::Path::new(&root);
    if std::fs::symlink_metadata(root_path).is_err() {
        return Err(Error::RootUnavailable(root_id));
    }
    let prefixes = state.with_index(|db| db.list_path_exclusions(root_id))?;
    Ok(prefixes
        .into_iter()
        .map(|prefix| {
            let exists_on_disk = prefix_exists_on_disk(root_path, &prefix);
            StoredExclusion {
                prefix,
                exists_on_disk,
            }
        })
        .collect())
}

/// Off the main thread for the reason given on [`open_index`].
///
/// **The rule this command exists to enforce: a prefix is validated by
/// `WalkRules::new` before it is stored**, and a refusal reaches the person
/// as `RulesError`'s own sentence. Storing a prefix the walk will later
/// refuse is the failure mode the whole validator was written against
/// (`rules.rs:28-49`): the rule would silently match nothing, and under D29
/// the folder it named would keep going to the provider.
///
/// **The candidate alone, not the stored set plus the candidate.**
/// `WalkRules::new` does not build an aggregate pattern set at all — it
/// validates one prefix at a time, each in its own throwaway builder
/// (`rules.rs:200-205,380-386`) — so probing the whole set here would answer
/// `Ok` for combinations that, measured directly against this repository's
/// pinned `ignore`/`globset`, do not actually compile as one pattern set
/// (task-2 brief). The aggregate case is caught at walk time instead:
/// `walk_root` turns a failed combined compile into
/// `StopReason::RulesNotApplied` before phase 2 ever runs, and nothing is
/// sent to a provider on that path.
///
/// The empty string is refused before `WalkRules::new` even runs:
/// `validate_prefix` answers `Ok(None)` for it, which is not an error, and a
/// command that treated "no error" as "store it" would write a rule that
/// excludes nothing (review round 1, P2).
#[tauri::command(async)]
pub fn exclude_subfolder(
    state: State<'_, AppState>,
    root_id: i64,
    relative_path: String,
) -> Result<(), Error> {
    state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    if relative_path.is_empty() {
        return Err(Error::BlankExclusionRule);
    }
    mnema_walk::WalkRules::new(true, true, vec![relative_path.clone()])?;
    state.with_index(|db| db.add_path_exclusion(root_id, &relative_path))?;
    Ok(())
}

/// Off the main thread for the reason given on [`open_index`].
///
/// `Db::remove_path_exclusion` already answers whether a row went; this is a
/// thin pass-through, not a rewrap into `()` — Task 5's stale-rule control
/// needs "removed" told apart from "there was nothing there".
#[tauri::command(async)]
pub fn include_subfolder(
    state: State<'_, AppState>,
    root_id: i64,
    relative_path: String,
) -> Result<bool, Error> {
    state.with_index(|db| db.remove_path_exclusion(root_id, &relative_path))
}

/// The window needs a citation, not a chunk id. `mnema-index` already
/// re-exports `Citation` and it is `Serialize` (its derive in `write.rs`), so this
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
    /// The occurrence identity a re-index can invalidate but `chunk_id` alone
    /// cannot reveal — carried so a later `source_around` call can be pinned
    /// against it (owner-Codex P1 on PR #22; `mnema_index::Citation`'s own
    /// doc comment explains each field).
    pub document_id: String,
    pub ord: i64,
    pub root_id: Option<i64>,
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
/// the window's mirror (`ui/src/lib/ipc.ts`, `ContentArmReport`) is a union
/// with one member per `kind`, not one member with a nested field.
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
        reachable: i64,
        inspected: i64,
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
                reachable,
                inspected,
            } => Self::Answered {
                matched: chunks.len(),
                embedded,
                total,
                reachable,
                inspected,
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
/// failing inside the closure become `ContentArmReport::Failed` instead
/// of escaping `with_index`'s own `?` — e.g. `NoSuchSpace` from a
/// dangling `meta.active_space` (`models.rs:243-258`). That outer `?`
/// covers `IndexNotOpen` (unreachable once `open_index` has succeeded —
/// `state.rs:108` is the only write to `db`) and `StatePoisoned`
/// (reachable: a poison between `search`'s own arm-toggle read and the
/// credential read at `bridge.rs:284` still lands here — measured inert
/// either way, since every command answers a poisoned state alike).
/// Pinned by `an_index_failure_inside_the_content_arm_stays_local_to_it`.
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

/// A retrieved [`Hit`] as a prompt [`Passage`]: the source text verbatim, and a
/// meta line that is `relative_path` and the rendered locator joined by ` · `,
/// each dropped when empty (spec §7.1). The join-non-empty is what keeps a
/// document with no coordinate (`Coordinate::None`) from trailing a bare " · ",
/// and a document with no path (`write.rs:76-80`) from leading one. The locator
/// is the *same* `Coordinate::render` the citation UI shows, so the model reads
/// what the person will.
///
/// The one caller is [`ask`], which maps `hits.iter().map(passage_from_hit)`.
fn passage_from_hit(hit: &Hit) -> mnema_rag::Passage {
    let locator = hit.coordinate.render();
    let meta = [hit.relative_path.as_deref().unwrap_or(""), locator.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    mnema_rag::Passage {
        text: hit.text.clone(),
        meta,
    }
}

/// Whether the chat step may run, read from `meta` and the credential store the
/// way [`ContentArmReport`] is (spec §6). Not `Serialize` and not `Debug`: the
/// window never sees this — it sees [`AskAnswer`] — and `Ready` carries the key
/// for the immediate `complete` call, which must not reach a log line.
///
/// [`ask`] is the caller that reads it: its `let ChatReadiness::Ready { model,
/// key }` destructures the value and every other variant opens the
/// citations-only branch.
enum ChatReadiness {
    /// `META_CHAT_MODEL` absent, empty, or whitespace only.
    NoModel,
    /// A model is set but no key has been entered (v1 OpenRouter needs one).
    NoKey,
    /// A model is set but the credential store could not be read.
    KeyUnreadable,
    /// A model and a key: the only state that opens the generation branch.
    Ready { model: String, key: String },
}

/// The gate. `?` still stops the whole command on a poisoned or unopened index
/// (as every command does); `NoKey`/`KeyUnreadable` become states, not errors,
/// so a missing key answers with citations rather than failing.
fn chat_readiness(state: &State<'_, AppState>) -> Result<ChatReadiness, Error> {
    let model = state.with_index(|db| db.meta_get(mnema_index::META_CHAT_MODEL))?;
    let Some(model) = model.filter(|m| !m.trim().is_empty()) else {
        return Ok(ChatReadiness::NoModel);
    };
    match crate::models::key(state) {
        Ok(key) => Ok(ChatReadiness::Ready { model, key }),
        Err(Error::NoKey) => Ok(ChatReadiness::NoKey),
        Err(Error::Secrets(_)) => Ok(ChatReadiness::KeyUnreadable),
        Err(e) => Err(e),
    }
}

/// The two arm rows, read the one way `search` and `ask` must agree on.
/// `meta`'s own rule (`arm_is_on`, D106) decides on/off.
fn read_arms(state: &State<'_, AppState>) -> Result<Arms, Error> {
    state.with_index(|db| {
        Ok(Arms {
            text: arm_is_on(db.meta_get(mnema_index::META_SEARCH_TEXT_ARM)?),
            content: arm_is_on(db.meta_get(mnema_index::META_SEARCH_CONTENT_ARM)?),
        })
    })
}

/// Both arms, resolved and fused, in place of the lexical arm alone D29 left
/// `search` with — now the *one* copy `search` and `ask` share, so the two can
/// never drift (spec §5). Returns the hits plus each arm's report, because the
/// `content_override` merge (the most drift-prone half) belongs here, not
/// duplicated at each caller.
///
/// The content arm's network embed runs in [`resolve_content_query`] *before*
/// the read snapshot opens (I5, spec §5): `retrieve` takes `&State`, not an
/// already-open `&Db`, precisely so it can hold the "embed → snapshot" order
/// itself. Pinned by `the_content_arm_embeds_the_query_before_it_locks_the_index`.
///
/// The key is asked for only when `arms.content` is on and only
/// [`Error::NoKey`] then turns into no provider, so a text-only search touches
/// no credential store and a broken one costs only the content arm — pinned by
/// `a_text_only_search_does_not_touch_a_credential_store_it_does_not_need` and
/// `a_broken_credential_store_does_not_take_the_text_arm_down_with_it`.
fn retrieve(
    state: &State<'_, AppState>,
    query: &str,
    arms: Arms,
    limit: i64,
) -> Result<(Vec<Hit>, TextArmReport, ContentArmReport), Error> {
    let (provider, content_failure) = if arms.content {
        match crate::models::key(state) {
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
        resolve_content_query(state, &provider, query, arms.content, content_failure)?;

    state.with_index(|db| {
        db.read_snapshot(|db| {
            let found = mnema_search::search(
                db,
                content_query,
                query,
                arms.text,
                SEARCH_QUERY_RULE,
                SEARCH_FUSION_RULE,
                limit,
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
                        document_id: c.document_id,
                        ord: c.ord,
                        root_id: c.root_id,
                    });
                }
            }

            let content = content_override.unwrap_or_else(|| found.content.into());
            Ok((hits, found.text.into(), content))
        })
    })
}

/// Off the main thread for the reason given on [`open_index`].
///
/// The thin caller over [`read_arms`] and [`retrieve`]: the whole retrieval
/// chain — the arms, the content arm's embed before the snapshot (I5), fusion,
/// and citation resolution inside one [`mnema_index::Db::read_snapshot`] — now
/// lives in [`retrieve`], so `search` and a later `ask` share one copy and
/// cannot drift (spec §5). All this command still owns is the shape it answers
/// the window with.
#[tauri::command(async)]
pub fn search(state: State<'_, AppState>, query: String) -> Result<SearchAnswer, Error> {
    if query.trim().is_empty() {
        return Err(Error::QueryBlank);
    }
    let arms = read_arms(&state)?;
    let (hits, text, content) = retrieve(&state, &query, arms, SEARCH_LIMIT)?;
    Ok(SearchAnswer {
        hits,
        text,
        content,
    })
}

/// A citation in a generated answer: the existing [`Hit`], plus which anchor
/// resolved to it. Not the server's `Citation` (no `bbox`/`snippet`/verify
/// fields — spec §6, D124): the desktop set.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskCitation {
    pub anchor: usize,
    pub chunk_id: i64,
    pub text: String,
    pub relative_path: Option<String>,
    pub section_title: Option<String>,
    pub coordinate: Coordinate,
    /// See [`Hit::document_id`] — the same identity, echoed from the [`Hit`]
    /// this citation was resolved from.
    pub document_id: String,
    pub ord: i64,
    pub root_id: Option<i64>,
}

/// Why generation did not produce an answer (spec §6). Ports the server's two
/// guards (`service.py:66-68,80-82`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RefusalKind {
    /// `Ready` but retrieval found nothing — chat is not called at all.
    NoCandidates,
    /// `Ready`, chat was called, and the model answered with nothing.
    EmptyCompletion,
}

/// What [`ask`] answers with. Different states are different variants, never a
/// `null` (the shape [`TextArmReport`]/[`ContentArmReport`] share). Every
/// variant carries the arm reports, because retrieval is identical across them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AskAnswer {
    Generated {
        answer: String,
        citations: Vec<AskCitation>,
        text: TextArmReport,
        content: ContentArmReport,
    },
    CitationsOnly {
        citations: Vec<Hit>,
        text: TextArmReport,
        content: ContentArmReport,
    },
    Refused {
        /// Renamed on the wire because [`RefusalKind`] is itself
        /// `#[serde(tag = "kind")]`: the field's own `kind` would collide with
        /// this enum's tag and one would silently overwrite the other. The
        /// payload is `{"kind":"refused","reason":{"kind":"noCandidates"}}`
        /// (ruling R4).
        #[serde(rename = "reason")]
        kind: RefusalKind,
        text: TextArmReport,
        content: ContentArmReport,
    },
}

/// How many passages `ask` puts in the prompt (port `app/api/ask.py:18`,
/// `top_k = Field(8)`), passed to [`retrieve`] as its `limit`.
const ASK_TOP_K: i64 = 8;

/// The longest query `ask` accepts (port `app/api/ask.py:17`,
/// `Field(max_length=2048)`). Characters, not bytes — Python `str` length
/// counts code points, and so does `query.chars().count()` below.
const MAX_ASK_QUERY: usize = 2048;

/// Answer a question over the index with cited sources (spec §4). Retrieval is
/// the shared [`retrieve`]; generation runs iff [`ChatReadiness::Ready`] (the
/// private gate, spec §7.2). Off the main thread for the reason [`search`]
/// gives.
///
/// The query guards run first, before `read_arms` or `retrieve`, so a blank
/// or over-long query is rejected before any index or network work (both
/// halves of `ask.py:17`'s `min_length=1, max_length=2048`; the blank guard
/// keeps a meaningless question from reaching the billable query embed —
/// the D115 mechanism through this caller — and the length guard resolves
/// spec §12). Then the four branches, in order: any non-`Ready` readiness
/// answers with the citations retrieval already found
/// ([`AskAnswer::CitationsOnly`]) and never reaches the chat model — the
/// gate. `Ready` with no hits refuses before calling chat (`NoCandidates`,
/// `service.py:66-68`); a `Ready` call the model answers blankly refuses
/// after (`EmptyCompletion`, `service.py:80-82`); otherwise the anchors the
/// model wrote become citations.
#[tauri::command(async)]
pub fn ask(state: State<'_, AppState>, query: String) -> Result<AskAnswer, Error> {
    if query.trim().is_empty() {
        return Err(Error::QueryBlank);
    }
    let chars = query.chars().count();
    if chars > MAX_ASK_QUERY {
        return Err(Error::QueryTooLong {
            chars,
            limit: MAX_ASK_QUERY,
        });
    }

    let arms = read_arms(&state)?;
    let (hits, text, content) = retrieve(&state, &query, arms, ASK_TOP_K)?;

    let ChatReadiness::Ready { model, key } = chat_readiness(&state)? else {
        return Ok(AskAnswer::CitationsOnly {
            citations: hits,
            text,
            content,
        });
    };

    if hits.is_empty() {
        return Ok(AskAnswer::Refused {
            kind: RefusalKind::NoCandidates,
            text,
            content,
        });
    }

    let passages: Vec<mnema_rag::Passage> = hits.iter().map(passage_from_hit).collect();
    let base = state.provider_base().to_string();
    match mnema_rag::answer(&base, &key, &model, &query, &passages, None)? {
        None => Ok(AskAnswer::Refused {
            kind: RefusalKind::EmptyCompletion,
            text,
            content,
        }),
        Some(a) => {
            let citations = a
                .cited
                .iter()
                .map(|&n| {
                    // resolve_anchors guarantees 1 <= n <= passages.len() ==
                    // hits.len(), so hits[n - 1] is always in range (spec §6).
                    let h = &hits[n - 1];
                    AskCitation {
                        anchor: n,
                        chunk_id: h.chunk_id,
                        text: h.text.clone(),
                        relative_path: h.relative_path.clone(),
                        section_title: h.section_title.clone(),
                        coordinate: h.coordinate.clone(),
                        document_id: h.document_id.clone(),
                        ord: h.ord,
                        root_id: h.root_id,
                    }
                })
                .collect();
            Ok(AskAnswer::Generated {
                answer: a.text,
                citations,
                text,
                content,
            })
        }
    }
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
    /// `ui/src/lib/ipc.ts` spells these strings out by hand in
    /// `TextArmReport` / `ContentArmReport`, and a rename here leaves that
    /// union describing a `kind` the backend no longer sends.
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
                reachable: 0,
                inspected: 0,
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

    /// `matched`, `embedded`, `total`, `reachable` and `inspected` carry the
    /// arm's own numbers, not a placeholder: replacing `chunks.len()` with
    /// `0`, or dropping any of the new fields, in the `From` impl above must
    /// fail this, without needing a live provider to reach the content
    /// arm's `Answered` case at all. `inspected: 5` is distinct from every
    /// other field here on purpose, so a `From` that copied `reachable` (or
    /// zeroed `inspected`) fails this specific field, not merely the shape.
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
                reachable: 9,
                inspected: 5,
            }),
            ContentArmReport::Answered {
                matched: 2,
                embedded: 7,
                total: 12,
                reachable: 9,
                inspected: 5,
            }
        ));
    }

    /// The test moved from PR 3 (D121): `Coordinate::None` must not leave a
    /// dangling `" · "`, and a missing path must not lead one — join-non-empty
    /// is what `passage_from_hit` exists to guarantee.
    #[test]
    fn a_passage_joins_the_path_and_the_locator_and_never_dangles_the_separator() {
        use mnema_core::Coordinate;

        let both = Hit {
            chunk_id: 1,
            text: "body".into(),
            relative_path: Some("notes/a.txt".into()),
            section_title: None,
            coordinate: Coordinate::Page { number: 3 },
            document_id: "doc-1".into(),
            ord: 0,
            root_id: Some(1),
        };
        assert_eq!(passage_from_hit(&both).meta, "notes/a.txt · с. 3");

        // A document with no verifiable coordinate: the path alone, no trailing
        // " · " (the dangling-separator bug join-non-empty exists to prevent).
        let no_coord = Hit {
            coordinate: Coordinate::None,
            ..both.clone()
        };
        assert_eq!(passage_from_hit(&no_coord).meta, "notes/a.txt");

        // No path (write.rs:76-80), a real coordinate: the locator alone.
        let no_path = Hit {
            relative_path: None,
            ..both.clone()
        };
        assert_eq!(passage_from_hit(&no_path).meta, "с. 3");

        // Neither: an empty meta, which build_messages renders as a bare [N].
        let neither = Hit {
            relative_path: None,
            coordinate: Coordinate::None,
            ..both.clone()
        };
        assert_eq!(passage_from_hit(&neither).meta, "");

        // text is carried verbatim.
        assert_eq!(passage_from_hit(&both).text, "body");
    }

    #[test]
    fn text_arm_report_camel_case_spellings_are_pinned() {
        use serde_json::json;
        assert_eq!(
            serde_json::to_value(TextArmReport::Off).unwrap(),
            json!({ "kind": "off" })
        );
        assert_eq!(
            serde_json::to_value(TextArmReport::Answered { matched: 3 }).unwrap(),
            json!({ "kind": "answered", "matched": 3 })
        );
    }

    #[test]
    fn refusal_kind_wire_spellings_are_pinned() {
        use serde_json::json;
        assert_eq!(
            serde_json::to_value(RefusalKind::NoCandidates).unwrap(),
            json!({ "kind": "noCandidates" })
        );
        assert_eq!(
            serde_json::to_value(RefusalKind::EmptyCompletion).unwrap(),
            json!({ "kind": "emptyCompletion" })
        );
    }

    #[test]
    fn ask_answer_tags_and_refused_nesting_are_pinned() {
        use serde_json::json;
        let generated = AskAnswer::Generated {
            answer: "a".into(),
            citations: vec![],
            text: TextArmReport::Off,
            content: ContentArmReport::Off,
        };
        let gv = serde_json::to_value(&generated).unwrap();
        assert_eq!(gv["kind"], json!("generated"));
        assert_eq!(gv["answer"], json!("a")); // field name pinned — the TS side mirrors `answer`
        assert_eq!(gv["citations"], json!([])); // field name pinned ([] ≠ Null, so a rename/drop fails)

        let citations_only = AskAnswer::CitationsOnly {
            citations: vec![],
            text: TextArmReport::Off,
            content: ContentArmReport::Off,
        };
        let cv = serde_json::to_value(&citations_only).unwrap();
        assert_eq!(cv["kind"], json!("citationsOnly"));
        assert_eq!(cv["citations"], json!([])); // field name pinned

        let refused = AskAnswer::Refused {
            kind: RefusalKind::NoCandidates,
            text: TextArmReport::Off,
            content: ContentArmReport::Off,
        };
        let v = serde_json::to_value(&refused).unwrap();
        // The reason is nested under `reason`, not `kind` (ruling R4, bridge.rs:466-476).
        assert_eq!(v["kind"], json!("refused"));
        assert_eq!(v["reason"]["kind"], json!("noCandidates"));
    }

    #[test]
    fn ask_citation_field_names_are_pinned() {
        use serde_json::json;
        let c = AskCitation {
            anchor: 1,
            chunk_id: 42,
            text: "t".into(),
            relative_path: Some("a/b.md".into()),
            section_title: Some("S".into()),
            coordinate: Coordinate::None,
            document_id: "doc-1".into(),
            ord: 3,
            root_id: Some(7),
        };
        // Full-object compare, not per-field — same reason as
        // `hit_field_names_are_pinned` below: indexing a `serde_json::Value`
        // by key only ever looks up keys the test already names, so a field
        // this struct grows later (or one renamed to the snake_case spelling)
        // would pass every per-field assert here silently.
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            json!({
                "anchor": 1,
                "chunkId": 42,
                "text": "t",
                "relativePath": "a/b.md",
                "sectionTitle": "S",
                "coordinate": { "kind": "none" },
                "documentId": "doc-1",
                "ord": 3,
                "rootId": 7
            })
        );
    }

    #[test]
    fn hit_field_names_are_pinned() {
        use serde_json::json;
        let h = Hit {
            chunk_id: 7,
            text: "t".into(),
            relative_path: None,
            section_title: None,
            coordinate: Coordinate::Page { number: 2 },
            document_id: "doc-9".into(),
            ord: 4,
            root_id: None,
        };
        // Full-object compare, not per-field: `serde_json::Value` indexing returns
        // Null for an ABSENT key too, so `v["relativePath"] == null` would still
        // pass if the key were dropped (skip_serializing_if) or renamed — the
        // satisfied-by-zero hole the "assert both directions" rule exists to stop.
        // Comparing the whole object distinguishes present-null from absent and
        // matches the three sibling pin tests. `rootId: null` here is the other
        // half of the AskCitation test's `Some(7)`: together they pin both arms
        // of the Option.
        assert_eq!(
            serde_json::to_value(&h).unwrap(),
            json!({
                "chunkId": 7,
                "text": "t",
                "relativePath": null,
                "sectionTitle": null,
                "coordinate": { "kind": "page", "number": 2 },
                "documentId": "doc-9",
                "ord": 4,
                "rootId": null
            })
        );
    }
}
