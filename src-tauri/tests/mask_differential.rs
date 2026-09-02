//! A differential harness for the file mask: what the preview promises against
//! what the walk actually removes.
//!
//! The product answers "what disappears from the index if this mask is added"
//! twice. `tree::mask_preview` answers it before the press, as a number. The
//! walk answers it afterwards, by deleting `path` rows on the next scan. The
//! two are written independently and nothing has ever compared them, so a
//! preview that says 3 and a walk that removes 4 is a defect neither side's
//! own tests can see.
//!
//! This file draws a folder, a set of stored rules and one candidate mask from
//! a seed, materialises the draw on a real temporary volume, runs the real
//! walk over it, and demands the two answers agree. Deterministic end to end:
//! a run is a pure function of its seed, so a failure prints one number that
//! reproduces it.
//!
//! **Every edge is counted from what was read back**, off the disk or out of
//! the index — never from what the generator meant to draw. A twin the volume
//! folded into one entry, a name it stored in a different normalisation form:
//! both are the regime, and the oracle sees only the survivors. That is why
//! `snapshot` and `listed_files` read, and why the generator's `NameKind` is
//! a label for a report rather than an input to an assertion.
//!
//! **The walk stores the name the filesystem hands it, unnormalised**, so the
//! index-versus-disk equality holds on a form-sensitive volume as well as a
//! folding one — which matters because the NFC/NFD pair is only two files
//! there. `mnema_walk::relative_string` copies each component's `OsStr`
//! verbatim, and the crate's one normalisation, `rules.rs`'s `caseless_form`
//! (`nfd → case fold → nfc`), builds a comparison key for the mask and prefix
//! layers and never reaches the path that is written down.
//!
//! **What the generator never draws**, so that a disagreement is always about
//! the mask and never about one of these:
//!
//! - **`.gitignore`**, or any ignore file: the rules layer reading one is a
//!   separate question with its own tests.
//! - **Symlinks and FIFOs**: the walk names them `NotAFile` and never indexes
//!   them, so neither side of the comparison could see one.
//! - **Worker failures**: a document that failed to extract is not
//!   `status = 'indexed'` and so is invisible to the oracle by construction.
//! - **Ejected or unmounted volumes**: `mnema-ingest`'s randomised suite owns
//!   that failure, and here it would only make an incomplete walk.
//! - **Any change to the tree between the preview and the walk**: the whole
//!   claim under test is that two readings of the *same* state agree.
//! - **`target/`, `node_modules/` or `.git/` as directories**, for two
//!   different mechanisms. `node_modules` and `.git` are in `BUILTIN_DIRS`
//!   and always pruned, so a file under either is in neither the index nor
//!   the oracle's population. `target` is the anchored layer instead
//!   (`rules.rs`'s `ANCHORED_DIRS`, keyed on `Cargo.toml`), which prunes it
//!   only when that marker sits beside it — a `target/` drawn here would be
//!   walked or not depending on a file the generator does not draw. A *file*
//!   named `target` is drawn, and that is a different thing — see
//!   `BUILTIN_NAMES`.
//!
//! The harness never assembles `WalkRules` itself. Every walk goes through
//! `walk_job::start_walk_job`, which is what the window calls, so the rules
//! under test are the ones the product builds.

mod support;

/// Beside `support/mod.rs` rather than inside it — see that file's own header
/// for why a shared module cannot hold something only one binary uses.
#[path = "support/app.rs"]
mod app;

use std::collections::BTreeSet;
use std::path::Path;

use app::{app_in, call, main_webview, run_walk_and_capture_ending};
use mnema_desktop::state::AppState;
use serde_json::json;
use tauri::Manager;
use tauri::WebviewWindow;
use tauri::test::MockRuntime;
use tempfile::TempDir;

/// splitmix64 — the same three lines as
/// `crates/mnema-ingest/tests/randomised.rs`'s own `Rng`, written out rather
/// than pulled in for the reason given there: `rand`'s `StdRng` is allowed to
/// change its stream between releases, and a seed printed by a failing CI run
/// would then not reproduce anything locally.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`. `n` must not be zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const LATIN: &[&str] = &["invoice", "parcel", "route", "manifest", "courier", "depot"];
const CYRILLIC: &[&str] = &["накладна", "посилка", "маршрут", "кур'єр", "склад", "звіт"];
const EXTENSIONS: &[&str] = &["txt", "md"];
/// A FILE called `target` with no `Cargo.toml` beside it: the anchored layer
/// asks `is_dir` before it prunes (`rules.rs`'s `prunes_a_component`), so it
/// leaves this alone, the file is indexed, and both sides must take it only
/// for a literal candidate.
///
/// Not `node_modules`, and not `.git` either: both are in
/// `WalkRules::BUILTIN_DIRS`, which compiles to `!**/<name>` carrying no
/// trailing `/`, so that override layer prunes a *file* of either name as
/// readily as a directory. Neither would ever be indexed, so the oracle could
/// not see one. Being a dotfile is **not** what keeps `.git` out: the walk
/// sets `hidden(false)` (`rules.rs`) because a dotfile in a watched folder is
/// an ordinary document, and `.hidden` is a legal file mask.
const BUILTIN_NAMES: &[&str] = &["target"];
const DIRS: &[&str] = &["Archive", "Work", "old", "Вхідні"];

/// `й` composed and decomposed: the same word in two normalisation forms.
const FORM_NFC: &str = "зві\u{0439}"; // й
const FORM_NFD: &str = "зві\u{0438}\u{0306}"; // и + combining breve

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NameKind {
    Latin,
    Cyrillic,
    CaseTwin,
    FormTwin,
    DirNamedLikeFile,
    BuiltinName,
    CopyWithinRoot,
    CopyAcrossRoots,
}

impl NameKind {
    /// Task 4's edge report names each drawn shape by this string; nothing in
    /// this file's own assertion reads it.
    #[allow(dead_code)]
    fn label(self) -> &'static str {
        match self {
            NameKind::Latin => "latin",
            NameKind::Cyrillic => "cyrillic",
            NameKind::CaseTwin => "case-twin",
            NameKind::FormTwin => "form-twin",
            NameKind::DirNamedLikeFile => "dir-named-like-file",
            NameKind::BuiltinName => "builtin-name",
            NameKind::CopyWithinRoot => "copy-within-root",
            NameKind::CopyAcrossRoots => "copy-across-roots",
        }
    }
}

#[derive(Clone, Debug)]
struct Planned {
    root: usize,
    relative: String,
    bytes: Vec<u8>,
    /// Which shape of the draw this file is, for Task 4's edge report. The
    /// oracle never reads it: what a file turned out to be on the volume is
    /// read back off the volume, not taken from the generator's intent.
    #[allow(dead_code)]
    kind: NameKind,
}

/// The one mask the world is drawn around: the pattern the preview is asked
/// about and the walk is then run with.
#[derive(Clone, Debug)]
enum Candidate {
    Literal(String),
    Extension(String),
    Star,
    Class(String),
    Miss,
    CaseTwinOfStored(String),
    FormTwinOfStored(String),
    Invalid(String),
}

/// Task 3 asks the preview with `pattern` and reports with `label`; `is_invalid`
/// is what tells a refusal apart from a wrong number. None is read here.
#[allow(dead_code)]
impl Candidate {
    fn pattern(&self) -> &str {
        match self {
            Candidate::Star => "*",
            Candidate::Miss => "*.nothing-is-called-this",
            Candidate::Literal(p)
            | Candidate::Extension(p)
            | Candidate::Class(p)
            | Candidate::CaseTwinOfStored(p)
            | Candidate::FormTwinOfStored(p)
            | Candidate::Invalid(p) => p,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Candidate::Literal(_) => "literal",
            Candidate::Extension(_) => "extension",
            Candidate::Star => "star",
            Candidate::Class(_) => "class",
            Candidate::Miss => "miss",
            Candidate::CaseTwinOfStored(_) => "case-twin-of-stored",
            Candidate::FormTwinOfStored(_) => "form-twin-of-stored",
            Candidate::Invalid(_) => "invalid",
        }
    }

    fn is_invalid(&self) -> bool {
        matches!(self, Candidate::Invalid(_))
    }
}

struct World {
    roots: usize,
    files: Vec<Planned>,
    /// The stored rules S. Drawn here and applied by Task 3, which is the
    /// task that needs a world with rules already in force; `materialise`
    /// deliberately writes none of them, so this task's walk sees the whole
    /// draw.
    #[allow(dead_code)]
    prefixes: Vec<(usize, String)>,
    /// The other half of the stored rules S, and Task 3's consumer is the
    /// same: it stores these before it takes the before-picture.
    #[allow(dead_code)]
    masks: Vec<String>,
    /// The one mask under test. Task 3 asks the preview about it and then
    /// runs the walk with it; Task 4 counts which edge it reached.
    #[allow(dead_code)]
    candidate: Candidate,
}

fn draw_world(seed: u64) -> World {
    let mut rng = Rng::new(seed);
    let roots = 1 + rng.below(3);
    let mut files = Vec::new();
    let mut counter = 0u64;
    let mut fresh = |rng: &mut Rng,
                     root: usize,
                     dir: &str,
                     stem: &str,
                     ext: &str,
                     kind: NameKind|
     -> Planned {
        counter += 1;
        let relative = if dir.is_empty() {
            format!("{stem}.{ext}")
        } else {
            format!("{dir}/{stem}.{ext}")
        };
        Planned {
            root,
            relative,
            bytes: format!("{stem} {counter} {}", rng.next()).into_bytes(),
            kind,
        }
    };
    for root in 0..roots {
        let n = 3 + rng.below(6);
        for _ in 0..n {
            let dir = if rng.chance(50) { "" } else { rng.pick(DIRS) };
            let ext = rng.pick(EXTENSIONS);
            let (stem, kind) = if rng.chance(50) {
                (*rng.pick(LATIN), NameKind::Latin)
            } else {
                (*rng.pick(CYRILLIC), NameKind::Cyrillic)
            };
            files.push(fresh(&mut rng, root, dir, stem, ext, kind));
        }
        if rng.chance(60) {
            // Report.TXT beside report.txt — kept apart or folded by the volume.
            files.push(fresh(&mut rng, root, "", "report", "txt", NameKind::CaseTwin));
            files.push(fresh(&mut rng, root, "", "Report", "TXT", NameKind::CaseTwin));
        }
        if rng.chance(60) {
            files.push(fresh(&mut rng, root, "", FORM_NFC, "txt", NameKind::FormTwin));
            files.push(fresh(&mut rng, root, "", FORM_NFD, "txt", NameKind::FormTwin));
        }
        if rng.chance(50) {
            // A DIRECTORY named like a file, with a file inside it.
            files.push(fresh(
                &mut rng,
                root,
                "archive.txt",
                "inside",
                "md",
                NameKind::DirNamedLikeFile,
            ));
        }
        if rng.chance(50) {
            // A FILE named like a built-in rule: `target`, not `target/`.
            let name = rng.pick(BUILTIN_NAMES);
            files.push(Planned {
                root,
                relative: (*name).to_string(),
                bytes: b"a file, not a folder".to_vec(),
                kind: NameKind::BuiltinName,
            });
        }
    }
    // Copies: the same bytes under a second path, within and across roots.
    if !files.is_empty() && rng.chance(70) {
        let src = files[rng.below(files.len())].clone();
        files.push(Planned {
            root: src.root,
            relative: format!("copy-{}", src.relative.replace('/', "-")),
            bytes: src.bytes.clone(),
            kind: NameKind::CopyWithinRoot,
        });
    }
    if roots > 1 && rng.chance(70) {
        let src = files[rng.below(files.len())].clone();
        files.push(Planned {
            root: (src.root + 1) % roots,
            relative: src.relative.clone(),
            bytes: src.bytes.clone(),
            kind: NameKind::CopyAcrossRoots,
        });
    }
    // Stored rules S: 0–2 prefixes per root (existing directories only), 0–3 masks.
    let mut prefixes = Vec::new();
    for root in 0..roots {
        for _ in 0..rng.below(3) {
            let dirs: Vec<String> = files
                .iter()
                .filter(|f| f.root == root)
                .filter_map(|f| f.relative.split_once('/').map(|(d, _)| d.to_string()))
                .collect();
            if let Some(d) = dirs.get(rng.below(dirs.len().max(1))) {
                prefixes.push((root, d.clone()));
            }
        }
    }
    let stored_pool = ["*.md", "*.txt", "report.txt", "*звіт*", "manifest.*", "*.TXT"];
    let mut masks: Vec<String> = Vec::new();
    for _ in 0..rng.below(4) {
        let m = (*rng.pick(&stored_pool)).to_string();
        if !masks.contains(&m) {
            masks.push(m);
        }
    }
    let candidate = match rng.below(8) {
        0 => Candidate::Literal(
            files[rng.below(files.len())]
                .relative
                .rsplit('/')
                .next()
                .unwrap()
                .to_string(),
        ),
        1 => Candidate::Extension(format!("*.{}", rng.pick(EXTENSIONS))),
        2 => Candidate::Star,
        3 => Candidate::Class("[Rr]eport.*".to_string()),
        4 => Candidate::Miss,
        5 => Candidate::CaseTwinOfStored(
            masks
                .first()
                .map(|m| m.to_uppercase())
                .unwrap_or_else(|| "*.TXT".to_string()),
        ),
        6 => Candidate::FormTwinOfStored(format!("*{FORM_NFD}*")),
        _ => Candidate::Invalid("*[Г]*".to_string()),
    };
    // A twin candidate is only an edge when the mask it twins is actually
    // stored: without this, variants 5 and 6 draw a world where nothing
    // differs between the two forms and the edge is claimed but not reached.
    match &candidate {
        Candidate::FormTwinOfStored(_) => {
            let twinned = format!("*{FORM_NFC}*");
            if !masks.contains(&twinned) {
                masks.push(twinned);
            }
        }
        Candidate::CaseTwinOfStored(_) if masks.is_empty() => {
            masks.push("*.txt".to_string());
        }
        _ => {}
    }
    World {
        roots,
        files,
        prefixes,
        masks,
        candidate,
    }
}

/// What the volume does with two names that differ only by case, and with two
/// that differ only by normalisation form.
struct Regime {
    #[allow(dead_code)]
    case_sensitive: bool,
    #[allow(dead_code)]
    form_sensitive: bool,
}

/// Read off the volume, never inferred from a write succeeding — a second
/// write succeeds in both regimes.
///
/// Task 4 is the consumer: it requires a twin edge only where this probe found
/// two entries, because on a folding volume the second name is the first file
/// and there is no twin to reach.
#[allow(dead_code)]
fn probe_regime() -> Regime {
    let case = tempfile::tempdir().unwrap();
    std::fs::write(case.path().join("probe.txt"), b"a").unwrap();
    std::fs::write(case.path().join("Probe.txt"), b"b").unwrap();
    let form = tempfile::tempdir().unwrap();
    std::fs::write(form.path().join(format!("{FORM_NFC}.txt")), b"a").unwrap();
    std::fs::write(form.path().join(format!("{FORM_NFD}.txt")), b"b").unwrap();
    Regime {
        case_sensitive: std::fs::read_dir(case.path()).unwrap().count() == 2,
        form_sensitive: std::fs::read_dir(form.path()).unwrap().count() == 2,
    }
}

struct Built {
    app: tauri::App<MockRuntime>,
    /// Task 3 asks `mask_preview` through this window, the way the settings
    /// screen does.
    #[allow(dead_code)]
    webview: WebviewWindow<MockRuntime>,
    root_ids: Vec<i64>,
    _index: TempDir,
    _roots: Vec<TempDir>,
}

/// The world on a real volume, in a real index: temporary folders, the files
/// written into them, and one walk per root.
///
/// **It writes none of the world's stored rules.** Task 3 applies those, after
/// it has taken the before-picture this walk produces.
fn materialise(world: &World) -> Built {
    let index = tempfile::tempdir().unwrap();
    let app = app_in(index.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");
    let mut roots = Vec::new();
    let mut root_ids = Vec::new();
    for r in 0..world.roots {
        let dir = tempfile::tempdir().unwrap();
        for f in world.files.iter().filter(|f| f.root == r) {
            let path = dir.path().join(&f.relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &f.bytes).unwrap();
        }
        let id = call(
            &webview,
            "add_watched_folder",
            json!({ "path": dir.path().display().to_string() }),
        )
        .expect("add_watched_folder was rejected")
        .as_i64()
        .expect("add_watched_folder did not return an id");
        roots.push(dir);
        root_ids.push(id);
    }
    let built = Built {
        app,
        webview,
        root_ids,
        _index: index,
        _roots: roots,
    };
    let removed = walk_all(&built);
    assert_eq!(removed, 0, "the first walk over a fresh index removed something");
    built
}

/// Every walk is accepted on the WHOLE `Ended` event, not on `reason` alone:
/// `Completed` with `complete: false` is a real shape (`job.rs`'s
/// `Ended::complete`) and would silently shrink the `status = 'indexed'`
/// population the oracle reads. Returns the sum of `Ended.removed` over the
/// roots.
fn walk_all(built: &Built) -> u64 {
    let mut removed = 0;
    for &root in &built.root_ids {
        let ending = run_walk_and_capture_ending(&built.app, root);
        assert_eq!(ending["reason"], json!("completed"), "root {root}: {ending}");
        assert_eq!(
            ending["complete"],
            json!(true),
            "root {root}: an incomplete walk: {ending}"
        );
        assert_eq!(ending["skipped"], json!(0), "root {root}: {ending}");
        assert_eq!(ending["refused"], json!(0), "root {root}: {ending}");
        assert_eq!(ending["frozen"], json!([]), "root {root}: {ending}");
        removed += ending["removed"].as_u64().expect("Ended.removed");
    }
    removed
}

/// One row per indexed path, keyed by the root's ORDINAL in the world (not its
/// `root_id`, which differs between the worlds of different seeds), the
/// relative path, and the document id — the triple is what makes a path unique
/// across roots and copies.
type Key = (usize, String, String);

fn snapshot(built: &Built) -> BTreeSet<Key> {
    let mut out = BTreeSet::new();
    for (ordinal, &root) in built.root_ids.iter().enumerate() {
        let rows = built
            .app
            .state::<AppState>()
            .with_index(|db| db.indexed_files_under_root(root))
            .expect("reading the indexed files under a root");
        for row in rows {
            out.insert((ordinal, row.relative_path, row.document_id));
        }
    }
    out
}

/// The fixture builds the state, measured: every planned file that the
/// filesystem kept is in the index after the first walk, and nothing else is.
#[test]
fn a_drawn_world_is_indexed_as_drawn() {
    for seed in 0..4u64 {
        let world = draw_world(seed);
        let built = materialise(&world);
        let indexed: BTreeSet<(usize, String)> = snapshot(&built)
            .into_iter()
            .map(|(o, p, _)| (o, p))
            .collect();
        let on_disk: BTreeSet<(usize, String)> = listed_files(&built);
        assert_eq!(
            indexed, on_disk,
            "seed {seed}: the index and the disk disagree"
        );
    }
}

/// The disk, read independently of the product: every regular file under every
/// root, as `(root ordinal, relative path with `/`)`.
///
/// The two vectors are built in the same order, so the ordinal `i` names the
/// same root here as it does in `snapshot`.
fn listed_files(built: &Built) -> BTreeSet<(usize, String)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    let mut out = BTreeSet::new();
    for (i, dir) in built._roots.iter().enumerate() {
        let mut names = Vec::new();
        walk(dir.path(), dir.path(), &mut names);
        for n in names {
            out.insert((i, n));
        }
    }
    out
}

