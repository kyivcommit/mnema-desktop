# `list_subfolders` — the exclusion screen's folder tree, read off the disk
# (task 4 of PR 8a). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-subfolders.sh
#
# Thirteen cases, all against `src-tauri/src/tree.rs`. Ten are killed by tests
# in `tests/commands.rs` (`--test commands`), which is where a command is
# exercised the way the webview reaches it; three by `tree.rs`'s own `mod
# tests` (`--lib`), and those three are not a shortcut — see the next note.
#
# ⚠️ **Why three cases are killed by a unit test rather than through the IPC.**
# Two of them are about a directory whose name is not valid UTF-8, and that
# state **cannot be built on this project's macOS leg**: APFS refuses
# `create_dir` for such a name outright with `EILSEQ` ("Illegal byte sequence",
# measured on macOS 26.6.2 while writing the fixture the brief asked for), so an
# IPC test of it can only ever run on Linux. `read_subfolders` therefore takes a
# slice of `tree::Entry` — name, is_dir, is_symlink — rather than a `ReadDir`,
# which is what lets the branch be reached from a test on every platform. The
# third is the wire-shape test, which is about serde attributes on a type and
# needs no filesystem at all.
#
# ⚠️ **Two tests of this task are named by no case here, and both are
# deliberate.** They are, on one line each so a grep for either finds this note:
#   a_directory_whose_name_is_not_utf8_is_counted_and_never_named_lossily
#   a_wrong_case_relative_path_is_refused
# The first is `#[cfg(target_os = "linux")]` and the second
# `#[cfg(target_os = "macos")]`; naming either would make the harness's baseline
# read `0 passed` on the other platform and take this whole file's cases down
# with it, which is exactly the failure `pr8-exclusions-macos.sh` was split off
# to avoid. The behaviour each pins is covered here by another case: the
# unnameable pair by the two `--lib` cases below, and the wrong-case refusal by
# the spelling half of the containment rule, whose mutant this task measured
# going red against BOTH that test and the symlink one.
#
# ⚠️ **"Any unix leg", not "any CI leg"**, the same caveat `pr8-exclusions.sh`
# carries. Two cases below name `#[cfg(unix)]` tests (the symlink ones), which
# is harmless on this repository's two legs — `ubuntu-24.04` and `macos-14`,
# both unix — and would fail every case in this file the day a Windows leg
# exists, because the harness's baseline `--exact` selects nothing for a test
# that does not exist under that `#[cfg]`.
#
# ⚠️ **Two cases for one `if`, deliberately.** The containment rule is
# `resolved != expected || !resolved.starts_with(root_canonical)`, and each
# half is removed on its own, naming in advance the test that must go red for
# it: the spelling half is what catches a path reaching its target through a
# symlink, and the containment half is what catches an absolute
# `relative_path`, which `Path::join` answers verbatim so that the spelling
# half agrees with it. Removing both at once would have been one case that
# passes while either guard alone still stands.

# ---------------------------------------------------------------------------
# The state a row reports
# ---------------------------------------------------------------------------

# The row stops naming the rule that holds it. `ExcludedByAncestor` carries the
# prefix precisely so a person can find the rule to remove; collapsed into
# `Excluded`, the row offers a control over a rule that does not exist.
case_ "a folder held by an ancestor's rule must not report its own" \
  src-tauri/src/tree.rs \
  's~        return SubfolderState::ExcludedByAncestor \{\n            prefix: prefix\.clone\(\),\n        \};~        let _ = prefix;\n        return SubfolderState::Excluded; /* mutant: ancestor collapsed */~' \
  'return SubfolderState::Excluded; /* mutant: ancestor collapsed */' \
  mnema-desktop 'a_subfolder_under_an_excluded_ancestor_names_the_rule_that_holds_it' --test commands

# The precedence reversed: a folder that has BOTH its own rule and an excluded
# ancestor reports `Excluded`, and the window offers an "include" whose whole
# effect is to leave the row exactly where it was.
case_ "a folder's own rule must not outrank an excluded ancestor" \
  src-tauri/src/tree.rs \
  's~    if let Some\(prefix\) = prefixes\n        \.iter\(\)\n        \.find\(\|prefix\| is_ancestor_of\(prefix, relative_path\)\)~    if prefixes.iter().any(|prefix| prefix == relative_path) {\n        return SubfolderState::Excluded; /* mutant: own rule wins */\n    }\n    if let Some(prefix) = prefixes\n        .iter()\n        .find(|prefix| is_ancestor_of(prefix, relative_path))~' \
  'return SubfolderState::Excluded; /* mutant: own rule wins */' \
  mnema-desktop 'the_outermost_rule_is_the_one_a_held_subfolder_names' --test commands

# The innermost ancestor instead of the outermost. Both are ancestors and both
# hold the folder, so the variant is unchanged and only the VALUE moves — which
# is why the test asserts the prefix rather than the tag.
case_ "with several rules holding a folder, the outermost is the one named" \
  src-tauri/src/tree.rs \
  's~        \.find\(\|prefix\| is_ancestor_of\(prefix, relative_path\)\)~        .rev() /* mutant: innermost */\n        .find(|prefix| is_ancestor_of(prefix, relative_path))~' \
  '.rev() /* mutant: innermost */' \
  mnema-desktop 'the_outermost_rule_is_the_one_a_held_subfolder_names' --test commands

# The built-in list goes invisible: `.git` and `node_modules` render as
# ordinary folders with a working toggle that changes nothing about the walk,
# because the built-in layer prunes them regardless (`rules.rs:283-290`).
case_ "a name on WalkRules::BUILTIN_DIRS must not report as an ordinary folder" \
  src-tauri/src/tree.rs \
  's~        return SubfolderState::BuiltIn;~        return SubfolderState::Open; /* mutant: built-in invisible */~' \
  'return SubfolderState::Open; /* mutant: built-in invisible */' \
  mnema-desktop 'a_built_in_directory_is_marked_and_an_ordinary_dot_directory_is_not' --test commands

# The same defect one state over: a symlinked directory as an ordinary
# excludable folder. The walk runs `follow_links(false)` (`rules.rs:229`), so a
# rule naming it excludes nothing at all.
case_ "a symlinked directory must not report as an ordinary folder" \
  src-tauri/src/tree.rs \
  's~        return SubfolderState::Symlink;~        return SubfolderState::Open; /* mutant: symlink ordinary */~' \
  'return SubfolderState::Open; /* mutant: symlink ordinary */' \
  mnema-desktop 'a_symlinked_directory_is_its_own_state_and_a_link_to_no_directory_is_not_listed' --test commands

# "Ancestor" becomes "string prefix": the rule `a` starts holding the sibling
# `ab`, which it excludes nothing of. The fixture's third folder is what makes
# this killable — with only `a` and `b` in it, this mutant survives.
case_ "an ancestor is a path component boundary, not a string prefix" \
  src-tauri/src/tree.rs \
  's~    path\.len\(\) > prefix\.len\(\) && path\.as_bytes\(\)\[prefix\.len\(\)\] == b./. && path\.starts_with\(prefix\)~    path.starts_with(prefix) /* mutant: no boundary */~' \
  'path.starts_with(prefix) /* mutant: no boundary */' \
  mnema-desktop 'an_excluded_subfolder_is_marked_and_its_sibling_stays_open' --test commands

# ---------------------------------------------------------------------------
# Containment, one half at a time
# ---------------------------------------------------------------------------

# The path no longer has to resolve where its spelling says. A `relative_path`
# reaching its target through a symlink is then listed, and every exclusion
# rule the window offers under it names a path the walk never enumerates.
case_ "a path must resolve where its own spelling says, so a symlink cannot be listed through" \
  src-tauri/src/tree.rs \
  's~    if resolved != expected \|\| !resolved\.starts_with\(root_canonical\) \{~    if !resolved.starts_with(root_canonical) { /* mutant: spelling unchecked */~' \
  'if !resolved.starts_with(root_canonical) { /* mutant: spelling unchecked */' \
  mnema-desktop 'listing_through_a_symlink_is_refused_in_or_out_of_the_root' --test commands

# The other half. `Path::join` given an absolute path throws the root away and
# answers the absolute path, so `expected` and `resolved` agree — and without
# this half the command lists a directory outside the watched folder entirely.
case_ "an absolute relative_path must be refused, which only the containment half catches" \
  src-tauri/src/tree.rs \
  's~    if resolved != expected \|\| !resolved\.starts_with\(root_canonical\) \{~    if resolved != expected { /* mutant: containment unchecked */~' \
  'if resolved != expected { /* mutant: containment unchecked */' \
  mnema-desktop 'listing_an_absolute_path_is_refused' --test commands

# ---------------------------------------------------------------------------
# What the listing may not claim
# ---------------------------------------------------------------------------

# A refusal becomes an empty listing, which is a claim that the folder holds no
# subfolders — the answer a window draws as a tree with nothing left to
# exclude.
case_ "a folder that cannot be resolved is refused, never answered as an empty listing" \
  src-tauri/src/tree.rs \
  's~    let dir = subfolder_dir\(&root_canonical, root_id, &relative_path\)\?;~    let Ok(dir) = subfolder_dir(\&root_canonical, root_id, \&relative_path) else {\n        /* mutant: empty instead of a refusal */\n        return Ok(SubfolderListing { entries: Vec::new(), unnameable: 0 });\n    };~' \
  '/* mutant: empty instead of a refusal */' \
  mnema-desktop 'listing_a_folder_that_is_not_there_is_refused_rather_than_answered_empty' --test commands

# The filesystem's own order reaches the window, so the same folder redraws in
# a different order on another machine.
case_ "entries are sorted by name rather than left in read_dir order" \
  src-tauri/src/tree.rs \
  's~    listed\.sort_by\(\|a, b\| a\.name\.cmp\(&b\.name\)\);~    /* mutant: filesystem order */~' \
  '/* mutant: filesystem order */' \
  mnema-desktop 'list_subfolders_answers_the_directories_sorted_and_not_the_files' --test commands

# ---------------------------------------------------------------------------
# The name that cannot cross the wire (`--lib`; see this file's header)
# ---------------------------------------------------------------------------

# The name is rendered lossily instead of being omitted: a folder appears under
# a name that no longer opens it, and a rule saved from that name excludes
# nothing.
case_ "an unrepresentable name is omitted, never rendered lossily" \
  src-tauri/src/tree.rs \
  's~        let Some\(name\) = entry\.name\.to_str\(\) else \{\n            unnameable \+= 1;\n            continue;\n        \};~        let lossy = entry.name.to_string_lossy().into_owned(); /* mutant: lossy name */\n        let name = lossy.as_str();~' \
  '/* mutant: lossy name */' \
  mnema-desktop 'tree::tests::a_directory_whose_name_is_not_utf8_is_counted_and_never_named' --lib

# The count is thrown away, so a folder holding entries this wire type cannot
# name looks emptier than it is — and nothing on the reply says so.
case_ "the omitted entries are counted on the reply, not silently dropped" \
  src-tauri/src/tree.rs \
  's~    SubfolderListing \{\n        entries: listed,\n        unnameable,\n    \}~    SubfolderListing {\n        entries: listed,\n        unnameable: 0, /* mutant: nothing was omitted */\n    }~' \
  'unnameable: 0, /* mutant: nothing was omitted */' \
  mnema-desktop 'tree::tests::a_directory_whose_name_is_not_utf8_is_counted_and_never_named' --lib

# The wire contract: `relative_path` crosses in snake_case and the window reads
# `relativePath` as undefined — a folder tree whose every row has no path to
# send back to `exclude_subfolder`.
case_ "Subfolder crosses the wire in camelCase" \
  src-tauri/src/tree.rs \
  's~#\[serde\(rename_all = "camelCase"\)\]\npub struct Subfolder \{~/* mutant: no camelCase rename */\npub struct Subfolder {~' \
  '/* mutant: no camelCase rename */' \
  mnema-desktop 'tree::tests::the_subfolder_wire_shape_is_camel_case' --lib
