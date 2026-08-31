use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};
use std::path::Path;
use thiserror::Error;

/// The rule layers, in the order they are applied. All of them are live at
/// every walk rather than one-off actions: a rule that newly excludes an
/// already-indexed file removes it on the next walk, which is what makes
/// "I excluded that folder" mean "it is no longer findable" (§5).
///
/// §5's three — the built-in list, the in-tree `.gitignore` stack, and the
/// user's path prefixes — plus a fourth this crate grew for PR 8b: the user's
/// **file masks**. The fourth is not a variant of the third and does not live
/// beside it. A prefix is a **path**, comes from disk, and rides the shared
/// `Override`; a mask is a **glob over one file name**, is typed, is global
/// rather than per-root, and has to be file-only — which the `Override` cannot
/// express. [`MaskLayer`] carries the measurement behind every clause of that
/// sentence.
#[derive(Debug, Default, Clone)]
pub struct WalkRules {
    builtin: bool,
    gitignore: bool,
    /// Always exactly what the caller typed once a `WalkRules` exists —
    /// never rewritten, only accepted or refused whole. `WalkRules::new` is
    /// the only public way to populate this (besides `default`/`none`,
    /// which leave it empty), and `validate_prefix` is a whitelist: every
    /// stored entry is, by construction, a `/`-joined sequence of one or
    /// more non-empty components, none of them `.` or `..`, none containing
    /// a backslash or a control character, none beginning or ending with
    /// whitespace, and — for the first component only — not shaped like a
    /// Windows drive letter or a home-directory shorthand (review fix round
    /// 3, Critical finding). `builder()` relies on that: it does not
    /// re-check any of it before turning a prefix into a pattern.
    user_prefixes: Vec<String>,
    /// The user's file masks, already validated and compiled — see
    /// [`MaskLayer`] for what this layer is and, more importantly, for the
    /// failure modes it does and does not share with the `Override` the two
    /// fields above feed.
    masks: MaskLayer,
}

/// A user-supplied exclusion prefix that cannot become a rule at all — not
/// "not yet combined with the rest," but wrong on its own. Returned from
/// `WalkRules::new` so a caller with a save dialog in front of the user can
/// refuse the rule right there, which is the only place a human can fix it.
/// A prefix that compiles alone but only fails once combined with the rest
/// of the rule set is a different failure, with nowhere left to report it —
/// see `Walked::rules_applied` (review fix round 1, Critical finding).
///
/// Round 1 caught glob metacharacters; round 2 caught a backslash, trailing
/// whitespace, `./` and an absolute path — and round 3 found leading
/// whitespace, a repeated `./` (`././Photos`, since a single strip only
/// runs once), `~/Photos`, and `Photos//sub`, none of them on round 2's
/// list. Enumerating bad forms one round at a time does not converge, so
/// `validate_prefix` is a whitelist now: it describes what a well-formed
/// prefix IS, component by component, and refuses everything that is not
/// that shape, rather than blacklisting specific ways to be wrong. Every
/// one of these still shares the same failure it was always about: `new`
/// used to return `Ok`, the named folder was not excluded, and nothing
/// anywhere said so. Today that costs an index holding what the user asked
/// it not to; once the embedding stage exists it costs more, because D29
/// ships v1 with no local models and every indexed document will then go to
/// a third-party provider.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RulesError {
    #[error("exclusion rule {prefix:?} could not be compiled: {reason}")]
    InvalidPrefix { prefix: String, reason: String },
    /// A leading `/`, a trailing `/`, or a doubled `/` all produce a
    /// zero-length path component (`Photos//sub` splits to `["Photos", "",
    /// "sub"]`) — refused rather than silently collapsed, since collapsing
    /// `//` to `/` is itself a normalisation this round stopped doing.
    #[error(
        "exclusion rule {prefix:?} has an empty path component — remove the leading, trailing, \
         or doubled `/`"
    )]
    EmptyComponent { prefix: String },
    /// `.` and `..` are not folder names, they are navigation — and `..`
    /// specifically is how a prefix would climb out of the watched root.
    /// `././Photos` reaches this on its first component; round 2's single
    /// `strip_prefix("./")` only ever ran once, so it missed this repeated
    /// form.
    #[error(
        "exclusion rule {prefix:?} has a `{component}` path component — name the folder \
         directly, not `.` or `..`"
    )]
    DotComponent { prefix: String, component: String },
    /// `gitignore.rs` compiles every glob with `backslash_escape(true)`
    /// unconditionally, so a `\` anywhere in the pattern — not only a
    /// trailing one, which `globset::escape` cannot help with because it
    /// does not touch backslash at all — is read as an escape character.
    /// `a\bee` compiles to the literal `abee`: the named folder survives,
    /// and an unrelated `abee/` is excluded in its place. No rewrite is
    /// unambiguous, so this is refused rather than normalised.
    #[error("exclusion rule {prefix:?} cannot contain a backslash — name the folder without one")]
    ContainsBackslash { prefix: String },
    /// A control character cannot be part of a real folder name that a
    /// person typed on purpose.
    #[error("exclusion rule {prefix:?} contains a control character, which cannot name a folder")]
    ContainsControlCharacter { prefix: String },
    /// `add_line` silently trims TRAILING whitespace unless the line ends
    /// in `\ ` (an escaped space, which nothing here ever emits) — but
    /// LEADING whitespace is a different, equally silent failure: it
    /// compiles and matches only a folder that literally has that leading
    /// space, which is almost never what a stray keystroke meant. Either
    /// edge is refused (review fix round 3, Important finding — round 2
    /// checked only the trailing edge).
    #[error(
        "exclusion rule {prefix:?} has a path component that begins or ends with whitespace — \
         remove it"
    )]
    SurroundingWhitespace { prefix: String },
    /// A single ASCII letter followed by `:` as the first component — the
    /// shape of a Windows drive letter. `Path::is_absolute()` cannot be
    /// used to catch this the way round 2 caught a Unix absolute path: on
    /// Windows, `Path::new("/private").is_absolute()` is FALSE (Windows
    /// needs a drive or a UNC prefix — confirmed against the doc comment on
    /// `Path::is_absolute` itself, `library/std/src/path.rs:2536-2539` in
    /// the toolchain this crate builds with: "`c:\windows` is absolute,
    /// while `c:temp` and `\temp` are not"), so the round 2 check refused
    /// `/private` on macOS while silently accepting it — compiling to
    /// `!//private`, matching nothing — on Windows. Checking the shape of
    /// the first component instead of asking the platform is what makes
    /// this refuse the same inputs everywhere (review fix round 3, Critical
    /// finding).
    #[error(
        "exclusion rule {prefix:?} starts with what looks like a Windows drive letter — \
         exclusion rules are relative to the watched folder, not an absolute path"
    )]
    DriveLetterPrefix { prefix: String },
    /// `~` as the first component is a shell convention for a home
    /// directory that this crate does not expand — taken literally, it
    /// names an ordinary folder called `~`, which essentially never exists
    /// under a watched root, so the rule silently excludes nothing. Not
    /// one of the five properties an ordinary well-formed component has to
    /// have, but the same failure shape as the drive-letter case: a token
    /// that looks like it anchors somewhere outside the watched root,
    /// refused for the same reason.
    #[error(
        "exclusion rule {prefix:?} starts with `~` — exclusion rules are relative to the \
         watched folder, not a shorthand for a home directory"
    )]
    HomeDirectoryShorthand { prefix: String },
    /// A mask names a **file**, and a file's name never contains `/`. Refused
    /// rather than reinterpreted as a path, because a path is what a *prefix*
    /// expresses and a prefix comes from disk (D-a), where the byte-equality
    /// question is already settled; confining a typed rule to one path
    /// component is what keeps it from having to be a correct path as well.
    ///
    /// It cannot be left to the compile probe: measured on the pinned
    /// `globset 0.4.19`, `logs/*.tmp` compiles without error and then matches
    /// nothing, because [`MaskLayer`] asks about a name. The cost, named:
    /// `logs/*.tmp` cannot be expressed in v1, and a person who wants it
    /// excludes the folder instead.
    #[error(
        "file mask {mask:?} cannot contain `/` — a mask names a file, and a folder is excluded \
         with an exclusion rule instead"
    )]
    MaskContainsSlash { mask: String },
    /// The same platform-dependent trap [`RulesError::ContainsBackslash`]
    /// refuses for a prefix, arriving by a different door.
    /// `globset`'s `backslash_escape` is on where `\` is not a path separator
    /// and off where it is, so `a\bee.txt` matches `abee.txt` here and
    /// `a\bee.txt` on Windows. No rewrite is unambiguous, so it is refused.
    #[error("file mask {mask:?} cannot contain a backslash — name the file without one")]
    MaskContainsBackslash { mask: String },
    /// A control character cannot be part of a real file name that a person
    /// typed on purpose.
    #[error("file mask {mask:?} contains a control character, which cannot name a file")]
    MaskContainsControlCharacter { mask: String },
    /// `globset` does **not** trim what the `.gitignore` line parser trims:
    /// measured, `"*.pdf "` matches `"report.pdf "` and not `"report.pdf"`. So
    /// a stray keystroke compiles into a mask for a name almost nobody has, and
    /// nothing says so — the under-exclusion direction. Refused at both edges,
    /// exactly as [`RulesError::SurroundingWhitespace`] refuses it for a
    /// prefix.
    #[error("file mask {mask:?} begins or ends with whitespace — remove it")]
    MaskSurroundingWhitespace { mask: String },
    /// 🔴 The one `.gitignore` edge decided by **refusal** rather than by
    /// giving it a literal meaning, and the asymmetry with `#` is deliberate.
    /// A leading `#` has no competing intent — the only thing `#notes.txt` can
    /// mean is the file of that name, so it is taken literally and pinned by a
    /// case. A leading `!` has two: to a person who knows `.gitignore` it means
    /// *re-include*, and to this layer it is an ordinary character. Serving the
    /// second silently gives them a mask for a file name almost nobody has,
    /// which is the under-exclusion direction; a sentence is the only thing
    /// that can tell them the first is not on offer.
    ///
    /// Scoped to the leading position: `!` inside a character class is
    /// `globset`'s own negation and keeps working.
    #[error(
        "file mask {mask:?} starts with `!` — a mask only ever excludes, so there is nothing for \
         a `!` to put back"
    )]
    MaskStartsWithExclamationMark { mask: String },
    /// A mask that passes every check above and still cannot compile on its
    /// own — `[`, for one, is an unclosed character class. The mirror of
    /// [`RulesError::InvalidPrefix`], and, unlike that one, it is the whole of
    /// the compile story rather than half of it: see [`MaskLayer`] for why this
    /// layer has no aggregate failure to disclose.
    #[error("file mask {mask:?} could not be compiled: {reason}")]
    InvalidMask { mask: String, reason: String },
}

/// The user's file masks, compiled: a **file-only** layer that lives inside
/// `builder()`'s one `filter_entry` closure rather than in the `Override`, and
/// a public predicate so a caller can ask what it will remove without running
/// a walk.
///
/// **Why it is not in the `Override`, which is where every other rule layer
/// lives.** `ignore` decides file from directory on one condition —
/// `if !glob.is_only_dir() || is_dir` (`ignore-0.4.31/src/gitignore.rs:273`) —
/// so a pattern that is not explicitly directory-only matches **both**, and
/// `.gitignore` syntax can express directory-only (a trailing `/`) while it
/// cannot express file-only. Measured with `!*.pdf` as the only override over a
/// tree holding `archive.pdf/keep.txt`: the walk kept `["notes.txt"]` alone —
/// a whole subtree gone on a rule a person wrote about files. A mask therefore
/// cannot live there at all, which is what
/// `a_mask_never_prunes_a_directory` pins.
///
/// **And why it is inside the existing closure rather than a second one.**
/// `WalkBuilder::filter_entry` *replaces* the predicate rather than adding one
/// ("only one filter predicate can be applied to a `WalkBuilder`. Calling this
/// subsequent times overrides previous filter predicates",
/// `ignore-0.4.31/src/walk.rs:1042-1044`), and the one slot already holds the
/// `ANCHORED_DIRS` layer. A second call would silently un-anchor
/// `target`/`build`/`dist` — pinned by
/// `a_stored_mask_does_not_un_anchor_the_builtin_layer`.
///
/// 🔴 **Which failure modes this shares with the `Override`, and which it does
/// not.** It does **not** feed [`crate::Walked::rules_applied`]: that flag is
/// the combined `Override`'s, and a mask is not part of that set, so **a mask
/// can never be the input that empties the built-in list**. Nothing about the
/// mask layer can make `.git` or `node_modules` start being indexed.
///
/// The mirror of that is the part worth stating rather than discovering: a mask
/// failure therefore has **no equivalent of that signal**, and needs its own
/// answer. The answer taken here is to remove the failure rather than report
/// it. Every mask is validated and compiled **alone**, in `validate_mask`, and
/// kept as its own compiled matcher; this layer is a `Vec` scanned with `any`,
/// never a `GlobSet` whose combined automaton has a size limit. So the
/// aggregate "every rule silently stopped applying" state that `rules_applied`
/// exists to disclose does not arise here — there is no aggregate step in which
/// it could. If this ever becomes a `GlobSet` for speed, that is the moment a
/// mask needs a disclosure of its own, and this paragraph is the notice.
///
/// **What it does share:** a mask removal is recorded nowhere — no `PreSkip`,
/// no `unreadable` — exactly like every other rule layer (see
/// `Walked::unreadable`: rule removals are not read failures). And it is
/// **global**: `WalkRules` carries no root, so one mask removes matching files
/// under every watched folder, each on its own next walk.
#[derive(Debug, Default, Clone)]
pub struct MaskLayer {
    /// One compiled matcher per mask, in the order given. Empty is the common
    /// case and is what `default()` produces.
    globs: Vec<globset::GlobMatcher>,
}

impl MaskLayer {
    /// Whether a mask removes the file at `relative_path`.
    ///
    /// 🔴 **The single answer to "would this mask match", and the reason it is
    /// public.** The walk asks it, and Task 10's `mask_preview` counts with it;
    /// a preview standing on a second copy of the rule would disagree with the
    /// walk at exactly the edges this layer's cases pin down — the `.gitignore`
    /// parser edges, the ASCII-only case folding, the normalisation forms.
    /// `the_mask_predicate_answers_exactly_what_the_walk_removes` is the guard
    /// that they stay one answer.
    ///
    /// Asked of the **last component** of the path, which is what makes a mask
    /// apply at every depth without any pattern-level anchoring: the walk hands
    /// it a bare file name, a caller holding an indexed relative path hands it
    /// `Work/report.pdf`, and both are asking about `report.pdf`.
    pub fn matches(&self, relative_path: &str) -> bool {
        let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
        self.globs.iter().any(|glob| glob.is_match(name))
    }
}

impl WalkRules {
    /// Directories excluded unconditionally: never a document folder,
    /// regardless of what sits beside them.
    pub const BUILTIN_DIRS: &'static [&'static str] = &[
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        "__pycache__",
        ".mypy_cache",
        ".pytest_cache",
        ".gradle",
        ".idea",
        ".vscode",
        ".venv",
        "venv",
    ];

    /// Directories excluded only when a sibling marker file names them as
    /// build output for a real project. `target`, `build` and `dist` are
    /// also ordinary English words — `Projects/House/build/permits.pdf` is
    /// a document, not build output, and nothing beside it says otherwise
    /// (review fix round 1, Important finding, measured). A glob cannot see
    /// a sibling file, so this is checked with `filter_entry` instead of an
    /// override pattern.
    const ANCHORED_DIRS: &'static [(&'static str, &'static [&'static str])] = &[
        ("target", &["Cargo.toml"]),
        (
            "build",
            &[
                "package.json",
                "CMakeLists.txt",
                "setup.py",
                "pyproject.toml",
                "Makefile",
            ],
        ),
        (
            "dist",
            &[
                "package.json",
                "CMakeLists.txt",
                "setup.py",
                "pyproject.toml",
                "Makefile",
            ],
        ),
    ];

    /// Names that are a file in every case anyone has met — kept apart from
    /// `BUILTIN_DIRS`, which is documented as directories, rather than
    /// pretending to be a thirteenth entry in that one (review fix round 1,
    /// Minor finding).
    ///
    /// ⚠️ **The pattern it compiles to prunes a DIRECTORY of that name as
    /// well**, and a doc comment here once said the opposite. `!**/.DS_Store`
    /// carries no trailing `/`, and gitignore semantics match a directory of
    /// that name at any depth — measured against this repository's pinned
    /// `ignore 0.4.31` (`Cargo.lock`; a first probe used `^0.4.31`, which
    /// resolved to 0.4.33, and was re-run pinned):
    /// `matched(".DS_Store", is_dir = true).is_ignore()` is `true`, as is
    /// `sub/.DS_Store`, while `notes` and `.git/hooks` are `false`. The
    /// product's own evidence is `a_directory_named_like_a_built_in_file_is_built_in_too`
    /// (`src-tauri/tests/commands.rs`) and the drift guard, both of which run
    /// against the pinned version. So a folder somebody really named
    /// `.DS_Store` is pruned with its whole subtree, and anything asking
    /// "what does the walk prune" has to include this list. That is the
    /// defect [`WalkRules::builtin_layers`] exists to make unrepeatable: it
    /// asks the compiled matcher rather than either list.
    ///
    /// `pub` so a test can generate a fixture from it. A guard whose fixture
    /// is written from the same enumeration it checks agrees with itself —
    /// which is exactly how this list stayed missing for a round.
    pub const BUILTIN_FILES: &'static [&'static str] = &[".DS_Store"];

    /// Fails when a user prefix is not, by construction, a well-formed
    /// relative path — see `validate_prefix` and `RulesError`. Nothing is
    /// rewritten any more (round 2's `./` strip is gone): a prefix is
    /// either exactly the shape `builder()` can turn into a rule, or it is
    /// refused with a message naming what to type instead. Trying to
    /// exclude `target/` (`WalkRules::new(true, ..)`) never fails here: the
    /// built-in list is a fixed set of literals this crate controls, not
    /// user input.
    pub fn new(
        builtin: bool,
        gitignore: bool,
        user_prefixes: Vec<String>,
    ) -> Result<Self, RulesError> {
        let mut normalized = Vec::with_capacity(user_prefixes.len());
        for prefix in &user_prefixes {
            if let Some(clean) = validate_prefix(prefix)? {
                normalized.push(clean);
            }
        }
        Ok(Self {
            builtin,
            gitignore,
            user_prefixes: normalized,
            // Masks are the builder step's, never `new`'s — see
            // `with_masks` for why `new` keeps its signature.
            masks: MaskLayer::default(),
        })
    }

    /// No rules at all. For tests that are about enumeration itself.
    pub fn none() -> Self {
        Self::default()
    }

    /// The user's file masks, whole — a builder step rather than a fourth
    /// argument to [`WalkRules::new`], because `new` has 36 real call sites
    /// (two of them production, `src-tauri/src/walk_job.rs` and
    /// `src-tauri/src/bridge.rs`) and none of the other 34 cares about masks.
    /// It **replaces** the set rather than adding to it, the same way `new`
    /// takes the whole prefix vector: there is one mask set per walk, and a
    /// caller assembling it from storage has it in one piece.
    ///
    /// Fails when a mask is not, by construction, a glob over a single file
    /// name — see `validate_mask` and the `Mask…` arms of [`RulesError`].
    /// Nothing is rewritten: a mask is either exactly the shape [`MaskLayer`]
    /// can compile, or it is refused with a sentence naming what to type
    /// instead. An empty string is the one non-error, meaning "no rule" — the
    /// blank row in an editor — and it stores nothing.
    pub fn with_masks(mut self, masks: Vec<String>) -> Result<Self, RulesError> {
        let mut globs = Vec::with_capacity(masks.len());
        for mask in &masks {
            if let Some(compiled) = validate_mask(mask)? {
                globs.push(compiled);
            }
        }
        self.masks = MaskLayer { globs };
        Ok(self)
    }

    /// The compiled mask layer, so a caller can ask what a mask would remove
    /// without running a walk — the entry point Task 10's `mask_preview` counts
    /// with. See [`MaskLayer::matches`] for why it is the *same* matcher rather
    /// than a second one, and [`WalkRules::with_masks`] for how to build a
    /// throwaway `WalkRules` around one candidate mask, which is also what
    /// validates it.
    pub fn masks(&self) -> &MaskLayer {
        &self.masks
    }

    /// The question `WalkRules::new` asks of ONE user prefix, exposed so a
    /// caller with a folder in front of a person can ask it **before**
    /// offering a control that would then refuse.
    ///
    /// `Ok(())` for the empty string, exactly as `new` treats it — "no rule",
    /// not a malformed one.
    ///
    /// **A wrapper over `validate_prefix`, never a second copy of it.** The
    /// whitelist has grown across three review rounds (see that function's own
    /// doc comment), the last of which found four forms the round before had
    /// let through; a caller that re-implemented "which names are excludable"
    /// would be a fourth round waiting to happen, and it would disagree
    /// silently. The compile-probe is included, so a prefix that passes every
    /// component check and still cannot compile alone is refused here too.
    pub fn check_prefix(prefix: &str) -> Result<(), RulesError> {
        validate_prefix(prefix).map(|_| ())
    }

    /// The override patterns the built-in layers contribute, in the order
    /// `builder()` adds them.
    ///
    /// **One function, two readers**, and that is the whole reason it exists:
    /// `builder()` turns these into the walk's `Override`, and
    /// [`WalkRules::builtin_layers`] turns the same strings into the matcher
    /// that answers what the walk will prune. Before fix round 2 those were
    /// two loops over two constants in two places, and they had already
    /// drifted — one of them read `BUILTIN_FILES` and the other did not.
    ///
    /// Matching the directory itself prunes its subtree, so a trailing `/**`
    /// form is redundant (review fix round 1, Minor finding) and, at the
    /// scale user prefixes can reach, is what pushes the combined pattern set
    /// past the engine's size limit (the third path in round 1's Critical
    /// finding).
    fn builtin_override_patterns() -> Vec<String> {
        Self::BUILTIN_DIRS
            .iter()
            .chain(Self::BUILTIN_FILES)
            .map(|name| format!("!**/{name}"))
            .collect()
    }

    /// The walk's **unconditional** layers, compiled once for one watched
    /// root, so a caller can ask what the walk will prune without running it.
    ///
    /// Written for one caller and one question: the desktop shell's folder
    /// listing has to know, per row, whether offering "exclude this" would be
    /// offering a control that does nothing, and its `exclude_subfolder` has
    /// to refuse the same paths the listing marks. Answering it in the shell
    /// would mean a second reading of these layers, and the two would drift.
    ///
    /// 🔴 **It asks the compiled `Override` — the same patterns `builder()`
    /// adds, from the same function — rather than reading the constants.**
    /// That is fix round 2's correction and it is not a refactor: the previous
    /// version enumerated the lists by hand, read `BUILTIN_DIRS`, and argued
    /// in its own doc that `BUILTIN_FILES` could not matter because those
    /// "name files, not directories". `!**/.DS_Store` carries no trailing `/`
    /// and prunes a *directory* of that name at any depth, so a folder called
    /// `.DS_Store` was offered as ordinary and excludable. Asking globset
    /// removes the whole class: no reading of gitignore semantics is done
    /// here, and a name added to either list — or a third list added to
    /// [`WalkRules::builtin_override_patterns`] — is covered without touching
    /// this function.
    ///
    /// **What it covers, and it is now two things rather than a list:**
    ///
    /// 1. **Every override-based built-in layer**, asked from the compiled
    ///    matcher, per component — the patterns match the directory itself
    ///    and the walker then never descends, so `.git/hooks` is pruned
    ///    because `.git` is, and the question has to be asked of each
    ///    component rather than of the whole path.
    /// 2. **`ANCHORED_DIRS`**, which is the one built-in layer that is *not*
    ///    an override: `filter_entry` prunes those names only when one of the
    ///    marker files sits in the directory's own parent, which no glob can
    ///    express. The marker itself is looked up the same way `filter_entry`
    ///    looks it up, `parent.join(marker).is_file()` — but **not the entry**:
    ///    `filter_entry` also requires `entry.file_type().is_dir()` and this
    ///    does not, because it is given a path rather than a directory entry.
    ///    The difference is visible exactly once, on a symlink named like an
    ///    anchored directory beside that name's marker, and it is disclosed
    ///    where it shows: `src-tauri/src/tree.rs`'s precedence doc.
    ///
    /// **What it deliberately does NOT cover, and must never be read as
    /// covering:**
    ///
    /// - **The in-tree `.gitignore` stack** (`git_ignore`/`git_exclude`, both
    ///   gated on `gitignore`). Deciding it means compiling the same ignore
    ///   stack the walk builds, per directory, from files inside the tree. A
    ///   folder this answers `false` for may still be skipped by a
    ///   `.gitignore`; `false` means "no unconditional layer prunes it",
    ///   never "it will be indexed".
    /// - **The user's own exclusion rules.** Those are the caller's to report,
    ///   and they are the ones whose control does something.
    /// - **Symlinks.** `follow_links(false)` is a property of the walker, not
    ///   of a path, and the caller that needs it can see the link itself.
    ///
    /// ⚠️ **How this can still go stale, stated as what is true rather than as
    /// a complete list** — fix round 2 wrote "the one way" here and named a
    /// grep that "lists every candidate", and fix round 3 measured that false.
    /// Point 1 covers every future *override* for free. Everything else in
    /// `builder()` that can remove a directory from the walk is a hand-written
    /// mirror or nothing at all, and it arrives in **at least two** shapes:
    ///
    /// - another `filter_entry`, like `ANCHORED_DIRS` — which
    ///   `rg -n "filter_entry|over\.add"` over this file does find;
    /// - a `WalkBuilder` **setting** that starts pruning, which that grep does
    ///   **not** find. Measured: flipping `hidden(false)` to `hidden(true)`
    ///   prunes every dot-directory in the walk while this function keeps
    ///   answering `false` for them — the exact defect this predicate exists to
    ///   prevent, reachable by changing one word.
    ///
    /// So the check is **read `builder()`**, not run a grep. What backs that
    /// up rather than replacing it: `builtin_layers_agree_with_what_the_walk_enumerates`
    /// compares this against a real walk, and its fixture now holds an ordinary
    /// dot-directory on neither list, so the `hidden` instance above goes red
    /// there. A setting whose effect no directory in that fixture shows is
    /// still invisible, and no test here can fix that.
    ///
    /// ⚠️ **Both layers are gated on `builtin` inside `builder()`, and this
    /// assumes it is on.** Both production call sites pass `true` —
    /// `src-tauri/src/walk_job.rs:128` and `src-tauri/src/bridge.rs:439`; only
    /// tests pass `false`. A caller that built rules with `builtin: false`
    /// must not use this.
    pub fn builtin_layers(root: &Path) -> BuiltinLayers {
        let mut builder = OverrideBuilder::new(root);
        for pattern in Self::builtin_override_patterns() {
            let _ = builder.add(&pattern);
        }
        BuiltinLayers {
            root: root.to_path_buf(),
            // Unreachable with the fixed literals above — they are this
            // crate's own, not user input — and if it ever were reached the
            // walk would refuse to index at all rather than index more:
            // `builder()`'s own `Err` arm answers `rules_applied = false`, and
            // `walk_root` stops before phase 2 on that. So an empty matcher
            // here cannot make anything reach a provider that would not have.
            over: builder.build().unwrap_or_else(|_| Override::empty()),
        }
    }

    /// Builds the walker for this call, plus whether the override-based
    /// layers (the unconditional built-in list, user prefixes, `.DS_Store`)
    /// actually combined into a working pattern set. `builder()` itself
    /// stays infallible — a walk always runs — but a caller that ignores
    /// the second value cannot tell "excluded nothing because there was
    /// nothing to exclude" from "excluded nothing because the pattern
    /// engine silently gave up." See `Walked::rules_applied`.
    pub(crate) fn builder(&self, root: &Path) -> (WalkBuilder, bool) {
        let mut b = WalkBuilder::new(root);
        // A symlink cycle is an endless walk, and the same bytes under two
        // names are one document anyway (§5).
        b.sort_by_file_path(|a, b| a.cmp(b))
            .follow_links(false)
            // Dotfiles are ordinary documents in a watched folder; the
            // built-in list names the dot-directories that are not.
            .hidden(false)
            .git_global(false)
            .git_exclude(self.gitignore)
            .git_ignore(self.gitignore)
            // Load-bearing, and the reason `tests/rules.rs` exists: the
            // default is TRUE, under which no ignore rule applies outside a
            // git repository — silently.
            .require_git(false)
            // `ignore` and `parents` both default to true, and `parents`
            // means "climb above the root looking for more `.ignore` files
            // to apply" — so left alone, a `.ignore` file above the watched
            // root silently removes files from inside it, with nothing
            // incrementing `unreadable` to say so. The three rule layers in
            // the design (§5: built-in list, in-tree `.gitignore`, user
            // rules) are the whole of the rules; a file outside the root is
            // not one of them.
            .ignore(false)
            .parents(false);

        let builtin = self.builtin;
        // 🔴 One `filter_entry` call, holding two layers. `WalkBuilder`'s
        // doc: "only one filter predicate can be applied to a `WalkBuilder`.
        // Calling this subsequent times overrides previous filter
        // predicates" (`walk.rs:1042-1044`) — a second call for the masks
        // would silently un-anchor `target`/`build`/`dist`.
        let masks = self.masks.clone();
        b.filter_entry(move |entry| {
            // The root itself is never a candidate — pruning it would empty
            // the whole walk, not remove one directory from it.
            if entry.depth() == 0 {
                return true;
            }
            let Some(name) = entry.file_name().to_str() else {
                return true;
            };
            // The user's masks, and they are **not** gated on `builtin`:
            // turning the built-in list off is a statement about this crate's
            // own list, never about a rule the person typed.
            //
            // 🔴 **`is_file`, not `!is_dir`, and the difference is a
            // disclosure.** Both never prune a directory, which is the rule
            // this layer exists for; but `!is_dir` also swallows everything
            // this crate names in `PreSkip` — a symlink, a dangling symlink, a
            // FIFO — and an entry whose `file_type()` cannot be read at all.
            // None of those is ever indexed either way, so the only thing
            // `!is_dir` would change is that `enumerate` stops *saying* the
            // walk met them (`PreSkipRule::NotAFile`, `NotAFileSubtree`,
            // `Unreadable`). Asking the positive question keeps the mask's
            // effect confined to what a mask is about: files that would
            // otherwise be indexed.
            if entry.file_type().is_some_and(|t| t.is_file()) && masks.matches(name) {
                return false;
            }
            if !builtin {
                return true;
            }
            for &(dir, markers) in Self::ANCHORED_DIRS {
                if dir != name {
                    continue;
                }
                if !entry.file_type().is_some_and(|t| t.is_dir()) {
                    // Named like an anchored dir but not one — e.g. a file
                    // called `target` — nothing to anchor against.
                    return true;
                }
                let Some(parent) = entry.path().parent() else {
                    return true;
                };
                // Prune only when a marker sits right beside it — the one
                // check an override glob cannot express (review fix round
                // 1, Important finding).
                return !markers.iter().any(|marker| parent.join(marker).is_file());
            }
            true
        });

        let mut over = OverrideBuilder::new(root);
        if self.builtin {
            // Through the one function that produces these patterns, never a
            // second loop over the same constants: fix round 2 found the two
            // readings had already drifted — `BUILTIN_FILES` was added here
            // and left out of the predicate that answers what this prunes.
            for pattern in Self::builtin_override_patterns() {
                let _ = over.add(&pattern);
            }
        }
        // Already validated by `WalkRules::new` — a well-formed relative
        // path, exactly as typed (see the doc comment on `user_prefixes`).
        // Nothing left to do here but turn each one into a rooted pattern.
        for prefix in &self.user_prefixes {
            let _ = over.add(&anchored_pattern(prefix));
        }
        match over.build() {
            Ok(built) => {
                b.overrides(built);
                (b, true)
            }
            // Every override-based layer silently stops applying — the
            // built-in list included, since it shares this one `Override`
            // with the user prefixes. `filter_entry`'s anchored layer is
            // unaffected: it does not go through the pattern engine.
            Err(_) => (b, false),
        }
    }
}

/// The walk's unconditional layers for one watched root, compiled once.
///
/// Built by [`WalkRules::builtin_layers`], whose doc comment carries the
/// argument for every part of this — what it covers, what it deliberately does
/// not, and the one way it can go stale.
pub struct BuiltinLayers {
    root: std::path::PathBuf,
    over: Override,
}

impl BuiltinLayers {
    /// Whether the walk prunes the directory at `relative_path`, or an
    /// ancestor of it, whatever the user's rules say.
    ///
    /// Per component, because the layers prune a directory and the walker then
    /// never descends: the whole path is never what matched.
    pub fn prunes(&self, relative_path: &str) -> bool {
        let mut parent = self.root.clone();
        for component in relative_path.split('/') {
            let path = parent.join(component);
            if self.over.matched(&path, true).is_ignore() {
                return true;
            }
            let anchored = WalkRules::ANCHORED_DIRS.iter().any(|(dir, markers)| {
                *dir == component && markers.iter().any(|marker| parent.join(marker).is_file())
            });
            if anchored {
                return true;
            }
            parent = path;
        }
        false
    }
}

/// A prefix is a path the user typed, not a glob pattern: escaped so that a
/// folder named `Photos [2023]` excludes those exact bytes instead of
/// `[2023]` being read as a character class (review fix round 1, Critical
/// finding) — and, since round 2, explicitly rooted with a leading `/` so
/// it is anchored to the watched root regardless of how many path
/// components it has.
///
/// Without the leading `/`, `gitignore.rs`'s line parser prepends `**/` to
/// any pattern that contains no `/` at all, matching at every depth rather
/// than only at the root: a one-component rule `private` would remove
/// `Work/deep/deeper/private/` as well as the top-level `private/`, which
/// is more than the user asked for and — because it deletes on the next
/// walk (see the doc comment on `WalkRules`) — the dangerous direction to
/// get wrong. A pattern that already has a `/` in it (`Work/private`) is
/// anchored to the root by that same parser without help, so the leading
/// `/` here is a no-op for those, not a special case (review fix round 2,
/// Important finding).
fn anchored_pattern(normalized_prefix: &str) -> String {
    format!("!/{}", globset::escape(normalized_prefix))
}

/// Whitelists a prefix instead of blacklisting ways to be wrong — round 1
/// caught glob metacharacters, round 2 caught four more specific shapes,
/// and round 3 found four MORE that round 2's checks let through, which is
/// what "enumerating bad forms one round at a time does not converge"
/// means in practice. Every one of these forms shares one root cause:
/// `ignore`'s override matcher is, underneath, exactly a `.gitignore` line
/// parser (`overrides.rs` forwards straight into
/// `GitignoreBuilder::add_line`), so a prefix that is not a plain,
/// unadorned relative path can compile to `Ok` and then match something
/// other than what it names — usually nothing at all.
///
/// A well-formed prefix is a `/`-joined sequence of one or more components,
/// each of which is non-empty, is not `.` or `..`, contains no `\` and no
/// control character, and does not begin or end with whitespace — plus,
/// for the FIRST component only, is not shaped like a Windows drive letter
/// or a home-directory shorthand. Anything else is refused outright, with a
/// message naming what is wrong; nothing is silently rewritten any more,
/// not even round 2's `./` strip (that normalisation is exactly how
/// `./Photos` became a rule that compiled fine and matched nothing).
///
/// `Ok(None)` is the one remaining non-error, deliberately narrow: a
/// literal empty string, meaning "no rule" — the same as a blank row in an
/// exclusion list, not an attempt to name a folder that then got silently
/// misread. Every other degenerate shape (`/`, `./`, `//`) is a component
/// that fails the whitelist and is therefore `Err`, not `Ok(None)` — round
/// 2's doc comment claimed otherwise for two of those three without having
/// measured it (review fix round 3, Minor finding).
fn validate_prefix(prefix: &str) -> Result<Option<String>, RulesError> {
    if prefix.is_empty() {
        return Ok(None);
    }
    for (index, component) in prefix.split('/').enumerate() {
        validate_component(prefix, component, index == 0)?;
    }

    // Compile-probe, alone, in a throwaway `OverrideBuilder`: catches a
    // single prefix pathological enough on its own to exceed the pattern
    // engine's size limit even though every whitelist check above passed
    // (review fix round 1, Critical finding, third path — pinned by
    // `a_single_prefix_past_the_size_limit_is_refused_by_new` after review
    // fix round 3 found this block could be deleted entirely without
    // reddening any test). Must build the exact pattern `builder()` will
    // use, leading `/` included, or this probe and the real walk could
    // disagree about what compiles.
    let mut probe = OverrideBuilder::new(Path::new("."));
    probe
        .add(&anchored_pattern(prefix))
        .and_then(|built| built.build())
        .map(|_| ())
        .map_err(|err| RulesError::InvalidPrefix {
            prefix: prefix.to_string(),
            reason: err.to_string(),
        })?;

    Ok(Some(prefix.to_string()))
}

/// A mask, against what a mask IS rather than against a list of ways to be
/// wrong — the lesson `validate_prefix` paid three review rounds for. A
/// well-formed mask is a glob over **one file name**: it holds no `/`, no `\`
/// and no control character, does not begin or end with whitespace, does not
/// begin with `!`, and compiles on its own.
///
/// 🔴 **The `.gitignore` parser edges are decided here, not inherited.** A
/// prefix reaches `GitignoreBuilder::add_line` through `OverrideBuilder`; a
/// mask never does, because [`MaskLayer`] owns it instead — so `#`, `!`,
/// trailing whitespace and `\` do not arrive with the meanings that parser
/// gives them, and each needed a decision of its own. Measured on the pinned
/// `globset 0.4.19` and pinned by a case each: `#` and a `[!a]` class are
/// ordinary and are kept (`a_leading_hash_in_a_mask_is_an_ordinary_character`,
/// `a_leading_exclamation_mark_in_a_mask_is_refused`'s first half); a leading
/// `!`, either whitespace edge and a `\` anywhere are refused, each for the
/// reason written on its `RulesError` arm. "Whatever the library does" is not a
/// decision — it is how `./Photos` became a rule that compiled fine and matched
/// nothing.
///
/// `Ok(None)` is the one non-error: a literal empty string, meaning "no rule",
/// the same blank-row case `validate_prefix` allows. It must never become a
/// stored mask — an empty glob compiles fine and matches the empty name, which
/// no walk would ever ask about, so the mistake would be invisible everywhere
/// except [`MaskLayer::matches`].
///
/// **Case-insensitive, and only here** (owner's ruling, 2026-08-31). The reason
/// is the failure direction, not simplicity: case-sensitive is `globset`'s
/// default and costs nothing, so this flag is the extra line. Case-sensitive
/// would mean a person writes `*.pdf`, `REPORT.PDF` is indexed anyway, and
/// under D29 its text goes to a third-party provider — D-a's under-exclusion
/// hole, arriving through a typed rule. Case-insensitive errs toward excluding
/// too much, which a person can see and undo. The prefix layer keeps `globset`'s
/// default, because its rules come from disk and need no help.
///
/// ⚠️ **Two things that ruling does NOT close, measured rather than assumed.**
///
/// - **Unicode normalisation is a separate axis.** `caf\u{e9}.pdf` (NFC) and
///   `cafe\u{301}.pdf` (NFD, the form macOS hands out) are different byte
///   strings under any case folding, and measured here they do not match each
///   other in either direction. Nothing normalises anything.
/// - **The folding is ASCII only.** `globset` compiles a case-insensitive glob
///   to a non-Unicode regex and then asks for case insensitivity — measured,
///   `ÜBUNG.TXT` compiles to `(?-u)(?i)^\xc3\x9cBUNG\.TXT$` — so `(?i)` folds
///   the ASCII bytes and leaves the two bytes of `Ü` alone. `É.txt` does not
///   match `é.txt`.
///
/// Both are pinned by cases (`a_mask_does_not_bridge_unicode_normalisation`,
/// `mask_case_folding_is_ascii_only`) so that a later session finds the answer
/// written down instead of discovering it.
fn validate_mask(mask: &str) -> Result<Option<globset::GlobMatcher>, RulesError> {
    if mask.is_empty() {
        return Ok(None);
    }
    if mask.contains('/') {
        return Err(RulesError::MaskContainsSlash {
            mask: mask.to_string(),
        });
    }
    if mask.contains('\\') {
        return Err(RulesError::MaskContainsBackslash {
            mask: mask.to_string(),
        });
    }
    if mask.chars().any(|c| c.is_control()) {
        return Err(RulesError::MaskContainsControlCharacter {
            mask: mask.to_string(),
        });
    }
    if mask != mask.trim() {
        return Err(RulesError::MaskSurroundingWhitespace {
            mask: mask.to_string(),
        });
    }
    if mask.starts_with('!') {
        return Err(RulesError::MaskStartsWithExclamationMark {
            mask: mask.to_string(),
        });
    }
    globset::GlobBuilder::new(mask)
        .case_insensitive(true)
        .build()
        .map(|glob| Some(glob.compile_matcher()))
        .map_err(|err| RulesError::InvalidMask {
            mask: mask.to_string(),
            reason: err.to_string(),
        })
}

/// One component of a `/`-split prefix against the whitelist described on
/// `validate_prefix`. `whole` is the original, un-split prefix, carried
/// through only so an error message can name the rule the user typed
/// rather than the fragment that failed.
fn validate_component(whole: &str, component: &str, is_first: bool) -> Result<(), RulesError> {
    if component.is_empty() {
        return Err(RulesError::EmptyComponent {
            prefix: whole.to_string(),
        });
    }
    if component == "." || component == ".." {
        return Err(RulesError::DotComponent {
            prefix: whole.to_string(),
            component: component.to_string(),
        });
    }
    if component.contains('\\') {
        return Err(RulesError::ContainsBackslash {
            prefix: whole.to_string(),
        });
    }
    if component.chars().any(|c| c.is_control()) {
        return Err(RulesError::ContainsControlCharacter {
            prefix: whole.to_string(),
        });
    }
    if component != component.trim() {
        return Err(RulesError::SurroundingWhitespace {
            prefix: whole.to_string(),
        });
    }
    if is_first {
        if is_drive_letter(component) {
            return Err(RulesError::DriveLetterPrefix {
                prefix: whole.to_string(),
            });
        }
        if component == "~" {
            return Err(RulesError::HomeDirectoryShorthand {
                prefix: whole.to_string(),
            });
        }
    }
    Ok(())
}

/// A single ASCII letter followed by `:` and nothing else — `C:`, not
/// `CD:` and not `C:foo`. The shape `library/std/src/path.rs`'s own
/// Windows `Prefix::Disk` parsing looks for at the start of a path.
fn is_drive_letter(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
