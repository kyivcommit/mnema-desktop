//! The ask event log (spec 2026-11-05-ask-event-log): one JSON line per step
//! of a question — the translation, every search, every model call, the
//! answer — in `<data_dir>/logs/ask-YYYY-MM-DD.jsonl`. Always on and local
//! only (owner, 2026-11-05). Passage texts and prompts are not written.
//!
//! The turn is held in a thread-local: `ask` runs on one thread from start to
//! answer, so the steps deep inside it log without every signature carrying
//! a logger. A step logged with no turn open writes nothing.

use std::cell::RefCell;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// A model's raw reply is cut to this many bytes, on a character boundary.
pub const RAW_LIMIT: usize = 4096;
/// Log files older than this are removed.
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);
/// All log files together are kept under this: a turn writes a few KB, so
/// only a runaway loop reaches it, and then the oldest days go first.
const CAP_BYTES: u64 = 50 * 1024 * 1024;
/// The size check runs on a day's first write and then every this many.
const CHECK_EVERY: u64 = 200;

static WRITES: AtomicU64 = AtomicU64::new(0);

static TURNS: AtomicU64 = AtomicU64::new(0);

struct Turn {
    dir: PathBuf,
    id: String,
}

thread_local! {
    static CURRENT: RefCell<Option<Turn>> = const { RefCell::new(None) };
}

/// Opens a turn on this thread until the guard drops.
pub fn begin(data_dir: &Path) -> TurnGuard {
    let now = millis();
    let id = format!("{now}-{}", TURNS.fetch_add(1, Ordering::Relaxed));
    CURRENT.with(|c| {
        *c.borrow_mut() = Some(Turn {
            dir: data_dir.join("logs"),
            id,
        });
    });
    TurnGuard
}

pub struct TurnGuard;

impl Drop for TurnGuard {
    fn drop(&mut self) {
        CURRENT.with(|c| *c.borrow_mut() = None);
    }
}

/// Writes one event of the open turn. A failure is reported on stderr and
/// otherwise ignored: the log must never cost a question its answer.
pub fn event(kind: &str, fields: Value) {
    CURRENT.with(|c| {
        let current = c.borrow();
        let Some(turn) = current.as_ref() else { return };
        let mut line = json!({ "ts": millis(), "turn": turn.id, "event": kind });
        if let (Some(line), Value::Object(fields)) = (line.as_object_mut(), fields) {
            line.extend(fields);
        }
        if let Err(e) = write_line(&turn.dir, &line.to_string()) {
            eprintln!("mnema: the ask log was not written: {e}");
        }
    });
}

/// `text` cut to [`RAW_LIMIT`] bytes on a character boundary.
pub fn raw(text: &str) -> String {
    if text.len() <= RAW_LIMIT {
        return text.to_string();
    }
    let mut end = RAW_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn write_line(dir: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(SystemTime::now()));
    let fresh = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    if fresh {
        prune(dir, SystemTime::now());
    }
    if fresh || WRITES.fetch_add(1, Ordering::Relaxed) % CHECK_EVERY == 0 {
        cap(dir, CAP_BYTES, &file_name(SystemTime::now()));
    }
    Ok(())
}

/// `ask-YYYY-MM-DD.jsonl`, by the UTC date.
fn file_name(at: SystemTime) -> String {
    let days = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400;
    let (y, m, d) = civil(days as i64);
    format!("ask-{y:04}-{m:02}-{d:02}.jsonl")
}

/// Removes this log's own files older than [`KEEP`], judged by the date in
/// the name: nothing else in the directory is touched.
fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let oldest = file_name(now.checked_sub(KEEP).unwrap_or(UNIX_EPOCH));
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_log_name(&name) && name < oldest {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Removes this log's oldest files until all of them together fit in
/// `limit` bytes. Today's file (`today`) is never removed.
fn cap(dir: &Path, limit: u64, today: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<(String, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let size = e.metadata().ok()?.len();
            is_log_name(&name).then_some((name, size))
        })
        .collect();
    logs.sort();
    let mut total: u64 = logs.iter().map(|(_, size)| size).sum();
    for (name, size) in logs {
        if total <= limit || name == today {
            break;
        }
        if std::fs::remove_file(dir.join(&name)).is_ok() {
            total -= size;
        }
    }
}

fn is_log_name(name: &str) -> bool {
    let Some(date) = name
        .strip_prefix("ask-")
        .and_then(|n| n.strip_suffix(".jsonl"))
    else {
        return false;
    };
    date.len() == 10
        && date.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's
/// `civil_from_days`).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(dir: &Path) -> Vec<Value> {
        let path = dir.join("logs").join(file_name(SystemTime::now()));
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn events_of_one_turn_share_its_id_and_nothing_is_written_outside_a_turn() {
        let dir = tempfile::tempdir().unwrap();
        event("stray", json!({}));
        {
            let _turn = begin(dir.path());
            event("question", json!({ "query": "солпадеїн" }));
            event("answer", json!({ "kind": "generated" }));
        }
        event("stray", json!({}));
        let got = lines(dir.path());
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0]["event"], "question");
        assert_eq!(got[0]["query"], "солпадеїн");
        assert_eq!(got[0]["turn"], got[1]["turn"]);
        let _turn = begin(dir.path());
        event("question", json!({}));
        assert_ne!(lines(dir.path())[2]["turn"], got[0]["turn"]);
    }

    #[test]
    fn a_raw_reply_is_cut_on_a_character_boundary() {
        let long = "ї".repeat(RAW_LIMIT);
        let cut = raw(&long);
        assert!(cut.len() <= RAW_LIMIT + "…".len(), "{}", cut.len());
        assert!(cut.ends_with('…'));
        assert_eq!(raw("short"), "short");
    }

    #[test]
    fn old_logs_are_pruned_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(20_000 * 86_400);
        let day = |n: u64| file_name(now - Duration::from_secs(n * 86_400));
        // "a.txt" and "ask-1999.jsonl" sort below every log date: only the
        // name check keeps them.
        let foreign = ["notes.txt", "ask-old.jsonl", "a.txt", "ask-1999.jsonl"];
        for name in [day(8), day(6)]
            .into_iter()
            .chain(foreign.map(String::from))
        {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        prune(dir.path(), now);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut want: Vec<String> = foreign.map(String::from).to_vec();
        want.push(day(6));
        want.sort();
        assert_eq!(left, want);
    }

    #[test]
    fn over_the_cap_the_oldest_days_go_first_and_today_stays() {
        let dir = tempfile::tempdir().unwrap();
        let day = |n: u64| file_name(UNIX_EPOCH + Duration::from_secs((20_000 - n) * 86_400));
        for n in [3, 2, 1, 0] {
            std::fs::write(dir.path().join(day(n)), vec![b'x'; 100]).unwrap();
        }
        std::fs::write(dir.path().join("a.txt"), vec![b'x'; 1000]).unwrap();
        cap(dir.path(), 250, &day(0));
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        // 400 bytes of logs, 250 allowed: days 3 and 2 go, 1 and today stay;
        // the foreign file counts for nothing and stays.
        let mut want = vec![day(1), day(0), "a.txt".to_string()];
        want.sort();
        assert_eq!(left, want);
        // Today alone over the cap is kept: the log never deletes the file
        // it is writing.
        cap(dir.path(), 10, &day(0));
        assert!(dir.path().join(day(0)).exists());
        assert!(!dir.path().join(day(1)).exists());
    }

    #[test]
    fn the_file_is_named_by_its_date() {
        assert_eq!(file_name(UNIX_EPOCH), "ask-1970-01-01.jsonl");
        // 2026-11-05 is day 20762 (checked against Python's datetime).
        assert_eq!(
            file_name(UNIX_EPOCH + Duration::from_secs(20_762 * 86_400 + 3600)),
            "ask-2026-11-05.jsonl"
        );
    }

    #[test]
    fn an_unwritable_log_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the directory should be: create_dir_all fails.
        std::fs::write(dir.path().join("logs"), "x").unwrap();
        let _turn = begin(dir.path());
        event("question", json!({}));
    }
}
