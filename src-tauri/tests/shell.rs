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
