//! The app-preferences file: one small JSON object beside the index.
//!
//! One file, several owners. The locale was the only key while `locale.rs` also
//! owned the file; from PR 9 the hotkey is written from somewhere else, so the
//! read-modify-write lives here once instead of once per key. Two writers that
//! interleave would each write the object the other had not yet added its key
//! to, and the loser's key would simply not be in the file — hence
//! [`PREFS_LOCK`], which serialises the whole read → merge → write → rename.
//!
//! Nothing here is user-visible text: every string in this module is a JSON key
//! or a file name (`tests/locale_guard.rs`).

use crate::paths;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

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
/// previous backup is overwritten. Without it the write would silently destroy
/// the only copy of a file somebody may have hand-edited, or that a newer
/// version wrote in a shape this one cannot read; that is what-disappears item
/// 2, and the rename is what makes it recoverable. A file that is merely
/// *absent* is not malformed and leaves no backup. If the backup itself cannot
/// be made, the write fails rather than proceeding without it.
pub fn write_key(data_dir: &Path, key: &str, value: serde_json::Value) -> std::io::Result<()> {
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
            std::fs::rename(&path, path.with_extension("json.corrupt"))?;
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
    std::fs::write(&tmp, &body)?;
    // TODO(win): std::fs::rename errors on Windows when `path` already exists,
    // instead of replacing it atomically. This project's CI does not run
    // Windows; the win-pve live pass must confirm. If it fails there, replace
    // this with remove-then-rename or the `ReplaceFileW` API.
    std::fs::rename(&tmp, &path)
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
        assert!(
            b_done_rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "the second writer finished while the first was still inside the critical section"
        );

        release_tx.send(()).unwrap();
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
