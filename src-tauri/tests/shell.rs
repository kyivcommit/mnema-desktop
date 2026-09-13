//! The shell's pure seams — the parts of the tray/launcher wiring that do not
//! need a real window manager, so they run headlessly on both CI legs. The
//! tray itself, the global shortcut, transparency, and positioning are OS-level
//! and are verified by the live `cargo tauri dev` run, not here.
//!
//! The tray MENU's id order/labels used to be asserted here too, against the
//! pure `MENU_ITEMS` array. §D129 made labels locale-dependent, so that array
//! is gone; the order guard now lives in `tray.rs` itself
//! (`TRAY_ITEM_IDS`), because on macOS even a plain `muda::Menu` — not only
//! the tray icon — requires the main thread to construct, and `#[test]`
//! functions do not run on it. See `tray.rs`'s `TRAY_ITEM_IDS` doc comment.

use tauri::WebviewWindowBuilder;
use tauri::test::{mock_builder, mock_context, noop_assets};

// Not for `focus_launcher`: it reads `Memory` from managed state and panics
// with "state not managed" without it. Use `mock_app_with_memory` for that.
fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

fn mock_app_with_memory() -> tauri::App<tauri::test::MockRuntime> {
    // `focus_launcher` reads the launcher-position memory from managed state
    // (D155); the production `.setup` manages it, a test does it here.
    mock_builder()
        .manage(mnema_desktop::launcher_position::Memory::default())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

#[test]
fn focus_launcher_targets_the_launcher_window() {
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    // Found and acted on the launcher.
    assert!(
        mnema_desktop::focus_launcher(app.handle()),
        "focus_launcher did not find the `launcher` window"
    );
}

#[test]
fn focus_launcher_reports_a_missing_launcher() {
    // The other direction: with no `launcher` window (only some other label),
    // it must report false rather than silently targeting the wrong window —
    // this is what fails while the ported code still looks for `main`.
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("failed to build the main webview");
    assert!(
        !mnema_desktop::focus_launcher(app.handle()),
        "focus_launcher acted on a window that is not the launcher"
    );
}

#[test]
fn place_records_where_it_put_the_launcher() {
    // The production seam behind every show of a hidden launcher: after
    // `place`, memory holds what the mock runtime reports as the window's
    // position, (0, 0). Deleting the final `set_applied` leaves it at None.
    use tauri::Manager;
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    let window = app
        .get_webview_window("launcher")
        .expect("no launcher window");
    let memory = app.state::<mnema_desktop::launcher_position::Memory>();
    assert_eq!(memory.applied(), None, "applied before any show?");

    mnema_desktop::launcher_position::place(&window, &memory, None, false);

    assert_eq!(
        memory.applied(),
        Some(tauri::PhysicalPosition::new(0, 0)),
        "place did not record where it put the launcher"
    );
}

#[test]
fn focus_launcher_leaves_a_visible_launcher_where_it_is() {
    // The mock runtime reports every window as visible, which makes it the
    // fixture for the other branch of `focus_launcher` (review P2-3): a
    // launcher that is already up is focused, not re-placed — a drag that no
    // focus loss has recorded yet must not be undone by a second instance's
    // callback. Deleting the visibility branch runs `place` and sets applied.
    use tauri::Manager;
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    let memory = app.state::<mnema_desktop::launcher_position::Memory>();

    assert!(mnema_desktop::focus_launcher(app.handle()));

    assert_eq!(memory.applied(), None, "a visible launcher was re-placed");
}

#[test]
fn a_fallback_show_then_an_untouched_hide_keeps_the_saved_position() {
    // The mock runtime's `available_monitors` is `[]`, so `reachable` returns
    // `None` here regardless of what is saved: every `place` call on the mock
    // is the fallback path. Before D155's fix, the fallback show left the
    // dragged `left` untouched, and an untouched hide right after compared
    // the new default against the stale drag, scored it as a move, and
    // overwrote the file with the default (0, 0) — losing the saved position
    // with no drag anywhere in the sequence.
    use mnema_desktop::launcher_position::{self, Memory};
    use tauri::Manager;
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    let window = app
        .get_webview_window("launcher")
        .expect("no launcher window");
    let memory = app.state::<Memory>();
    let dir = tempfile::tempdir().unwrap();
    launcher_position::write(dir.path(), tauri::PhysicalPosition::new(640, 80)).unwrap();
    let prefs_path = mnema_desktop::paths::prefs_path(dir.path());
    let before = std::fs::read(&prefs_path).unwrap();

    // A drag recorded earlier in the session, matching the saved value.
    memory.set_applied(Some(tauri::PhysicalPosition::new(5, 5)));
    memory.moved(tauri::PhysicalPosition::new(640, 80));

    launcher_position::place(&window, &memory, Some(dir.path()), false);
    launcher_position::remember(&window.as_ref().window(), &memory, dir.path(), false);

    let after = std::fs::read(&prefs_path).unwrap();
    assert_eq!(
        before, after,
        "an untouched hide after a fallback show rewrote prefs.json"
    );
    assert_eq!(
        launcher_position::read(dir.path()),
        Some(tauri::PhysicalPosition::new(640, 80)),
        "the saved position was overwritten by the fallback default"
    );
}

#[test]
fn the_command_surface_still_builds() {
    let app = mock_builder()
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("the invoke handler no longer builds");
    // A build-time smoke that PR 2 kept the command registration intact:
    // `invoke_handler` still composes and the mock app builds. PR 2 adds no
    // webview command (dismissal is webview-side via `core:window:allow-hide`,
    // showing is Rust-side). NB: this proves the handler builds — it does NOT
    // detect a later task *adding* a command; that would still compile.
    let _ = app;
}

#[test]
fn remember_writes_the_launcher_position_it_finds() {
    // The production seam behind the `Focused(false)` arm: the mock runtime
    // answers `outer_position` with (0, 0); with (5, 5) recorded as applied,
    // that is a move, and the file must say so. Deleting `remember`'s body —
    // or its `outer_position` read — leaves the file without the key.
    use mnema_desktop::launcher_position::{self, Memory};
    use tauri::Manager;
    let app = mock_app();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    // `get_window` needs the `unstable` feature, which is off here; go through
    // the webview window instead — `remember`'s parameter type is the same
    // `Window<R>` a real `WindowEvent::Focused` handler already holds.
    let webview_window = app
        .get_webview_window("launcher")
        .expect("no launcher window");
    let window = webview_window.as_ref().window();
    let dir = tempfile::tempdir().unwrap();
    let memory = Memory::default();
    memory.set_applied(Some(tauri::PhysicalPosition::new(5, 5)));

    launcher_position::remember(&window, &memory, dir.path(), false);

    assert_eq!(
        launcher_position::read(dir.path()),
        Some(tauri::PhysicalPosition::new(0, 0)),
        "remember did not write what the window reported"
    );
    // Wayland: the same call writes nothing (the value is not a position there),
    // and records nothing in memory either. A FRESH memory, so the assertion
    // rests on the Wayland branch and not on the equality short-circuit above.
    let dir2 = tempfile::tempdir().unwrap();
    let fresh = Memory::default();
    fresh.set_applied(Some(tauri::PhysicalPosition::new(5, 5)));
    launcher_position::remember(&window, &fresh, dir2.path(), true);
    assert_eq!(
        launcher_position::read(dir2.path()),
        None,
        "wrote under Wayland"
    );
    assert_eq!(fresh.left(), None, "Wayland recorded a move in memory");
}
