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
    user_prefixes: Vec<String>,
}

/// A user-supplied exclusion prefix that cannot become a rule at all — not
/// "not yet combined with the rest," but wrong on its own. Returned from
/// `WalkRules::new` so a caller with a save dialog in front of the user can
/// refuse the rule right there, which is the only place a human can fix it.
/// A prefix that compiles alone but only fails once combined with the rest
/// of the rule set is a different failure, with nowhere left to report it —
/// see `Walked::rules_applied` (review fix round 1, Critical finding).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RulesError {
    #[error("exclusion rule {prefix:?} could not be compiled: {reason}")]
    InvalidPrefix { prefix: String, reason: String },
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

    /// Fails only when a user prefix cannot become a rule at all — see
    /// `validate_prefix` and `RulesError`. Trying to exclude `target/`
    /// (`WalkRules::new(true, ..)`) never fails here: the built-in list is
    /// a fixed set of literals this crate controls, not user input.
    pub fn new(
        builtin: bool,
        gitignore: bool,
        user_prefixes: Vec<String>,
    ) -> Result<Self, RulesError> {
        for prefix in &user_prefixes {
            validate_prefix(prefix)?;
        }
        Ok(Self {
            builtin,
            gitignore,
            user_prefixes,
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
                // size limit (the third path in the Critical finding).
                let _ = over.add(&format!("!**/{dir}"));
            }
            for file in Self::BUILTIN_FILES {
                let _ = over.add(&format!("!**/{file}"));
            }
        }
        for prefix in &self.user_prefixes {
            let trimmed = prefix.trim_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            // A prefix is a path the user typed, not a pattern: escaped so
            // that a folder named `Photos [2023]` excludes those exact
            // bytes instead of `[2023]` being read as a character class
            // (review fix round 1, Critical finding).
            let _ = over.add(&format!("!{}", globset::escape(trimmed)));
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

/// A prefix is validated alone, in a throwaway `OverrideBuilder`, before it
/// is ever accepted into a `WalkRules` — turning "the pattern engine
/// rejected it" from something `builder()` discovers per-walk, too late for
/// a human to act on, into something `WalkRules::new` refuses outright.
/// Two ways a single, already-escaped prefix can still fail to compile:
/// `globset::escape` does not escape a trailing backslash — a glob-syntax
/// escape character in its own right — so a prefix ending in one still
/// fails; and a single prefix long and specific enough on its own can
/// exceed the pattern engine's size limit without any other rule involved
/// (review fix round 1, Critical finding).
fn validate_prefix(prefix: &str) -> Result<(), RulesError> {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(());
    }
    let pattern = format!("!{}", globset::escape(trimmed));
    let mut probe = OverrideBuilder::new(Path::new("."));
    probe
        .add(&pattern)
        .and_then(|built| built.build())
        .map(|_| ())
        .map_err(|err| RulesError::InvalidPrefix {
            prefix: prefix.to_string(),
            reason: err.to_string(),
        })
}
