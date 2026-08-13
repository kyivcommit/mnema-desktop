//! What happens when two connections in one process want the write lock.
//!
//! Until the Tauri shell there was only ever one connection, so this could not
//! be observed. The shell opens a second: a multi-hour indexing job writes while
//! the webview also writes, and WAL permits exactly one writer at a time.

use std::sync::mpsc;
use std::time::Duration;

use mnema_index::open;

/// A writer that arrives second must wait for the first, not fail.
///
/// This was expected to be a bug and is not: SQLite's own default `busy_timeout`
/// is zero, but rusqlite sets 5000 ms on every connection it opens, so the
/// behaviour has been right all along by inheritance. The test is here because
/// inheritance is not a decision — `open` now sets the value itself, and this is
/// what would notice if either the line or the wait disappeared.
///
/// It does not assert how long the wait is, only that it is a wait. The exact
/// timeout is a judgement about how long a window may appear frozen, and a test
/// restating the constant would protect nothing.
#[test]
fn a_writer_that_arrives_second_waits_for_the_first_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");

    let first = open(&path).unwrap();
    let second = open(&path).unwrap();

    // The first writer takes the lock and keeps it.
    first.conn().execute_batch("BEGIN IMMEDIATE").unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let outcome = second.insert_watched_root("/tmp/second");
        finished_tx.send(outcome.is_ok()).unwrap();
    });

    // Without this the assertion below would also pass if the thread had never
    // run at all, which is the way this test could pass for the wrong reason.
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the second writer never started");

    assert!(
        finished_rx
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "the second writer returned while the first still held the lock, \
         so it was refused instead of made to wait"
    );

    first.conn().execute_batch("COMMIT").unwrap();

    assert!(
        finished_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the second writer never finished after the lock was released"),
        "the second writer woke up but its insert failed"
    );
    writer.join().unwrap();
}

/// A read that spans several statements sees one state, not a state per
/// statement.
///
/// The settings screen is the caller this exists for: it reads how many chunks
/// are embedded, how many exist and how many were refused, and puts the three in
/// front of a person to be read against each other. Each read on its own is an
/// implicit transaction, and the embedding job commits on its **own** connection
/// — the window's mutex does not reach it — so without [`Db::read_snapshot`] the
/// trio can straddle a committed batch and describe a state the index never
/// held.
///
/// The write below lands **between** two reads inside one snapshot, from a
/// second connection, and is committed before the second read runs. Both halves
/// are asserted: the snapshot does not see it, and a read taken afterwards does
/// — without the second, this test would pass against a database nobody wrote
/// to, which is the way it could be satisfied by nothing at all.
#[test]
fn a_read_snapshot_does_not_see_a_write_another_connection_commits_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");

    let reader = open(&path).unwrap();
    let writer = open(&path).unwrap();
    reader.insert_watched_root("/tmp/first").unwrap();

    let count = |db: &mnema_index::Db| -> i64 {
        db.conn()
            .query_row("SELECT count(*) FROM watched_root", [], |r| r.get(0))
            .unwrap()
    };

    let (before, after) = reader
        .read_snapshot(|db| {
            let before = count(db);
            // A different connection, a real commit, finished before the read
            // below starts. `insert_watched_root` is not in a transaction this
            // closure opened, so nothing here is waiting on anything.
            writer.insert_watched_root("/tmp/second").unwrap();
            Ok((before, count(db)))
        })
        .expect("the snapshot closes cleanly");

    assert_eq!(before, 1, "the fixture wrote one root before the snapshot");
    assert_eq!(
        after, before,
        "a second read inside one snapshot saw a write committed after the first — the three \
         counts on the settings screen can then describe a state the index never held"
    );
    assert_eq!(
        count(&reader),
        2,
        "the write never landed, so the assertion above holds over a database nobody changed"
    );
}
