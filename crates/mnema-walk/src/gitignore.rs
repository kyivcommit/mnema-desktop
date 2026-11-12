//! The in-tree `.gitignore` stack, decided by this crate rather than by
//! `ignore`'s walker (D202, folders tree PR 3, owner's ruling 2026-11-12).
//!
//! **Why not the walker.** The folders tree has to show which rows `.gitignore`
//! names, and a person can tick one to index it anyway (an *include*). The
//! walker's own stack is private to it, so the tree would need a second
//! reading of `.gitignore` — and two readings drift. The dangerous drift is
//! "the tree says ignored, the scan indexes it": that text goes to the
//! provider (D29). So both the walk's `filter_entry` and the tree's listing ask
//! THIS layer, and nothing else in the product reads a `.gitignore`.
//!
//! **What it reproduces**, measured in `ignore-0.4.31/src/dir.rs` under the
//! configuration `WalkRules::builder` ran before this layer (`require_git(false)`,
//! `git_global(false)`, `parents(false)`, case-sensitive):
//!
//! - every `.gitignore` from the root down applies; a nested `.git` (a
//!   submodule) is NOT a boundary, because `has_git` only turns on under
//!   `require_git(true)` (`dir.rs:345-357`, `:593`);
//! - `<dir>/.git/info/exclude` is read for every folder holding a `.git`
//!   entry; a `.git` FILE (a worktree) names no such path and reads nothing
//!   (`resolve_git_commondir` with `git_type = None`, `:1067`);
//! - precedence: up the chain from the deepest folder, the first match wins,
//!   separately for `.gitignore` and for `exclude` — and then any `.gitignore`
//!   match outranks any `exclude` match (`matched_ignore`, `:548-684`);
//! - a held folder holds everything under it: the walker never descends, so a
//!   `!` line deeper down cannot bring a file back — git behaves the same;
//! - a line that does not parse costs that line, the rest still apply, and
//!   is an error reported through [`GitignoreLayer::take_errors`]; an ignore
//!   file that cannot be READ is passed over silently (`create_gitignore`,
//!   `:1016-1051`, and `push_ignore_io` in `lib.rs`).
//!
//! `tests/gitignore.rs` holds the layer to `ignore` itself on fixtures and on
//! random trees.

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Who holds one row, as far as `.gitignore` and the includes go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitState {
    /// No `.gitignore` names the row or a folder above it.
    Clear,
    /// Held: `by` is the topmost path on the chain a `.gitignore` names (the
    /// row itself or a folder above it), and no include lifts it.
    Ignored { by: String },
    /// Held as `Ignored`, but an include lies strictly below this folder, so
    /// the walk goes in — and keeps only the included paths.
    IgnoredOpen { by: String },
    /// Would be held, and `include` (the row itself or a folder above it)
    /// lifts it.
    Included { include: String },
}

struct DirIgnore {
    gitignore: Gitignore,
    exclude: Gitignore,
}

pub struct GitignoreLayer {
    root: PathBuf,
    includes: Vec<PathBuf>,
    dirs: Mutex<HashMap<PathBuf, Arc<DirIgnore>>>,
    /// Per folder (relative): the topmost path that holds it, if any. Ignoring
    /// the includes — those are asked separately.
    held: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
    errors: Mutex<Vec<ignore::Error>>,
}

impl GitignoreLayer {
    /// `includes` are root-relative paths a person ticked past `.gitignore`;
    /// [`crate::WalkRules::with_includes`] validates them as it does prefixes.
    pub fn new(root: &Path, includes: &[String]) -> Self {
        Self {
            root: root.to_path_buf(),
            includes: includes.iter().map(PathBuf::from).collect(),
            dirs: Mutex::new(HashMap::new()),
            held: Mutex::new(HashMap::new()),
            errors: Mutex::new(Vec::new()),
        }
    }

    /// The state of the row at `relative_path` (`/`-joined, never empty).
    pub fn state(&self, relative_path: &str, is_dir: bool) -> GitState {
        self.state_of(Path::new(relative_path), is_dir)
    }

    /// What the walk asks: whether to prune this entry. An entry an include
    /// covers is never pruned, and the WALK never reads its `.gitignore` files.
    /// [`GitignoreLayer::state`] still does (it names who holds the row before
    /// asking about includes) — read and discarded, nothing the tree shows.
    pub(crate) fn prunes(&self, relative: &Path, is_dir: bool) -> bool {
        if self.covering_include(relative).is_some() {
            return false;
        }
        matches!(self.state_of(relative, is_dir), GitState::Ignored { .. })
    }

    /// Every error met reading an ignore file so far, each once.
    pub fn take_errors(&self) -> Vec<ignore::Error> {
        std::mem::take(&mut *self.errors.lock().unwrap())
    }

    /// The topmost rows at or under `relative_dir` that `.gitignore` holds —
    /// the folder itself when it is held. Does not enter a held folder, a
    /// symlink, an entry `skip(relative, is_dir)` refuses (the built-in list),
    /// or a folder it cannot read (that one gives no rows: the safe direction,
    /// nothing gets included). Names that are not UTF-8 are passed over: no
    /// rule can carry them. Ignores the includes, so build the layer without
    /// them for this question.
    pub fn held_below(&self, relative_dir: &str, skip: &dyn Fn(&str, bool) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        if !relative_dir.is_empty() && self.is_held(Path::new(relative_dir), true) {
            out.push(relative_dir.to_string());
            return out;
        }
        self.collect_held(relative_dir, skip, &mut out);
        out.sort();
        out
    }

    fn collect_held(&self, dir: &str, skip: &dyn Fn(&str, bool) -> bool, out: &mut Vec<String>) {
        let Ok(read) = std::fs::read_dir(self.root.join(dir)) else {
            return;
        };
        let mut names: Vec<(String, bool)> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let kind = e.file_type().ok()?;
                if kind.is_symlink() {
                    return None;
                }
                Some((e.file_name().into_string().ok()?, kind.is_dir()))
            })
            .collect();
        names.sort();
        for (name, is_dir) in names {
            let rel = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if skip(&rel, is_dir) {
                continue;
            }
            if self.is_held(Path::new(&rel), is_dir) {
                out.push(rel);
            } else if is_dir {
                self.collect_held(&rel, skip, out);
            }
        }
    }

    fn state_of(&self, relative: &Path, is_dir: bool) -> GitState {
        let parent = relative.parent().unwrap_or(Path::new(""));
        let by = self
            .held_by(parent)
            .or_else(|| self.named(relative, is_dir).then(|| relative.to_path_buf()));
        let Some(by) = by else {
            return GitState::Clear;
        };
        if let Some(include) = self.covering_include(relative) {
            return GitState::Included {
                include: text(include),
            };
        }
        // No include IS `relative` here: `covering_include` answered above.
        if is_dir && self.includes.iter().any(|i| i.starts_with(relative)) {
            return GitState::IgnoredOpen { by: text(&by) };
        }
        GitState::Ignored { by: text(&by) }
    }

    fn is_held(&self, relative: &Path, is_dir: bool) -> bool {
        let parent = relative.parent().unwrap_or(Path::new(""));
        self.held_by(parent).is_some() || self.named(relative, is_dir)
    }

    fn covering_include(&self, relative: &Path) -> Option<&PathBuf> {
        // `Path::starts_with` compares whole components: `ab` is not under `a`.
        self.includes.iter().find(|i| relative.starts_with(i))
    }

    /// The topmost path holding the FOLDER `dir` (the root is never held).
    fn held_by(&self, dir: &Path) -> Option<PathBuf> {
        if dir.as_os_str().is_empty() {
            return None;
        }
        if let Some(known) = self.held.lock().unwrap().get(dir) {
            return known.clone();
        }
        let parent = dir.parent().unwrap_or(Path::new(""));
        let answer = self
            .held_by(parent)
            .or_else(|| self.named(dir, true).then(|| dir.to_path_buf()));
        self.held
            .lock()
            .unwrap()
            .insert(dir.to_path_buf(), answer.clone());
        answer
    }

    /// Whether the ignore files of the folders above `relative` name it, by
    /// `ignore`'s precedence (module doc).
    fn named(&self, relative: &Path, is_dir: bool) -> bool {
        let path = self.root.join(relative);
        let mut gitignore = Match::None;
        let mut exclude = Match::None;
        let mut dir = relative.parent();
        while let Some(d) = dir {
            let ignores = self.dir_ignore(d);
            if gitignore.is_none() {
                gitignore = ignores.gitignore.matched(&path, is_dir).map(|_| ());
            }
            if exclude.is_none() {
                exclude = ignores.exclude.matched(&path, is_dir).map(|_| ());
            }
            dir = d.parent();
        }
        let decided = if gitignore.is_none() {
            exclude
        } else {
            gitignore
        };
        decided.is_ignore()
    }

    fn dir_ignore(&self, dir: &Path) -> Arc<DirIgnore> {
        if let Some(known) = self.dirs.lock().unwrap().get(dir) {
            return known.clone();
        }
        let abs = self.root.join(dir);
        let gitignore = self.compile(&abs, &abs.join(".gitignore"));
        // No `.git` beside it, or a `.git` FILE: the path does not exist and
        // `compile` reads nothing — `ignore`'s own outcome for both.
        let exclude = self.compile(&abs, &abs.join(".git/info/exclude"));
        let built = Arc::new(DirIgnore { gitignore, exclude });
        self.dirs
            .lock()
            .unwrap()
            .insert(dir.to_path_buf(), built.clone());
        built
    }

    /// `create_gitignore`'s steps for one file: add it only when it exists,
    /// keep what parsed, record every error — except an I/O one. `ignore`
    /// drops those (`maybe_push_ignore_io`, `lib.rs:380-384`), so a
    /// `.gitignore` that cannot be read was passed over and the walk stayed
    /// complete; recording it would stop every deletion under the root for as
    /// long as the file stays unreadable (final review, Important 1).
    fn compile(&self, dir: &Path, file: &Path) -> Gitignore {
        let mut builder = GitignoreBuilder::new(dir);
        if file.exists()
            && let Some(err) = builder.add(file)
            && !err.is_io()
        {
            self.errors.lock().unwrap().push(err);
        }
        match builder.build() {
            Ok(gi) => gi,
            Err(err) => {
                self.errors.lock().unwrap().push(err);
                Gitignore::empty()
            }
        }
    }
}

fn text(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
