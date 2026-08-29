use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::path::Path;
use thiserror::Error;

/// The three layers, in the order they are applied. All of them are live at
/// every walk rather than one-off actions: a rule that newly excludes an
/// already-indexed file removes it on the next walk, which is what makes
/// "I excluded that folder" mean "it is no longer findable" (§5).
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

    /// A file, not a directory. `BUILTIN_DIRS` is documented as directories
    /// and this one is not, so it gets its own list rather than pretending
    /// to be a thirteenth entry in that one (review fix round 1, Minor
    /// finding).
    const BUILTIN_FILES: &'static [&'static str] = &[".DS_Store"];

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
        })
    }

    /// No rules at all. For tests that are about enumeration itself.
    pub fn none() -> Self {
        Self::default()
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

    /// Whether one of the walk's **unconditional** layers prunes the directory
    /// at `relative_path` under `root`, or an ancestor of it — so no user
    /// exclusion rule can change whether anything under it is indexed.
    ///
    /// Written for one caller and one question: the desktop shell's folder
    /// listing has to know, per row, whether offering "exclude this" would be
    /// offering a control that does nothing. Answering it from the shell would
    /// mean a second reading of these constants, and the two would drift.
    ///
    /// **The two layers it covers, re-derived from `builder()` rather than
    /// listed from memory**, and each is read from the same constant
    /// `builder()` reads:
    ///
    /// 1. [`WalkRules::BUILTIN_DIRS`], turned into `!**/{dir}` overrides. The
    ///    pattern matches the directory itself, which prunes its whole
    ///    subtree — so the question is asked of **every component**, not of
    ///    the last one. `.git/hooks` is pruned because `.git` is.
    /// 2. `ANCHORED_DIRS`, checked by `filter_entry`: pruned only when one of
    ///    that name's marker files sits **in its own parent directory**. The
    ///    marker is looked up here the same way `filter_entry` looks it up,
    ///    `parent.join(marker).is_file()`, against the parent this walk of the
    ///    components has reached. `filter_entry` prunes the entry, which
    ///    prunes its subtree, so this too is asked of every component.
    ///
    /// **What it deliberately does NOT cover, and must never be read as
    /// covering:**
    ///
    /// - **The in-tree `.gitignore` stack** (`git_ignore`/`git_exclude`, both
    ///   gated on `gitignore`). Deciding it means compiling the same ignore
    ///   stack the walk builds, per directory, from files inside the tree.
    ///   A folder this function answers `false` for may still be skipped by a
    ///   `.gitignore`; `false` means "no unconditional layer prunes it", never
    ///   "it will be indexed".
    /// - **The user's own exclusion rules.** Those are the caller's to report,
    ///   and they are the ones whose control does something.
    /// - **`BUILTIN_FILES`.** Those name files, not directories, so no
    ///   directory listing can meet one.
    /// - **Symlinks.** `follow_links(false)` is a property of the walker, not
    ///   of a path, and the caller that needs it can see the link itself.
    ///
    /// ⚠️ **Both layers are gated on `builtin` inside `builder()`, and this
    /// function assumes it is on.** Both production call sites pass `true` —
    /// `src-tauri/src/walk_job.rs:128` and `src-tauri/src/bridge.rs:418`; only
    /// tests pass `false`. A caller that built rules with `builtin: false`
    /// must not use this.
    pub fn pruned_by_builtin_layers(root: &Path, relative_path: &str) -> bool {
        let mut parent = root.to_path_buf();
        for component in relative_path.split('/') {
            if Self::BUILTIN_DIRS.contains(&component) {
                return true;
            }
            let anchored = Self::ANCHORED_DIRS.iter().any(|(dir, markers)| {
                *dir == component && markers.iter().any(|marker| parent.join(marker).is_file())
            });
            if anchored {
                return true;
            }
            parent.push(component);
        }
        false
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
        b.filter_entry(move |entry| {
            // The root itself is never a candidate — pruning it would empty
            // the whole walk, not remove one directory from it.
            if !builtin || entry.depth() == 0 {
                return true;
            }
            let Some(name) = entry.file_name().to_str() else {
                return true;
            };
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
            for dir in Self::BUILTIN_DIRS {
                // Matching the directory itself prunes its subtree — a
                // trailing `/**` form is redundant (review fix round 1,
                // Minor finding) and, at the scale user prefixes can reach,
                // is what pushes the combined pattern set past the engine's
                // size limit (the third path in round 1's Critical finding).
                let _ = over.add(&format!("!**/{dir}"));
            }
            for file in Self::BUILTIN_FILES {
                let _ = over.add(&format!("!**/{file}"));
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
