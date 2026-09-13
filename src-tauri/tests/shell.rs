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

fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

#[test]
fn focus_launcher_targets_the_launcher_window() {
    let app = mock_app();
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
    let app = mock_app();
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("failed to build the main webview");
    assert!(
        !mnema_desktop::focus_launcher(app.handle()),
        "focus_launcher acted on a window that is not the launcher"
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
