use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::path::Path;

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

impl WalkRules {
    /// Directories that are build output, dependency caches or version-control
    /// internals. Not a taste list: measured on this checkout, `target/` alone
    /// is 41 GB and 383,864 of 384,275 files.
    pub const BUILTIN_DIRS: &'static [&'static str] = &[
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        "target",
        "build",
        "dist",
        ".venv",
        "venv",
        "__pycache__",
        ".mypy_cache",
        ".pytest_cache",
        ".gradle",
        ".idea",
        ".vscode",
        ".DS_Store",
    ];

    pub fn new(builtin: bool, gitignore: bool, user_prefixes: Vec<String>) -> Self {
        Self {
            builtin,
            gitignore,
            user_prefixes,
        }
    }

    /// No rules at all. For tests that are about enumeration itself.
    pub fn none() -> Self {
        Self::default()
    }

    pub(crate) fn builder(&self, root: &Path) -> WalkBuilder {
        let mut b = WalkBuilder::new(root);
        // Locality only, not correctness: this sorts siblings within one
        // directory, which is not the same as the flattened relative-path
        // order the caller needs (see the comment on the final sort in
        // `enumerate`, `lib.rs`).
        b.sort_by_file_path(|a, b| a.cmp(b))
            // A symlink cycle is an endless walk, and the same bytes under two
            // names are one document anyway (§5).
            .follow_links(false)
            // Dotfiles are ordinary documents in a watched folder; the built-in
            // list names the dot-directories that are not.
            .hidden(false)
            .git_global(false)
            .git_exclude(self.gitignore)
            .git_ignore(self.gitignore)
            // Load-bearing, and the reason `tests/rules.rs` exists: the default
            // is TRUE, under which no ignore rule applies outside a git
            // repository — silently.
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

        let mut over = OverrideBuilder::new(root);
        if self.builtin {
            for dir in Self::BUILTIN_DIRS {
                // `!` is an exclusion in this builder's grammar; `**` so the
                // directory is removed at any depth, not only at the root.
                let _ = over.add(&format!("!**/{dir}/**"));
                let _ = over.add(&format!("!**/{dir}"));
            }
        }
        for prefix in &self.user_prefixes {
            let trimmed = prefix.trim_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            let _ = over.add(&format!("!{trimmed}/**"));
            let _ = over.add(&format!("!{trimmed}"));
        }
        if let Ok(built) = over.build() {
            b.overrides(built);
        }
        b
    }
}
