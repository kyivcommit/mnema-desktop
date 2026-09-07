//! The interface theme (D146, PR 10b): a persisted choice between following the
//! operating system and forcing light or dark. The file half mirrors
//! `locale.rs` — same key discipline, same `prefs::write_key`, same fallback on
//! anything unreadable. The apply half deliberately does NOT: a theme rebuilds
//! no menu, so it needs no main thread (see `set_theme`).

use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

const THEME_KEY: &str = "theme";

fn choice_to_str(c: ThemeChoice) -> &'static str {
    match c {
        ThemeChoice::System => "system",
        ThemeChoice::Light => "light",
        ThemeChoice::Dark => "dark",
    }
}

fn choice_from_str(s: &str) -> ThemeChoice {
    match s {
        "light" => ThemeChoice::Light,
        "dark" => ThemeChoice::Dark,
        _ => ThemeChoice::System, // "system" and anything unknown
    }
}

/// Reads the persisted choice. Missing file, unreadable JSON, a `theme` that is
/// not a string or not one of the three values — all `System`, because this
/// runs at start-up with nowhere to report to, exactly as `locale::read_choice`.
pub fn read_choice(data_dir: &Path) -> ThemeChoice {
    let all = crate::prefs::read_all(data_dir);
    choice_from_str(
        all.get(THEME_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("system"),
    )
}

/// Persists the choice, keeping every other key in the file — `prefs.json` also
/// holds the locale and the shortcut, and a write from here must not cost them.
pub fn write_choice(data_dir: &Path, choice: ThemeChoice) -> std::io::Result<()> {
    crate::prefs::write_key(
        data_dir,
        THEME_KEY,
        serde_json::Value::String(choice_to_str(choice).into()),
    )
}

/// What the runtime is told. `System` is `None` — "follow the OS" — and not
/// `Some(what the OS reports now)`, which would freeze the appearance at the
/// moment of the click.
pub fn forced(choice: ThemeChoice) -> Option<Theme> {
    match choice {
        ThemeChoice::System => None,
        ThemeChoice::Light => Some(Theme::Light),
        ThemeChoice::Dark => Some(Theme::Dark),
    }
}

/// Applies a choice to every window's native chrome, then tells every webview.
///
/// Per window and not `AppHandle::set_theme`, although both exist: on macOS and
/// Linux the theme is app-wide whichever is called (`window/mod.rs:1897`), on
/// Windows each window is told separately either way, and only the per-window
/// call is implemented by Tauri's mock runtime — `AppHandle::set_theme` there is
/// `unimplemented!()` (`tauri-2.11.5/src/test/mock_runtime.rs:257`). The
/// integration test in `tests/commands.rs` pins reachability, the persist/apply
/// round trip, and the `theme-changed` broadcast, and it would panic if this
/// loop were replaced by `AppHandle::set_theme`, which is the `unimplemented!()`
/// path under the mock.
///
/// ⚠️ **No headless test distinguishes this loop running from this loop
/// deleted, and that is a property of the runtime rather than a gap somebody
/// left.** The mock runtime's per-window `set_theme` dispatcher returns
/// `Ok(())` and records nothing (`mock_runtime.rs:1118`), and its `theme()`
/// getter answers a constant regardless of what was set — the same limit
/// `prefs::set_hotkey`'s tray relabel runs into. Best-effort (`let _ =`) like
/// every relabel in `locale::apply_locale`: a window that refuses is a stale
/// frame, and the file is already written. Verified by running the
/// application and watching each window's chrome change, not by this suite.
fn apply_to_windows<R: Runtime>(app: &AppHandle<R>, choice: ThemeChoice) {
    let theme = forced(choice);
    for window in app.webview_windows().values() {
        let _ = window.set_theme(theme);
    }
    let _ = app.emit("theme-changed", choice_to_str(choice));
}

/// The boot: the persisted choice, applied to the native chrome before either
/// window is shown. Without this a restart would leave the settings window's
/// frame following the OS while its document follows `data-theme` — two
/// answers to one question. Runs in `.setup` after `manage_state`, because it
/// reads the data dir from `AppState`. No webview is listening yet, so the
/// emit reaches nobody; each window is expected to ask `get_theme` itself when
/// it boots (PR 10b's UI half).
pub fn apply_persisted<R: Runtime>(app: &AppHandle<R>) {
    let choice = read_choice(app.state::<crate::state::AppState>().data_dir());
    apply_to_windows(app, choice);
}

/// The IPC shape: a string, for the same reason `LocaleReply` is one — the
/// webview wants `"system"|"light"|"dark"`, not a Rust-shaped encoding.
#[derive(Serialize)]
pub struct ThemeReply {
    pub choice: String,
}

/// Reads the choice back for a window that has just booted.
///
/// Unlike locale, `AppState` carries no cached field for this: `set_theme`
/// writes the file and nothing else, so `state` here exists only to reach
/// `data_dir()` and this command re-reads `read_choice` fresh on every call,
/// the same source `apply_persisted` reads once at boot.
#[tauri::command(async)]
pub fn get_theme(state: tauri::State<'_, crate::state::AppState>) -> ThemeReply {
    ThemeReply {
        choice: choice_to_str(read_choice(state.data_dir())).into(),
    }
}

/// `(async)`, unlike `set_locale` — and the difference is not an oversight.
/// `set_locale` rebuilds the tray menu inline, and `muda::Menu::new` panics off
/// the main thread on macOS (`locale.rs`, above `set_locale`). This command
/// rebuilds nothing: `WebviewWindow::set_theme` hands a `SetTheme` message to
/// the runtime, and from a worker thread that message is posted through the
/// event-loop proxy rather than handled inline (`tauri-runtime-wry-2.11.4/src/
/// lib.rs:235-255`, `send_user_message`). Either attribute would be correct;
/// `(async)` keeps the file write off the event loop.
///
/// Order: persist → apply → broadcast. A write that fails surfaces as
/// `Error::Prefs` and applies nothing, so a rejection means "nothing changed",
/// which is the sentence the window draws beside it.
#[tauri::command(async)]
pub fn set_theme<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    choice: String,
) -> Result<(), crate::error::Error> {
    let choice = choice_from_str(&choice);
    write_choice(state.data_dir(), choice)?;
    apply_to_windows(&app, choice);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn choice_round_trips_through_prefs() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_choice(dir.path()), ThemeChoice::System); // no file → System
        // Light, then Dark, then System: every write is a CHANGE from what the
        // file held, and the last one shows an explicit "system" reads back as
        // System rather than only the absence of the key doing so.
        for choice in [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System] {
            write_choice(dir.path(), choice).unwrap();
            assert_eq!(read_choice(dir.path()), choice);
        }
    }

    #[test]
    fn unreadable_or_unknown_prefs_fall_back_to_system() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(paths::prefs_path(dir.path()), b"{ not json").unwrap();
        assert_eq!(read_choice(dir.path()), ThemeChoice::System);
        std::fs::write(paths::prefs_path(dir.path()), br#"{"theme":"sepia"}"#).unwrap();
        assert_eq!(read_choice(dir.path()), ThemeChoice::System);
        // Not a string at all: `as_str()` answers None, same arm as a missing key.
        std::fs::write(paths::prefs_path(dir.path()), br#"{"theme":true}"#).unwrap();
        assert_eq!(read_choice(dir.path()), ThemeChoice::System);
    }

    #[test]
    fn write_preserves_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            paths::prefs_path(dir.path()),
            br#"{"locale":"uk","hotkey":"Alt+Space"}"#,
        )
        .unwrap();
        write_choice(dir.path(), ThemeChoice::Dark).unwrap();
        let raw = std::fs::read_to_string(paths::prefs_path(dir.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["locale"], "uk", "foreign key dropped: {raw}");
        assert_eq!(parsed["hotkey"], "Alt+Space", "foreign key dropped: {raw}");
        assert_eq!(parsed["theme"], "dark", "theme not written: {raw}");
    }

    #[cfg(unix)]
    #[test]
    fn failed_write_keeps_the_previous_choice() {
        // The same invariant `locale::tests::failed_write_keeps_the_previous_choice`
        // pins, on this key: `write_key` is temp + rename, so a write that cannot
        // create the sibling temp file surfaces an error AND leaves the file as
        // it was. An in-place write would go red here — the file itself stays
        // writable inside the read-only dir and would be overwritten.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_choice(dir.path(), ThemeChoice::Dark).unwrap();

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = write_choice(dir.path(), ThemeChoice::Light);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            failed.is_err(),
            "a write into a read-only dir must surface an error"
        );
        assert_eq!(
            read_choice(dir.path()),
            ThemeChoice::Dark,
            "a failed write must not change the persisted choice"
        );
    }

    #[test]
    fn system_forces_nothing_and_each_explicit_choice_forces_itself() {
        // `None` is "follow the OS from now on", which is NOT the same as
        // `Some(whatever the OS reports right now)`: the latter would freeze the
        // appearance at the moment of the click and stop following.
        assert_eq!(forced(ThemeChoice::System), None);
        assert_eq!(forced(ThemeChoice::Light), Some(Theme::Light));
        assert_eq!(forced(ThemeChoice::Dark), Some(Theme::Dark));
    }
}
