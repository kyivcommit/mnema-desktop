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

fn mock_app_with_memory() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(mnema_desktop::provider::HiddenAt::default())
        .manage(mnema_desktop::ReturnToLauncher::default())
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

// `show_settings` is the one path to the settings window: the tray item and the
// launcher's ⌘, both go through it. The pair distinguishes the label it looks
// for — a function that targets any other window fails one of the two.
#[test]
fn show_settings_targets_the_settings_window() {
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "settings", Default::default())
        .build()
        .expect("failed to build the settings webview");
    assert!(
        mnema_desktop::show_settings(app.handle(), false),
        "show_settings did not find the `settings` window"
    );
}

#[test]
fn show_settings_reports_a_missing_settings_window() {
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    assert!(
        !mnema_desktop::show_settings(app.handle(), false),
        "show_settings acted on a window that is not the settings window"
    );
}

fn app_with_both_windows() -> tauri::App<tauri::test::MockRuntime> {
    let app = mock_app_with_memory();
    for label in ["launcher", "settings"] {
        WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .expect("failed to build a webview");
    }
    app
}

// Settings opened from the launcher (⌘,) hand the person back to it on close —
// once: the mark is consumed, so a later close of a settings window opened
// some other way does not bring the launcher up.
#[test]
fn closing_settings_opened_from_the_launcher_brings_the_launcher_back_once() {
    let app = app_with_both_windows();
    mnema_desktop::show_settings(app.handle(), true);
    assert!(
        mnema_desktop::hide_settings(app.handle()),
        "the launcher was not brought back"
    );
    assert!(
        !mnema_desktop::hide_settings(app.handle()),
        "the mark outlived the close it was set for"
    );
}

// The return reports the launcher it brought back, not the mark: with no
// launcher window to show, nothing came back.
#[test]
fn closing_settings_with_no_launcher_window_brings_nothing_back() {
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "settings", Default::default())
        .build()
        .expect("failed to build the settings webview");
    mnema_desktop::show_settings(app.handle(), true);
    assert!(
        !mnema_desktop::hide_settings(app.handle()),
        "hide_settings reported a launcher that does not exist"
    );
}

// The tray opens settings with no launcher behind them — and a tray open after
// a ⌘, one supersedes it: the last way in decides the way out.
#[test]
fn closing_settings_opened_from_the_tray_leaves_the_launcher_hidden() {
    let app = app_with_both_windows();
    mnema_desktop::show_settings(app.handle(), true);
    mnema_desktop::show_settings(app.handle(), false);
    assert!(
        !mnema_desktop::hide_settings(app.handle()),
        "a tray-opened settings window brought the launcher up on close"
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
fn the_command_surface_still_builds() {
    let app = mock_builder()
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("the invoke handler no longer builds");
    // A build-time smoke that PR 2 kept the command registration intact:
    // `invoke_handler` still composes and the mock app builds. NB: this proves
    // the handler builds — it does NOT detect a later task *adding* a command;
    // that would still compile.
    let _ = app;
}

// §1: the shortcut is a show, not a toggle. In a normal window a second press
// means "I am here", and a hide on it is what the popup did. `HiddenAt` is the
// observable: `hide_launcher` marks it, `focus_launcher` does not. The mock
// reports every window visible, so the old `is_visible` branch hid on the very
// first press here.
#[test]
fn a_second_shortcut_focuses_and_does_not_hide() {
    use tauri::Manager;
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    let hidden_at = app.state::<mnema_desktop::provider::HiddenAt>();

    mnema_desktop::toggle_launcher(app.handle());
    assert_eq!(hidden_at.get(), None, "the first press hid the launcher");

    mnema_desktop::toggle_launcher(app.handle());
    assert_eq!(hidden_at.get(), None, "the second press hid the launcher");
}

// §6 is unchanged by the normal window: a close hides, it does not quit. The
// mock has no window manager to raise the event on, so this reads the arm that
// implements the rule — the same text-guard shape `lib.rs`'s own tests use.
#[test]
fn closing_the_window_hides_and_does_not_exit() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let src = std::fs::read_to_string(&path).expect("lib.rs reads");
    let at = src
        .find("tauri::WindowEvent::CloseRequested")
        .expect("the CloseRequested arm is gone");
    let end = src[at..]
        .find("api.prevent_close();")
        .expect("the CloseRequested arm no longer prevents the close");
    let arm = &src[at..at + end];
    // Prose in this arm quotes `app.exit(0)` while explaining why it is not
    // here; a guard that reads prose as code fails on its own documentation.
    let code = arm
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("hide_launcher"),
        "a launcher close no longer hides the launcher: {code}"
    );
    assert!(
        !code.contains("exit("),
        "a window close now exits the application, and §6 says the tray is the only exit: {code}"
    );
}

// The one thing worth checking about the deleted layout state: its survivor,
// `HiddenAt`, is provider state and is managed, because `start_cold_timer`
// panics without it. A text guard because `.setup` cannot be run on the mock;
// the production half is searched so this test's own literal cannot satisfy it.
#[test]
fn the_focus_mark_is_managed_for_the_cold_timer() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let src = std::fs::read_to_string(&path).expect("lib.rs reads");
    let production = &src[..src.find("#[cfg(test)]").expect("no #[cfg(test)] marker")];
    assert!(
        production.contains(".manage(provider::HiddenAt::default())"),
        "the window's focus mark is no longer managed, and the cold timer panics without it"
    );
}

// The mock has no window manager to raise `Focused` on, so this reads the two
// arms that implement the rule, the same text-guard shape the close guard above
// uses: losing focus arms the clock, and the focus coming back has to reach the
// load path — otherwise a return of focus is unloaded behind by the clock the
// loss started.
#[test]
fn focus_in_after_a_cold_show_loads_the_models() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let src = std::fs::read_to_string(&path).expect("lib.rs reads");
    fn arm<'a>(src: &'a str, event: &str) -> &'a str {
        let at = src
            .find(event)
            .unwrap_or_else(|| panic!("the {event} arm is gone"));
        let rest = &src[at..];
        &rest[..rest
            .find("\n            }")
            .expect("the arm is not one match arm")]
    }
    assert!(
        arm(&src, "tauri::WindowEvent::Focused(false)")
            .contains("launcher_hidden(window.app_handle())"),
        "a focus loss no longer arms the clock"
    );
    assert!(
        arm(&src, "tauri::WindowEvent::Focused(true)")
            .contains("launcher_shown(window.app_handle())"),
        "a focus-in no longer shows the launcher, so nothing loads the models again"
    );
}

#[test]
fn hide_launcher_records_when_it_hid() {
    // The primitive, not the mechanism: the mock cannot tell visible from
    // hidden, so the close-to-tray path itself is the live run's.
    use tauri::Manager;
    let app = mock_app_with_memory();
    WebviewWindowBuilder::new(&app, "launcher", Default::default())
        .build()
        .expect("failed to build the launcher webview");
    let hidden_at = app.state::<mnema_desktop::provider::HiddenAt>();
    assert_eq!(hidden_at.get(), None);

    mnema_desktop::hide_launcher(app.handle());
    let first = hidden_at.get().expect("the hide left no mark");

    std::thread::sleep(std::time::Duration::from_millis(5));
    mnema_desktop::hide_launcher(app.handle());
    let second = hidden_at.get().unwrap();
    assert!(second > first, "a second hide did not move the mark");
}
