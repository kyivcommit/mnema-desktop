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
    /// Always in canonical form once a `WalkRules` exists: no leading `./`,
    /// no surrounding `/`, never empty, never absolute, never containing a
    /// backslash or trailing whitespace. `WalkRules::new` is the only public
    /// way to populate this (besides `default`/`none`, which leave it
    /// empty), and it refuses or normalises everything on the way in — see
    /// `normalize_prefix`. `builder()` relies on that: it does not re-trim
    /// or re-check these before turning them into patterns.
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
/// Four variants, not one: review fix round 2 measured that `ignore`'s
/// override matcher is, underneath, exactly a `.gitignore` line parser
/// (`overrides.rs` forwards straight into `GitignoreBuilder::add_line`), so
/// every one of that parser's quirks applies to a user prefix. Three of them
/// compile to `Ok` and then match the wrong thing — the fourth kind of wrong
/// this type has to name — so each gets a distinct, actionable message
/// instead of being folded into `InvalidPrefix`, which is reserved for
/// "the pattern engine rejected this outright."
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RulesError {
    #[error("exclusion rule {prefix:?} could not be compiled: {reason}")]
    InvalidPrefix { prefix: String, reason: String },
    /// `gitignore.rs` compiles every glob with `backslash_escape(true))`
    /// unconditionally, so a `\` anywhere in the pattern — not only a
    /// trailing one, which `globset::escape` cannot help with because it
    /// does not touch backslash at all — is read as an escape character.
    /// `a\bee` compiles to the literal `abee`: the named folder survives,
    /// and an unrelated `abee/` is excluded in its place. No rewrite is
    /// unambiguous, so this is refused rather than normalised.
    #[error("exclusion rule {prefix:?} cannot contain a backslash — name the folder without one")]
    ContainsBackslash { prefix: String },
    /// `add_line` silently trims trailing whitespace unless the line ends in
    /// `\ ` (an escaped space) — reachable through us, since nothing here
    /// ever emits that escape — so a prefix naming a folder with a trailing
    /// space compiles to a pattern for a same-named folder WITHOUT one.
    #[error(
        "exclusion rule {prefix:?} has trailing whitespace, which the pattern engine silently \
         drops — remove it"
    )]
    TrailingWhitespace { prefix: String },
    /// A `/`-prefixed pattern can only ever match the beginning of a path
    /// relative to the override root — so an absolute filesystem path (the
    /// shape a folder picker or a paste from Finder produces) compiles to a
    /// pattern that can never match anything under any watched root, ever,
    /// with `new` returning `Ok`. Refused outright: there is no watched
    /// root in scope here to make it relative against safely. A leading
    /// `./` is a different, genuinely relative idiom and is normalised
    /// instead — see `normalize_prefix`.
    #[error(
        "exclusion rule {prefix:?} is an absolute path — exclusion rules are relative to the \
         watched folder"
    )]
    AbsolutePrefix { prefix: String },
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

    /// Fails when a user prefix cannot become a rule at all, or normalises
    /// it into the one canonical form `builder()` trusts without
    /// re-checking — see `normalize_prefix` and `RulesError`. Trying to
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
        // Already normalised by `WalkRules::new` — no leading `./`, no
        // surrounding `/`, never absolute, never containing a backslash or
        // trailing whitespace, never empty. Nothing left to do here but
        // turn each one into a rooted pattern.
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

/// Turns a raw, user-typed prefix into the canonical form `WalkRules`
/// stores — or refuses it, with a message a person can act on, when there
/// is no safe way to compile it at all. `Ok(None)` means "no rule": an
/// empty prefix, or one that normalises down to nothing (bare `/`s, a bare
/// `./`), the same as a blank line in an exclusion list rather than an
/// error.
///
/// `ignore`'s override matcher is, underneath, exactly a `.gitignore` line
/// parser (`overrides.rs` forwards straight into
/// `GitignoreBuilder::add_line`), so every one of that parser's quirks
/// applies to a prefix that reaches it. Review fix round 2 measured three
/// that compile to `Ok` and then match the wrong thing — a defect in the
/// same shape as round 1's escaping bug, just in territory `globset::escape`
/// does not cover:
///
/// 1. A `\` anywhere (not only trailing) is a live escape character to the
///    pattern compiler regardless of escaping — refused, `ContainsBackslash`.
/// 2. Trailing whitespace is silently trimmed by the pattern compiler —
///    refused, `TrailingWhitespace`, checked against the prefix's own
///    `trim_end()` so only genuinely trailing whitespace (not a legitimate
///    leading space, which the compiler does NOT trim) is caught.
/// 3. An absolute path can never match anything relative to a watched root
///    — refused, `AbsolutePrefix`, checked on the raw prefix, before any
///    trimming removes the very leading `/` that makes it absolute. A
///    leading `./` is different: genuinely relative once dropped, so it is
///    stripped rather than refused.
///
/// (The fourth thing round 2 found — a one-component prefix matching at
/// every depth instead of only the root — is not a prefix defect at all,
/// so there is nothing to normalise or refuse here for it; it is fixed in
/// `anchored_pattern` instead, at the one place the pattern is built.)
fn validate_prefix(prefix: &str) -> Result<Option<String>, RulesError> {
    if prefix.contains('\\') {
        return Err(RulesError::ContainsBackslash {
            prefix: prefix.to_string(),
        });
    }
    if prefix != prefix.trim_end() {
        return Err(RulesError::TrailingWhitespace {
            prefix: prefix.to_string(),
        });
    }
    // Checked on the RAW prefix, before any trimming: stripping a leading
    // `/` first would erase the very evidence an absolute path is absolute
    // (review fix round 2, Critical finding). Anything that reaches the
    // trim below is therefore already known not to start with `/`, so only
    // a TRAILING one is ever trimmed here — a leading one would mean this
    // function already returned.
    if Path::new(prefix).is_absolute() {
        return Err(RulesError::AbsolutePrefix {
            prefix: prefix.to_string(),
        });
    }
    let trimmed = prefix.trim_end_matches('/');
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Ok(None);
    }

    // Compile-probe, alone, in a throwaway `OverrideBuilder`: catches a
    // single prefix pathological enough on its own to exceed the pattern
    // engine's size limit even after every other check above has passed
    // (review fix round 1, Critical finding, third path). Must build the
    // exact pattern `builder()` will use, leading `/` included, or this
    // probe and the real walk could disagree about what compiles.
    let mut probe = OverrideBuilder::new(Path::new("."));
    probe
        .add(&anchored_pattern(trimmed))
        .and_then(|built| built.build())
        .map(|_| ())
        .map_err(|err| RulesError::InvalidPrefix {
            prefix: prefix.to_string(),
            reason: err.to_string(),
        })?;

    Ok(Some(trimmed.to_string()))
}
