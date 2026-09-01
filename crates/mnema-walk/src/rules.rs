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
    /// 🔴 The two names no file can have. A mask is compared against a file's
    /// **own name**, and no directory entry is ever named exactly `.` or `..` —
    /// those are the folder itself and its parent, and the walk never hands
    /// either to the mask layer. So the rule compiles, stores, and can never
    /// fire: the under-exclusion shape, arriving as a rule the person believes
    /// they wrote. The mirror of [`RulesError::DotComponent`] for a prefix,
    /// which refuses the same two tokens for a different reason.
    ///
    /// **Only those two exact names.** `.hidden` and `..чернетка` are ordinary
    /// file names — the walk sets `hidden(false)` because a dotfile in a watched
    /// folder is an ordinary document — and they stay legal
    /// (`a_mask_that_can_never_name_a_file_is_refused` shows both halves).
    #[error(
        "file mask {mask:?} can never match — no file is named `.` or `..`; those are the \
         folder itself and its parent"
    )]
    MaskCanNeverNameAFile { mask: String },
    /// 🔴 A character class holding a character outside ASCII. `globset`
    /// compiles with Unicode **off** — `(?-u)`, hardcoded at
    /// `globset-0.4.19/src/glob.rs:675`, with no switch on `GlobBuilder` — so
    /// `[...]` is a class of **bytes**, and a letter outside ASCII is two or
    /// more of them. The rule that compiles is not the rule that was typed, and
    /// measurement says it goes wrong in **both** directions, not one:
    ///
    /// - anchored, it matches nothing — `[Г]file.txt` does not match
    ///   `Гfile.txt`, because the class matches one byte where the letter is
    ///   two;
    /// - wrapped in `*`, it matches by byte and takes names that hold no such
    ///   letter at all — `*[Г]*` matches `авто.txt`, because the folded `г` is
    ///   `D0 B3` and `а` begins `D0`.
    ///
    /// Both measured on the pinned `globset 0.4.19` and pinned by
    /// `a_character_class_of_non_ascii_letters_is_refused`. Under D29 that is a
    /// rule a person believes holds text back while the walk indexes something
    /// else and sends it to a third-party provider — so this is refused with a
    /// sentence rather than accepted and disclosed. A privacy control that
    /// silently does something else is worse than no control.
    ///
    /// **Scoped to the inside of a class, and nowhere else.** A non-ASCII
    /// **literal** is fine and must keep working — `Копія*` matches
    /// `КОПІЯ звіту.docx` since Task 9b, because [`caseless_form`] bridges case
    /// and normalisation on both sides of a byte comparison of whole letters.
    /// `?` is not refused either: its breakage is a property of the **name**,
    /// not of the mask (`?.txt` fails to match `й.txt` in an all-ASCII mask,
    /// and `??.txt` matches it — measured), so refusing every `?` would cut
    /// through the healthy case. That one is disclosed in the editor's
    /// explainer instead.
    #[error(
        "file mask {mask:?} has a character class holding a character outside ASCII — a class \
         is compared byte by byte, so it can never mean what it says; add one mask per name \
         instead"
    )]
    MaskNonAsciiCharacterClass { mask: String },
}

/// 🔴 **The one comparison rule, and the only place case or normalisation is
/// decided.** Both sides go through this: each mask once when it is compiled
/// (`validate_mask`), each file name once per entry ([`MaskLayer::matches`]).
/// It is one function rather than two so that a mask and a name cannot be
/// transformed differently — the defect class review Important 4.1 caught once
/// already, where the predicate and the walk answered different questions.
///
/// **`NFC(fold(NFD(x)))`, and the order was measured rather than assumed.**
/// Unicode's canonical caseless matching (Unicode 16.0 §3.13, D145) is
/// `NFD(toCasefold(NFD(x)))`; the outer form is free **for the equivalence
/// relation** — NFC and NFD are in bijection, so either gives the same answer
/// to "are these two names the same" — and NFC is taken because a composed
/// pattern is shorter and closer to what the person typed. **It is not free
/// for the matcher**: `globset` compares bytes, `?` counts bytes, and a
/// composed name is shorter than its decomposed spelling, so the outer form
/// decides what `?` — and the contents of `[...]` — see, pinned by
/// `a_mask_compares_names_in_composed_form` (independent review, Important 2).
/// **The inner
/// NFD is not free**, and the measurement is the reason it is here: `U+0345`
/// COMBINING GREEK YPOGEGRAMMENI has combining class 240 and case-folds to
/// `U+03B9` GREEK SMALL LETTER IOTA, which has class 0, so folding *destroys*
/// the ordering information a later normalisation needs. Run on
/// `α U+0345 U+0300` against its own canonical ordering `α U+0300 U+0345`:
/// `NFC(fold(x))` gives `U+03B1 U+1F76` for one and `U+1F70 U+03B9` for the
/// other, while `NFC(fold(NFD(x)))` gives `U+1F70 U+03B9` for both.
///
/// **Full case folding, not `str::to_lowercase`**, and that too was measured.
/// They are different operations and the difference is not exotic: Greek final
/// sigma `ς` lowercases to itself while `Σ` lowercases to `σ`, so
/// `to_lowercase` leaves `ΛΟΓΟΣ` and `λογος` unequal; and `ß` lowercases to
/// itself while it folds to `ss`. They agree on Cyrillic, on `ı` and on `İ`.
/// Case folding is the operation caseless matching is defined in terms of;
/// lowercasing only resembles it.
///
/// **Three properties this relies on, each measured over every Unicode scalar
/// value before the code was written**, because each of them would be a defect
/// nothing else here would catch:
///
/// - it never produces any of `* ? [ ] { } ! , - \ /`, so a literal in a mask
///   cannot become a metacharacter and `validate_mask` may check the mask
///   **before** transforming it;
/// - it never empties a non-empty string, so the empty mask stays the only
///   `Ok(None)`;
/// - it is idempotent, so a name transformed twice is the name transformed
///   once — which is what lets a mask be transformed at build time and compared
///   against a name transformed at walk time.
///
/// On ASCII it is exactly `to_ascii_lowercase` (measured over all 128), so
/// `*.PDF` still matches `report.pdf` — the common case, and the whole reason
/// the case-insensitivity ruling exists.
///
/// ⚠️ **It transforms the inside of a character class too, and that is not the
/// only place this can narrow — see the paragraph below for a second, separate
/// shape.** The mask is one string; nothing here parses `[...]` and steps
/// around it. Usually that is exactly right — `[А-Я]` becomes `[а-я]` and so
/// means the same rule as its lowercase spelling. Two shapes where it is not,
/// both measured: a range that crosses ASCII punctuation (`[A-z]` becomes
/// `[a-z]`, so a mask that removed `_x.txt` no longer does — one of several
/// measured narrowing shapes, not the only one this commit introduces), and a
/// letter whose fold is more than one character (`[ß]` becomes `[ss]`, a class
/// of one `s`, so `[ß]x.txt` matches `sx.txt` — measured).
///
/// ⚠️ **Since Task 11 the second of those, and the `[А-Я]` example above, can
/// no longer be stored at all**: `validate_mask` refuses a class holding any
/// character outside ASCII ([`RulesError::MaskNonAsciiCharacterClass`]) before
/// this transform ever runs, so those sentences describe what the transform
/// WOULD do rather than a rule anybody can save. The `[A-z]` narrowing is
/// unaffected and is still recorded rather than fixed — it is all ASCII, and
/// fixing it means not folding inside `[...]`, which is a parser this layer
/// does not have.
///
/// **A second, unrelated narrowing shape lives on the name side, in the outer
/// `.nfc()`: composition can swallow the literal a mask is looking for.**
/// Measured: `cafe*` matches `cafe\u{0301}.pdf` before this commit and does not
/// after, because `.nfc()` composes `e\u{0301}` into `é` before the literal
/// `cafe` gets to match it; `caf\u{e9}*` still matches the same name, because
/// both sides fold to the same composed form. Unlike the class-content case
/// above, this one narrows in an ordinary literal, outside any `[...]`.
/// Pinned by `a_mask_literal_can_be_swallowed_by_a_following_combining_mark`
/// (independent review, Important 1). Neither list here is claimed to be
/// exhaustive — each is what has been measured so far, not a closed count.
///
/// **Cost**, measured on this machine over 300 000 names: 572 ns for a mixed
/// Cyrillic name, 315 ns for a plain ASCII one. An ASCII fast path was
/// considered and dropped: it would be a second implementation of the rule for
/// a saving that a walk of hundreds of thousands of files spends on one `stat`.
fn caseless_form(name: &str) -> String {
    use caseless::Caseless;
    use unicode_normalization::UnicodeNormalization;

    name.chars().nfd().default_case_fold().nfc().collect()
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
    ///
    /// Each was compiled from `caseless_form(mask)` — the transform is paid
    /// **once, here, at build time**, never once per name per mask.
    globs: Vec<globset::GlobMatcher>,
}

impl MaskLayer {
    /// Whether a mask removes the file at `relative_path`.
    ///
    /// 🔴 **The single answer to "would this mask match", and the reason it is
    /// public.** The walk asks it, and Task 10's `mask_preview` counts with it;
    /// a preview standing on a second copy of the rule would disagree with the
    /// walk at exactly the edges this layer's cases pin down — the `.gitignore`
    /// parser edges, and now the case folding and the normalisation form, both
    /// of which are decided by [`caseless_form`] on this side of `globset`
    /// rather than by a flag inside it.
    /// `the_mask_predicate_and_the_walk_agree_on_regular_files` is the guard
    /// that they stay one answer, and four of its names are non-ASCII precisely
    /// so that the transform is part of what it checks.
    ///
    /// 🔴 **It answers about the name, not about the entry, and the walk asks it
    /// SECOND.** The walk's condition is `is_file() && matches(name)`, so on a
    /// symlink, a FIFO, or an entry whose `file_type()` cannot be read at all,
    /// this predicate says "removed" where the walk removes nothing. The skew is
    /// one-directional — everything the walk removes, this predicate also calls
    /// removed, never the reverse — so it can overstate a mask's reach and can
    /// never understate it. It is unreachable through `Found::relative`, which
    /// carries regular files only; it becomes reachable the moment a caller
    /// counts over a **disk listing** instead. Task 10's `mask_preview` must
    /// therefore count over indexed relative paths, or it will show a person
    /// more files than the next walk will take.
    ///
    /// Asked of the **last component** of the path, which is what makes a mask
    /// apply at every depth without any pattern-level anchoring: the walk hands
    /// it a bare file name, a caller holding an indexed relative path hands it
    /// `Work/report.pdf`, and both are asking about `report.pdf`.
    ///
    /// 🔴 **The name is folded and normalised here and nowhere else, once per
    /// call.** The walk hands over the raw `file_name()` and does no folding of
    /// its own, so there is exactly one site at which a name meets
    /// [`caseless_form`] and exactly one at which a mask does — which makes "the
    /// walk and the predicate compare the same way" true by construction rather
    /// than by a matching pair of edits that a later session could break one
    /// half of. The transform is paid once per entry, not once per entry per
    /// mask, which is why the loop is inside it and not around it.
    pub fn matches(&self, relative_path: &str) -> bool {
        // The common case is no masks at all, and it should not pay for a
        // transform. `any` over an empty slice is `false` either way, so this
        // is a cost guard and not a rule.
        if self.globs.is_empty() {
            return false;
        }
        let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
        let name = caseless_form(name);
        self.globs.iter().any(|glob| glob.is_match(&name))
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

    /// Whether two masks are **the same rule**: whether the matcher can tell
    /// them apart at all.
    ///
    /// **What a caller may conclude from `true`.** That the two are
    /// interchangeable, everywhere, for every name — not that they look alike.
    /// Every mask's glob is compiled from `caseless_form(mask)`
    /// ([`validate_mask`]), so two masks with the same comparison form compile
    /// to the **identical** `GlobMatcher` and there is no name, on any
    /// platform, that one accepts and the other refuses. Storing both is
    /// therefore two rows and one rule.
    ///
    /// **What a caller may NOT conclude from `false`.** Not that the two remove
    /// different files. `*.pdf` and `*.[pP][dD][fF]` are different rules by
    /// this comparison and take the same files; so do `*.pdf` and `*`, on every
    /// name the first one names. This answers "are these the same rule", never
    /// "do these overlap" — nothing here computes what a glob matches.
    ///
    /// **A wrapper over [`caseless_form`], never a second copy of it**, and the
    /// reason is [`WalkRules::check_prefix`]'s, one layer over: the transform
    /// is `nfd → default_case_fold → nfc`, three steps a caller reaching for
    /// `to_lowercase` gets wrong in two separate ways at once — `ß`/`SS` folds
    /// to more than one character, and an accented name spelled NFD on disk
    /// never composes. A caller standing on its own copy of this would
    /// disagree with the walk silently, at exactly the edges the mask layer was
    /// built to pin.
    ///
    /// 🔴 **It is deliberately not a way to get the folded spelling.** The
    /// comparison form is not something to store, echo or offer for editing:
    /// the storage layer keeps what a person typed, byte for byte
    /// (`migrations.rs`, `file_mask.pattern`), and folding at that layer would
    /// silently rewrite it. A predicate is the whole of what a caller needs to
    /// say "you already have this rule", and it cannot be misused to say it in
    /// a spelling nobody entered.
    pub fn same_mask_rule(a: &str, b: &str) -> bool {
        caseless_form(a) == caseless_form(b)
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

        match self.compiled_override(root) {
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

    /// The override this rule set compiles for one root: the built-in layers
    /// when they are on, then the user's prefixes, in `builder()`'s own order.
    ///
    /// **One function, two readers**, the same argument
    /// [`WalkRules::builtin_override_patterns`] carries one level down and for
    /// the same measured reason. [`WalkRules::builder`] hands the result to the
    /// walker; [`WalkRules::applied_to`] hands it to the predicate that answers
    /// what the walk will remove without running one. Two loops over the same
    /// two sources is exactly the shape that had already drifted once on the
    /// built-in half, and the drift was invisible until a folder named
    /// `.DS_Store` was offered as ordinary.
    fn compiled_override(&self, root: &Path) -> Result<Override, ignore::Error> {
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
        over.build()
    }

    /// **The WHOLE rule set, compiled for one watched root, so a caller can ask
    /// what the next walk will remove from a path the index already holds.**
    ///
    /// 🔴 **Why this exists at all, and it is a defect report rather than a
    /// convenience.** [`WalkRules::masks`] answers about the mask layer alone,
    /// and Task 10's `mask_preview` counted with it — so the preview reasoned
    /// about ONE rule while the next walk applies the whole set. With `*.pdf`
    /// already stored and no scan run yet, previewing `*.txt` over a document
    /// indexed at both `copy.pdf` and `copy.txt` answered "no document stops
    /// being findable — each is also indexed under another path", and the next
    /// scan applied both masks and took the document. The direction of that
    /// error is the worst available: it **understates** the loss, on the one
    /// screen whose job is to state it.
    ///
    /// **What it covers**, and it is the walk's own layers, asked through the
    /// walk's own compiled matchers rather than re-read here:
    ///
    /// - the **masks**, through [`MaskLayer::matches`] — asked of the last
    ///   component only, because a mask is file-only (D-c) and the walk never
    ///   prunes a directory on one;
    /// - the **override layers** — the built-in list and the user's path
    ///   prefixes — from [`WalkRules::compiled_override`], the same function
    ///   `builder()` gives the walker, asked per component for
    ///   [`BuiltinLayers::prunes`]'s reason: those patterns prune a directory
    ///   and the walker then never descends;
    /// - the **anchored build-output layer**, looked up the way `filter_entry`
    ///   looks it up.
    ///
    /// **What it deliberately does NOT cover** is exactly
    /// [`WalkRules::builtin_layers`]'s list, and for the same reasons: the
    /// in-tree `.gitignore` stack, and symlinks. `false` means "no rule in this
    /// set removes it", never "it will be indexed".
    ///
    /// ⚠️ **A pattern set that does not compile answers as an EMPTY override
    /// here**, rather than as "everything is removed" or as a refusal. In that
    /// state `builder()` answers `rules_applied = false` and `walk_root` stops
    /// before phase 2, so the walk removes nothing at all — and a caller
    /// computing a difference between two rule sets gets the same override on
    /// both sides, which can only make a marginal count larger, never smaller.
    /// Overstating is the direction a person can see and undo; understating is
    /// the one this whole entry point exists to close.
    pub fn applied_to(&self, root: &Path) -> AppliedRules {
        AppliedRules {
            root: root.to_path_buf(),
            over: self
                .compiled_override(root)
                .unwrap_or_else(|_| Override::empty()),
            masks: self.masks.clone(),
        }
    }
}

/// One watched root's whole rule set, compiled — built by
/// [`WalkRules::applied_to`], whose doc comment carries the argument for every
/// part of it.
pub struct AppliedRules {
    root: std::path::PathBuf,
    over: Override,
    masks: MaskLayer,
}

impl AppliedRules {
    /// Whether this rule set removes the **file** at `relative_path`.
    ///
    /// 🔴 **A file**, and the word is load-bearing in both directions. The
    /// masks are asked of the last component only, because the walk's condition
    /// is `is_file() && matches(name)` and a mask must never prune a directory
    /// that shares its name (D-c, `a_mask_never_prunes_a_directory`) — so a
    /// stored `*.pdf` does not make this answer `true` for
    /// `archive.pdf/keep.txt`. The override and anchored layers are asked of
    /// every component, because those DO prune directories and the walker then
    /// never descends.
    ///
    /// Meant for a caller holding an indexed relative path — a regular file the
    /// walk already took. Over a **disk listing** it inherits
    /// [`MaskLayer::matches`]'s one-directional skew, which is that function's
    /// own doc comment to read, not a second thing to remember.
    pub fn removes_file(&self, relative_path: &str) -> bool {
        self.masks.matches(relative_path)
            || prunes_a_component(&self.root, &self.over, relative_path, false)
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
        prunes_a_component(&self.root, &self.over, relative_path, true)
    }
}

/// Whether any component of `relative_path` is pruned by `over` or by the
/// anchored build-output layer — the one component walk, shared by
/// [`BuiltinLayers::prunes`], which is given a directory, and
/// [`AppliedRules::removes_file`], which is given a file.
///
/// **Per component**, because both layers prune a directory and the walker then
/// never descends: the whole path is never what matched, so `.git/hooks` is
/// pruned because `.git` is.
///
/// `last_is_dir` is the only thing the two callers disagree about, and it says
/// what the LAST component is: a directory for the folder listing, a file for
/// an indexed path. Every component before it is a directory either way.
/// Nothing in this repository currently answers differently for the two —
/// `builtin_override_patterns` emits no directory-only pattern (none carries a
/// trailing `/`) and `anchored_pattern` emits none either — so today this is a
/// truthful argument rather than a load-bearing one. It becomes load-bearing
/// the moment a directory-only pattern is added, which is why it is asked
/// rather than assumed: the anchored layer is already gated on it here, exactly
/// as `filter_entry` gates it on `entry.file_type().is_dir()`.
fn prunes_a_component(
    root: &Path,
    over: &Override,
    relative_path: &str,
    last_is_dir: bool,
) -> bool {
    let mut parent = root.to_path_buf();
    let mut components = relative_path.split('/').peekable();
    while let Some(component) = components.next() {
        let is_dir = last_is_dir || components.peek().is_some();
        let path = parent.join(component);
        if over.matched(&path, is_dir).is_ignore() {
            return true;
        }
        let anchored = is_dir
            && WalkRules::ANCHORED_DIRS.iter().any(|(dir, markers)| {
                *dir == component && markers.iter().any(|marker| parent.join(marker).is_file())
            });
        if anchored {
            return true;
        }
        parent = path;
    }
    false
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
/// is the failure direction, not simplicity: a case-sensitive mask would mean a
/// person writes `*.pdf`, `REPORT.PDF` is indexed anyway, and under D29 its text
/// goes to a third-party provider — D-a's under-exclusion hole, arriving through
/// a typed rule. Erring toward excluding too much is the direction a person can
/// see and undo. The prefix layer keeps `globset`'s default, because its rules
/// come from disk and need no help.
///
/// 🔴 **How that ruling is delivered, and why it is not `case_insensitive(true)`
/// any more.** Task 9 set that flag and measured what it actually bought:
/// `globset` compiles with Unicode **off** — `(?-u)` is hardcoded at
/// `globset-0.4.19/src/glob.rs:675` and `GlobBuilder` offers no switch — so its
/// `(?i)` folded ASCII bytes and nothing else. `ÜBUNG.TXT` became
/// `(?-u)(?i)^\xc3\x9cBUNG\.TXT$`, `Копія*` did not match `КОПІЯ звіту.docx`,
/// and normalisation was not bridged in either direction. The ruling had shipped
/// narrower than it was worded, in the under-exclusion direction.
///
/// So the folding happens **outside** the library instead: both the mask here
/// and the file name in [`MaskLayer::matches`] go through [`caseless_form`], and
/// the glob itself is compiled **case-sensitive** — `globset`'s default, so
/// there is no flag to see. 🔴 **Exactly one folding mechanism is live.** The
/// flag came off in the same commit that added the transform, deliberately: two
/// of them coexisting would let a bug in the new one be hidden by the old one on
/// every ASCII input, which is most of them.
///
/// Ordering, and it is safe rather than lucky: the checks above run on the mask
/// **as typed**, and the transform runs after. Measured over every Unicode
/// scalar value, `caseless_form` produces none of `* ? [ ] { } ! , - \ /` and
/// never empties a string, so nothing it does can turn a mask that passed these
/// checks into one that should not have.
///
/// The pair of cases that pin the two closed holes are
/// `a_mask_bridges_unicode_normalisation_in_both_directions` and
/// `a_mask_folds_case_outside_ascii`; `a_mask_matches_a_file_name_whatever_its_ascii_case`
/// is the guard on the common case they must not cost —
/// `*.PDF` still removing `report.pdf`.
///
/// ⚠️ **The one thing folding cannot reach, and Task 11's answer to it.**
/// Because the compiled regex is byte-oriented, a character class of non-ASCII
/// letters (`[Гґ]`) is a class of **bytes**, and folding its contents changes
/// that not at all — the byte semantics were never about case. It was booked
/// here as a disclosure and is now a **refusal**:
/// [`RulesError::MaskNonAsciiCharacterClass`], checked by
/// `holds_non_ascii_character_class` and pinned by
/// `a_character_class_of_non_ascii_letters_is_refused`. The measurement that
/// booked it turned out to understate the harm — such a class does not only
/// match nothing when anchored, it matches by BYTE when wrapped in `*` and
/// takes names the person never named — which is what turned a note into a
/// refusal.
fn validate_mask(mask: &str) -> Result<Option<globset::GlobMatcher>, RulesError> {
    if mask.is_empty() {
        return Ok(None);
    }
    if mask == "." || mask == ".." {
        return Err(RulesError::MaskCanNeverNameAFile {
            mask: mask.to_string(),
        });
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
    if holds_non_ascii_character_class(mask) {
        return Err(RulesError::MaskNonAsciiCharacterClass {
            mask: mask.to_string(),
        });
    }
    // Case-sensitive — `globset`'s default — because the folding is done by
    // `caseless_form` on both sides instead. The mask pays for it once, here.
    // The `mask` field on `RulesError::InvalidMask` names the mask as the
    // person typed it. `reason` does not: it is `globset`'s own
    // `err.to_string()`, and quotes the *folded* pattern it tried to
    // compile — someone who typed `[A-_]x.txt` reads about `'[a-_]x.txt'`.
    // Booked to Task 11, which owns the localisation catalogue and can give
    // `reason` its own wording instead of passing it through raw (review,
    // Important 4).
    globset::GlobBuilder::new(&caseless_form(mask))
        .build()
        .map(|glob| Some(glob.compile_matcher()))
        .map_err(|err| RulesError::InvalidMask {
            mask: mask.to_string(),
            reason: err.to_string(),
        })
}

/// Whether any `[...]` in `mask` holds a character outside ASCII — the whole of
/// [`RulesError::MaskNonAsciiCharacterClass`]'s question, and deliberately not
/// one character more.
///
/// **Asked of the mask as TYPED, before [`caseless_form`], and that is a
/// decision with a measured cost.** Folding first would accept one shape this
/// refuses: `U+212A` KELVIN SIGN folds to an ordinary ASCII `k`, so
/// `[\u{212A}].txt` really does match both `k.txt` and `K.txt` (measured), and
/// it is refused here anyway. Asking as typed also refuses one shape folding
/// would let through, and that one is the reason the trade goes this way:
/// `[ß]` folds to `[ss]` — a class of a single `s`, so `[ß]x.txt` matches
/// `sx.txt` (measured) — which is the same "not the rule that was typed"
/// failure by a different mechanism. Refusing a compatibility character almost
/// nobody types costs less than accepting a rule that quietly means something
/// else.
///
/// **No escape handling, and it is not an omission.** `validate_mask` refuses a
/// `\` anywhere before this runs ([`RulesError::MaskContainsBackslash`]), so
/// there is no escaped `[` or `]` for this scan to misread. If that refusal is
/// ever relaxed, this function is one of the places that has to learn about it.
///
/// The two positions `globset` treats specially inside a class are stepped
/// over: a leading `!` is its negation, and a `]` immediately after the opening
/// (or after that `!`) is a literal `]` rather than the end of the class. An
/// unterminated `[` is scanned to the end of the mask, so `[Г` is refused here
/// rather than by the compile probe — either sentence is about the same broken
/// class, and this one names the reason a person can act on.
fn holds_non_ascii_character_class(mask: &str) -> bool {
    let mut chars = mask.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '[' {
            continue;
        }
        if chars.peek() == Some(&'!') {
            chars.next();
        }
        if chars.peek() == Some(&']') {
            chars.next();
        }
        for inside in chars.by_ref() {
            if inside == ']' {
                break;
            }
            if !inside.is_ascii() {
                return true;
            }
        }
    }
    false
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
