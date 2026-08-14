use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use mnema_index::{Db, open};
use mnema_ingest::{StopReason, WalkReport, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;

use crate::{Corpus, EvalError};

/// A corpus laid out on disk and walked into a fresh index.
///
/// **No `PartialEq`, deliberately, against the crate's general rule.** This
/// holds a live database connection and a temporary directory; equality of
/// two of them is not a question with an answer. `Debug` is written by hand
/// below for the same reason — `Db` itself has none.
pub struct IndexedCorpus {
    db: Db,
    root_id: i64,
    report: WalkReport,
    /// Field order is load-bearing: Rust drops fields in declaration order,
    /// so `db` closes its connection before this directory is deleted.
    /// Windows refuses to delete a file that is still open, which would
    /// turn a reversed order into a leaked temporary directory there.
    _dir: tempfile::TempDir,
}

impl std::fmt::Debug for IndexedCorpus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexedCorpus")
            .field("root_id", &self.root_id)
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl IndexedCorpus {
    pub fn build(corpus: &Corpus, worker: &Path) -> Result<IndexedCorpus, EvalError> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("corpus");
        for document in &corpus.documents {
            let path = root.join(&document.id);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &document.text)?;
        }

        // No `register_vector_extension()` first: `open` calls it itself
        // (`mnema-index/src/open.rs:94`) before it opens the connection, and
        // repeat calls are free.
        let db = open(&dir.path().join("index.sqlite"))?;
        let root_str = root
            .to_str()
            .ok_or_else(|| EvalError::Corpus("the temporary path is not UTF-8".to_string()))?;
        let root_id = db.insert_watched_root(root_str)?;

        let pool = Pool::new(PoolConfig {
            workers: 1,
            batch: 100,
            // Well under the two-minute default: a synthetic text file that
            // takes ten seconds means something is wrong, and a test that fails
            // is better than one that waits. Same reasoning as `slice.rs`.
            timeout: Duration::from_secs(10),
            ..PoolConfig::new(worker)
        })
        .map_err(|e| EvalError::Corpus(format!("the extraction pool did not start: {e}")))?;

        let cancel = AtomicBool::new(false);
        let report = walk_root(
            &pool,
            &db,
            root_id,
            &root,
            &WalkRules::none(),
            &cancel,
            &mut |_progress| {},
        )
        .map_err(|e| EvalError::Corpus(format!("the walk did not finish: {e}")))?;

        // An early stop is a build failure, not a quietly partial corpus: the
        // three counts sum to `found` only when the walk completed, so every
        // number a caller reads from `report` is otherwise short by however
        // much was left unwalked.
        if report.stopped != StopReason::Completed {
            return Err(EvalError::Corpus(format!(
                "the walk stopped early: {:?}",
                report.stopped
            )));
        }

        Ok(IndexedCorpus {
            db,
            root_id,
            report,
            _dir: dir,
        })
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn root_id(&self) -> i64 {
        self.root_id
    }

    pub fn report(&self) -> &WalkReport {
        &self.report
    }

    /// Looks up `relative` — a corpus path, as it appears in `Document::id` —
    /// and answers the document it was indexed under, or `None` if the walk
    /// left no `path` row for it.
    pub fn document_id(&self, relative: &str) -> Result<Option<String>, EvalError> {
        Ok(self
            .db
            .path_entry(self.root_id, relative)?
            .map(|entry| entry.document_id))
    }
}
