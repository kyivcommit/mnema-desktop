use ignore::WalkBuilder;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct WalkRules {
    pub builtin: bool,
    pub gitignore: bool,
    pub user_prefixes: Vec<String>,
}

impl WalkRules {
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
            .follow_links(false)
            .hidden(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
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
        b
    }
}
