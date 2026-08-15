//! Fixtures shared by the files under `tests/`.
//!
//! `tests/support/mod.rs` — a directory, not `tests/support.rs` — is how a
//! function is shared between integration test binaries without either one
//! owning the other. Cargo turns every file that sits directly inside
//! `tests/` into its own test binary; a *module* nested one directory down is
//! not one of those files.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use mnema_mock_provider::{MockServer, Reply};

/// The extraction worker binary.
///
/// **A deliberate copy of `crates/mnema-ingest/tests/support/mod.rs:61-118`.**
/// `cargo` sets `CARGO_BIN_EXE_*` only for binaries of the package being
/// tested, and the worker belongs to `mnema-extract`, a separate crate; a
/// dev-dependency would not help either, since cargo builds a dependency's
/// library and not its binaries.
///
/// So the path is derived from this test binary's own, and the worker is built
/// before it is named — a clean tree would otherwise fail or use a stale one.
pub fn worker() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        // …/target/<profile>/deps/<binary>-<hash>
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps");
        let target_dir = profile_dir
            .parent()
            .expect("<target>/<profile> sits inside <target>");
        let profile = profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the profile directory is named");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/mnema-eval sits two levels below the workspace root");

        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .args([
                "build",
                "-p",
                "mnema-extract",
                "--bin",
                "mnema-extract-worker",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        // `debug` is what the dev profile is called on disk, and naming it
        // explicitly is an error; every other profile is passed through.
        if profile != "debug" {
            cargo.args(["--profile", profile]);
        }
        let status = cargo.status().expect("cargo runs");
        assert!(
            status.success(),
            "the extraction worker did not build, so this whole file is unanswered \
             rather than passing"
        );

        let path = profile_dir.join(format!(
            "mnema-extract-worker{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            path.exists(),
            "cargo reported success but {} is not there",
            path.display()
        );
        path
    })
}

/// A two-document corpus and two questions. `q-1`'s query names one term
/// from each document: no chunk holds both, so `AllTerms` finds nothing and
/// `AnyTerm` finds both — two chunks, not one.
///
/// `q-2`'s query is the OTHER document's lone term: every rule returns that
/// one chunk, never `q-2`'s own gold — `rank` is `None` while the count is
/// `Some(1)`, telling a count apart from a rank.
///
/// `#[allow(dead_code)]`: not every binary that declares `mod support;`
/// calls this one.
#[allow(dead_code)]
pub fn small_fixture() -> (
    mnema_eval::Corpus,
    mnema_eval::QuestionSet,
    mnema_eval::IndexedCorpus,
) {
    let corpus = mnema_eval::Corpus {
        documents: vec![
            mnema_eval::Document {
                id: "uk/one.md".to_string(),
                language: mnema_eval::Language::Uk,
                text: "Договір складено у двох примірниках. Кожен має однакову силу.".to_string(),
            },
            mnema_eval::Document {
                id: "uk/two.md".to_string(),
                language: mnema_eval::Language::Uk,
                text: "Комісія відклала розгляд заяви до наступного засідання.".to_string(),
            },
        ],
    };
    let questions = mnema_eval::QuestionSet {
        questions: vec![
            mnema_eval::Question {
                id: "q-1".to_string(),
                language: mnema_eval::Language::Uk,
                class: mnema_eval::Class::Literal,
                text: "договір комісія".to_string(),
                document: "uk/one.md".to_string(),
                answers: vec!["Договір складено у двох примірниках.".to_string()],
            },
            mnema_eval::Question {
                id: "q-2".to_string(),
                language: mnema_eval::Language::Uk,
                class: mnema_eval::Class::Literal,
                text: "комісія".to_string(),
                document: "uk/one.md".to_string(),
                answers: vec!["Договір складено у двох примірниках.".to_string()],
            },
        ],
    };
    let indexed = mnema_eval::IndexedCorpus::build(&corpus, worker()).unwrap();
    (corpus, questions, indexed)
}

/// The model name [`small_fixture_with_vectors`] adopts. A label for the
/// mock provider, not a real model — `content_arm` never reaches past it.
#[allow(dead_code)]
pub const FIXTURE_MODEL: &str = "baai/bge-m3";

const FIXTURE_WIDTH: usize = 1024;

/// [`small_fixture`], with every chunk already embedded into an active
/// space under [`FIXTURE_MODEL`] — each chunk on its own axis, so a knn
/// query against any axis has a nearest neighbour to return.
#[allow(dead_code)]
pub fn small_fixture_with_vectors() -> (
    mnema_eval::Corpus,
    mnema_eval::QuestionSet,
    mnema_eval::IndexedCorpus,
) {
    let (corpus, questions, indexed) = small_fixture();
    let db = indexed.db();
    let space = db
        .adopt_embedding_model(
            FIXTURE_MODEL,
            FIXTURE_WIDTH as i64,
            "credential-ref",
            CHUNKER,
        )
        .expect("adopt an embedding model")
        .space_id;
    let mut axis = 0usize;
    for document in &corpus.documents {
        let document_id = indexed
            .document_id(&document.id)
            .expect("look up the document")
            .expect("the document was indexed");
        for (chunk_id, _text) in db.chunks_of_document(&document_id).expect("chunks") {
            db.upsert_vector(space, chunk_id, &axis_vector(axis))
                .expect("vector");
            axis += 1;
        }
    }
    (corpus, questions, indexed)
}

const CHUNKER: &str = "chunker-v1";

fn axis_vector(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0; FIXTURE_WIDTH];
    v[axis] = 1.0;
    v
}

/// A provider that answers any of its first `n` requests with a valid
/// vector, and counts how many it actually received.
#[allow(dead_code)]
pub struct CountingMock {
    server: MockServer,
}

impl CountingMock {
    #[allow(dead_code)]
    pub fn base(&self) -> String {
        self.server.base().to_string()
    }

    /// Requests already answered. Sound only after the calls under test
    /// have returned — see [`MockServer::request_if_any`].
    #[allow(dead_code)]
    pub fn request_count(&self) -> usize {
        std::iter::from_fn(|| self.server.request_if_any()).count()
    }
}

/// Reply `i` carries `axis_vector(i)`, not one vector repeated `n` times —
/// so two different questions' answers can be told apart by which chunk
/// their query vector landed nearest to.
#[allow(dead_code)]
pub fn mock_counting_requests(n: usize) -> CountingMock {
    let replies = (0..n)
        .map(|i| {
            let row: Vec<String> = axis_vector(i).iter().map(|v| v.to_string()).collect();
            Reply::ok(&format!(
                r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
                row.join(",")
            ))
        })
        .collect();
    CountingMock {
        server: MockServer::new(replies),
    }
}
