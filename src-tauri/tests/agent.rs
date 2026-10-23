//! The agent handlers, called as `agent::handle` with a real index behind them.

mod support;

#[path = "support/app.rs"]
mod app;
use app::app_in;
use support::scan::{run_scan_watching, scan_to_completion};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mnema_core::agent_wire::{ErrKind, Op};
use mnema_desktop::agent::handle;
use mnema_desktop::scan_state::{Entry, ScanSnapshot};
use mnema_desktop::state::AppState;
use serde_json::Value;
use tauri::Manager;

fn fixture_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("animals.txt"),
        "the quick brown fox jumps over the lazy dog",
    )
    .unwrap();
    dir
}

/// An app with the index open and `dir` watched and scanned.
fn indexed(data: &std::path::Path, dir: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = unfolded(data);
    let state = app.state::<AppState>();
    state
        .with_index(|db| db.insert_watched_root(dir.to_str().unwrap()))
        .unwrap();
    scan_to_completion(app.handle());
    app
}

fn unfolded(data: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = app_in(data);
    app.state::<AppState>().open_index().unwrap();
    app
}

fn search(state: &AppState, query: &str, k: u32) -> Value {
    handle(
        state,
        Op::Search {
            query: query.into(),
            k: Some(k),
        },
    )
    .expect("search failed")
}

fn passage(
    state: &AppState,
    id: &str,
    context: u32,
) -> Result<Value, mnema_core::agent_wire::WireError> {
    handle(
        state,
        Op::Passage {
            id: id.into(),
            context: Some(context),
        },
    )
}

fn first_id(reply: &Value) -> String {
    reply["passages"][0]["id"]
        .as_str()
        .expect("no passage")
        .to_string()
}

#[test]
fn search_answers_verbatim_passages_with_their_place() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let reply = search(&state, "fox", 8);
    let p = &reply["passages"][0];
    assert!(p["text"].as_str().unwrap().contains("fox"), "{reply}");
    assert_eq!(p["path"], "animals.txt");
    assert_eq!(
        p["absolutePath"].as_str().unwrap(),
        dir.path().join("animals.txt").display().to_string()
    );
    assert_eq!(p["freshness"], "current");
    assert!(
        p["locator"].as_str().is_some_and(|l| !l.is_empty()),
        "{reply}"
    );
    assert_eq!(reply["empty"], false);
    assert!(reply["arms"]["text"].is_object() && reply["arms"]["content"].is_object());
}

#[test]
fn search_with_k_one_answers_one_passage() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    std::fs::write(dir.path().join("more.txt"), "another fox in another file").unwrap();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    assert_eq!(
        search(&state, "fox", 8)["passages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        search(&state, "fox", 1)["passages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn search_on_an_empty_index_is_empty_not_an_error() {
    let data = tempfile::tempdir().unwrap();
    let app = unfolded(data.path());
    let reply = search(&app.state::<AppState>(), "fox", 8);
    assert_eq!(reply["passages"].as_array().unwrap().len(), 0, "{reply}");
    assert_eq!(reply["empty"], true);
}

#[test]
fn a_file_changed_after_indexing_is_marked_in_search_and_passage() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "fox", 8));
    std::fs::write(
        dir.path().join("animals.txt"),
        "a different and noticeably longer body of text",
    )
    .unwrap();
    let after = search(&state, "fox", 8);
    assert_eq!(after["passages"][0]["freshness"], "fileChanged", "{after}");
    let p = passage(&state, &id, 2).unwrap();
    assert_eq!(p["freshness"], "fileChanged", "{p}");
}

#[test]
fn passage_of_a_fresh_id_gives_that_text() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let found = search(&state, "fox", 8);
    let p = passage(&state, &first_id(&found), 2).unwrap();
    let focused: Vec<&Value> = p["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["focus"] == true)
        .collect();
    assert!(!focused.is_empty(), "{p}");
    assert!(focused[0]["text"].as_str().unwrap().contains("fox"));
    assert_eq!(p["path"], "animals.txt");
    assert_eq!(p["freshness"], "current");
}

#[test]
fn passage_after_reindex_is_stale_not_another_place() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "fox", 8));
    std::fs::write(
        dir.path().join("animals.txt"),
        "entirely other words, with a fox still in them",
    )
    .unwrap();
    scan_to_completion(app.handle());
    let e = passage(&state, &id, 2).expect_err("an old id answered");
    assert_eq!(e.kind, ErrKind::StaleId, "{e:?}");
}

#[test]
fn passage_of_an_unknown_chunk_is_not_found() {
    let data = tempfile::tempdir().unwrap();
    let app = unfolded(data.path());
    let e = passage(&app.state::<AppState>(), "999999:deadbeef:0", 2).unwrap_err();
    assert_eq!(e.kind, ErrKind::NotFound, "{e:?}");
}

#[test]
fn passage_context_bounds_the_blocks() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let paragraphs: Vec<String> = (0..7)
        .map(|i| format!("paragraph{i} {}", format!("filler{i} ").repeat(120)))
        .collect();
    std::fs::write(dir.path().join("long.txt"), paragraphs.join("\n\n")).unwrap();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "paragraph3", 1));
    let blocks = |context| {
        let p = passage(&state, &id, context).unwrap();
        let b = p["blocks"].as_array().unwrap().clone();
        let focus = b.iter().filter(|x| x["focus"] == true).count();
        (b.len(), focus)
    };
    let (all0, focus0) = blocks(0);
    assert!(
        focus0 > 0 && focus0 < 7,
        "fixture must focus a part: {focus0}"
    );
    assert_eq!(all0, focus0, "context 0 is the focus alone");
    let (all2, focus2) = blocks(2);
    assert_eq!(focus2, focus0);
    assert!(all2 > focus0 && all2 <= focus0 + 4, "{all2} vs {focus0}");
}

#[test]
fn status_carries_the_scan_state_and_folders() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let s = handle(&app.state::<AppState>(), Op::Status).unwrap();
    assert!(s["scan"]["revision"].as_u64().unwrap() > 0, "{s}");
    assert_eq!(s["folders"][0]["path"], dir.path().display().to_string());
    assert_eq!(s["folders"][0]["available"], true);
    drop(dir);
    let s = handle(&app.state::<AppState>(), Op::Status).unwrap();
    assert_eq!(s["folders"][0]["available"], false, "{s}");
}

#[test]
fn rescan_while_running_says_running_not_error() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = unfolded(data.path());
    app.state::<AppState>()
        .with_index(|db| db.insert_watched_root(dir.path().to_str().unwrap()))
        .unwrap();
    let seen: Arc<Mutex<Option<Value>>> = Arc::default();
    let slot = Arc::clone(&seen);
    run_scan_watching(
        app.handle(),
        Entry::Full,
        Duration::from_secs(60),
        move |state, now| {
            let mut slot = slot.lock().unwrap();
            if slot.is_none() && matches!(now.snapshot, ScanSnapshot::Running { .. }) {
                *slot =
                    Some(handle(state, Op::Rescan { wait: None }).expect("busy is not an error"));
            }
        },
    );
    let reply = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the scan never reported Running");
    assert_eq!(reply["started"], false, "{reply}");
    assert_eq!(reply["running"], true, "{reply}");
}

#[test]
fn rescan_wait_returns_the_report() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let reply = handle(&app.state::<AppState>(), Op::Rescan { wait: Some(true) }).unwrap();
    assert_eq!(reply["started"], true, "{reply}");
    assert_eq!(reply["running"], false, "{reply}");
    assert!(reply["report"]["reason"].is_string(), "{reply}");
}
