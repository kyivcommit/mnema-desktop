use std::ffi::{c_char, c_int};
use std::path::Path;
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::Error;
use crate::migrations::apply;

/// How long a statement waits for the write lock before reporting SQLITE_BUSY.
///
/// Five seconds outlasts every write this application makes — each is a single
/// statement, or a batch of them over one document — while still being short
/// enough that a genuinely stuck writer surfaces as an error rather than as a
/// window that has quietly stopped responding.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The outcome of the one registration attempt this process makes. Holds the
/// sqlite return code rather than an `Error` so the result can be replayed to
/// every later caller: `Error` is not `Clone`, a raw code is.
static REGISTER: OnceLock<Result<(), c_int>> = OnceLock::new();

/// The real signature of a SQLite extension entry point, as libsqlite3-sys
/// types the argument to `sqlite3_auto_extension`.
type EntryPoint = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> c_int;

/// Registers sqlite-vec as an auto-extension for the whole process.
///
/// This is process-global and MUST run before any connection is opened —
/// connections opened earlier never see the extension. It is therefore called
/// unconditionally at start-up, including in "no model" mode where no vector
/// table will ever exist; registering without a table costs nothing. G7.0 §5.7.
///
/// Registration happens once; every later call replays the first outcome,
/// failure included.
pub fn register_vector_extension() -> Result<(), Error> {
    let outcome = REGISTER.get_or_init(|| {
        // SAFETY: `sqlite3_vec_init` is a SQLite extension entry point, which C
        // calls as `int (*)(sqlite3*, char**, const sqlite3_api_routines*)`, and
        // that is the type we transmute it to. The transmute is needed only
        // because the sqlite-vec crate under-declares the symbol as
        // `pub fn sqlite3_vec_init();` — an extern declaration narrower than the
        // function it names. Calling it through that declaration would be the
        // unsound path; restoring the true signature before SQLite invokes it is
        // the sound one. Both sides are ordinary function pointers, so the
        // transmute is between two types of identical size and representation.
        let rc = unsafe {
            let entry: EntryPoint = std::mem::transmute::<unsafe extern "C" fn(), EntryPoint>(
                sqlite_vec::sqlite3_vec_init,
            );
            rusqlite::ffi::sqlite3_auto_extension(Some(entry))
        };
        if rc == rusqlite::ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(rc)
        }
    });
    outcome.map_err(Error::ExtensionRegistration)
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn schema_version(&self) -> Result<i64, Error> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }
}

/// Opens (creating if absent) the index database and brings it to the current
/// schema version. Idempotent: opening an up-to-date database migrates nothing.
pub fn open(path: &Path) -> Result<Db, Error> {
    // Registering here rather than trusting the caller to have done it: a
    // connection opened before registration silently lacks vec0 and only fails
    // much later, at the first vector statement. Repeat calls are free.
    //
    // Nothing exercises this line, and that is worth knowing before relying on
    // it. Every test reaches `open` through a helper that has already
    // registered, so replacing this call with a no-op leaves the whole crate
    // green — measured, not assumed. It is belt and braces for a caller that
    // does not exist yet; the paragraph above is an argument, not a result.
    register_vector_extension()?;

    let mut conn = Connection::open(path)?;
    // WAL permits one writer at a time, and from the Tauri shell onwards there
    // are two connections in the process: a multi-hour indexing job, and
    // whatever the window is doing. A writer that arrives second has to wait,
    // or a user adding a folder mid-indexing is told "database is locked" where
    // a short pause was the correct answer.
    //
    // This line changes nothing today and is not there by mistake. SQLite's own
    // default is zero, but rusqlite has never used it: `open_with_flags` calls
    // `sqlite3_busy_timeout(db, 5000)` before handing the connection over
    // (rusqlite 0.40.1, `src/inner_connection.rs:118`) — measured at 5000 ms
    // here on a connection that set nothing. What this makes explicit is that
    // the value is this repository's decision rather than a dependency's
    // default, on a pre-1.0 dependency the workspace policy keeps current.
    //
    // It does not rescue a transaction that reads before it writes: that upgrade
    // fails with SQLITE_BUSY immediately and no timeout applies, which is why
    // writers here take the lock at BEGIN IMMEDIATE.
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    apply(&mut conn)?;
    Ok(Db { conn })
}
