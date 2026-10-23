//! The agent API's handlers: one function per request, answering with the
//! functions the launcher uses (`bridge::retrieve`, `tree::document_view_in`),
//! so retrieval never forks. No socket here; the wire format is
//! `mnema_core::agent_wire`.

use std::path::Path;
use std::time::{Duration, Instant};

use mnema_core::agent_wire::{ErrKind, Op, PassageId, WireError};
use serde_json::{Value, json};

use crate::bridge::{Hit, read_arms, retrieve};
use crate::error::Error;
use crate::scan_state::{Entry, ScanSnapshot};
use crate::state::AppState;
use crate::tree::{DocumentView, freshness_of};

const DEFAULT_K: u32 = 8;
const DEFAULT_CONTEXT: u32 = 2;
const RESCAN_POLL: Duration = Duration::from_millis(500);
const RESCAN_CAP: Duration = Duration::from_secs(600);

fn err(kind: ErrKind, message: impl Into<String>) -> WireError {
    WireError {
        kind,
        message: message.into(),
    }
}

fn internal(e: impl std::fmt::Display) -> WireError {
    err(ErrKind::Internal, e.to_string())
}

/// The `kind` tag `Freshness` serializes with.
fn freshness_kind(freshness: &impl serde::Serialize) -> Result<Value, WireError> {
    let v = serde_json::to_value(freshness).map_err(internal)?;
    Ok(v.get("kind").cloned().unwrap_or(Value::Null))
}

/// The root's absolute path joined with the relative one; `None` when either
/// is unknown.
fn absolute(
    roots: &[(i64, String)],
    root_id: Option<i64>,
    relative: Option<&str>,
) -> Option<String> {
    let (root_id, relative) = (root_id?, relative?);
    let (_, root) = roots.iter().find(|(id, _)| *id == root_id)?;
    Some(Path::new(root).join(relative).display().to_string())
}

fn roots(state: &AppState) -> Result<Vec<(i64, String)>, WireError> {
    state
        .with_index(|db| {
            Ok(db
                .list_watched_roots()?
                .into_iter()
                .map(|r| (r.id, r.absolute_path))
                .collect())
        })
        .map_err(internal)
}

pub fn handle(state: &AppState, op: Op) -> Result<Value, WireError> {
    match op {
        Op::Search { query, k } => search(state, &query, k.unwrap_or(DEFAULT_K)),
        Op::Passage { id, context } => passage(state, &id, context.unwrap_or(DEFAULT_CONTEXT)),
        Op::Status => status(state),
        Op::Rescan { wait } => rescan(state, wait.unwrap_or(false), RESCAN_POLL, RESCAN_CAP),
    }
}

fn search(state: &AppState, query: &str, k: u32) -> Result<Value, WireError> {
    let arms = read_arms(state).map_err(internal)?;
    let embed = state.roles().embed;
    let (hits, text, content, _) =
        retrieve(state, query, arms, i64::from(k), embed).map_err(internal)?;
    let roots = roots(state)?;
    let passages = hits
        .iter()
        .map(|h: &Hit| {
            let freshness =
                freshness_of(state, &h.document_id, h.root_id, h.relative_path.as_deref())
                    .map_err(internal)?;
            Ok(json!({
                "id": PassageId {
                    chunk_id: h.chunk_id,
                    document_id: h.document_id.clone(),
                    ord: h.ord,
                }
                .format(),
                "text": h.text,
                "path": h.relative_path,
                "absolutePath": absolute(&roots, h.root_id, h.relative_path.as_deref()),
                "locator": h.coordinate.render(),
                "section": h.section_title,
                "freshness": freshness_kind(&freshness)?,
            }))
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    Ok(json!({
        "empty": passages.is_empty(),
        "passages": passages,
        "arms": { "text": text, "content": content },
    }))
}

fn passage(state: &AppState, id: &str, context: u32) -> Result<Value, WireError> {
    let id =
        PassageId::parse(id).ok_or_else(|| err(ErrKind::BadRequest, "malformed passage id"))?;
    let citation = state
        .with_index(|db| db.read_snapshot(|db| db.citation(id.chunk_id)))
        .map_err(internal)?
        .ok_or_else(|| err(ErrKind::NotFound, "no such passage"))?;
    let stale = || {
        err(
            ErrKind::StaleId,
            "the passage changed since it was found; search again",
        )
    };
    if citation.document_id != id.document_id || citation.ord != id.ord {
        return Err(stale());
    }
    let view = crate::tree::document_view_in(
        state,
        &citation.document_id,
        citation.root_id,
        citation.relative_path.as_deref(),
        Some((id.chunk_id, citation.ord, &citation.text)),
    )
    .map_err(internal)?;
    let (blocks, focus_ids, freshness) = match view {
        DocumentView::File {
            blocks,
            focus_block_ids,
            freshness,
            ..
        } => (blocks, focus_block_ids, freshness),
        DocumentView::Unavailable { reason } => {
            return Err(internal(format!("the document is unavailable: {reason:?}")));
        }
    };
    let at = |block_id: i64| blocks.iter().position(|b| b.block_id == block_id);
    let (Some(first), Some(last)) = (
        focus_ids.first().and_then(|b| at(*b)),
        focus_ids.last().and_then(|b| at(*b)),
    ) else {
        return Err(stale());
    };
    let context = context as usize;
    let lo = first.saturating_sub(context);
    let hi = (last + context).min(blocks.len() - 1);
    let blocks: Vec<Value> = blocks[lo..=hi]
        .iter()
        .map(|b| {
            json!({
                "text": b.text,
                "kind": b.kind,
                "pageNo": b.page_no,
                "sectionTitle": b.section_title,
                "focus": focus_ids.contains(&b.block_id),
            })
        })
        .collect();
    let roots = roots(state)?;
    Ok(json!({
        "id": id.format(),
        "path": citation.relative_path,
        "absolutePath": absolute(&roots, citation.root_id, citation.relative_path.as_deref()),
        "freshness": freshness_kind(&freshness)?,
        "blocks": blocks,
    }))
}

fn status(state: &AppState) -> Result<Value, WireError> {
    let folders: Vec<Value> = roots(state)?
        .into_iter()
        .map(|(_, path)| json!({ "available": Path::new(&path).is_dir(), "path": path }))
        .collect();
    Ok(json!({
        "scan": serde_json::to_value(state.scan_state()).map_err(internal)?,
        "folders": folders,
    }))
}

/// `poll` and `cap` are parameters so a test need not wait the real ten minutes.
fn rescan(state: &AppState, wait: bool, poll: Duration, cap: Duration) -> Result<Value, WireError> {
    // Only the busy slot is an answer; any other refusal is a fault.
    let started = match crate::scan_job::start(state, Entry::Full) {
        Ok(()) => true,
        Err(Error::JobAlreadyRunning) => false,
        Err(e) => return Err(internal(e)),
    };
    if !wait {
        return Ok(json!({ "started": started, "running": true }));
    }
    let deadline = Instant::now() + cap;
    loop {
        let scan = state.scan_state();
        match scan.snapshot {
            ScanSnapshot::Running { .. } if Instant::now() < deadline => std::thread::sleep(poll),
            ScanSnapshot::Running { .. } => {
                return Ok(json!({ "started": started, "running": true, "report": Value::Null }));
            }
            ScanSnapshot::Ended { report } => {
                return Ok(json!({
                    "started": started,
                    "running": false,
                    "report": serde_json::to_value(report).map_err(internal)?,
                }));
            }
            ScanSnapshot::Idle => {
                return Ok(json!({ "started": started, "running": false, "report": Value::Null }));
            }
        }
    }
}
