//! The app-preferences file: one small JSON object beside the index.
//!
//! One file, several owners. The locale was the only key while `locale.rs` also
//! owned the file; from PR 9 the hotkey is written from somewhere else, so the
//! read-modify-write lives here once instead of once per key. Two writers that
//! interleave would each write the object the other had not yet added its key
//! to, and the loser's key would simply not be in the file — hence
//! [`PREFS_LOCK`], which serialises the whole read → merge → write → rename.
//!
//! Nothing here is user-visible text: every string in this module's PRODUCTION
//! code is a JSON key or a file name. That is the scope `tests/locale_guard.rs`
//! checks — it stops at the first `#[cfg(test)]` — and it is the scope claimed
//! here: the assertion messages in `mod tests` below are ordinary English prose
//! and are not covered by either.

use crate::error::Error;
use crate::paths;
use crate::state::AppState;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri_plugin_global_shortcut::Shortcut;

/// The shortcut a fresh installation gets, and what an absent or unparsable
/// stored value falls back to — exactly as the locale falls back to `Auto`.
pub const DEFAULT_HOTKEY: &str = "Alt+Space";

/// The preferences key the shortcut is stored under.
const HOTKEY_KEY: &str = "hotkey";

/// What the operating system says about the shortcut, as a fact rather than an
/// intention.
///
/// ⚠️ **`Registered` is not a claim of exclusivity, and may not be worded as
/// one.** D128 measured macOS co-registering a shortcut another application
/// already holds: both register successfully and both fire. So this says
/// registered, never "works" or "is yours".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeyStatus {
    Registered,
    Unavailable { reason: String },
}

/// The shortcut the state believes the operating system is holding, and what
/// happened when it was asked to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyState {
    pub shortcut: String,
    pub status: HotkeyStatus,
}

/// Whether this application launches at login, **read back from the operating
/// system** after every change rather than echoed from the request.
///
/// `Unknown` exists because the read itself can fail, and a failed read must
/// not render as "off" — that would show a person a switch in the position
/// opposite to the one the machine is actually in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AutostartState {
    Enabled,
    Disabled,
    Unknown { reason: String },
}

/// Everything the Application section draws.
///
/// `platform` is [`crate::models::Platform`], reused rather than re-derived, so
/// the shortcut glyphs come from the build that was compiled and never from
/// `navigator.userAgent` — that type's own doc records this project measuring a
/// plausible proxy wrong twice, on two platforms.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPrefs {
    pub hotkey: HotkeyState,
    pub autostart: AutostartState,
    pub version: String,
    pub platform: crate::models::Platform,
}

/// Serialises every read-modify-write of the preferences file.
///
/// Process-wide, which is what this guards: two threads of this application
/// writing different keys at once. It says nothing about two *processes* — the
/// temp-file-plus-rename below is what keeps a concurrent reader from seeing a
/// half-written file, and a second instance of the app is prevented elsewhere
/// (`tauri-plugin-single-instance`).
static PREFS_LOCK: Mutex<()> = Mutex::new(());

/// A per-call temp-file extension, so two writers never name the same sibling.
///
/// The single fixed `prefs.json.tmp` this replaces was safe only because one
/// writer existed. With the lock above it would be safe again — but the lock is
/// a process-wide claim and the file name is not, so the uniqueness is kept at
/// the file name too rather than resting on the lock alone.
fn temp_extension() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("json.{}.{n}.tmp", std::process::id())
}

/// Replaces `to` with `from`. Both places this module puts one file where
/// another already is go through here, and that is the point.
///
/// TODO(win): std::fs::rename errors on Windows when `to` already exists,
/// instead of replacing it atomically. This project's CI does not run
/// Windows; the win-pve live pass must confirm. If it fails there, replace
/// this with remove-then-rename or the `ReplaceFileW` API.
///
/// The note above was written for the temp file replacing `prefs.json` and is
/// just as true of `prefs.json` replacing a previous `prefs.json.corrupt`: a
/// file malformed a SECOND time would fail its backup rename, and `write_key`
/// would then refuse every write — neither the locale nor the hotkey could be
/// saved until somebody deleted the file by hand. One helper, so the Windows
/// workaround has one place to be written and the live pass has one site to
/// aim at rather than one of two.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// Everything the preferences file holds, or nothing.
///
/// Every failure — a missing file, a directory that is not there, unreadable
/// bytes, JSON that does not parse, JSON whose top level is not an object —
/// answers an empty map rather than an error, because the first caller is
/// start-up and there is nowhere to report an error to yet. Deliberately NOT
/// symmetric with [`write_key`]: this never repairs, renames or writes
/// anything. Reading preferences must not be able to lose a file.
pub fn read_all(data_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let Ok(bytes) = std::fs::read(paths::prefs_path(data_dir)) else {
        return serde_json::Map::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Writes one key, preserving every other key already in the file.
///
/// Forward-safe: a field a newer version wrote survives a write from this one.
/// The whole read → merge → write-temp → rename runs under [`PREFS_LOCK`], so a
/// second writer's key cannot be computed from an object that is already stale
/// by the time it lands.
///
/// **A file that is present but is not a JSON object is renamed to the sibling
/// `prefs.json.corrupt` before it is replaced**, one generation kept — a
/// previous backup is overwritten **where [`replace_file`] can overwrite**,
/// which today means everywhere but Windows; read its `TODO(win)` before
/// relying on the second half of that sentence. Without it the write would
/// silently destroy
/// the only copy of a file somebody may have hand-edited, or that a newer
/// version wrote in a shape this one cannot read; that is what-disappears item
/// 2, and the rename is what makes it recoverable. A file that is merely
/// *absent* is not malformed and leaves no backup. If the backup itself cannot
/// be made, the write fails rather than proceeding without it.
///
/// **Present is decided from the bytes, not from text.** A file that is not
/// valid UTF-8 is malformed and is kept; reading it as a string would put it on
/// the same arm as a file that does not exist, and destroy it. That is the one
/// state where the two readings differ, and
/// `a_file_of_invalid_utf8_is_backed_up_rather_than_overwritten` is the only
/// test that can tell them apart.
pub fn write_key(data_dir: &Path, key: &str, value: serde_json::Value) -> std::io::Result<()> {
    // Poisoning is absorbed rather than propagated: a writer that panicked
    // must not turn every later write into a panic. Nothing this lock protects
    // is left inconsistent by a panic — it guards no shared value, and what
    // holds the FILE's integrity is the temp-plus-rename below, which either
    // lands whole or does not land.
    let _guard = PREFS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(data_dir)?;
    let path = paths::prefs_path(data_dir);
    // Three states, and they are not two: no readable file at all, a file that
    // is not a JSON object, and an object to merge into. Only the middle one
    // has anything to lose.
    let existing = std::fs::read(&path).ok();
    let parsed = existing
        .as_deref()
        .map(serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>);
    let mut obj = match parsed {
        Some(Ok(object)) => object,
        Some(Err(_)) => {
            replace_file(&path, &path.with_extension("json.corrupt"))?;
            serde_json::Map::new()
        }
        None => serde_json::Map::new(),
    };
    // After the read, before the write: the one point at which a test can hold
    // the critical section open and watch a second writer fail to enter it.
    test_hook(data_dir);
    obj.insert(key.to_string(), value);
    let body = serde_json::to_vec_pretty(&serde_json::Value::Object(obj))?;
    // Atomic: write a sibling temp file, then rename over the target. POSIX
    // rename() is atomic, so a reader never observes a partially-written file.
    let tmp = path.with_extension(temp_extension());
    // Both failures clean up after themselves, and with a per-call unique name
    // that is not optional. The fixed `prefs.json.tmp` this replaced was reused
    // by the next attempt, so at most one stale file could exist; a unique name
    // means nothing ever overwrites the leftover, and every failed write would
    // add another file to the user's data directory permanently. The removal is
    // best-effort — the error worth reporting is the one that got us here, not
    // a failure to tidy up after it.
    if let Err(e) = std::fs::write(&tmp, &body) {
        let _ = std::fs::remove_file(&tmp); // a partial write leaves a file too
        return Err(e);
    }
    if let Err(e) = replace_file(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Every modifier spelling `global-hotkey` accepts, uppercased — the exact
/// arms of the modifier `match` at `global-hotkey-0.8.0/src/hotkey.rs:198-224`,
/// including the four `CmdOrCtrl` aliases it resolves per platform.
///
/// 🔴 **A list rather than "the token before the last `+`"**, because the guard
/// below has to decide whether a string is modifiers *and nothing else*, and
/// the parser's own answer for that is three different errors with three
/// different sentences.
const MODIFIER_SPELLINGS: [&str; 12] = [
    "OPTION",
    "ALT",
    "CONTROL",
    "CTRL",
    "COMMAND",
    "CMD",
    "SUPER",
    "SHIFT",
    "COMMANDORCONTROL",
    "COMMANDORCTRL",
    "CMDORCTRL",
    "CMDORCONTROL",
];

fn is_modifier_token(token: &str) -> bool {
    MODIFIER_SPELLINGS.contains(&token.trim().to_uppercase().as_str())
}

/// Whether the string names no key at all — empty, whitespace, or modifiers
/// only.
///
/// 🔴 **Over the token LIST, not over "does it contain a `+`"**, and the
/// difference is the commoner press of the two. Measured:
///
/// - `""` and `"Alt"` are single tokens, never reach the modifier match
///   (`hotkey.rs:174-178`), and come back as `UnsupportedKey` — whose sentence
///   asks the reader to report the string to `github.com/tauri-apps/muda`.
/// - `"Ctrl+Alt"` — two modifiers held down, which is what a person actually
///   presses — takes the other path entirely and comes back as `InvalidFormat`
///   from `key.ok_or_else(…)` (`hotkey.rs:229`), a third sentence with a third
///   shape.
///
/// A guard written as "the string has no `+`" passes every other fixture in
/// this module and lets that one through.
fn names_no_key(shortcut: &str) -> bool {
    shortcut.trim().is_empty() || shortcut.split('+').all(is_modifier_token)
}

/// Reads the stored shortcut and registers it, once, at start-up.
///
/// 🔴 **A free function rather than a block inside `.setup`, and that is a
/// testability decision.** `tests/support/app.rs`'s `app_in` never runs
/// `.setup`, so a boot written inline there is reachable from no test in
/// `tests/commands.rs`: the stored-garbage fixtures would build a state nothing
/// executes, and the mutant that makes a failed registration fatal could not be
/// killed.
///
/// One parameter, not two. [`AppState`] already holds `data_dir`, and a second
/// source for one fact lets a caller pass a directory the state does not use.
///
/// **A failed registration is not fatal, and this returns no `Result` so that
/// it cannot become fatal by accident.** The plugin builder's
/// `with_shortcut(…)?` is what made a shortcut another application already
/// holds a reason not to start (D128); the product is degraded, not broken —
/// the tray's «Показати пошук» still opens the launcher.
///
/// An absent **or unparsable** stored value falls back to [`DEFAULT_HOTKEY`],
/// exactly as the locale falls back to `Auto`, and for the same reason: this
/// runs before there is anywhere to report a complaint to.
///
/// 🔴 **Calling this from `.setup` does not deadlock, and the reason is worth
/// writing down because the obvious reading says it should.** The real
/// registrar's `register` goes through the plugin's `run_main_thread!`, which
/// posts a task and then blocks on `rx.recv()` — and `.setup` already runs on
/// the main thread, inside Tauri's `Ready` handler. It works because
/// `run_on_main_thread` does not always post: `send_user_message` compares the
/// calling thread against the event loop's and runs the task **inline** when
/// they are the same (`tauri-runtime-wry-2.11.4/src/lib.rs:239-248`). So the
/// boot's registration completes before `run_on_main_thread` returns. A command
/// arriving later is on a worker thread, takes the other branch, and blocks
/// until the event loop services it — which is why all three commands are
/// `(async)`. ⚠️ Nothing here needs a thread of its own, and giving it one
/// would move this call off the main thread and into exactly the wait it does
/// not currently have.
pub fn install_hotkey(state: &AppState) -> HotkeyState {
    let stored = read_all(state.data_dir())
        .get(HOTKEY_KEY)
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let shortcut = match stored {
        Some(s) if !names_no_key(&s) && s.parse::<Shortcut>().is_ok() => s,
        _ => DEFAULT_HOTKEY.to_string(),
    };
    let status = match state.with_shortcuts(|r| r.register(&shortcut)) {
        Ok(()) => HotkeyStatus::Registered,
        Err(reason) => HotkeyStatus::Unavailable { reason },
    };
    let installed = HotkeyState { shortcut, status };
    state.set_hotkey_state(installed.clone());
    installed
}

/// Everything the Application section draws, in one answer.
///
/// Not a `Result`: every state of the operating system is a state this reports
/// — an unavailable shortcut and an unreadable autostart are values here, not
/// rejections. `#[tauri::command(async)]` like its two siblings, because
/// `is_enabled` reaches the operating system.
#[tauri::command(async)]
pub fn app_prefs<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> AppPrefs {
    AppPrefs {
        hotkey: state.hotkey(),
        autostart: read_autostart(&state),
        // The same version the About box shows (`build_app_menu`), which is the
        // one the bundle carries rather than this crate's own constant.
        version: app.package_info().version.to_string(),
        platform: crate::models::Platform::of_this_build(),
    }
}

/// Asks the operating system, and turns a failed read into `Unknown` rather
/// than into "off".
fn read_autostart(state: &AppState) -> AutostartState {
    match state.with_autolaunch(|a| a.is_enabled()) {
        Ok(true) => AutostartState::Enabled,
        Ok(false) => AutostartState::Disabled,
        Err(reason) => AutostartState::Unknown { reason },
    }
}

/// Changes the global shortcut, in the order D-b's transition table fixes.
///
/// 🔴 **`(async)`, and that is correctness rather than performance.**
/// `GlobalShortcut::register` and `unregister` post a closure with
/// `run_on_main_thread` and then block on `rx.recv()`
/// (`tauri-plugin-global-shortcut-2.3.2/src/lib.rs:75-86`). A blocking
/// `#[tauri::command]` runs inline on the main thread, so it would post a task
/// to the very thread it is occupying and wait for ever.
///
/// The steps, and each is a row of that table:
///
/// 1. refuse an empty or modifier-only string **in our own words, before the
///    parser runs** — see [`names_no_key`];
/// 2. parse, and pass the parser's own sentence through, now reserved for the
///    one case it is right about;
/// 3. refuse a shortcut with no modifier at all, again ours: the library
///    accepts `"Space"`, and binding it takes the space bar away system-wide;
/// 4. unregister the current shortcut **only if its status is `Registered`** —
///    unregistering something that was never registered is an error the person
///    did not cause, and from an `Unavailable` start there is no `unregister`
///    call at all;
/// 5. register the new one, and on failure best-effort re-register the old;
/// 6. **update the state FIRST, then persist.**
///
/// 🔴 **Step 6's order is what makes the table consistent rather than a list of
/// exceptions.** Whatever `prefs.json` ends up holding, `HotkeyState` reports
/// the shortcut the operating system is actually holding, which is the only
/// fact the window is entitled to state. A persist failure therefore leaves the
/// new shortcut registered and the old one on disk — the named residual — and
/// is reported as [`Error::Prefs`], which already exists.
#[tauri::command(async)]
pub fn set_hotkey(
    state: tauri::State<'_, AppState>,
    shortcut: String,
) -> Result<HotkeyState, Error> {
    let lang = state.locale().effective;
    if names_no_key(&shortcut) {
        return Err(Error::HotkeyRefused(
            crate::locale::t(lang, crate::locale::Key::HotkeyNeedsAKey).to_string(),
        ));
    }
    let parsed = shortcut
        .parse::<Shortcut>()
        .map_err(|e| Error::HotkeyUnparsable(e.to_string()))?;
    if parsed.mods.is_empty() {
        return Err(Error::HotkeyRefused(
            crate::locale::t(lang, crate::locale::Key::HotkeyNeedsAModifier).to_string(),
        ));
    }

    let current = state.hotkey();
    if current.status == HotkeyStatus::Registered {
        // The OS still holds the old one if this fails, so the state is left
        // saying exactly that and the new shortcut is never attempted.
        state
            .with_shortcuts(|r| r.unregister(&current.shortcut))
            .map_err(Error::HotkeyUnavailable)?;
    }

    if let Err(reason) = state.with_shortcuts(|r| r.register(&shortcut)) {
        // Best-effort restoration. If it also fails, the operating system now
        // holds NOTHING, and a state that went on claiming `Registered` would
        // be a lie the window would draw — so it becomes `Unavailable` carrying
        // the RE-REGISTRATION's sentence, while the reply carries the new
        // shortcut's, which is the refusal the person actually asked about.
        if let Err(restoring) = state.with_shortcuts(|r| r.register(&current.shortcut)) {
            state.set_hotkey_state(HotkeyState {
                shortcut: current.shortcut,
                status: HotkeyStatus::Unavailable { reason: restoring },
            });
        }
        return Err(Error::HotkeyUnavailable(reason));
    }

    let registered = HotkeyState {
        shortcut: shortcut.clone(),
        status: HotkeyStatus::Registered,
    };
    state.set_hotkey_state(registered.clone());
    write_key(
        state.data_dir(),
        HOTKEY_KEY,
        serde_json::Value::String(shortcut),
    )?;
    Ok(registered)
}

/// Turns launch-at-login on or off, and then **asks the operating system what
/// it now says** instead of echoing the request.
///
/// The difference is a real state and not a nicety: an enable that reports
/// success while the machine still has it off is what a person would otherwise
/// see as a switch that stayed where they put it and did nothing.
#[tauri::command(async)]
pub fn set_autostart(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AutostartState, Error> {
    state
        .with_autolaunch(|a| if enabled { a.enable() } else { a.disable() })
        .map_err(Error::Autostart)?;
    Ok(read_autostart(&state))
}

/// What a test installs to be called from inside [`write_key`]'s critical
/// section. It is handed the data directory, so a hook belonging to one test
/// ignores every other test's writes in the same binary.
#[cfg(test)]
type Hook = std::sync::Arc<dyn Fn(&Path) + Send + Sync>;

#[cfg(test)]
static TEST_HOOK: Mutex<Option<Hook>> = Mutex::new(None);

#[cfg(test)]
fn set_test_hook(hook: Option<Hook>) {
    *TEST_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

/// Cloned out of its mutex before it is called, so the hook may park for as
/// long as it likes without holding anything but [`PREFS_LOCK`].
#[cfg(test)]
fn test_hook(data_dir: &Path) {
    let hook = TEST_HOOK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook {
        hook(data_dir);
    }
}

#[cfg(not(test))]
#[inline]
fn test_hook(_data_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::Duration;

    fn corrupt_path(data_dir: &Path) -> PathBuf {
        paths::prefs_path(data_dir).with_extension("json.corrupt")
    }

    /// Every entry in `data_dir`, by file name, sorted — so a test can say what
    /// the directory holds rather than only what one file in it holds.
    fn file_names(data_dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(data_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_key_joins_the_one_already_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(paths::prefs_path(dir.path()), br#"{"locale":"uk"}"#).unwrap();

        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();

        let all = read_all(dir.path());
        assert_eq!(
            all.get("hotkey"),
            Some(&json!("Ctrl+Space")),
            "the key just written is missing: {all:?}"
        );
        assert_eq!(
            all.get("locale"),
            Some(&json!("uk")),
            "the key that was already there was dropped: {all:?}"
        );
    }

    #[test]
    fn the_locale_joins_a_key_it_knows_nothing_about() {
        // The mirror of the test above, and it is the one that catches an
        // implementation special-cased to whichever key its first test used.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(paths::prefs_path(dir.path()), br#"{"hotkey":"Ctrl+Space"}"#).unwrap();

        crate::locale::write_choice(dir.path(), crate::locale::LocaleChoice::Uk).unwrap();

        let all = read_all(dir.path());
        assert_eq!(
            all.get("locale"),
            Some(&json!("uk")),
            "the locale was not written: {all:?}"
        );
        assert_eq!(
            all.get("hotkey"),
            Some(&json!("Ctrl+Space")),
            "the key that was already there was dropped: {all:?}"
        );
    }

    #[test]
    fn with_no_file_there_is_nothing_to_read_and_the_directory_gets_created() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("does-not-exist-yet"); // deliberately NOT created

        assert!(
            read_all(&dir).is_empty(),
            "a directory that does not exist cannot hold preferences"
        );

        write_key(&dir, "hotkey", json!("Ctrl+Space")).unwrap();

        assert!(paths::prefs_path(&dir).exists(), "the file was not created");
        assert_eq!(read_all(&dir).get("hotkey"), Some(&json!("Ctrl+Space")));
    }

    #[test]
    fn a_malformed_file_is_kept_byte_for_byte_beside_the_one_that_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let original: &[u8] = b"{ not json";
        std::fs::write(paths::prefs_path(dir.path()), original).unwrap();

        assert!(
            read_all(dir.path()).is_empty(),
            "a file that is not a JSON object must read as no preferences"
        );

        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();

        let all = read_all(dir.path());
        assert_eq!(all.len(), 1, "the replacement must hold one key: {all:?}");
        assert_eq!(all.get("hotkey"), Some(&json!("Ctrl+Space")));
        assert!(
            corrupt_path(dir.path()).exists(),
            "the malformed file was replaced with no backup beside it"
        );
        assert_eq!(
            std::fs::read(corrupt_path(dir.path())).unwrap(),
            original,
            "the malformed file must survive byte-for-byte in its backup"
        );
    }

    #[test]
    fn a_file_of_invalid_utf8_is_backed_up_rather_than_overwritten() {
        // Not text at all — so it is not "unreadable" in the sense a missing
        // file is. It is a present file this build cannot parse, and it is the
        // only copy of whatever wrote it. Reading the file as BYTES is what puts
        // it on the malformed arm, where it is kept; reading it as a string puts
        // it on the same arm as a file that is not there, and destroys it.
        let dir = tempfile::tempdir().unwrap();
        let original: &[u8] = b"\xff\xfe{";
        std::fs::write(paths::prefs_path(dir.path()), original).unwrap();

        assert!(
            read_all(dir.path()).is_empty(),
            "bytes that are not text must read as no preferences"
        );

        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();

        let all = read_all(dir.path());
        assert_eq!(all.len(), 1, "the replacement must hold one key: {all:?}");
        assert_eq!(all.get("hotkey"), Some(&json!("Ctrl+Space")));
        assert!(
            corrupt_path(dir.path()).exists(),
            "a file that is not text was replaced with no backup beside it"
        );
        assert_eq!(
            std::fs::read(corrupt_path(dir.path())).unwrap(),
            original,
            "the unparseable bytes must survive byte-for-byte in the backup"
        );
    }

    #[test]
    fn each_call_names_a_different_temp_file() {
        // Two writers must never pick the same sibling to write into. The fixed
        // `prefs.json.tmp` this replaced was safe only while one writer existed.
        let first = temp_extension();
        let second = temp_extension();
        assert_ne!(
            first, second,
            "two writers would be writing into the same temp file"
        );
        assert!(
            first.ends_with(".tmp") && second.ends_with(".tmp"),
            "a temp name that is not a temp name: {first} / {second}"
        );
    }

    #[test]
    fn a_write_that_fails_at_the_rename_leaves_no_temp_file_behind() {
        // The one path on which a temp file exists at the moment the write
        // gives up. With a per-call unique name nothing later would ever
        // overwrite it, so every failure would leave another
        // `prefs.json.<pid>.<n>.tmp` in the user's data directory, forever —
        // the fixed name this replaced cleaned up after itself by being reused.
        //
        // `prefs.json` as a DIRECTORY is what makes the rename fail while
        // everything before it succeeds: reading it fails, so no backup is
        // attempted; the data directory is writable, so the temp file really is
        // created; and renaming a file onto a directory cannot succeed.
        let dir = tempfile::tempdir().unwrap();
        let occupied = paths::prefs_path(dir.path());
        std::fs::create_dir(&occupied).unwrap();
        std::fs::write(occupied.join("kept.txt"), b"not the preferences").unwrap();

        let failed = write_key(dir.path(), "hotkey", json!("Ctrl+Space"));

        assert!(
            failed.is_err(),
            "a rename onto a directory cannot have succeeded"
        );
        assert_eq!(
            file_names(dir.path()),
            vec!["prefs.json".to_string()],
            "a failed write left something behind in the data directory"
        );
        assert_eq!(
            std::fs::read(occupied.join("kept.txt")).unwrap(),
            b"not the preferences",
            "a failed write must not disturb what was already there"
        );
    }

    #[test]
    fn a_missing_file_leaves_no_backup_behind() {
        // The mirror of the test above: absent is not malformed, and an
        // implementation that renames unconditionally passes that one alone.
        let dir = tempfile::tempdir().unwrap();

        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();

        assert_eq!(
            file_names(dir.path()),
            vec!["prefs.json".to_string()],
            "a first write must leave the file and nothing else"
        );
    }

    #[test]
    fn only_the_most_recent_malformed_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(paths::prefs_path(dir.path()), b"{ broken once").unwrap();
        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();
        std::fs::write(paths::prefs_path(dir.path()), b"{ broken twice").unwrap();
        write_key(dir.path(), "hotkey", json!("Alt+Space")).unwrap();

        assert_eq!(
            std::fs::read(corrupt_path(dir.path())).unwrap(),
            b"{ broken twice",
            "the backup must hold the file that was actually replaced"
        );
        assert_eq!(
            file_names(dir.path()),
            vec!["prefs.json".to_string(), "prefs.json.corrupt".to_string()],
            "one generation is kept, and no temp file is left behind"
        );
    }

    #[test]
    fn a_second_writer_waits_for_the_first_to_leave_the_critical_section() {
        // The lock's absence made observable. A "two threads, N writes each,
        // then check nothing was lost" test passes on an unlocked
        // implementation whenever the interleaving happens not to occur; this
        // one parks the first writer INSIDE the critical section and asserts,
        // in both directions, that the second cannot get past it.
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let (parked_tx, parked_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let (b_done_tx, b_done_rx) = std::sync::mpsc::channel::<()>();

        // `SyncSender` is `Sync`, a `Receiver` is not — hence the mutex around
        // the one the hook waits on. The path test keeps a `write_key` from
        // any other test in this binary out of the park.
        let release_rx = std::sync::Mutex::new(release_rx);
        let hook_dir = dir_path.clone();
        let hook: Hook = std::sync::Arc::new(move |p: &Path| {
            if p != hook_dir {
                return;
            }
            parked_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        });
        set_test_hook(Some(hook));

        let a_dir = dir_path.clone();
        let a = std::thread::spawn(move || write_key(&a_dir, "hotkey", json!("Ctrl+Space")));
        parked_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the first writer never reached the hook");
        // Cleared while the first writer is still parked in it: the hook was
        // cloned out of the mutex before it was called, and from here on no
        // other test in this binary can be parked by it.
        set_test_hook(None);

        let b_dir = dir_path.clone();
        let b = std::thread::spawn(move || {
            let r = write_key(&b_dir, "locale", json!("uk"));
            b_done_tx.send(()).unwrap();
            r
        });
        // Read, release, THEN assert. A panic between the wait and the release
        // would leave thread A parked with `PREFS_LOCK` held for the life of
        // this test binary, and every later test that writes preferences would
        // block on it — cargo would hit its timeout instead of reporting a
        // failure. Releasing first costs the guard nothing: under a mutant, A
        // still takes the release out of the channel's buffer and finishes, and
        // the failure below is still a failure.
        let b_finished_while_a_was_parked =
            b_done_rx.recv_timeout(Duration::from_millis(500)).is_ok();
        release_tx.send(()).unwrap();
        assert!(
            !b_finished_while_a_was_parked,
            "the second writer finished while the first was still inside the critical section"
        );
        a.join().unwrap().unwrap();
        assert!(
            b_done_rx.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the second writer never finished after the first was released"
        );
        b.join().unwrap().unwrap();

        let all = read_all(&dir_path);
        assert_eq!(
            all.get("hotkey"),
            Some(&json!("Ctrl+Space")),
            "the first writer's key was lost: {all:?}"
        );
        assert_eq!(
            all.get("locale"),
            Some(&json!("uk")),
            "the second writer's key was lost: {all:?}"
        );
    }

    #[test]
    fn a_top_level_array_reads_as_no_preferences() {
        // Valid JSON, wrong shape. The code path this replaces only ever met
        // an object, so nothing said what happened to anything else.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(paths::prefs_path(dir.path()), b"[1,2]").unwrap();

        assert!(read_all(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_write_into_a_read_only_directory_fails_and_keeps_what_was_there() {
        // What makes temp-file-plus-rename load-bearing rather than stylistic,
        // asserted for an arbitrary key. `locale.rs` pins the same property
        // through the locale.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_key(dir.path(), "hotkey", json!("Ctrl+Space")).unwrap();

        // Read-only data dir: creating the sibling temp file must fail.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = write_key(dir.path(), "hotkey", json!("Alt+Space"));
        // Restore perms first, so the tempdir cleans up whatever the asserts do.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            failed.is_err(),
            "a write into a read-only dir must surface an error"
        );
        assert_eq!(
            read_all(dir.path()).get("hotkey"),
            Some(&json!("Ctrl+Space")),
            "a failed write must not change what was persisted"
        );
    }
}
