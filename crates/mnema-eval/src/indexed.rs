use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use mnema_index::{Db, open};
use mnema_ingest::{StopReason, WalkReport, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;

use crate::{Corpus, EvalError};

fn index_error(e: mnema_index::Error) -> EvalError {
    EvalError::Index(e.to_string())
}

/// A corpus laid out on disk and walked into a fresh index.
pub struct IndexedCorpus {
    db: Db,
    root_id: i64,
    report: WalkReport,
    _dir: tempfile::TempDir,
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

        // No `register_vector_extension()` first: `open` calls it itself before
        // it opens the connection (`mnema-index/src/open.rs:94`), and repeat
        // calls are free. That line's own comment says nothing in the
        // repository exercises it, because every other caller reaches `open`
        // through a helper that has already registered — this one does not, so
        // it is the first caller that does.
        let db = open(&dir.path().join("index.sqlite")).map_err(index_error)?;
        let root_str = root
            .to_str()
            .ok_or_else(|| EvalError::Corpus("the temporary path is not UTF-8".to_string()))?;
        let root_id = db.insert_watched_root(root_str).map_err(index_error)?;

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

    /// The `document.id` a corpus path was indexed under, if it was.
    pub fn document_id(&self, relative: &str) -> Result<Option<String>, EvalError> {
        Ok(self
            .db
            .path_entry(self.root_id, relative)
            .map_err(index_error)?
            .map(|entry| entry.document_id))
    }
}
