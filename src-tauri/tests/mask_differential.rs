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
//!
//! **What the run must have built, and what it cost.** `Reached` counts the
//! shapes each seed actually reached and `required` says which of them a
//! default run owes, so a generator that quietly stopped drawing something
//! fails the run instead of passing it more cheaply. Twelve seeds: 2.6 s
//! inside the test binary, 3.2 s wall for `cargo test` over a warm target.
//! `MNEMA_FUZZ_RUNS=200` takes 34 s and is green, so the invariant is not a
//! property of the default twelve. Measured 2026-09-02 on an Apple M2 Max
//! with `cargo test -p mnema-desktop --test mask_differential` and
//! `MNEMA_FUZZ_RUNS=200 cargo test -p mnema-desktop --test mask_differential`;
//! the Linux CI leg is unmeasured.
//!
//! Two generator weights were changed to make twelve seeds enough, both
//! measured rather than guessed, and neither of them the seed count — the run
//! size is the spec's, and buying coverage with more seeds would hide which
//! draw was too thin:
//!
//! - **The candidate kind is `seed % 8`, not a uniform draw.** Eight kinds
//!   drawn uniformly left `literal` and `invalid` unreached over the default
//!   twelve. Stratifying covers all eight in any eight consecutive seeds, for
//!   any `MNEMA_FUZZ_BASE`, while the world around the candidate stays drawn
//!   from the stream.
//! - **A literal candidate names one half of a purpose-built copy pair** on
//!   every other literal seed. `paths-taken-document-stays` needs a candidate
//!   that takes one of two paths sharing a document while the other survives
//!   S, and the copies the draw already made never satisfied it: they carry
//!   the extension they were copied from, and `*.txt` or `*.md` in S had
//!   already taken the other half.

mod support;

/// Beside `support/mod.rs` rather than inside it — see that file's own header
/// for why a shared module cannot hold something only one binary uses.
#[path = "support/app.rs"]
mod app;

use std::collections::{BTreeMap, BTreeSet};
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
/// A FILE called `target` with no `Cargo.toml` beside it: on the walk side,
/// `filter_entry`'s anchored-dir branch (`rules.rs:854-857`, "Named like an
/// anchored dir but not one") asks `is_dir` before it prunes, and a file
/// isn't one, so the walk leaves this alone and the file is indexed. On the
/// preview side, `prunes_a_component` (`rules.rs`) makes the same `is_dir`
/// check, so both sides must take it only for a literal candidate.
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
    /// The edge report names each drawn shape by this string. It labels a
    /// shape that was found in the index; it never stands in for one.
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
    /// Which shape of the draw this file is, for the edge report. The oracle
    /// never reads it: what a file turned out to be on the volume is read back
    /// off the volume, not taken from the generator's intent. The report reads
    /// it only to NAME a path the index was found to hold.
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

    /// The edge report names the drawn candidate by this string. Three of the
    /// eight are counted from read-back rather than from the draw: the two
    /// twins only once `add_mask` has answered `AlreadyStored` for them, and
    /// `invalid` only once both commands have refused it.
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
    /// then walked with it. `Reached` counts which edge it reached.
    candidate: Candidate,
}

fn draw_world(seed: u64) -> World {
    let mut rng = Rng::new(seed);
    let roots = 1 + rng.below(3);
    let mut files = Vec::new();
    let mut counter = 0u64;
    let mut fresh =
        |rng: &mut Rng, root: usize, dir: &str, stem: &str, ext: &str, kind: NameKind| -> Planned {
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
            files.push(fresh(
                &mut rng,
                root,
                "",
                "report",
                "txt",
                NameKind::CaseTwin,
            ));
            files.push(fresh(
                &mut rng,
                root,
                "",
                "Report",
                "TXT",
                NameKind::CaseTwin,
            ));
        }
        if rng.chance(60) {
            files.push(fresh(
                &mut rng,
                root,
                "",
                FORM_NFC,
                "txt",
                NameKind::FormTwin,
            ));
            files.push(fresh(
                &mut rng,
                root,
                "",
                FORM_NFD,
                "txt",
                NameKind::FormTwin,
            ));
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
    let stored_pool = [
        "*.md",
        "*.txt",
        "report.txt",
        "*звіт*",
        "manifest.*",
        "*.TXT",
    ];
    let mut masks: Vec<String> = Vec::new();
    for _ in 0..rng.below(4) {
        let m = (*rng.pick(&stored_pool)).to_string();
        if !already_stored(&masks, &m) {
            masks.push(m);
        }
    }
    // 🔴 **The candidate kind is stratified by the seed, not drawn from the
    // stream.** Eight kinds drawn uniformly need far more than twelve seeds to
    // show up at least once each: measured over the default run, `literal` and
    // `invalid` never came up at all, and Task 2 measured that sixteen seeds
    // were needed at the uniform weight. The spec fixes the run at twelve
    // seeds, so the fix belongs in the generator. `seed % 8` covers all eight
    // kinds in ANY eight consecutive seeds, whatever `MNEMA_FUZZ_BASE` is set
    // to, and `draw_world` stays a pure function of its seed. The world around
    // the candidate is still drawn from the stream, so a longer run gives each
    // kind many different worlds rather than one repeated.
    let candidate = match (seed % 8) as usize {
        0 => {
            // Two literals, alternating between consecutive literal seeds, so
            // the default run gets one of each.
            //
            // The second is the plain one: the name of a file the world
            // already has. The first exists for a state nothing else reaches
            // — "a path went, the document stayed". That needs a candidate
            // that takes exactly ONE of two paths sharing a document, so an
            // extension or a star is no good (both halves go, and the document
            // with them), and it needs BOTH halves to have survived the stored
            // rules S. Measured over the default run, the copies the draw
            // already makes never satisfied the second condition: they carry
            // the extension they were copied from, and `*.txt` or `*.md` in S
            // had already taken one half.
            //
            // Hence a purpose-built pair under `.log`, which no mask in
            // `stored_pool` can reach — not the two extension masks, not
            // `report.txt`, not `*звіт*`, not `manifest.*`. It is indexed like
            // any other plain text: the walk filters on nothing but the rules,
            // which is why the extension-less `target` file is indexed too.
            if (seed / 8).is_multiple_of(2) {
                let shared = b"one document, two paths".to_vec();
                files.push(Planned {
                    root: 0,
                    relative: "ledger.log".to_string(),
                    bytes: shared.clone(),
                    kind: NameKind::CopyWithinRoot,
                });
                files.push(Planned {
                    root: 0,
                    relative: "copy-ledger.log".to_string(),
                    bytes: shared,
                    kind: NameKind::CopyWithinRoot,
                });
                Candidate::Literal("copy-ledger.log".to_string())
            } else {
                Candidate::Literal(
                    files[rng.below(files.len())]
                        .relative
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_string(),
                )
            }
        }
        1 => Candidate::Extension(format!("*.{}", rng.pick(EXTENSIONS))),
        2 => Candidate::Star,
        3 => Candidate::Class("[Rr]eport.*".to_string()),
        4 => Candidate::Miss,
        5 => {
            // 🔴 The twin must differ from every stored mask by BYTES while
            // folding equal to one of them. `"*.TXT".to_uppercase()` is
            // `*.TXT` itself, so drawing the twin off the first stored mask
            // could hand `add_mask` a pattern byte-identical to a stored one:
            // the answer is still `AlreadyStored`, the seed still passes, and
            // the `case-twin-of-stored` edge is reported as reached without a
            // single letter having changed case (Task 2 review, minor 5).
            //
            // So the base is the first stored mask whose upper case satisfies
            // BOTH halves of what the edge claims, and each half is asked of
            // the authority on it. Different by bytes: the draw is compared
            // against itself. One rule to the store: `already_stored`, which
            // is `WalkRules::same_mask_rule` — the predicate `bridge::add_mask`
            // itself asks. A `to_uppercase() != m` test would answer the second
            // half by proxy and be wrong the day the fold stops covering
            // Cyrillic, since `*ЗВІТ*` differs by bytes either way.
            //
            // If the draw left no such mask — none at all, or only `*.TXT` —
            // one is backfilled. `*.md` is the backfill because it is the one
            // pool entry that cannot fold into `*.TXT`; `*.txt` would be
            // refused as already stored and leave the same hole.
            let base = masks
                .iter()
                .find(|m| {
                    let upper = m.to_uppercase();
                    !masks.contains(&upper) && already_stored(&masks, &upper)
                })
                .cloned();
            let base = base.unwrap_or_else(|| {
                let backfill = "*.md".to_string();
                if !already_stored(&masks, &backfill) {
                    masks.push(backfill.clone());
                }
                backfill
            });
            Candidate::CaseTwinOfStored(base.to_uppercase())
        }
        6 => Candidate::FormTwinOfStored(format!("*{FORM_NFD}*")),
        _ => Candidate::Invalid("*[Г]*".to_string()),
    };
    // A twin candidate is only an edge when the mask it twins is actually
    // stored: without this, variant 6 draws a world where nothing differs
    // between the two forms and the edge is claimed but not reached. Variant 5
    // arranges its own base above, because choosing it and guaranteeing it are
    // the same decision there.
    if let Candidate::FormTwinOfStored(_) = &candidate {
        let twinned = format!("*{FORM_NFC}*");
        if !already_stored(&masks, &twinned) {
            masks.push(twinned);
        }
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
/// predicate calls equal, and `Reached`'s edge report, which counts what each
/// seed actually reached rather than what it drew.
fn already_stored(masks: &[String], pattern: &str) -> bool {
    masks
        .iter()
        .any(|stored| mnema_walk::WalkRules::same_mask_rule(stored, pattern))
}

/// What the volume does with two names that differ only by case, and with two
/// that differ only by normalisation form.
struct Regime {
    case_sensitive: bool,
    form_sensitive: bool,
}

/// Read off the volume, never inferred from a write succeeding — a second
/// write succeeds in both regimes.
///
/// `required` is the consumer: it demands a twin edge only where this probe
/// found two entries, because on a folding volume the second name is the first
/// file and there is no twin to reach. The demand runs both ways — a twin edge
/// reached where the probe said there is none fails the run just as loudly,
/// since the probe and the generator would then disagree about the volume.
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
/// ordinal is the draw's own index and is equal across worlds by
/// construction, whereas `root_id` (`watched_root.id`, an `INTEGER PRIMARY
/// KEY`) would be equal only by accident of insert order.
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
/// `root_id`, which is equal across worlds only by accident of insert order —
/// the ordinal is the draw's own index and is equal by construction), the
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
        let rows = snapshot(&built);
        let indexed: BTreeSet<(usize, String)> =
            rows.iter().map(|(o, p, _)| (*o, p.clone())).collect();
        // Dropping the document id is what makes the comparison below possible
        // at all — the disk has no document ids to compare against — and it is
        // also how two index rows for one path would collapse into one entry
        // and read as agreement. So the collapse is checked for rather than
        // assumed away (Task 2 review, deferred minor 6).
        assert_eq!(
            rows.len(),
            indexed.len(),
            "seed {seed}: the index holds two rows for one path: {rows:?}"
        );
        let on_disk: BTreeSet<(usize, String)> = listed_files(&built);
        // Two empty sets are equal, and a fixture that wrote nothing, or a
        // walk that indexed nothing, would satisfy the equality while
        // measuring no state at all (deferred minor 7). `draw_world` gives
        // every root at least three files, so this is a floor the generator
        // already promises rather than a new demand on it.
        assert!(
            !indexed.is_empty() && !on_disk.is_empty(),
            "seed {seed}: nothing to compare — index {indexed:?}, disk {on_disk:?}"
        );
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

    // The name edges, counted off world 0's INDEX rather than off the draw. A
    // `Planned` whose path is not in `indexed0` never became a file the walk
    // could see — the volume folded it into another name, or stored it in a
    // normalisation form these bytes do not spell — and the shape it was drawn
    // as was therefore not reached, whatever the generator intended.
    //
    // The document id is kept, not dropped: the two copy shapes are not
    // states of one path at all, they are a relation between two paths, and
    // presence alone cannot see them. The two twin shapes are still states of
    // one path each; for them presence of both spellings (the check below)
    // suffices, and the document id is unused.
    let indexed0: BTreeMap<(usize, String), String> = manifest
        .iter()
        .map(|(o, p, id)| ((*o, p.clone()), id.clone()))
        .collect();
    for f in &world.files {
        // The path itself, first: nothing below can be true of a file the
        // index does not hold.
        let Some(document) = indexed0.get(&(f.root, f.relative.clone())) else {
            continue;
        };
        let counted = match f.kind {
            // A twin counts only if BOTH spellings are in the index — one
            // entry means the volume folded them, and that is the regime, not
            // a pair of names to compare a mask against.
            NameKind::CaseTwin | NameKind::FormTwin => world
                .files
                .iter()
                .filter(|g| g.kind == f.kind && g.root == f.root)
                .all(|g| indexed0.contains_key(&(g.root, g.relative.clone()))),
            // A copy is not a path, it is a RELATION: two paths, one document.
            // Presence of this one path says nothing about it — the second
            // write could have landed on the same name, or the index could
            // have given the two files separate documents — so the state is
            // read as the index states it, by asking for another path whose
            // document id is this one's. Same two-element shape as the twins,
            // keyed by document instead of by spelling.
            NameKind::CopyWithinRoot | NameKind::CopyAcrossRoots => {
                let across = f.kind == NameKind::CopyAcrossRoots;
                world.files.iter().any(|g| {
                    g.bytes == f.bytes
                        && (g.root, &g.relative) != (f.root, &f.relative)
                        && (g.root == f.root) != across
                        && indexed0.get(&(g.root, g.relative.clone())) == Some(document)
                })
            }
            _ => true,
        };
        if counted {
            reached.names.insert(f.kind.label());
        }
    }
    // The candidate edge. Five of the eight are what the harness SENT, and
    // sending them is the whole of reaching them. The other three are claims
    // about an answer that has not come back yet: the two twins are only twins
    // once the store has said the two spellings are one rule, and `invalid` is
    // only invalid once both commands have refused it. Those are counted
    // below, where the answer is in hand.
    match &world.candidate {
        Candidate::CaseTwinOfStored(_) | Candidate::FormTwinOfStored(_) | Candidate::Invalid(_) => {
        }
        sent => {
            reached.candidates.insert(sent.label());
        }
    }

    store_rules(&w0, &world); // world 0 stores S
    store_rules(&a, &world); // world A stores S
    store_rules(&b, &world); // world B stores S
    let persisted = stored_rules(&w0);
    // Bound rather than compared in place, because the prefix half of it is
    // read again further down: the `removed-under-a-stored-prefix` edge is counted
    // against the prefixes world A really holds, not against the ones the
    // generator drew.
    let a_rules = stored_rules(&a);
    assert_eq!(
        persisted, a_rules,
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

    // The third branch of the invariant. The first two compare a promise
    // against a walk; this one says that when there is no promise to make,
    // both commands say so in the SAME words. A preview that refused with one
    // sentence and a save that refused with another would send somebody
    // hunting for the difference between two rules that are the same rule.
    //
    // The `Err` side of `call` is the command's `Error` serialised the way the
    // webview receives it — `Serialize for Error` writes its `Display` string
    // (`src-tauri/src/error.rs`) — so this compares the exact sentence a person
    // would read, not two variant names that happen to differ.
    //
    // The seed ends here: an invalid candidate is never stored and never
    // walked, so there is no S+m world to compare against S.
    if world.candidate.is_invalid() {
        let previewed = preview.expect_err("the preview must refuse an invalid candidate");
        let saved = call(
            &w0.webview,
            "add_mask",
            json!({ "pattern": world.candidate.pattern() }),
        )
        .expect_err("the save must refuse it too");
        assert_eq!(
            previewed, saved,
            "seed {seed}: the preview and the save refused {:?} with different sentences",
            world.candidate
        );
        // Counted here rather than at the draw: `invalid` names a candidate
        // the product refused, and until both refusals are in hand the seed
        // has only claimed one.
        reached.candidates.insert(world.candidate.label());
        reached.states.insert("refused");
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

    // The outcome edges. Every one of them is read out of a walk that ran or a
    // preview that answered; none is inferred from the rules that were drawn.
    if !removed_a.is_empty() {
        reached.states.insert("already-taken-by-stored");
    }
    // A path removed under S that sits beneath a prefix world A really holds.
    // This is what proves a drawn prefix was not inert — the line above is
    // satisfied by the masks alone. The NAME is the whole claim: a path was
    // removed and it lay under a stored prefix. It does not say the prefix
    // removed it, because nothing here can tell which rule took a row, and a
    // mask that happens to catch a file under an excluded folder counts the
    // same. The state worth reaching is a stored prefix that matches real
    // indexed paths at all, and that is what this answers.
    let under_a_prefix = removed_a.iter().any(|(ordinal, path, _)| {
        a_rules.1.iter().any(|(o, prefix)| {
            o == ordinal && (path == prefix || path.starts_with(&format!("{prefix}/")))
        })
    });
    if under_a_prefix {
        reached.states.insert("removed-under-a-stored-prefix");
    }
    if gone_b.len() > gone_a.len() {
        reached.states.insert("documents-gone");
    }
    if preview_paths > 0 && preview_documents == 0 {
        reached.states.insert("paths-taken-document-stays");
    }
    if preview_paths == 0 {
        reached.states.insert("nothing-taken");
    }
    if candidate_was_stored_already {
        reached.states.insert("candidate-already-stored");
        // The twin edges, counted from the STORE's answer. `alreadyStored` is
        // `WalkRules::same_mask_rule` saying these two spellings are one rule,
        // which is the only authority on the question — a `to_lowercase` here
        // would be this file inventing a second one.
        reached.candidates.insert(world.candidate.label());
    }

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
    /// which the test asserts. It is also the `candidate-already-stored` edge,
    /// and the only authority on whether the two spellings really are one rule.
    candidate_was_stored_already: bool,
}

/// Which shapes the run actually reached — every one of them counted from what
/// was read back off the disk, out of the index or out of the store, never from
/// what the generator meant to draw.
///
/// This is the answer to "the seeds passed, but did they build anything?". A
/// generator that quietly stopped drawing copies, or a volume that folded every
/// twin away, would leave every assertion in this file green over a world with
/// no interesting state in it, and only this report would say so.
#[derive(Default, Debug)]
struct Reached {
    /// Shapes of NAME found in world 0's index: `NameKind::label`.
    names: BTreeSet<&'static str>,
    /// Shapes of CANDIDATE the seed put to the two commands.
    candidates: BTreeSet<&'static str>,
    /// Shapes of OUTCOME the two walks and the preview produced.
    states: BTreeSet<&'static str>,
}

/// What a default run must have reached by the time it ends.
///
/// The two twin names are the only entries that depend on the machine, and
/// they are decided by [`probe_regime`] rather than by a `cfg!` on the
/// platform: a macOS volume can be case-sensitive if it was formatted that way,
/// and a Linux `tmpfs` is always case- and form-sensitive, so the question is
/// about the volume the temporary directories landed on and nothing else.
///
/// Everything else is required unconditionally, which makes the comparison in
/// `the_preview_and_the_walk_agree_on_every_seed` two-sided: an edge outside
/// this set is a generator that reached a state this function forgot to name,
/// and that is as much a fixture defect as a missing one.
fn required(regime: &Regime) -> Reached {
    let mut want = Reached::default();
    for k in [
        NameKind::Latin,
        NameKind::Cyrillic,
        NameKind::DirNamedLikeFile,
        NameKind::BuiltinName,
        NameKind::CopyWithinRoot,
        NameKind::CopyAcrossRoots,
    ] {
        want.names.insert(k.label());
    }
    if regime.case_sensitive {
        want.names.insert(NameKind::CaseTwin.label());
    }
    if regime.form_sensitive {
        want.names.insert(NameKind::FormTwin.label());
    }
    for c in [
        "literal",
        "extension",
        "star",
        "class",
        "miss",
        "case-twin-of-stored",
        "form-twin-of-stored",
        "invalid",
    ] {
        want.candidates.insert(c);
    }
    for s in [
        // The S walk removed something at all.
        "already-taken-by-stored",
        // …and something it removed sat under a stored PREFIX. Separate from
        // the line above because a prefix can be inert while masks do all the
        // removing: `Вхідні` typed in one normalisation form against a volume
        // that stored the folder in the other would match nothing, and the
        // combined state would still be reached by the masks (Task 3
        // re-review).
        "removed-under-a-stored-prefix",
        // The candidate cost a document its last path.
        "documents-gone",
        // …and the opposite: a path went, the document stayed, because a copy
        // of it survived under another path.
        "paths-taken-document-stays",
        // The preview promised nothing would go.
        "nothing-taken",
        // The store already held the candidate under another spelling.
        "candidate-already-stored",
        // Both commands refused the candidate with one sentence.
        "refused",
    ] {
        want.states.insert(s);
    }
    want
}

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

    // Every seed above could pass over a world with nothing in it. This is the
    // claim that the run built the states it says it tests, and it is checked
    // in both directions: an edge the run never reached is a generator that
    // stopped drawing something, and an edge it reached that `required` did not
    // name is a generator that drew something nobody is watching. Both are
    // defects in this file, never in the product — which is why the message
    // says so before anyone starts reading `tree.rs`.
    let want = required(&probe_regime());
    let missing_names: Vec<_> = want.names.difference(&reached.names).collect();
    let missing_candidates: Vec<_> = want.candidates.difference(&reached.candidates).collect();
    let missing_states: Vec<_> = want.states.difference(&reached.states).collect();
    let unexpected_names: Vec<_> = reached.names.difference(&want.names).collect();
    let unexpected_candidates: Vec<_> = reached.candidates.difference(&want.candidates).collect();
    let unexpected_states: Vec<_> = reached.states.difference(&want.states).collect();
    assert!(
        missing_names.is_empty()
            && missing_candidates.is_empty()
            && missing_states.is_empty()
            && unexpected_names.is_empty()
            && unexpected_candidates.is_empty()
            && unexpected_states.is_empty(),
        "the default run over seeds {base}..{} never built some state — the fixture, not the \
         product, is wrong:\n  names missing {missing_names:?}, unexpected {unexpected_names:?}\
         \n  candidates missing {missing_candidates:?}, unexpected {unexpected_candidates:?}\
         \n  states missing {missing_states:?}, unexpected {unexpected_states:?}\
         \n  reached: {reached:?}",
        base + runs
    );
}

/// The third branch of the invariant: a candidate the rules refuse is refused
/// by the preview and by the save with the SAME sentence, and no walk runs.
///
/// The randomised test reaches this too, on the seeds that draw
/// `Candidate::Invalid`. This one is here because the branch should not be
/// reachable only by luck: it names the pattern, and it fails on the day the
/// generator stops drawing invalid candidates rather than going quiet.
#[test]
fn an_invalid_candidate_is_refused_alike_by_preview_and_save() {
    let world = World {
        seed: 0,
        roots: 1,
        files: vec![Planned {
            root: 0,
            relative: "звіт.txt".into(),
            bytes: b"x".to_vec(),
            kind: NameKind::Cyrillic,
        }],
        prefixes: vec![],
        masks: vec![],
        candidate: Candidate::Invalid("*[Г]*".into()),
    };
    let built = materialise(&world);
    let before = snapshot(&built);
    let document_id = before
        .iter()
        .find(|(ordinal, path, _)| *ordinal == 0 && path == "звіт.txt")
        .map(|(_, _, id)| id.clone())
        .expect("the fixture's one file must be indexed before the refusal");
    let expected: BTreeSet<Key> = [(0, "звіт.txt".to_string(), document_id)]
        .into_iter()
        .collect();
    assert_eq!(before, expected, "a refusal must not touch the index");
    let previewed = call(
        &built.webview,
        "mask_preview",
        json!({ "pattern": "*[Г]*" }),
    )
    .expect_err("a class with a non-ASCII letter must be refused by the preview");
    let saved = call(&built.webview, "add_mask", json!({ "pattern": "*[Г]*" }))
        .expect_err("…and by the save");
    assert_eq!(
        previewed, saved,
        "the preview and the save must refuse with one sentence"
    );
    assert_eq!(
        snapshot(&built),
        expected,
        "a refusal must not touch the index"
    );
}

/// The blank string is the one named exception: not malformed, previewed as two
/// zeros, refused on save as `Error::BlankMask`. The generator never draws it,
/// so this test is where it lives.
///
/// The two commands disagreeing here is the point rather than a defect: a
/// preview of the empty mask is a truthful "this removes nothing", while
/// storing it would put a rule that removes nothing into the list, where it
/// reads as protection.
#[test]
fn the_blank_candidate_previews_as_zeros_and_is_refused_on_save() {
    let world = World {
        seed: 0,
        roots: 1,
        files: vec![Planned {
            root: 0,
            relative: "звіт.txt".into(),
            bytes: b"x".to_vec(),
            kind: NameKind::Cyrillic,
        }],
        prefixes: vec![],
        masks: vec![],
        // Unused below: `materialise` never reads `World::candidate`, and this
        // test sends the blank string directly rather than through it. Kept
        // only because the struct requires a value.
        candidate: Candidate::Miss,
    };
    let built = materialise(&world);
    let before = snapshot(&built);
    let document_id = before
        .iter()
        .find(|(ordinal, path, _)| *ordinal == 0 && path == "звіт.txt")
        .map(|(_, _, id)| id.clone())
        .expect("the fixture's one file must be indexed before the calls");
    let expected: BTreeSet<Key> = [(0, "звіт.txt".to_string(), document_id)]
        .into_iter()
        .collect();
    assert_eq!(
        before, expected,
        "neither the preview nor the refused save may touch the index"
    );
    assert_eq!(
        call(&built.webview, "mask_preview", json!({ "pattern": "" }))
            .expect("the blank mask is not malformed"),
        json!({ "paths": 0, "documents": 0 })
    );
    call(&built.webview, "add_mask", json!({ "pattern": "" }))
        .expect_err("storing a blank mask must be refused");
    assert_eq!(
        snapshot(&built),
        expected,
        "neither the preview nor the refused save may touch the index"
    );
}

/// "Validated before the index is read" needs a negative control. On an OPEN
/// index the two orders of the two checks give the same sentence, so an open
/// index can never tell them apart. With the index CLOSED they separate: a
/// malformed mask must still get the validation sentence, and a valid one the
/// index refusal, and that is the only way to see which check ran first.
#[test]
fn a_malformed_candidate_is_refused_before_the_index_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    // No `open_index` on purpose.
    let malformed = call(&webview, "mask_preview", json!({ "pattern": "*[Г]*" }))
        .expect_err("a malformed mask must be refused with no index open");
    let valid = call(&webview, "mask_preview", json!({ "pattern": "*.txt" }))
        .expect_err("a valid mask has no index to count against");
    assert_ne!(
        malformed, valid,
        "the malformed mask must be refused by validation, the valid one by the closed index"
    );
    assert!(
        malformed
            .as_str()
            .expect("the refusal crosses the IPC as a string")
            .contains("[Г]"),
        "the sentence must be about the mask typed: {malformed}"
    );
    let closed = call(&webview, "add_mask", json!({ "pattern": "*.txt" }))
        .expect_err("saving needs an index too");
    assert_eq!(
        valid, closed,
        "the valid mask meets the same closed-index refusal on both commands"
    );
}

fn setting(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}
