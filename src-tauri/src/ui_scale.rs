//! The interface scale (owner's decision, 2026-10-21): a persisted choice
//! between three sizes, applied as webview PAGE ZOOM in every window.
//!
//! The file half mirrors `theme.rs` — same key discipline, same
//! `prefs::write_key`, same fallback on anything unreadable. The apply half is
//! `WebviewWindow::set_zoom` (`tauri-2.11.5/src/webview/webview_window.rs:2515`),
//! which scales the whole document rather than any one font: available on
//! macOS 11+, and this application requires macOS 14 or later
//! (`docs/BUILD.md:49`), so the floor sits below this build's own.
//!
//! Two deliberate differences from `theme.rs`, each commented where it happens
//! rather than only here: there is no broadcast, and no per-window rebuild that
//! would put the apply back on the main thread.

use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

/// The three sizes the settings window offers. The wire strings are the
/// contract `ui/src/` codes against, so they are spelled once, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiScale {
    Small,
    Medium,
    Large,
}

// snake_case, like every other key in this file: the camelCase spelling is the
// wire shape (`AppPrefs.uiScale`), not what a person reading `prefs.json` sees.
const UI_SCALE_KEY: &str = "ui_scale";

pub(crate) fn to_str(scale: UiScale) -> &'static str {
    match scale {
        UiScale::Small => "small",
        UiScale::Medium => "medium",
        UiScale::Large => "large",
    }
}

/// `"medium"` and anything unknown. Written as a catch-all rather than as a
/// `"medium"` arm plus an error: this runs at start-up and in a command with
/// nowhere to report a complaint to, exactly as `theme::choice_from_str`, and
/// `Medium` is the scale that changes nothing.
fn from_str(s: &str) -> UiScale {
    match s {
        "small" => UiScale::Small,
        "large" => UiScale::Large,
        _ => UiScale::Medium, // "medium" and anything unknown
    }
}

impl UiScale {
    /// The factor `WebviewWindow::set_zoom` is handed.
    ///
    /// Large stops at 1.15, and the ceiling is the launcher's own layout, not
    /// taste: `layoutFor` needs `L 240 + W 480 + MIN.TRANSCRIPT 520 = 1240` CSS
    /// px for both side panels, and the configured window is 1440 device px —
    /// at 1.2 that is 1200 CSS px and the conversations panel auto-hides the
    /// moment «Large» is pressed on a fresh install. 1440 / 1.15 = 1252.
    /// Raising this means re-checking that arithmetic.
    fn factor(self) -> f64 {
        match self {
            UiScale::Small => 0.9,
            UiScale::Medium => 1.0,
            UiScale::Large => 1.15,
        }
    }
}

/// Reads the persisted choice. Missing file, unreadable JSON, a `uiScale` that
/// is not a string or not one of the three values — all `Medium`, for the
/// reason `from_str` gives.
pub(crate) fn read_choice(data_dir: &Path) -> UiScale {
    let all = crate::prefs::read_all(data_dir);
    from_str(
        all.get(UI_SCALE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("medium"),
    )
}

/// Persists the choice, keeping every other key in the file — `prefs.json` also
/// holds the locale, the hotkey and the theme, and a write from here must not
/// cost them.
fn write_choice(data_dir: &Path, choice: UiScale) -> std::io::Result<()> {
    crate::prefs::write_key(
        data_dir,
        UI_SCALE_KEY,
        serde_json::Value::String(to_str(choice).into()),
    )
}

/// Applies a choice to every window's webview as page zoom.
///
/// Best-effort (`let _ =`) for the reason every relabel in
/// `locale::apply_locale` gives: the file is already written, and a window that
/// refuses is a stale frame rather than a failed change.
///
/// ⚠️ **No headless test distinguishes this loop running from this loop
/// deleted, and that is a property of the runtime rather than a gap somebody
/// left.** The mock runtime implements `set_zoom` as `Ok(())` and records
/// nothing (`tauri-2.11.5/src/test/mock_runtime.rs:583`) — the same limit the
/// per-window `set_theme` loop has. It is verified by running the application
/// and looking at each window, not by this suite.
fn apply_to_windows<R: Runtime>(app: &AppHandle<R>, choice: UiScale) {
    let factor = choice.factor();
    for window in app.webview_windows().values() {
        let _ = window.set_zoom(factor);
    }
    // No event here, unlike `theme::apply_to_windows`. The theme broadcasts
    // because the operating system can change it behind the application's
    // back, so every document must be told; only this command changes the
    // scale, and the settings window that asked takes the choice its own reply
    // confirmed (`Ok` means the file holds it). A window that later remounts
    // reads it back through `app_prefs`.
}

/// Serialises the whole of a scale change: persist → apply.
///
/// `PREFS_LOCK` (inside `prefs::write_key`) covers the file write alone and is
/// released before `apply_to_windows` runs, so without this two overlapping
/// changes could persist in one order and apply in the other — the file left
/// saying one scale while every window is drawn at the other. Under this lock
/// the second change cannot begin until the first has applied, so the order of
/// the applies is the order of the writes.
///
/// Lock order is UI_SCALE_LOCK → PREFS_LOCK: this is taken first and
/// `write_choice` takes the other inside it. Nothing takes them the other way
/// round, because nothing but this module takes this one at all.
///
/// ⚠️ **No test holds this ordering.** The theme's twin is pinned by
/// `theme.rs`'s `a_change_parked_after_its_persist_keeps_the_next_one_out`
/// (a `#[cfg(test)]` hook between persist and apply); this module has no such
/// hook, so deleting the lock leaves the suite green and the guarantee rests on
/// the mutex alone. Recorded rather than glossed: copying that test here is
/// the way to make it evidence.
static UI_SCALE_LOCK: Mutex<()> = Mutex::new(());

/// The body of `set_ui_scale`, as a free function over the data directory.
///
/// `pub(crate)` and not `pub`, for `theme::change_theme`'s reason: the only
/// caller outside this module's own tests is the command below, and a wider
/// door is one a future main-thread caller walks through.
pub(crate) fn change_scale<R: Runtime>(
    app: &AppHandle<R>,
    data_dir: &Path,
    choice: UiScale,
) -> Result<(), crate::error::Error> {
    // Poisoning is absorbed rather than propagated, exactly as `PREFS_LOCK`
    // does one module over: this mutex guards no value, so there is no
    // corrupted state to refuse access to, and propagating the poison would
    // only stop every later change from correcting what a panic left behind.
    let _one_at_a_time = UI_SCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_choice(data_dir, choice)?;
    apply_to_windows(app, choice);
    Ok(())
}

/// The boot: the persisted scale, applied before either window is shown.
/// Without this a restart would draw every window at `1.0` while the settings
/// window's control still says otherwise. Runs in `.setup` after
/// `manage_state`, because it reads the data dir from `AppState`.
///
/// Under `UI_SCALE_LOCK`, like the only other place that applies this choice:
/// the rule is that nothing decides a choice and applies it outside the lock,
/// and a rule with an exception in it is one a later caller copies.
pub fn apply_persisted<R: Runtime>(app: &AppHandle<R>) {
    let _one_at_a_time = UI_SCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let choice = read_choice(app.state::<crate::state::AppState>().data_dir());
    apply_to_windows(app, choice);
}

/// `(async)`, like `set_theme` and unlike `set_locale` — and for the same
/// reason the theme's command gives: this rebuilds no menu, `set_zoom` from a
/// worker thread posts its message through the event-loop proxy rather than
/// running inline (`tauri-runtime-wry-2.11.4/src/lib.rs:235-255`), and
/// `(async)` keeps the file write off the event loop.
///
/// Order: persist → apply, one change at a time — `change_scale`'s single
/// critical section, not two steps a second command may cut into. A write that
/// fails surfaces as `Error::Prefs` and applies nothing, so a rejection means
/// "nothing changed", which is the sentence the window draws beside it.
#[tauri::command(async)]
pub fn set_ui_scale<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    choice: String,
) -> Result<(), crate::error::Error> {
    change_scale(&app, state.data_dir(), from_str(&choice))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn every_choice_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        for choice in [UiScale::Small, UiScale::Medium, UiScale::Large] {
            write_choice(dir.path(), choice).unwrap();
            assert_eq!(read_choice(dir.path()), choice);
            assert_eq!(
                crate::prefs::read_all(dir.path()).get(UI_SCALE_KEY),
                Some(&serde_json::Value::String(to_str(choice).into())),
                "the stored spelling is the wire contract"
            );
        }
    }

    #[test]
    fn every_stored_string_is_understood_and_anything_else_is_medium() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_choice(dir.path()), UiScale::Medium, "no file yet");
        let set = |value: serde_json::Value| {
            crate::prefs::write_key(dir.path(), UI_SCALE_KEY, value).unwrap();
            read_choice(dir.path())
        };
        assert_eq!(set(serde_json::json!("small")), UiScale::Small);
        assert_eq!(set(serde_json::json!("medium")), UiScale::Medium);
        assert_eq!(set(serde_json::json!("large")), UiScale::Large);
        // Unknown, wrong case, and not a string at all — one arm, because the
        // reader answers `as_str()` for all of them.
        for bad in [
            serde_json::json!("gigantic"),
            serde_json::json!(""),
            serde_json::json!("Medium"),
            serde_json::json!(120),
            serde_json::json!(true),
            serde_json::json!(null),
        ] {
            assert_eq!(set(bad.clone()), UiScale::Medium, "{bad} must be medium");
        }
        std::fs::write(paths::prefs_path(dir.path()), b"{ not json").unwrap();
        assert_eq!(read_choice(dir.path()), UiScale::Medium, "unreadable file");
    }

    #[test]
    fn the_three_factors_are_the_contract() {
        // Pinned as numbers rather than as "distinct": these three are what the
        // settings window's three options promise, and a layout tuned at 0.9
        // and 1.15 is what a change would have to be re-checked against.
        assert_eq!(UiScale::Small.factor(), 0.9);
        assert_eq!(UiScale::Medium.factor(), 1.0);
        assert_eq!(UiScale::Large.factor(), 1.15);
    }
}
