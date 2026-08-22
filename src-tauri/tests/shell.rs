//! The shell's pure seams — the parts of the tray/launcher wiring that do not
//! need a real window manager, so they run headlessly on both CI legs. The
//! tray itself, the global shortcut, transparency, and positioning are OS-level
//! and are verified by the live `cargo tauri dev` run, not here.

use mnema_desktop::tray::MENU_ITEMS;
use tauri::WebviewWindowBuilder;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn the_tray_menu_is_the_spec_menu_in_order() {
    // §8: статус · Показати пошук · Відкрити налаштування · Пауза індексації ·
    // Перевірити оновлення · Вийти.
    let ids: Vec<&str> = MENU_ITEMS.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [
            "status",
            "show_search",
            "open_settings",
            "pause_indexing",
            "check_updates",
            "quit"
        ],
        "the tray menu drifted from spec §8"
    );
    // Every id is unique — a duplicate id would make `on_menu_event` ambiguous.
    let mut seen = std::collections::HashSet::new();
    for (id, label) in MENU_ITEMS {
        assert!(seen.insert(*id), "duplicate tray menu id: {id}");
        assert!(!label.is_empty(), "empty label for tray id {id}");
    }
}

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
    // No PR 2 command carries a data payload — dismissal is webview-side via
    // `core:window:allow-hide`, showing is Rust-side. This is a guard against a
    // later task quietly adding an ungoverned command to the shell.
    let _ = app;
}
