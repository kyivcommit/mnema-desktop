//! The folders tree's tick, as ONE change to a root's path rules.
//!
//! A tick is not always one rule. Ticking a file under an excluded folder
//! takes the folder's rule away and puts one on every sibling left unticked
//! (push-down); unticking the last ticked entry of a folder folds the
//! siblings' rules back into one rule on the folder (fold-back); clicking a
//! mixed box removes every rule under the folder. Since the folders tree PR 3
//! a tick may also store or take away an *include* past `.gitignore` (D202).
//! [`plan_tick`] decides the whole change from the disk and the stored rules,
//! and [`set_tick`] writes it in one transaction — never as several commands
//! from the window, because the moment between two of them (the folder on,
//! its siblings not yet excluded) is exactly when a scan would index what the
//! person kept out (spec C, D29). [`preview_tick`] answers the same plan and
//! writes nothing, so a question can name its cost.

use crate::error::Error;
use crate::tree::{SubfolderListing, SubfolderState};
use mnema_walk::{GitignoreLayer, WalkRules};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What the person did to one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tick {
    /// Unticked an indexed entry.
    Off,
    /// Ticked an empty box — an excluded entry, one under an excluded folder,
    /// or one `.gitignore` holds.
    On,
    /// Clicked a mixed box: tick everything inside.
    All,
}

/// The rules a tick takes away and the ones it stores, each sorted: `added` /
/// `removed` are exclusions, `included` / `unincluded` are includes past
/// `.gitignore`. Empty is a real answer: nothing changed (the entry has no
/// control, or is gone).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleChange {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub included: Vec<String>,
    pub unincluded: Vec<String>,
}

/// A root's stored path rules, both kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Stored {
    pub exclusions: Vec<String>,
    pub includes: Vec<String>,
}

/// Store `prefix` in one set, unless the same change takes it away (then it
/// simply stays) or it is stored already.
fn put(added: &mut Vec<String>, removed: &mut Vec<String>, stored: &[String], prefix: &str) {
    if let Some(i) = removed.iter().position(|p| p == prefix) {
        removed.remove(i);
    } else if !stored.iter().any(|p| p == prefix) && !added.iter().any(|p| p == prefix) {
        added.push(prefix.to_string());
    }
}

/// Take `prefix` away from one set, if it is stored or this change was about
/// to store it.
fn take(added: &mut Vec<String>, removed: &mut Vec<String>, stored: &[String], prefix: &str) {
    if let Some(i) = added.iter().position(|p| p == prefix) {
        added.remove(i);
    } else if stored.iter().any(|p| p == prefix) && !removed.iter().any(|p| p == prefix) {
        removed.push(prefix.to_string());
    }
}

fn applied(stored: &[String], added: &[String], removed: &[String]) -> Vec<String> {
    stored
        .iter()
        .filter(|p| !removed.contains(p))
        .chain(added.iter())
        .cloned()
        .collect()
}

impl RuleChange {
    fn add(&mut self, stored: &Stored, prefix: &str) {
        put(
            &mut self.added,
            &mut self.removed,
            &stored.exclusions,
            prefix,
        );
    }

    fn remove(&mut self, stored: &Stored, prefix: &str) {
        take(
            &mut self.added,
            &mut self.removed,
            &stored.exclusions,
            prefix,
        );
    }

    fn include(&mut self, stored: &Stored, prefix: &str) {
        put(
            &mut self.included,
            &mut self.unincluded,
            &stored.includes,
            prefix,
        );
    }

    fn uninclude(&mut self, stored: &Stored, prefix: &str) {
        take(
            &mut self.included,
            &mut self.unincluded,
            &stored.includes,
            prefix,
        );
    }

    /// The stored rules as they would be after this change.
    fn applied_to(&self, stored: &Stored) -> Stored {
        Stored {
            exclusions: applied(&stored.exclusions, &self.added, &self.removed),
            includes: applied(&stored.includes, &self.included, &self.unincluded),
        }
    }

    fn sorted(mut self) -> Self {
        self.added.sort();
        self.removed.sort();
        self.included.sort();
        self.unincluded.sort();
        self
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Whether `prefix` is a strict ancestor of `path`, on a component boundary —
/// `ab` is not under `a`.
fn under(path: &str, prefix: &str) -> bool {
    path.len() > prefix.len() && path.as_bytes()[prefix.len()] == b'/' && path.starts_with(prefix)
}

/// A row that the walk indexes as it stands, for folding (the plan's ruling
/// 4): `UnusableName` counts, because it IS indexed and carries no rule. Since
/// PR 3 an included row counts, and so does a held folder the walk goes into
/// for an include below it (plan ruling 6).
fn ticked(state: &SubfolderState) -> bool {
    matches!(
        state,
        SubfolderState::Open
            | SubfolderState::UnusableName
            | SubfolderState::GitIncluded { .. }
            | SubfolderState::GitIgnoredOpen { .. }
    )
}

fn held_by_gitignore(state: &SubfolderState) -> bool {
    matches!(
        state,
        SubfolderState::GitIgnored { .. } | SubfolderState::GitIgnoredOpen { .. }
    )
}

/// One level of the disk under a set of rules — see [`plan_tick`].
pub(crate) type ReadLevel<'a> = dyn FnMut(&str, &Stored) -> Result<SubfolderListing, Error> + 'a;

/// The state of the row at `path` under `rules`, read from its parent's level.
fn state_at(
    level: &mut ReadLevel<'_>,
    path: &str,
    rules: &Stored,
) -> Result<Option<SubfolderState>, Error> {
    Ok(level(parent(path), rules)?
        .entries
        .into_iter()
        .find(|e| e.relative_path == path)
        .map(|e| e.state))
}

/// The whole change one tick makes. `level(dir, rules)` lists one level of
/// the disk classified under `rules` — the command hands it the real
/// directory, read through the same `read_subfolders` the tree draws from, so
/// the siblings a push-down rules out are the disk's, not the window's.
/// `held_below(dir)` lists the topmost rows at or under `dir` that `.gitignore`
/// holds, ignoring includes ([`GitignoreLayer::held_below`]).
pub(crate) fn plan_tick(
    root_id: i64,
    stored: &Stored,
    path: &str,
    tick: Tick,
    level: &mut ReadLevel<'_>,
    held_below: &mut dyn FnMut(&str) -> Vec<String>,
) -> Result<RuleChange, Error> {
    let mut change = RuleChange::default();
    let Some(state) = state_at(level, path, stored)? else {
        return Ok(change);
    };
    match (tick, &state) {
        (Tick::Off, SubfolderState::Open | SubfolderState::GitIncluded { .. }) => {
            // The row's OWN include goes back to `.gitignore`; anything else
            // that is on gets a rule (plan ruling 4).
            match &state {
                SubfolderState::GitIncluded { include } if include == path => {
                    change.uninclude(stored, path)
                }
                _ => change.add(stored, path),
            }
            // Fold-back: climb while the folder has nothing ticked left. The
            // root is never folded — no rule can name it — and neither is a
            // folder `.gitignore` holds: a rule there would change nothing
            // (spec, rule 3; plan ruling 6).
            let mut dir = parent(path);
            while !dir.is_empty() {
                let now = change.applied_to(stored);
                let listing = level(dir, &now)?;
                if listing.entries.iter().any(|e| ticked(&e.state)) {
                    break;
                }
                if state_at(level, dir, &now)?.is_some_and(|s| held_by_gitignore(&s)) {
                    break;
                }
                // Only the direct children's own rules (the plan's ruling 7):
                // a deeper rule is harmless under the folder's and acts again
                // after a later push-down.
                for e in &listing.entries {
                    change.remove(stored, &e.relative_path);
                }
                change.add(stored, dir);
                dir = parent(dir);
            }
        }
        (Tick::On, SubfolderState::Excluded) => change.remove(stored, path),
        (Tick::On, SubfolderState::ExcludedByAncestor { prefix }) => {
            // Push-down, from the outermost rule to the ticked entry: every
            // rule on the chain goes, every sibling off the chain gets one.
            let mut dir = prefix.clone();
            loop {
                change.remove(stored, &dir);
                let now = change.applied_to(stored);
                let listing = level(&dir, &now)?;
                let next = match path[dir.len() + 1..].find('/') {
                    Some(i) => &path[..dir.len() + 1 + i],
                    None => path,
                };
                // The next link of the chain is ruled out here too and taken
                // back one step later (`remove` cancels an `add` of the same
                // change), so it needs no case of its own.
                for e in &listing.entries {
                    if matches!(e.state, SubfolderState::BuiltIn | SubfolderState::Symlink) {
                        continue;
                    }
                    let carriable = WalkRules::check_prefix(&e.relative_path).is_ok();
                    // A sibling `.gitignore` holds needs no rule to stay out,
                    // so a name no rule can carry is no reason to refuse
                    // (plan ruling 7) — it simply gets none.
                    if !carriable && held_by_gitignore(&e.state) {
                        continue;
                    }
                    // A sibling no rule can carry would start being indexed
                    // without a word: refuse the whole tick (the plan's ruling 5).
                    if !carriable {
                        return Err(Error::CannotExcludeBeside {
                            root_id,
                            relative_path: e.relative_path.clone(),
                        });
                    }
                    change.add(stored, &e.relative_path);
                }
                if next == path {
                    break;
                }
                dir = next.to_string();
            }
            change.remove(stored, path);
        }
        // `GitIncluded` too: an included folder with a rule inside is mixed
        // in the window, and its click must do what it asks (final review,
        // Important 2).
        (
            Tick::All,
            SubfolderState::Open
            | SubfolderState::GitIgnoredOpen { .. }
            | SubfolderState::GitIncluded { .. },
        ) => {
            for prefix in stored.exclusions.iter().filter(|p| under(p, path)) {
                change.remove(stored, prefix);
            }
            for prefix in stored.includes.iter().filter(|p| under(p, path)) {
                change.uninclude(stored, prefix);
            }
            // Each topmost row `.gitignore` holds, not one include on the
            // folder: a `.gitignore` line added later must still act under it
            // (plan ruling 8).
            for held in held_below(path) {
                // A name no include can carry stays out — the safe direction.
                if WalkRules::check_prefix(&held).is_ok() {
                    change.include(stored, &held);
                }
            }
        }
        _ => {}
    }
    // Whatever a tick turned ON that `.gitignore` still holds gets its include
    // in the same change — a push-down under a folder `.gitignore` also names
    // is one press (plan ruling 5).
    if tick == Tick::On {
        let now = change.applied_to(stored);
        if let Some(SubfolderState::GitIgnored { .. }) = state_at(level, path, &now)? {
            WalkRules::check_prefix(path)?;
            change.include(stored, path);
        }
    }
    Ok(change.sorted())
}

/// [`plan_tick`] against the real folder at `root` (already canonical): the
/// levels through the listing's own `read_level`, under the walk's own
/// `.gitignore` layer built from the rules each step proposes.
pub(crate) fn plan_on_disk(
    root: &Path,
    root_id: i64,
    stored: &Stored,
    mask_names: &[String],
    skips: &[mnema_index::SkippedFile],
    path: &str,
    tick: Tick,
) -> Result<RuleChange, Error> {
    let walk = WalkRules::none().with_masks(mask_names.to_vec())?;
    let layers = WalkRules::builtin_layers(root);
    let mut level = |dir: &str, rules: &Stored| {
        let git = GitignoreLayer::new(root, &rules.includes);
        let level_rules = crate::tree::LevelRules::new(
            root,
            &layers,
            &rules.exclusions,
            walk.masks(),
            mask_names,
            skips,
            &git,
        );
        let resolved = crate::tree::subfolder_dir(root, root_id, dir)?;
        crate::tree::read_level(&resolved, root_id, dir, &level_rules)
    };
    let bare = GitignoreLayer::new(root, &[]);
    let masks = walk.masks().clone();
    let mut held_below = |dir: &str| {
        bare.held_below(dir, &|relative, is_dir| {
            layers.prunes_entry(relative, is_dir) || (!is_dir && masks.matches(relative))
        })
    };
    plan_tick(root_id, stored, path, tick, &mut level, &mut held_below)
}

/// The guards `set_tick` and `preview_tick` share, then the plan.
///
/// The same guards as `exclude_subfolder` (`bridge.rs`): an unknown root, a
/// root that is not there, a blank path, a path no rule can carry.
fn planned(
    state: &crate::state::AppState,
    root_id: i64,
    relative_path: &str,
    tick: Tick,
) -> Result<RuleChange, Error> {
    let root = state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(Error::RootUnavailable(root_id));
    }
    if relative_path.is_empty() {
        return Err(Error::BlankExclusionRule);
    }
    // Only a tick that STORES this path as a rule has to pass the validator.
    // Taking a rule away must always work, including one an older build
    // stored under a name today's validator refuses (`include_subfolder`).
    if tick == Tick::Off {
        WalkRules::check_prefix(relative_path)?;
    }
    let root_canonical = root_path
        .canonicalize()
        .map_err(|_| Error::RootUnavailable(root_id))?;
    let (stored, mask_names, skips) = state.with_index(|db| {
        Ok::<_, mnema_index::Error>((
            Stored {
                exclusions: db.list_path_exclusions(root_id)?,
                includes: db.list_gitignore_includes(root_id)?,
            },
            db.list_masks()?,
            crate::tree::whole_file_skips(db.skips_for_root(root_id)?),
        ))
    })?;
    plan_on_disk(
        &root_canonical,
        root_id,
        &stored,
        &mask_names,
        &skips,
        relative_path,
        tick,
    )
}

/// Off the main thread for the reason given on [`crate::bridge::open_index`].
///
/// The change is planned from the disk and the stored rules and written in
/// one transaction ([`mnema_index::Db::change_path_rules`]).
///
/// ⚠️ The plan is read outside the index lock and written inside it. The only
/// writer of these rules is this window, one press at a time, so nothing moves
/// them in between; a second writer would need the read inside the write.
#[tauri::command(async)]
pub fn set_tick(
    state: tauri::State<'_, crate::state::AppState>,
    root_id: i64,
    relative_path: String,
    tick: Tick,
) -> Result<RuleChange, Error> {
    let change = planned(&state, root_id, &relative_path, tick)?;
    if change != RuleChange::default() {
        state.with_index(|db| {
            db.change_path_rules(
                root_id,
                &mnema_index::PathRuleChange {
                    excluded_removed: change.removed.clone(),
                    excluded_added: change.added.clone(),
                    included_removed: change.unincluded.clone(),
                    included_added: change.included.clone(),
                },
            )
        })?;
    }
    Ok(change)
}

/// What [`set_tick`] would change, written nowhere: the window's question
/// about a mixed box names how many rules go and how many rows `.gitignore`
/// named start being indexed (plan ruling 9).
#[tauri::command(async)]
pub fn preview_tick(
    state: tauri::State<'_, crate::state::AppState>,
    root_id: i64,
    relative_path: String,
    tick: Tick,
) -> Result<RuleChange, Error> {
    planned(&state, root_id, &relative_path, tick)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Subfolder, SubfolderListing, SubfolderState};
    use std::collections::HashMap;

    /// One directory's entries, by name, with the kind of entry each is and
    /// how the listing would classify it if no rule held it.
    type Disk = HashMap<&'static str, Vec<(&'static str, bool, Base)>>;

    /// The state an entry has when no rule of the person's holds it — the
    /// part of the classification that does not depend on the prefixes.
    #[derive(Clone, Copy)]
    enum Base {
        Plain,
        BuiltIn,
        Masked,
        NotRead,
        Unusable,
    }

    fn is_ancestor(prefix: &str, path: &str) -> bool {
        path.len() > prefix.len()
            && path.as_bytes()[prefix.len()] == b'/'
            && path.starts_with(prefix)
    }

    /// A fake `level`: classifies each entry the way `subfolder_state` does —
    /// built-in, masked and not-read first, then the person's rules, then the
    /// name — over the prefixes it is handed.
    fn level_of(disk: &Disk) -> impl FnMut(&str, &Stored) -> Result<SubfolderListing, Error> + '_ {
        move |dir, rules| {
            let prefixes = &rules.exclusions;
            let entries = disk
                .get(dir)
                .unwrap_or_else(|| panic!("plan_tick read {dir:?}, which the fixture has not"))
                .iter()
                .map(|&(name, is_dir, base)| {
                    let relative_path = if dir.is_empty() {
                        name.to_string()
                    } else {
                        format!("{dir}/{name}")
                    };
                    let ancestor = prefixes.iter().find(|p| is_ancestor(p, &relative_path));
                    let state = match base {
                        Base::BuiltIn => SubfolderState::BuiltIn,
                        Base::Masked => SubfolderState::Masked {
                            mask: "*.log".to_string(),
                        },
                        Base::NotRead => SubfolderState::NotRead,
                        _ if ancestor.is_some() => SubfolderState::ExcludedByAncestor {
                            prefix: ancestor.unwrap().clone(),
                        },
                        _ if prefixes.contains(&relative_path) => SubfolderState::Excluded,
                        Base::Unusable => SubfolderState::UnusableName,
                        Base::Plain => SubfolderState::Open,
                    };
                    Subfolder {
                        name: name.to_string(),
                        relative_path,
                        is_dir,
                        state,
                        failure: None,
                        failed_below: 0,
                    }
                })
                .collect();
            Ok(SubfolderListing {
                entries,
                unnameable: 0,
            })
        }
    }

    fn plan(disk: &Disk, prefixes: &[&str], path: &str, tick: Tick) -> Result<RuleChange, Error> {
        let stored = Stored {
            exclusions: prefixes.iter().map(|p| p.to_string()).collect(),
            includes: Vec::new(),
        };
        plan_tick(1, &stored, path, tick, &mut level_of(disk), &mut |_| {
            Vec::new()
        })
    }

    fn change(added: &[&str], removed: &[&str]) -> RuleChange {
        RuleChange {
            added: added.iter().map(|p| p.to_string()).collect(),
            removed: removed.iter().map(|p| p.to_string()).collect(),
            ..RuleChange::default()
        }
    }

    use Base::*;

    #[test]
    fn off_on_an_open_file_adds_its_rule_only() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain)]),
            ("a", vec![("b.pdf", false, Plain), ("c.pdf", false, Plain)]),
        ]);
        assert_eq!(
            plan(&disk, &[], "a/b.pdf", Tick::Off).unwrap(),
            change(&["a/b.pdf"], &[])
        );
    }

    #[test]
    fn off_on_the_last_ticked_folds_the_folder() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain), ("z", true, Plain)]),
            ("a", vec![("b.pdf", false, Plain), ("c.pdf", false, Plain)]),
        ]);
        assert_eq!(
            plan(&disk, &["a/b.pdf"], "a/c.pdf", Tick::Off).unwrap(),
            change(&["a"], &["a/b.pdf"])
        );
    }

    /// `A/B/x` is the last ticked entry of `B`, and `B` the last ticked entry
    /// of `A`: the fold climbs to `A` and stops at the root, which no rule can
    /// name — even though nothing at the top level is ticked any more.
    #[test]
    fn folding_climbs_while_the_parent_empties_and_stops_at_the_root() {
        let disk = Disk::from([
            ("", vec![("A", true, Plain)]),
            ("A", vec![("B", true, Plain), ("y.pdf", false, Plain)]),
            ("A/B", vec![("x", false, Plain), ("w", false, Plain)]),
        ]);
        assert_eq!(
            plan(&disk, &["A/y.pdf", "A/B/w"], "A/B/x", Tick::Off).unwrap(),
            change(&["A"], &["A/B/w", "A/y.pdf"])
        );
    }

    #[test]
    fn a_masked_or_not_read_sibling_does_not_block_folding() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain), ("z", true, Plain)]),
            (
                "a",
                vec![
                    ("b.pdf", false, Plain),
                    ("c.log", false, Masked),
                    ("p.jpg", false, NotRead),
                    ("node_modules", true, BuiltIn),
                ],
            ),
        ]);
        assert_eq!(
            plan(&disk, &[], "a/b.pdf", Tick::Off).unwrap(),
            change(&["a"], &[])
        );
    }

    /// The plan's ruling 4: a name no rule can carry IS indexed, so a folder
    /// holding one is not "all off".
    #[test]
    fn an_unusable_name_sibling_blocks_folding() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain), ("z", true, Plain)]),
            ("a", vec![("b.pdf", false, Plain), ("x ", false, Unusable)]),
        ]);
        assert_eq!(
            plan(&disk, &[], "a/b.pdf", Tick::Off).unwrap(),
            change(&["a/b.pdf"], &[])
        );
    }

    /// The plan's ruling 7: only the direct children's own rules fold into the
    /// folder's; a rule deeper down stays, and acts again after a push-down.
    #[test]
    fn folding_keeps_deeper_rules() {
        let disk = Disk::from([
            ("", vec![("A", true, Plain), ("z", true, Plain)]),
            ("A", vec![("B", true, Plain), ("old", true, Plain)]),
        ]);
        let planned = plan(&disk, &["A/old", "A/old/deep"], "A/B", Tick::Off).unwrap();
        assert_eq!(planned, change(&["A"], &["A/old"]));
    }

    #[test]
    fn on_under_an_excluded_folder_pushes_the_rule_down() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain)]),
            (
                "a",
                vec![
                    ("b.pdf", false, Plain),
                    ("c.pdf", false, Plain),
                    ("d", true, Plain),
                ],
            ),
        ]);
        assert_eq!(
            plan(&disk, &["a"], "a/b.pdf", Tick::On).unwrap(),
            change(&["a/c.pdf", "a/d"], &["a"])
        );
    }

    #[test]
    fn push_down_through_two_levels_opens_only_the_ticked_path() {
        let disk = Disk::from([
            ("", vec![("A", true, Plain)]),
            (
                "A",
                vec![
                    ("B", true, Plain),
                    ("C", true, Plain),
                    ("e.txt", false, Plain),
                ],
            ),
            (
                "A/B",
                vec![("c.pdf", false, Plain), ("d.pdf", false, Plain)],
            ),
        ]);
        assert_eq!(
            plan(&disk, &["A"], "A/B/c.pdf", Tick::On).unwrap(),
            change(&["A/B/d.pdf", "A/C", "A/e.txt"], &["A"])
        );
    }

    /// A rule on a folder in the middle of the chain goes too, or the ticked
    /// file stays held by it.
    #[test]
    fn push_down_removes_every_rule_on_the_chain() {
        let disk = Disk::from([
            ("", vec![("A", true, Plain)]),
            ("A", vec![("B", true, Plain)]),
            (
                "A/B",
                vec![("c.pdf", false, Plain), ("d.pdf", false, Plain)],
            ),
        ]);
        assert_eq!(
            plan(&disk, &["A", "A/B", "A/B/c.pdf"], "A/B/c.pdf", Tick::On).unwrap(),
            change(&["A/B/d.pdf"], &["A", "A/B", "A/B/c.pdf"])
        );
    }

    #[test]
    fn push_down_does_not_touch_a_sibling_whose_name_starts_with_the_path() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain)]),
            ("a", vec![("b", true, Plain), ("bc", true, Plain)]),
        ]);
        assert_eq!(
            plan(&disk, &["a"], "a/b", Tick::On).unwrap(),
            change(&["a/bc"], &["a"])
        );
    }

    #[test]
    fn push_down_refuses_beside_an_unusable_name() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain)]),
            ("a", vec![("b.pdf", false, Plain), ("x ", false, Unusable)]),
        ]);
        match plan(&disk, &["a"], "a/b.pdf", Tick::On) {
            Err(Error::CannotExcludeBeside {
                root_id: 1,
                relative_path,
            }) => assert_eq!(relative_path, "a/x "),
            other => panic!("expected CannotExcludeBeside, got {other:?}"),
        }
    }

    /// The plan's ruling 6: masked and not-read siblings get a rule too, so
    /// they stay out if the mask is later removed; built-in ones do not, the
    /// walk never reads them.
    #[test]
    fn push_down_rules_out_masked_and_not_read_siblings() {
        let disk = Disk::from([
            ("", vec![("a", true, Plain)]),
            (
                "a",
                vec![
                    ("b.pdf", false, Plain),
                    ("c.log", false, Masked),
                    ("p.jpg", false, NotRead),
                    ("node_modules", true, BuiltIn),
                ],
            ),
        ]);
        assert_eq!(
            plan(&disk, &["a"], "a/b.pdf", Tick::On).unwrap(),
            change(&["a/c.log", "a/p.jpg"], &["a"])
        );
    }

    #[test]
    fn on_on_an_excluded_folder_removes_its_own_rule_only() {
        let disk = Disk::from([("", vec![("a", true, Plain)])]);
        assert_eq!(
            plan(&disk, &["a", "a/deep"], "a", Tick::On).unwrap(),
            change(&[], &["a"])
        );
    }

    #[test]
    fn all_removes_every_rule_strictly_under_the_folder() {
        let disk = Disk::from([("", vec![("a", true, Plain), ("ab", true, Plain)])]);
        assert_eq!(
            plan(&disk, &["a/x", "a/y/z", "ab"], "a", Tick::All).unwrap(),
            change(&[], &["a/x", "a/y/z"])
        );
    }

    #[test]
    fn a_tick_on_a_state_without_a_control_changes_nothing() {
        let disk = Disk::from([(
            "",
            vec![
                ("node_modules", true, BuiltIn),
                ("c.log", false, Masked),
                ("p.jpg", false, NotRead),
            ],
        )]);
        for path in ["node_modules", "c.log", "p.jpg"] {
            for tick in [Tick::Off, Tick::On, Tick::All] {
                assert_eq!(
                    plan(&disk, &[], path, tick).unwrap(),
                    RuleChange::default(),
                    "{path} {tick:?}"
                );
            }
        }
    }

    /// A path the disk no longer has is an empty change: the window re-reads
    /// and draws what is there.
    #[test]
    fn a_path_that_is_not_there_changes_nothing() {
        let disk = Disk::from([("", vec![("a", true, Plain)])]);
        assert_eq!(
            plan(&disk, &[], "gone.pdf", Tick::Off).unwrap(),
            RuleChange::default()
        );
    }

    /// D202 PR 3: ticks over `.gitignore`, planned against a REAL folder and
    /// the walk's own `.gitignore` layer — the same `plan_on_disk` the
    /// commands call, so no fake reimplements `.gitignore` here.
    mod gitignore {
        use super::super::*;
        use std::fs;

        fn disk(files: &[(&str, &str)]) -> tempfile::TempDir {
            let root = tempfile::tempdir().unwrap();
            for (path, body) in files {
                let full = root.path().join(path);
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(full, body).unwrap();
            }
            root
        }

        fn strings(paths: &[&str]) -> Vec<String> {
            paths.iter().map(|p| p.to_string()).collect()
        }

        fn plan(
            root: &tempfile::TempDir,
            exclusions: &[&str],
            includes: &[&str],
            path: &str,
            tick: Tick,
        ) -> Result<RuleChange, Error> {
            let root = root.path().canonicalize().unwrap();
            let stored = Stored {
                exclusions: strings(exclusions),
                includes: strings(includes),
            };
            plan_on_disk(&root, 1, &stored, &[], &[], path, tick)
        }

        fn change(
            added: &[&str],
            removed: &[&str],
            included: &[&str],
            unincluded: &[&str],
        ) -> RuleChange {
            RuleChange {
                added: strings(added),
                removed: strings(removed),
                included: strings(included),
                unincluded: strings(unincluded),
            }
        }

        #[test]
        fn on_on_a_gitignored_row_adds_its_include_only() {
            let root = disk(&[(".gitignore", "*.log\n"), ("a.log", "x"), ("b.txt", "x")]);
            assert_eq!(
                plan(&root, &[], &[], "a.log", Tick::On).unwrap(),
                change(&[], &[], &["a.log"], &[])
            );
        }

        #[test]
        fn off_on_an_included_row_removes_its_include_and_adds_no_rule() {
            let root = disk(&[(".gitignore", "*.log\n"), ("a.log", "x"), ("b.txt", "x")]);
            assert_eq!(
                plan(&root, &[], &["a.log"], "a.log", Tick::Off).unwrap(),
                change(&[], &[], &[], &["a.log"])
            );
        }

        #[test]
        fn off_under_an_include_adds_an_exclusion() {
            let root = disk(&[
                (".gitignore", "vendor/\n"),
                ("vendor/a.txt", "x"),
                ("vendor/b.txt", "x"),
            ]);
            assert_eq!(
                plan(&root, &[], &["vendor"], "vendor/a.txt", Tick::Off).unwrap(),
                change(&["vendor/a.txt"], &[], &[], &[])
            );
        }

        #[test]
        fn on_under_a_rule_and_gitignore_pushes_down_and_includes_in_one_change() {
            let root = disk(&[
                (".gitignore", "build/\n"),
                ("build/x.txt", "x"),
                ("build/y.txt", "x"),
            ]);
            assert_eq!(
                plan(&root, &["build"], &[], "build/x.txt", Tick::On).unwrap(),
                change(&["build/y.txt"], &["build"], &["build/x.txt"], &[])
            );
        }

        #[test]
        fn folding_stops_at_a_folder_gitignore_holds() {
            let root = disk(&[
                (".gitignore", "build/\n"),
                ("build/x/keep.txt", "x"),
                ("build/x/other.txt", "x"),
            ]);
            assert_eq!(
                plan(
                    &root,
                    &[],
                    &["build/x/keep.txt"],
                    "build/x/keep.txt",
                    Tick::Off
                )
                .unwrap(),
                change(&[], &[], &[], &["build/x/keep.txt"])
            );
        }

        #[test]
        fn push_down_rules_out_a_gitignored_sibling_without_refusing_its_name() {
            let root = disk(&[
                (".gitignore", "a/c*\n*.log\n"),
                ("a/b.txt", "x"),
                ("a/c\\x", "x"),
                ("a/d.log", "x"),
            ]);
            assert_eq!(
                plan(&root, &["a"], &[], "a/b.txt", Tick::On).unwrap(),
                change(&["a/d.log"], &["a"], &[], &[])
            );
        }

        #[test]
        fn all_includes_the_topmost_held_rows_and_drops_rules_under_the_folder() {
            let root = disk(&[
                (".gitignore", "*.log\nbuild/\n"),
                ("a/x.log", "x"),
                ("a/build/z.txt", "x"),
                ("a/y.txt", "x"),
                ("a/sub/q.txt", "x"),
            ]);
            assert_eq!(
                plan(&root, &["a/sub"], &["a/build/z.txt"], "a", Tick::All).unwrap(),
                change(&[], &["a/sub"], &["a/build", "a/x.log"], &["a/build/z.txt"])
            );
        }

        #[test]
        fn all_on_a_held_open_folder_includes_the_folder() {
            let root = disk(&[
                (".gitignore", "build/\n"),
                ("build/x/keep.txt", "x"),
                ("build/y.txt", "x"),
            ]);
            assert_eq!(
                plan(&root, &[], &["build/x/keep.txt"], "build", Tick::All).unwrap(),
                change(&[], &[], &["build"], &["build/x/keep.txt"])
            );
        }

        /// Final review, Important 2: an included folder with an exclusion
        /// inside shows `–`; its click must take that exclusion away.
        #[test]
        fn all_on_an_included_folder_removes_the_exclusions_inside() {
            let root = disk(&[
                (".gitignore", "vendor/\n"),
                ("vendor/a.txt", "x"),
                ("vendor/b.txt", "x"),
            ]);
            assert_eq!(
                plan(&root, &["vendor/a.txt"], &["vendor"], "vendor", Tick::All).unwrap(),
                change(&[], &["vendor/a.txt"], &[], &[])
            );
        }

        #[test]
        fn an_included_sibling_blocks_folding() {
            let root = disk(&[
                (".gitignore", "*.log\n"),
                ("a/b.txt", "x"),
                ("a/c.log", "x"),
            ]);
            assert_eq!(
                plan(&root, &[], &["a/c.log"], "a/b.txt", Tick::Off).unwrap(),
                change(&["a/b.txt"], &[], &[], &[])
            );
        }

        #[test]
        fn on_refuses_an_include_no_rule_can_carry() {
            let root = disk(&[(".gitignore", "*.log\n"), ("a .log", "x")]);
            fs::rename(root.path().join("a .log"), root.path().join(" a.log")).unwrap();
            assert!(plan(&root, &[], &[], " a.log", Tick::On).is_err());
        }
    }
}
