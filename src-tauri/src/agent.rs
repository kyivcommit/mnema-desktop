//! The agent API's handlers: one function per request, answering with the
//! functions the launcher uses (`bridge::retrieve`, `tree::document_view_in`),
//! so retrieval never forks. The socket that carries requests to them lives
//! below; the wire format is `mnema_core::agent_wire`.
//!
//! **The socket exists only while the person has agent access on.** `start`
//! binds, `stop` removes the file and shuts every open stream down, and a
//! connection is registered under the same lock `stop` takes, so none can slip
//! in after it.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mnema_core::agent_wire::{
    Checked, Envelope, MAX_LINE, Reply, VERSION, parse_request, reply_line,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use mnema_core::agent_wire::{ErrKind, Op, PassageId, WireError};
use serde_json::{Value, json};

use crate::bridge::{ContentArmReport, Hit, TextArmReport, read_arms, retrieve};
use crate::error::Error;
use crate::scan_state::{Entry, ScanSnapshot};
use crate::state::AppState;
use crate::tree::{DocumentView, UnavailableReason, freshness_of};

const DEFAULT_K: u32 = 8;
const DEFAULT_CONTEXT: u32 = 2;
const RESCAN_POLL: Duration = Duration::from_millis(500);
const RESCAN_CAP: Duration = Duration::from_secs(600);

fn err(kind: ErrKind, message: impl Into<String>) -> WireError {
    WireError {
        kind,
        message: message.into(),
    }
}

fn internal(e: impl std::fmt::Display) -> WireError {
    err(ErrKind::Internal, e.to_string())
}

/// The `kind` tag `Freshness` serializes with.
fn freshness_kind(freshness: &impl serde::Serialize) -> Result<Value, WireError> {
    let v = serde_json::to_value(freshness).map_err(internal)?;
    Ok(v.get("kind").cloned().unwrap_or(Value::Null))
}

/// The root's absolute path joined with the relative one; `None` when either
/// is unknown.
fn absolute(
    roots: &[(i64, String)],
    root_id: Option<i64>,
    relative: Option<&str>,
) -> Option<String> {
    let (root_id, relative) = (root_id?, relative?);
    let (_, root) = roots.iter().find(|(id, _)| *id == root_id)?;
    Some(Path::new(root).join(relative).display().to_string())
}

fn roots(state: &AppState) -> Result<Vec<(i64, String)>, WireError> {
    state
        .with_index(|db| {
            Ok(db
                .list_watched_roots()?
                .into_iter()
                .map(|r| (r.id, r.absolute_path))
                .collect())
        })
        .map_err(internal)
}

pub fn handle(state: &AppState, op: Op) -> Result<Value, WireError> {
    match op {
        Op::Search { query, k } => search(state, &query, k.unwrap_or(DEFAULT_K)),
        Op::Passage { id, context } => passage(state, &id, context.unwrap_or(DEFAULT_CONTEXT)),
        Op::Status => status(state),
        Op::Rescan { wait } => rescan(state, wait.unwrap_or(false), RESCAN_POLL, RESCAN_CAP),
    }
}

fn search(state: &AppState, query: &str, k: u32) -> Result<Value, WireError> {
    let arms = read_arms(state).map_err(internal)?;
    let embed = state.roles().embed;
    let (hits, text, content, _) =
        retrieve(state, query, arms, i64::from(k), embed).map_err(internal)?;
    let roots = roots(state)?;
    let passages = hits
        .iter()
        .map(|h: &Hit| {
            let freshness =
                freshness_of(state, &h.document_id, h.root_id, h.relative_path.as_deref())
                    .map_err(internal)?;
            Ok(json!({
                "id": PassageId {
                    chunk_id: h.chunk_id,
                    document_id: h.document_id.clone(),
                    ord: h.ord,
                }
                .format(),
                "text": h.text,
                "path": h.relative_path,
                "absolutePath": absolute(&roots, h.root_id, h.relative_path.as_deref()),
                "locator": h.coordinate.render(),
                "section": h.section_title,
                "freshness": freshness_kind(&freshness)?,
            }))
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    // "Nothing here" is a finding only if an arm actually looked; with none run
    // (text off, content not configured or failed) it is "could not search".
    let looked = matches!(text, TextArmReport::Answered { .. })
        || matches!(content, ContentArmReport::Answered { .. });
    Ok(json!({
        "empty": passages.is_empty() && looked,
        "passages": passages,
        "arms": { "text": text, "content": content },
    }))
}

/// Why a passage's document cannot be shown around it, in words the agent can
/// act on. Every variant is named: a new reason must be decided here.
fn unavailable(reason: UnavailableReason) -> WireError {
    match reason {
        UnavailableReason::Rebuilding => err(
            ErrKind::StaleId,
            "the document is being re-indexed — run search_library again",
        ),
        UnavailableReason::TooLarge => err(
            ErrKind::NotFound,
            "the document is too large to show around the passage — use the passage text from search_library",
        ),
        UnavailableReason::NoBlocks => err(
            ErrKind::NotFound,
            "the document has no indexed text any more — run search_library again",
        ),
        UnavailableReason::NoSuchDocument => err(
            ErrKind::NotFound,
            "the document is no longer indexed — run search_library again",
        ),
    }
}

fn passage(state: &AppState, id: &str, context: u32) -> Result<Value, WireError> {
    let id =
        PassageId::parse(id).ok_or_else(|| err(ErrKind::BadRequest, "malformed passage id"))?;
    let citation = state
        .with_index(|db| db.read_snapshot(|db| db.citation(id.chunk_id)))
        .map_err(internal)?
        .ok_or_else(|| err(ErrKind::NotFound, "no such passage — run search again"))?;
    let stale = || {
        err(
            ErrKind::StaleId,
            "the passage changed since it was found; search again",
        )
    };
    if citation.document_id != id.document_id || citation.ord != id.ord {
        return Err(stale());
    }
    let view = crate::tree::document_view_in(
        state,
        &citation.document_id,
        citation.root_id,
        citation.relative_path.as_deref(),
        Some((id.chunk_id, citation.ord, &citation.text)),
    )
    .map_err(internal)?;
    let (blocks, focus_ids, freshness) = match view {
        DocumentView::File {
            blocks,
            focus_block_ids,
            freshness,
            ..
        } => (blocks, focus_block_ids, freshness),
        DocumentView::Unavailable { reason } => return Err(unavailable(reason)),
    };
    let at = |block_id: i64| blocks.iter().position(|b| b.block_id == block_id);
    let (Some(first), Some(last)) = (
        focus_ids.first().and_then(|b| at(*b)),
        focus_ids.last().and_then(|b| at(*b)),
    ) else {
        return Err(stale());
    };
    let context = context as usize;
    let lo = first.saturating_sub(context);
    let hi = (last + context).min(blocks.len() - 1);
    let blocks: Vec<Value> = blocks[lo..=hi]
        .iter()
        .map(|b| {
            json!({
                "text": b.text,
                "kind": b.kind,
                "pageNo": b.page_no,
                "sectionTitle": b.section_title,
                "focus": focus_ids.contains(&b.block_id),
            })
        })
        .collect();
    let roots = roots(state)?;
    Ok(json!({
        "id": id.format(),
        "path": citation.relative_path,
        "absolutePath": absolute(&roots, citation.root_id, citation.relative_path.as_deref()),
        "freshness": freshness_kind(&freshness)?,
        "blocks": blocks,
    }))
}

fn status(state: &AppState) -> Result<Value, WireError> {
    let folders: Vec<Value> = roots(state)?
        .into_iter()
        .map(|(_, path)| json!({ "available": Path::new(&path).is_dir(), "path": path }))
        .collect();
    Ok(json!({
        "scan": serde_json::to_value(state.scan_state()).map_err(internal)?,
        "folders": folders,
    }))
}

/// `poll` and `cap` are parameters so a test need not wait the real ten minutes.
///
/// With `wait`, a scan that is refused because some job holds the slot (an
/// embedding-only resume, say) is retried until the slot frees, and the reply
/// says `started: true` only for a scan this call started and waited for.
pub fn rescan(
    state: &AppState,
    wait: bool,
    poll: Duration,
    cap: Duration,
) -> Result<Value, WireError> {
    let deadline = Instant::now() + cap;
    let started = loop {
        // Only the busy slot is an answer; any other refusal is a fault.
        match crate::scan_job::start(state, Entry::Full) {
            Ok(()) => break true,
            Err(Error::JobAlreadyRunning) => {}
            Err(e) => return Err(internal(e)),
        }
        if !wait {
            return Ok(json!({ "started": false, "running": true }));
        }
        if Instant::now() >= deadline {
            return Ok(json!({ "started": false, "running": true, "report": Value::Null }));
        }
        std::thread::sleep(poll);
    };
    if !wait {
        return Ok(json!({ "started": started, "running": true }));
    }
    loop {
        let scan = state.scan_state();
        match scan.snapshot {
            ScanSnapshot::Running { .. } if Instant::now() < deadline => std::thread::sleep(poll),
            ScanSnapshot::Running { .. } => {
                return Ok(json!({ "started": started, "running": true, "report": Value::Null }));
            }
            ScanSnapshot::Ended { report } => {
                return Ok(json!({
                    "started": started,
                    "running": false,
                    "report": serde_json::to_value(report).map_err(internal)?,
                }));
            }
            ScanSnapshot::Idle => {
                return Ok(json!({ "started": started, "running": false, "report": Value::Null }));
            }
        }
    }
}

const MAX_CONNECTIONS: usize = 8;
/// How long a connection past the limit waits for a slot; under Codex's 60 s per call.
const SLOT_WAIT: Duration = Duration::from_secs(30);
const BUSY: &str = "Mnema is busy with other requests — run this call again in a few seconds";
const LOG_CAP: usize = 50;
/// `sun_path` on macOS holds 104 bytes, the terminator included.
const MAX_SOCKET_PATH: usize = 103;

/// `<data_dir>/agent.sock`.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent.sock")
}

/// One served request, as the settings window lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// RFC 3339, UTC.
    pub at: String,
    pub op: String,
    pub query: Option<String>,
    pub passages: Option<usize>,
    pub client: Option<String>,
}

#[derive(Default)]
struct Inner {
    running: bool,
    /// Bumped by every `start` and `stop`; an accept thread serves only the
    /// generation that spawned it.
    generation: u64,
    next_id: u64,
    streams: HashMap<u64, UnixStream>,
    /// When the current overflow began: one deadline for every connection that
    /// finds all slots taken until one frees.
    full_since: Option<Instant>,
    log: VecDeque<LogEntry>,
}

pub struct AgentServer {
    inner: Mutex<Inner>,
    /// Held by whoever changes the switch across persist + start/stop, which
    /// `start` and `stop` (locking `inner` themselves) cannot do alone.
    toggle: Mutex<()>,
    slot_wait: Duration,
}

impl Default for AgentServer {
    fn default() -> Self {
        Self::with_slot_wait(SLOT_WAIT)
    }
}

impl AgentServer {
    /// A server whose connections past the limit wait `slot_wait` instead of the default.
    pub fn with_slot_wait(slot_wait: Duration) -> Self {
        Self {
            inner: Mutex::default(),
            toggle: Mutex::default(),
            slot_wait,
        }
    }

    pub fn toggle_lock(&self) -> MutexGuard<'_, ()> {
        self.toggle.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the socket is open right now.
    pub fn is_running(&self) -> bool {
        self.lock().running
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Nothing here is left half-written by a panic.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn rfc3339(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn start_if_enabled<R: Runtime>(app: &AppHandle<R>) {
    let data_dir = app.state::<AppState>().data_dir().to_path_buf();
    let server = app.state::<AgentServer>();
    let _toggle = server.toggle_lock();
    if crate::prefs::agent_access(&data_dir) {
        if let Err(e) = start(app) {
            eprintln!("mnema: agent access could not start: {e}");
            // The switch must not say on with nothing behind it.
            if let Err(w) =
                crate::prefs::write_key(&data_dir, "agent_access", serde_json::Value::Bool(false))
            {
                eprintln!("mnema: agent access could not be switched off in the prefs: {w}");
            }
        }
    } else {
        // A crash can leave the file of a session that had access on.
        let _ = std::fs::remove_file(socket_path(&data_dir));
    }
}

pub fn start<R: Runtime>(app: &AppHandle<R>) -> Result<(), Error> {
    let server = app.state::<AgentServer>();
    let path = socket_path(app.state::<AppState>().data_dir());
    // Bound under another name and renamed once it is 0600, so the socket never
    // answers at its public path with a looser mode.
    let tmp = path.with_file_name("agent.tmp");
    let mut inner = server.lock();
    if inner.running {
        return Ok(());
    }
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(Error::AgentSocket(format!(
            "the socket path is {} bytes long and the limit is {MAX_SOCKET_PATH}",
            path.as_os_str().len()
        )));
    }
    // `rename` replaces whatever a crash left at `path`; a leftover `tmp` would
    // make `bind` fail.
    let _ = std::fs::remove_file(&tmp);
    let listener = UnixListener::bind(&tmp).map_err(|e| Error::AgentSocket(e.to_string()))?;
    let published = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = published {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::AgentSocket(e.to_string()));
    }
    // Polled rather than blocked in `accept`, so `stop` need not wake it.
    if let Err(e) = listener.set_nonblocking(true) {
        let _ = std::fs::remove_file(&path);
        return Err(Error::AgentSocket(e.to_string()));
    }
    inner.running = true;
    inner.generation += 1;
    let generation = inner.generation;
    drop(inner);
    let app = app.clone();
    std::thread::spawn(move || accept_loop(app, listener, generation));
    Ok(())
}

pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    let server = app.state::<AgentServer>();
    let path = socket_path(app.state::<AppState>().data_dir());
    let mut inner = server.lock();
    inner.running = false;
    inner.generation += 1;
    let _ = std::fs::remove_file(&path);
    for (_, stream) in inner.streams.drain() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
}

fn refuse(mut stream: UnixStream, kind: ErrKind, message: &str) {
    let line = reply_line(&Envelope {
        v: VERSION,
        reply: Reply::Err(WireError {
            kind,
            message: message.into(),
        }),
    });
    let _ = stream.write_all(line.as_bytes());
}

fn accept_loop<R: Runtime>(app: AppHandle<R>, listener: UnixListener, generation: u64) {
    'accept: loop {
        let server = app.state::<AgentServer>();
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => {
                if server.lock().generation != generation {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
        };
        // An accepted socket inherits the listener's non-blocking mode on macOS.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let mut inner = server.lock();
        if !inner.running || inner.generation != generation {
            return;
        }
        if inner.streams.len() >= MAX_CONNECTIONS {
            // One deadline for the whole overflow: behind connections that never
            // free their slot, only the first caller waits it out.
            let deadline = *inner.full_since.get_or_insert_with(Instant::now) + server.slot_wait;
            drop(inner);
            inner = loop {
                std::thread::sleep(Duration::from_millis(20));
                let inner = server.lock();
                if !inner.running || inner.generation != generation {
                    return;
                }
                if inner.streams.len() < MAX_CONNECTIONS {
                    break inner;
                }
                if Instant::now() >= deadline {
                    drop(inner);
                    refuse(stream, ErrKind::Internal, BUSY);
                    continue 'accept;
                }
            };
        }
        inner.full_since = None;
        let Ok(registered) = stream.try_clone() else {
            drop(inner);
            refuse(
                stream,
                ErrKind::Internal,
                "could not register the connection",
            );
            continue;
        };
        inner.next_id += 1;
        let id = inner.next_id;
        inner.streams.insert(id, registered);
        drop(inner);
        let app = app.clone();
        let spawned = std::thread::Builder::new().spawn(move || {
            let server = app.state::<AgentServer>();
            let _slot = Slot {
                server: &server,
                id,
            };
            serve(&app, &stream);
        });
        if spawned.is_err() {
            server.lock().streams.remove(&id);
        }
    }
}

/// Frees a connection's slot and its registered clone however `serve` ends,
/// a panic included.
struct Slot<'a> {
    server: &'a AgentServer,
    id: u64,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.server.lock().streams.remove(&self.id);
    }
}

fn serve<R: Runtime>(app: &AppHandle<R>, stream: &UnixStream) {
    let mut reader = BufReader::new(stream);
    let mut writer = stream;
    loop {
        let mut buf = Vec::new();
        let read = reader
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf);
        match read {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let ended = buf.last() == Some(&b'\n');
        if ended {
            buf.pop();
        }
        let reply = if buf.len() > MAX_LINE {
            let line = reply_line(&Envelope {
                v: VERSION,
                reply: Reply::Err(WireError {
                    kind: ErrKind::BadRequest,
                    message: format!("request longer than {MAX_LINE} bytes"),
                }),
            });
            let _ = writer.write_all(line.as_bytes());
            return;
        } else {
            answer(app, &buf)
        };
        if writer.write_all(reply.as_bytes()).is_err() || !ended {
            return;
        }
    }
}

fn answer<R: Runtime>(app: &AppHandle<R>, line: &[u8]) -> String {
    let parsed = std::str::from_utf8(line)
        .map_err(|_| WireError {
            kind: ErrKind::BadRequest,
            message: "request is not UTF-8".into(),
        })
        .and_then(|l| parse_request(l.trim_end_matches('\r')));
    let reply = match parsed {
        Err(e) => Reply::Err(e),
        Ok(Checked { client, op }) => {
            let (name, query) = match &op {
                Op::Search { query, .. } => ("search", Some(query.clone())),
                Op::Passage { .. } => ("passage", None),
                Op::Status => ("status", None),
                Op::Rescan { .. } => ("rescan", None),
            };
            let result = handle(&app.state::<AppState>(), op);
            let passages = match (&result, name) {
                (Ok(v), "search") => v["passages"].as_array().map(Vec::len),
                _ => None,
            };
            let server = app.state::<AgentServer>();
            let mut inner = server.lock();
            inner.log.push_front(LogEntry {
                at: rfc3339(SystemTime::now()),
                op: name.into(),
                query,
                passages,
                client,
            });
            inner.log.truncate(LOG_CAP);
            drop(inner);
            match result {
                Ok(v) => Reply::Ok(v),
                Err(e) => Reply::Err(e),
            }
        }
    };
    reply_line(&Envelope { v: VERSION, reply })
}

/// Newest first.
#[tauri::command]
pub fn agent_log(server: tauri::State<'_, AgentServer>) -> Vec<LogEntry> {
    server.lock().log.iter().cloned().collect()
}

/// Where the `mnema` command-line tool sits: beside the application binary.
///
/// Not a Tauri command any more: the Agents section connects the four CLIs
/// itself (`agent_clients.rs`), so nothing in the window asks for a path to
/// paste. [`crate::agent_clients::Env::from_process`] is the one caller left.
pub fn agent_command_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mnema")))
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn a_time_is_written_as_rfc_3339_utc() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        // A leap day, so the month/day arithmetic is exercised past February.
        let t = UNIX_EPOCH + Duration::from_secs(1_709_210_096);
        assert_eq!(rfc3339(t), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn a_dropped_slot_frees_its_connection() {
        let server = AgentServer::default();
        let (a, _b) = UnixStream::pair().unwrap();
        server.lock().streams.insert(7, a);
        assert_eq!(server.lock().streams.len(), 1);
        drop(Slot {
            server: &server,
            id: 7,
        });
        assert_eq!(server.lock().streams.len(), 0);
    }

    #[test]
    fn a_panicking_holder_still_frees_its_slot() {
        let server = AgentServer::default();
        let (a, _b) = UnixStream::pair().unwrap();
        server.lock().streams.insert(7, a);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _slot = Slot {
                server: &server,
                id: 7,
            };
            panic!("handler");
        }));
        assert!(r.is_err());
        assert_eq!(server.lock().streams.len(), 0);
    }

    #[test]
    fn every_unavailable_reason_has_its_own_kind_and_words() {
        let say = |r| {
            let e = unavailable(r);
            (e.kind, e.message)
        };
        assert_eq!(
            say(UnavailableReason::Rebuilding),
            (
                ErrKind::StaleId,
                "the document is being re-indexed — run search_library again".to_string()
            )
        );
        assert_eq!(
            say(UnavailableReason::TooLarge),
            (
                ErrKind::NotFound,
                "the document is too large to show around the passage — use the passage text from search_library".to_string()
            )
        );
        assert_eq!(
            say(UnavailableReason::NoBlocks),
            (
                ErrKind::NotFound,
                "the document has no indexed text any more — run search_library again".to_string()
            )
        );
        assert_eq!(
            say(UnavailableReason::NoSuchDocument),
            (
                ErrKind::NotFound,
                "the document is no longer indexed — run search_library again".to_string()
            )
        );
    }
}
