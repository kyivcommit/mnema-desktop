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
//! (`nfd → case fold → nfc`), builds a comparison key for the mask layer alone
//! — a prefix is matched as typed — and never reaches the path that is written
//! down.
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

    /// Task 4's edge report names the drawn candidate by this string; nothing
    /// in this file's own assertion reads it, which is what the attribute says.
    #[allow(dead_code)]
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
    /// The number this world is a pure function of, carried so that every
    /// failure raised deep inside `materialise` or `walk_all` prints the one
    /// value that reproduces it.
    seed: u64,
    roots: usize,
    files: Vec<Planned>,
    /// The stored rules S. Drawn here and applied by `store_rules`, which is
    /// what needs a world with rules already in force; `materialise`
    /// deliberately writes none of them, so the first walk sees the whole draw.
    prefixes: Vec<(usize, String)>,
    /// The other half of the stored rules S, stored before the before-picture
    /// is taken.
    masks: Vec<String>,
    /// The one mask under test: the preview is asked about it and world B is
    /// then walked with it. Task 4 counts which edge it reached.
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
                // The same directory can be drawn twice for the same root, and
                // the store folds the second write into the first
                // (`Db::add_path_exclusion`'s `ON CONFLICT DO NOTHING`). What
                // the generator claims to have drawn would then not be what any
                // world holds, so the draw is deduplicated here rather than the
                // read-back being loosened.
                if !prefixes.contains(&(root, d.clone())) {
                    prefixes.push((root, d.clone()));
                }
            }
        }
    }
    let stored_pool = ["*.md", "*.txt", "report.txt", "*звіт*", "manifest.*", "*.TXT"];
    let mut masks: Vec<String> = Vec::new();
    for _ in 0..rng.below(4) {
        let m = (*rng.pick(&stored_pool)).to_string();
        if !already_stored(&masks, &m) {
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
            if !already_stored(&masks, &twinned) {
                masks.push(twinned);
            }
        }
        Candidate::CaseTwinOfStored(_) if masks.is_empty() => {
            masks.push("*.txt".to_string());
        }
        _ => {}
    }
    World {
        seed,
        roots,
        files,
        prefixes,
        masks,
        candidate,
    }
}

/// Whether `add_mask` would answer `AlreadyStored` for `pattern` given `masks`.
///
/// The product's own predicate, not a `to_lowercase` of this file's own: the
/// stored pool holds `*.txt` and `*.TXT`, which are ONE rule to the mask layer
/// and would be one row in the store, so a generator that called them two drawn
/// masks would claim a draw no world ever holds. `WalkRules::same_mask_rule` is
/// what `bridge::add_mask` itself asks, so the two cannot drift apart — and it
/// is a predicate over two strings, not an assembled rule set, so the harness
/// still builds no `WalkRules` of its own.
///
/// ⚠️ **The coupling runs one way and can go quiet.** If `same_mask_rule` ever
/// widened — folding whitespace, say, or `?` against `*` — this call would
/// silently shrink every drawn mask set, and the equivalence asserts would
/// still pass because the store would shrink by the same rule. The harness
/// would go on being green while testing a smaller world than it reports. The
/// guard against that is not here: it is `mnema-walk`'s own cases for what the
/// predicate calls equal, and Task 4's edge report, which counts what each
/// seed actually reached rather than what it drew.
fn already_stored(masks: &[String], pattern: &str) -> bool {
    masks
        .iter()
        .any(|stored| mnema_walk::WalkRules::same_mask_rule(stored, pattern))
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
    /// `mask_preview` and `add_mask` are asked through this window, the way the
    /// settings screen asks them.
    webview: WebviewWindow<MockRuntime>,
    root_ids: Vec<i64>,
    /// The seed the world was drawn from, so a failure inside a walk prints the
    /// number that reproduces it rather than only what it saw.
    seed: u64,
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
        seed: world.seed,
        _index: index,
        _roots: roots,
    };
    let removed = walk_all(&built);
    assert_eq!(
        removed, 0,
        "seed {}: the first walk over a fresh index removed something",
        world.seed
    );
    built
}

/// Every walk is accepted on the WHOLE `Ended` event, not on `reason` alone:
/// `Completed` with `complete: false` is a real shape (`job.rs`'s
/// `Ended::complete`) and would silently shrink the `status = 'indexed'`
/// population the oracle reads. Returns the sum of `Ended.removed` over the
/// roots.
/// Every message names the seed and the root's ORDINAL in the world. The
/// `root_id` a message used to carry is a different number in each of a seed's
/// three worlds and names nothing a reader can find in the draw.
fn walk_all(built: &Built) -> u64 {
    let mut removed = 0;
    for (ordinal, &root) in built.root_ids.iter().enumerate() {
        let seed = built.seed;
        let ending = run_walk_and_capture_ending(&built.app, root);
        assert_eq!(
            ending["reason"],
            json!("completed"),
            "seed {seed}, root #{ordinal}: {ending}"
        );
        assert_eq!(
            ending["complete"],
            json!(true),
            "seed {seed}, root #{ordinal}: an incomplete walk: {ending}"
        );
        assert_eq!(
            ending["skipped"],
            json!(0),
            "seed {seed}, root #{ordinal}: {ending}"
        );
        assert_eq!(
            ending["refused"],
            json!(0),
            "seed {seed}, root #{ordinal}: {ending}"
        );
        assert_eq!(
            ending["frozen"],
            json!([]),
            "seed {seed}, root #{ordinal}: {ending}"
        );
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

/// Puts the world's stored rules S into the store the way the product does:
/// prefixes straight through `Db::add_path_exclusion`, which is what
/// `bridge::exclude_subfolder` reaches for, and masks through the real IPC
/// `add_mask`, which is the only thing that validates one.
fn store_rules(built: &Built, world: &World) {
    for (r, prefix) in &world.prefixes {
        let root = built.root_ids[*r];
        built
            .app
            .state::<AppState>()
            .with_index(|db| db.add_path_exclusion(root, prefix).map(|_| ()))
            .expect("storing a path exclusion");
    }
    for mask in &world.masks {
        // `Ok` here means the pattern was ACCEPTED, not that a row was written:
        // `add_mask` answers `AlreadyStored` inside `Ok` for a mask equivalent
        // to one already there. What the store ended up holding is a separate
        // question, asked of `stored_rules` by the caller.
        call(&built.webview, "add_mask", json!({ "pattern": mask }))
            .expect("add_mask rejected a drawn mask outright");
    }
}

/// What the store really holds, read back rather than assumed from the writes
/// having returned `Ok`: the masks, and each root's prefixes under that root's
/// ORDINAL.
fn stored_rules(built: &Built) -> (Vec<String>, Vec<(usize, String)>) {
    let state = built.app.state::<AppState>();
    let masks = state
        .with_index(|db| db.list_masks())
        .expect("listing the stored masks");
    let mut prefixes = Vec::new();
    for (ordinal, &root) in built.root_ids.iter().enumerate() {
        for p in state
            .with_index(|db| db.list_path_exclusions(root))
            .expect("listing the stored prefixes")
        {
            prefixes.push((ordinal, p));
        }
    }
    (masks, prefixes)
}

/// Walks every root under whatever the store holds and answers with SETS:
/// the paths the index no longer lists, and the documents that no longer
/// exist. `Ended.removed` is checked against the set it should describe —
/// a cheap, independent reading of the same fact, not a substitute for it.
fn removed_and_gone(built: &Built) -> (BTreeSet<Key>, BTreeSet<String>) {
    let before = snapshot(built);
    let reported = walk_all(built);
    let after = snapshot(built);
    let removed: BTreeSet<Key> = before.difference(&after).cloned().collect();
    assert_eq!(
        reported,
        removed.len() as u64,
        "seed {}: Ended.removed disagrees with the index listing: {removed:?}",
        built.seed
    );
    let ids: BTreeSet<String> = before.iter().map(|(_, _, id)| id.clone()).collect();
    let gone = ids
        .into_iter()
        .filter(|id| {
            !built
                .app
                .state::<AppState>()
                .with_index(|db| db.document_exists(id))
                .expect("asking whether a document still exists")
        })
        .collect();
    (removed, gone)
}

/// One seed, three worlds: world 0 is asked for the preview, world A is walked
/// under S, world B is walked under S plus the candidate.
fn run_seed(seed: u64, reached: &mut Reached) -> Option<Outcome> {
    // Nothing is counted yet — `Reached` is empty until Task 4 fills it, and
    // this discard is what Task 4's first `reached.states` line replaces.
    let _ = &reached;
    let world = draw_world(seed);

    // Steps 1–3 are the same in all three worlds, and that is CHECKED: the
    // three indexes are equal as sets of (ordinal, path, document id) before
    // any rule is stored, and each store holds exactly S afterwards.
    let w0 = materialise(&world);
    let a = materialise(&world);
    let b = materialise(&world);
    let manifest = snapshot(&w0);
    assert_eq!(
        manifest,
        snapshot(&a),
        "seed {seed}: world A was not indexed like world 0"
    );
    assert_eq!(
        manifest,
        snapshot(&b),
        "seed {seed}: world B was not indexed like world 0"
    );
    // No directory is shared between any two of the three worlds — index or
    // watched root. One in common would make two of the walks the same walk,
    // and the invariant would be comparing a state against itself. The brief's
    // two `assert_ne!` lines checked the indexes of world 0 against A and A
    // against B, which leaves 0 against B and every root unasked.
    let mut dirs: Vec<&Path> = Vec::new();
    for built in [&w0, &a, &b] {
        dirs.push(built._index.path());
        dirs.extend(built._roots.iter().map(|d| d.path()));
    }
    let distinct: BTreeSet<&Path> = dirs.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        dirs.len(),
        "seed {seed}: two of the three worlds share a directory: {dirs:?}"
    );

    store_rules(&w0, &world); // world 0 stores S
    store_rules(&a, &world); // world A stores S
    store_rules(&b, &world); // world B stores S
    let persisted = stored_rules(&w0);
    assert_eq!(
        persisted,
        stored_rules(&a),
        "seed {seed}: world A holds different rules"
    );
    assert_eq!(
        persisted,
        stored_rules(&b),
        "seed {seed}: world B holds different rules"
    );
    // `Db::list_masks` answers in `pattern` order, and the draw is in draw
    // order, so the drawn list is sorted to be compared against it — the
    // question here is which masks the store holds, not in what order it
    // hands them back.
    let mut drawn_masks = world.masks.clone();
    drawn_masks.sort();
    assert_eq!(
        persisted.0, drawn_masks,
        "seed {seed}: the store holds other masks than drawn"
    );
    // The other half of S, and it needs asking separately: three worlds that
    // all failed to store a prefix agree with each other, so the equality above
    // would pass over a rule set nobody has.
    //
    // Compared as typed. `Db::add_path_exclusion` inserts the prefix verbatim —
    // it neither validates nor rewrites it, and says so
    // (`crates/mnema-index/src/write.rs`) — and `list_path_exclusions` orders by
    // `path_prefix` within one root, so the drawn pairs are sorted into the
    // same shape: root ordinal first, then the prefix text.
    let mut drawn_prefixes = world.prefixes.clone();
    drawn_prefixes.sort();
    assert_eq!(
        persisted.1, drawn_prefixes,
        "seed {seed}: the store holds other prefixes than drawn"
    );

    // World 0 — the preview.
    let preview = call(
        &w0.webview,
        "mask_preview",
        json!({ "pattern": world.candidate.pattern() }),
    );

    // Task 4 inserts the refusal branch here.
    // Until it does, an invalid candidate simply ends the seed: this task
    // asserts nothing about a refusal, and `expect`ing a preview on one would
    // fail on a pattern the generator drew as unacceptable on purpose.
    if world.candidate.is_invalid() {
        return None;
    }

    // World A — the walk under S.
    let (removed_a, gone_a) = removed_and_gone(&a);

    // World B — the walk under S + m.
    //
    // ⚠️ **`Ok` does not mean a row was written.** `bridge::add_mask` answers
    // `AlreadyStored` for a candidate equivalent to a stored mask under
    // `WalkRules::same_mask_rule`, and the generator draws exactly that on
    // purpose: `CaseTwinOfStored`, `FormTwinOfStored`, and any `Extension` that
    // collides with the stored pool. Read back which of the two happened, and
    // assert what world B actually holds — otherwise those seeds pass as
    // `0 == 0` under a message that says the walk ran with the candidate.
    let added = call(
        &b.webview,
        "add_mask",
        json!({ "pattern": world.candidate.pattern() }),
    )
    .expect("add_mask refused a candidate the generator drew as valid");
    let kind = added["kind"].as_str().expect("add_mask answered no kind");
    assert!(
        kind == "stored" || kind == "alreadyStored",
        "seed {seed}: add_mask answered an unknown outcome {added}"
    );
    let candidate_was_stored_already = kind == "alreadyStored";
    let (b_masks, b_prefixes) = stored_rules(&b);
    let mut expected_masks = drawn_masks.clone();
    if !candidate_was_stored_already {
        expected_masks.push(world.candidate.pattern().to_string());
        expected_masks.sort();
    }
    assert_eq!(
        b_masks, expected_masks,
        "seed {seed}: after add_mask answered {kind:?}, world B holds masks that are neither S nor S+m"
    );
    assert_eq!(
        b_prefixes, drawn_prefixes,
        "seed {seed}: add_mask changed world B's prefixes"
    );
    let (removed_b, gone_b) = removed_and_gone(&b);

    let preview = preview.expect("mask_preview refused a candidate the generator drew as valid");
    let preview_paths = preview["paths"].as_i64().unwrap();
    let preview_documents = preview["documents"].as_i64().unwrap();
    // Task 4 inserts the `reached.states` lines here, on these names.
    Some(Outcome {
        preview_paths,
        preview_documents,
        removed_a,
        removed_b,
        gone_a,
        gone_b,
        candidate_was_stored_already,
    })
}

/// What one seed's three worlds produced: the preview's two numbers, and the
/// two walks read back as SETS. Sets rather than counts because a mask that
/// resurrects one path while taking another leaves every count untouched.
struct Outcome {
    preview_paths: i64,
    preview_documents: i64,
    removed_a: BTreeSet<Key>,
    removed_b: BTreeSet<Key>,
    gone_a: BTreeSet<String>,
    gone_b: BTreeSet<String>,
    /// Whether `add_mask` answered `AlreadyStored` for the candidate, so world
    /// B walked under S rather than S+m. Not a defect and not a skipped seed:
    /// it is the one state in which the preview MUST say nothing disappears,
    /// which the test asserts. Task 4 counts it as a reached edge.
    candidate_was_stored_already: bool,
}

/// Which drawn shapes the run actually reached. Empty here on purpose: Task 4
/// fills it and asserts on it, and an empty struct is what keeps this task's
/// `run_seed` signature the one that task extends.
#[derive(Default)]
struct Reached {}

/// The whole point: what the preview promises equals what the walk does,
/// as a difference between two real walks over two identical worlds.
#[test]
fn the_preview_and_the_walk_agree_on_every_seed() {
    let runs = setting("MNEMA_FUZZ_RUNS", 12) as u64;
    let base = setting("MNEMA_FUZZ_BASE", 0x5EED_0000) as u64;
    let mut reached = Reached::default();
    for seed in base..base + runs {
        // Drawn twice per seed, here and inside `run_seed`, and the two draws
        // are the same world: `draw_world` is a pure function of the seed, and
        // `Rng` is written out in this file precisely so it stays one.
        let world = draw_world(seed);
        let Some(o) = run_seed(seed, &mut reached) else {
            continue;
        };
        let context = format!(
            "seed {seed}: candidate {:?} over stored masks {:?} and prefixes {:?}",
            world.candidate, world.masks, world.prefixes
        );
        // Sets with inclusion, not counts: a mask that resurrected one path and
        // took another would keep the counts equal.
        let resurrected: Vec<&Key> = o.removed_a.difference(&o.removed_b).collect();
        assert!(
            resurrected.is_empty(),
            "{context}\n  adding the mask KEPT paths the S walk removed: {resurrected:?}"
        );
        let revived: Vec<&String> = o.gone_a.difference(&o.gone_b).collect();
        assert!(
            revived.is_empty(),
            "{context}\n  adding the mask KEPT documents the S walk lost: {revived:?}"
        );
        let only_in_b: Vec<&Key> = o.removed_b.difference(&o.removed_a).collect();
        assert_eq!(
            only_in_b.len() as i64,
            o.preview_paths,
            "{context}\n  the S+m walk removed {} paths the S walk did not: {only_in_b:?}\n  the preview promised {}",
            only_in_b.len(),
            o.preview_paths
        );
        let docs_only_in_b = o.gone_b.difference(&o.gone_a).count() as i64;
        assert_eq!(
            docs_only_in_b, o.preview_documents,
            "{context}\n  documents gone only under S+m: {docs_only_in_b}, preview promised {}",
            o.preview_documents
        );
        // A candidate the store already holds under another spelling is a
        // drawn edge, not an accident: `CaseTwinOfStored` and
        // `FormTwinOfStored` exist to reach it. The two equalities above are
        // satisfied by zero on both sides there, so the claim that makes those
        // seeds mean something is this one — the preview must promise nothing,
        // because nothing is what the walk will do.
        if o.candidate_was_stored_already {
            assert_eq!(
                (o.preview_paths, o.preview_documents),
                (0, 0),
                "{context}\n  the candidate was already stored under another spelling, so nothing can disappear, but the preview promised {} paths and {} documents",
                o.preview_paths,
                o.preview_documents
            );
        }
    }
}

fn setting(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}
