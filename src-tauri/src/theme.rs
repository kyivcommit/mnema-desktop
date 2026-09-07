//! The interface theme (D146, PR 10b): a persisted choice between following the
//! operating system and forcing light or dark. The file half mirrors
//! `locale.rs` — same key discipline, same `prefs::write_key`, same fallback on
//! anything unreadable. The apply half deliberately does NOT: a theme rebuilds
//! no menu, so it needs no main thread (see `set_theme`).

use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
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
/// integration test in `tests/commands.rs` pins reachability, the persist and
/// read-back round trip, and the `theme-changed` broadcast, and it would panic if this
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

/// Serialises the whole of a theme change: persist → apply → broadcast.
///
/// `PREFS_LOCK` (inside `prefs::write_key`) covers the file write alone and is
/// released before `apply_to_windows` runs, so without this two overlapping
/// changes could persist in one order and apply in the other — the file left
/// saying one choice while every window's chrome and both documents were told
/// the other. Under this lock the second change cannot begin until the first
/// has applied and broadcast, so the order of the broadcasts is the order of
/// the writes.
///
/// Lock order is THEME_LOCK → PREFS_LOCK: this is taken first and
/// `write_choice` takes the other inside it. Nothing takes them the other way
/// round, because nothing but this module takes this one at all.
static THEME_LOCK: Mutex<()> = Mutex::new(());

/// The body of `set_theme`, as a free function over the data directory — so a
/// test can drive two of them from two threads against a mock application,
/// which the IPC command itself gives no way to do.
///
/// `pub(crate)` and not `pub`: the only caller outside this module's own tests
/// is the command below, and a wider door is one a future main-thread caller
/// walks through.
pub(crate) fn change_theme<R: Runtime>(
    app: &AppHandle<R>,
    data_dir: &Path,
    choice: ThemeChoice,
) -> Result<(), crate::error::Error> {
    // Poisoning is absorbed rather than propagated, exactly as `PREFS_LOCK`
    // does one module over. This mutex guards no value, so there is no
    // corrupted state to refuse access to; what a panic in here could leave
    // behind is a file already written and windows not yet told, and
    // propagating the poison would not repair that — it would only stop every
    // later change from overwriting it.
    let _one_at_a_time = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_choice(data_dir, choice)?;
    // After the persist, before the apply: the one point at which a test can
    // hold this critical section open and watch a second change fail to enter
    // it. Compiled away outside `cfg(test)`.
    after_persist_hook(choice);
    apply_to_windows(app, choice);
    Ok(())
}

/// The boot: the persisted choice, applied to the native chrome before either
/// window is shown. Without this a restart would leave the settings window's
/// frame following the OS while its document follows `data-theme` — two
/// answers to one question. Runs in `.setup` after `manage_state`, because it
/// reads the data dir from `AppState`. No webview is listening yet, so the
/// emit reaches nobody; each window is expected to ask `get_theme` itself when
/// it boots (PR 10b's UI half).
///
/// Under `THEME_LOCK`, like the only other place that applies this choice.
/// This runs in `.setup`, before any webview can send `set_theme`, so the lock
/// is held here for the invariant rather than against a race this path can
/// currently lose: the rule is that nothing decides a choice and applies it
/// outside the lock, and a rule with an exception in it is one a later caller
/// copies.
pub fn apply_persisted<R: Runtime>(app: &AppHandle<R>) {
    let _one_at_a_time = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
/// updates no state field either — it writes the file and applies the
/// choice — so `state` here exists only to reach `data_dir()`. This command
/// re-reads `read_choice` fresh on every call, the same source
/// `apply_persisted` reads once at boot.
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
/// Order: persist → apply → broadcast, and one change at a time — the three
/// steps are `change_theme`'s single critical section, not three that a second
/// command may cut into. A write that fails surfaces as `Error::Prefs` and
/// applies nothing, so a rejection means "nothing changed", which is the
/// sentence the window draws beside it.
#[tauri::command(async)]
pub fn set_theme<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    choice: String,
) -> Result<(), crate::error::Error> {
    change_theme(&app, state.data_dir(), choice_from_str(&choice))
}

/// What a test installs to be called from inside [`change_theme`]'s critical
/// section, between the persist and the apply. It is handed the choice, so a
/// hook can park one change and let another through.
#[cfg(test)]
type Hook = std::sync::Arc<dyn Fn(ThemeChoice) + Send + Sync>;

#[cfg(test)]
static THEME_HOOK: Mutex<Option<Hook>> = Mutex::new(None);

#[cfg(test)]
fn set_test_hook(hook: Option<Hook>) {
    *THEME_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

/// Turn on [`THEME_HOOK`], the same shape `prefs::HOOK_TURN` gives its own
/// slot. The slot is one per binary, so two tests that install a hook cannot
/// overlap: otherwise one's teardown clears the other's hook before that
/// other's change has had a chance to park in it.
#[cfg(test)]
static HOOK_TURN: Mutex<()> = Mutex::new(());

#[cfg(test)]
#[must_use = "the hook is cleared when this is dropped"]
#[allow(dead_code)] // held for its `Drop`, the guard itself is never read
struct HookTurn(std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
impl Drop for HookTurn {
    fn drop(&mut self) {
        set_test_hook(None); // idempotent; covers the panic path
    }
}

#[cfg(test)]
fn take_hook_turn(hook: Hook) -> HookTurn {
    // Poisoning is absorbed: a test that panicked must not also poison the
    // next one's turn.
    let turn = HOOK_TURN.lock().unwrap_or_else(|e| e.into_inner());
    set_test_hook(Some(hook));
    HookTurn(turn)
}

/// Cloned out of its mutex before it is called, so the hook may park for as
/// long as it likes while holding only [`THEME_LOCK`].
#[cfg(test)]
fn after_persist_hook(choice: ThemeChoice) {
    let hook = THEME_HOOK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook {
        hook(choice);
    }
}

#[cfg(not(test))]
#[inline]
fn after_persist_hook(_choice: ThemeChoice) {}

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

    /// External review of PR #38, P2: persist → apply → broadcast has
    /// to be one ordered operation. A is parked between its persist and its
    /// apply, B is started, and when A is released the file, the last
    /// broadcast and the order of the broadcasts must all agree on B — because
    /// B could not get past `THEME_LOCK` until A had applied and broadcast.
    ///
    /// Same shape and the same honest limit as
    /// `prefs::tests::two_hotkey_changes_cannot_interleave`. The PASS
    /// direction rests on the mutex and not on a duration: `THEME_LOCK` is
    /// taken by the first statement of `change_theme` and held past the apply,
    /// so B cannot broadcast before A is released however long this test
    /// waits. The KILL direction is still a bounded wait — without the lock B
    /// is free to run the moment it is spawned, and the 300 ms below is all
    /// the time this test gives it to do so. A scheduler that starves B for
    /// longer defeats the kill; nothing observable from inside `change_theme`
    /// would tell "B has not started" from "B has not been scheduled yet".
    #[test]
    fn a_change_parked_after_its_persist_keeps_the_next_one_out() {
        use std::sync::Arc;
        use std::sync::mpsc::sync_channel;
        use std::time::Duration;
        use tauri::Listener;

        let dir = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();

        // Every broadcast, in the order it was emitted. What is measured:
        // `tests/commands.rs` reads a Rust-side listener with `try_recv`
        // right after the COMMAND returns, so a listener has run by then.
        // This test does not need the stronger "before `emit` returns": A's
        // emit completes before B may enter the lock, so any first-in
        // first-out delivery keeps the order this vector records.
        let broadcasts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let broadcasts = broadcasts.clone();
            app.listen("theme-changed", move |event| {
                broadcasts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(event.payload().to_string());
            });
        }

        let (parked_tx, parked_rx) = sync_channel::<()>(1);
        let (release_tx, release_rx) = sync_channel::<()>(1);
        let release_rx = Mutex::new(release_rx);
        // Only A parks. B has to be free to run the whole way through, or it
        // would be held up by this hook instead of by the lock and the test
        // would pass on the very mutant it names.
        let _turn = take_hook_turn(Arc::new(move |choice: ThemeChoice| {
            if choice != ThemeChoice::Dark {
                return;
            }
            parked_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv()
                .unwrap();
        }));

        let a = {
            let handle = app.handle().clone();
            let data_dir = dir.path().to_path_buf();
            std::thread::spawn(move || change_theme(&handle, &data_dir, ThemeChoice::Dark))
        };
        parked_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the first change never reached the hook after its persist");

        let b = {
            let handle = app.handle().clone();
            let data_dir = dir.path().to_path_buf();
            std::thread::spawn(move || change_theme(&handle, &data_dir, ThemeChoice::Light))
        };

        // The KILL window described above. The release comes before the
        // assertions, which are the calls this test deliberately allows to
        // fail: a panic from one of them with A still parked would strand A
        // holding `THEME_LOCK` for the rest of this test binary.
        std::thread::sleep(Duration::from_millis(300));
        release_tx.send(()).unwrap();

        a.join().unwrap().expect("the first change failed");
        b.join().unwrap().expect("the second change failed");

        let order = broadcasts.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(
            order,
            vec![
                serde_json::to_string("dark").unwrap(),
                serde_json::to_string("light").unwrap(),
            ],
            "the first change must apply and broadcast before the second may start"
        );
        assert_eq!(
            read_choice(dir.path()),
            ThemeChoice::Light,
            "the change that finished last must be the one on disk"
        );
    }
}
