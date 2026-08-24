//! Read-only enumeration for the launcher's left card (§7): watched folders,
//! their indexed files, and recently-indexed documents. Read-only, so it adds
//! no data-loss surface (spec §12); it simply reflects the index at query time.
//! PR 5's `source_around` will join this module.

use crate::error::Error;
use crate::state::AppState;
use mnema_index::Db;
use serde::Serialize;
use tauri::State;

/// The "Recent" tab cap. Tunable; the launcher shows a bounded list, not the
/// whole corpus.
const RECENTS_LIMIT: i64 = 50;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeListing {
    pub roots: Vec<TreeRoot>,
    pub recents: Vec<RecentDoc>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeRoot {
    pub root_id: i64,
    pub absolute_path: String,
    pub name: String,
    pub files: Vec<TreeFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeFile {
    pub relative_path: String,
    pub document_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentDoc {
    pub document_id: String,
    pub root_id: i64,
    pub relative_path: String,
    pub indexed_at: i64,
}

/// Display name for a watched root: the final path component, or the whole path
/// when there is none (e.g. `/`).
fn basename(absolute_path: &str) -> String {
    std::path::Path::new(absolute_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| absolute_path.to_string())
}

/// The whole catalogue, assembled from the `mnema-index` reads. A private helper
/// kept out of the command body for readability; `list_tree` is a thin
/// `with_index` wrapper over it. The command is exercised through the IPC in
/// Task 4.2's test, per the `commands.rs` doctrine (a direct call would not prove
/// it is registered or that its fields survive the camelCase rename).
fn build_tree_listing(db: &Db) -> Result<TreeListing, mnema_index::Error> {
    let roots = db
        .list_watched_roots()?
        .into_iter()
        .map(|r| {
            let files = db
                .indexed_files_under_root(r.id)?
                .into_iter()
                .map(|f| TreeFile {
                    relative_path: f.relative_path,
                    document_id: f.document_id,
                })
                .collect();
            Ok(TreeRoot {
                name: basename(&r.absolute_path),
                root_id: r.id,
                absolute_path: r.absolute_path,
                files,
            })
        })
        .collect::<Result<Vec<_>, mnema_index::Error>>()?;

    let recents = db
        .recent_indexed_documents(RECENTS_LIMIT)?
        .into_iter()
        .map(|d| RecentDoc {
            document_id: d.document_id,
            root_id: d.watched_root_id,
            relative_path: d.relative_path,
            indexed_at: d.created_at,
        })
        .collect();

    Ok(TreeListing { roots, recents })
}

/// Off the main thread for the reason given on [`crate::bridge::open_index`].
#[tauri::command(async)]
pub fn list_tree(state: State<'_, AppState>) -> Result<TreeListing, Error> {
    state.with_index(build_tree_listing)
}
