# `list_subfolders` — the exclusion screen's folder tree, read off the disk
# (task 4 of PR 8a, plus fix rounds 1 and 2). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-subfolders.sh
#
# Run by CI as well, since fix round 2: the `mutations` job names this file,
# `pr8-exclusions.sh` and `pr8-subfolders-linux.sh`. Before that every case in
# PR 8a was local evidence only.
#
# Cases mutate `src-tauri/src/tree.rs`, `src-tauri/src/bridge.rs` and
# `crates/mnema-walk/src/rules.rs`. The `mnema-walk` ones are here rather than
# in a `mnema-walk` case file because most of them are killed by
# `mnema-desktop` tests and this harness selects a test by package — the same
# reason `pr8-exclusions.sh` keeps its own `validate_prefix` case.
#
# ⚠️ **Some are not**, and this sentence said "One is not" while three already
# ran `-p mnema-walk --test rules` (fix round 2, B4 — a number in a comment is
# a definition, and it was two short). All three name the same test,
# `builtin_layers_agree_with_what_the_walk_enumerates`, for the same reason:
# the property each breaks is a disagreement between the predicate and the
# walk, and the drift guard is the only test that runs both. Which cases they
# are is one command away and cannot go stale:
#
#   grep -n "^  mnema-walk '" scripts/mutations/pr8-subfolders.sh
#
# 🔴 Fix round 3, item 4: the pattern used to be `" mnema-walk '"`, which also
# matched this very sentence — 4 lines for 3 cases. Anchored at the start of
# the line, so only a `case_`'s own target field can match. Output, so the
# next reader can tell a drift from a rerun without retyping the command:
#
#   293:  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules
#   397:  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules
#   408:  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules
#
# A case file is not one package here; the `case_` line names its own.
#
# Most cases run in `tests/commands.rs` (`--test commands`); the rest in
# `tree.rs`'s own `mod tests` (`--lib`), for the reason two notes down, or in
# `mnema-walk`'s `tests/rules.rs`.
#
# ⚠️ **No count of the cases here, deliberately.** A number maintained in the
# header of the file it counts still drifts — fix round 1 found the previous
# header's `#[cfg(unix)]` count wrong the day a case was added. Every number a
# reader needs is one command away and cannot go stale:
#
#   grep -c '^case_ '        scripts/mutations/pr8-subfolders.sh   # cases
#   grep -c ' --lib$'        scripts/mutations/pr8-subfolders.sh   # --lib cases
#   scripts/mutation-staleness.sh                                  # per-file counts
#
# (The three sentences above name which FILE each group mutates and which
# TARGET it runs under, which is what a reader acts on; those are properties of
# the case, not a tally that drifts.)
#
# ⚠️ **Why the `--lib` cases are killed by a unit test rather than through the
# IPC.** Two are about a directory whose name is not valid UTF-8, and that state
# **cannot be built on this project's macOS leg**: APFS refuses `create_dir`
# for such a name outright with `EILSEQ` ("Illegal byte sequence", measured on
# macOS 26.6.2), so an IPC test of it can only ever run on Linux.
# `read_subfolders` therefore takes a slice of `tree::Entry` — name, is_dir,
# is_symlink — rather than a `ReadDir`, which is what lets the branch be
# reached from a test on every platform. The third is the sort, over that same
# pure function, so the case cannot report STILL GREEN on a filesystem that
# happened to hand five names back already sorted (fix round 1, M2). The fourth
# is the wire-shape test, which is about serde attributes and needs no
# filesystem at all.
#
# ⚠️ **Platform gating: some cases here name `#[cfg(unix)]` tests**, which is
# harmless on this repository's two legs — `ubuntu-24.04` and `macos-14`, both
# unix — and would fail EVERY case in this file the day a Windows leg exists,
# because the harness's baseline `--exact` selects nothing for a test that does
# not exist under that `#[cfg]`. **No case here names a `target_os`-gated
# test**, which is the property that actually has to hold today, and it is
# checkable rather than remembered:
#
#   for t in $(grep -oE "mnema-desktop '[^']+'" scripts/mutations/pr8-subfolders.sh \
#               | sed "s/mnema-desktop '//;s/'//;s/^tree::tests:://" | sort -u); do
#     grep -B4 "fn $t()" src-tauri/tests/commands.rs src-tauri/src/tree.rs \
#       | grep -o 'cfg(target_os[^]]*)' ; done
#
# ⚠️ **Two tests of this task are named by no case in THIS file, both because
# of that rule.** On one line each so a grep for either finds this note:
#   a_directory_whose_name_is_not_utf8_is_counted_and_never_named_lossily
#   a_wrong_case_relative_path_is_refused
# The first is `#[cfg(target_os = "linux")]` and is named by
# `scripts/mutations/pr8-subfolders-linux.sh`, the Linux-only sibling this file
# needs for the one thing it cannot pin (see that file's own header). The
# second is `#[cfg(target_os = "macos")]` and is named nowhere: the behaviour
# it pins — a wrong-case path refused — is the spelling half of the containment
# rule, whose case is below and whose mutant was measured going red against
# both tests.
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
# because the built-in layer prunes them regardless (`rules.rs:446-447`).
case_ "a name on WalkRules::BUILTIN_DIRS must not report as an ordinary folder" \
  src-tauri/src/tree.rs \
  's~        return SubfolderState::BuiltIn;~        return SubfolderState::Open; /* mutant: built-in invisible */~' \
  'return SubfolderState::Open; /* mutant: built-in invisible */' \
  mnema-desktop 'a_built_in_directory_is_marked_and_an_ordinary_dot_directory_is_not' --test commands

# The same defect one state over: a symlinked directory as an ordinary
# excludable folder. The walk runs `follow_links(false)` (`rules.rs:388`), so a
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

# The OTHER half of the same `if`, and until fix round 1 it was defended by
# nothing: the case above removes the boundary check and keeps `starts_with`,
# so `starts_with` itself never had a case of its own. Measured before the test
# below existed: deleting it left `cargo test --workspace` at exit 0, and
# `list_subfolders` then reported `Work/2024` as
# `{"kind":"excludedByAncestor","prefix":"Home"}` — a row claiming a rule
# protects a folder the walk indexes, with no control offered to protect it for
# real. `Home` and `Work` are the same length, which is what lets the surviving
# first conjunct answer `true`.
#
# ⚠️ Double quotes, alone in this file, because the replacement has to carry a
# real `b'/'` and a single-quoted shell word cannot hold an apostrophe. Nothing
# in it is `$` or a backtick, so the shell passes it through unchanged, and the
# backslashes in the pattern half survive double quotes as they do single ones.
case_ "an ancestor must agree byte for byte, not merely have a separator where the prefix ends" \
  src-tauri/src/tree.rs \
  "s~    path\.len\(\) > prefix\.len\(\) && path\.as_bytes\(\)\[prefix\.len\(\)\] == b./. && path\.starts_with\(prefix\)~    path.len() > prefix.len() && path.as_bytes()[prefix.len()] == b'/' /* mutant: byte equality unchecked */~" \
  "== b'/' /* mutant: byte equality unchecked */" \
  mnema-desktop 'a_rule_on_a_different_folder_of_the_same_length_holds_nothing' --test commands

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
  mnema-desktop 'tree::tests::the_entries_are_sorted_by_name_whatever_order_they_arrive_in' --lib

# The directories-only filter, removed: files come back as rows in a folder
# tree. Named by the IPC sorted test, which is what keeps that test — and the
# `c.txt` half of its assertion — pinned now that the case above moved to the
# pure function.
case_ "only directories are listed, never the files beside them" \
  src-tauri/src/tree.rs \
  's~        if !entry\.is_dir \{~        if false { /* mutant: files are folders too */~' \
  'if false { /* mutant: files are folders too */' \
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

# ---------------------------------------------------------------------------
# Fix round 1 — the invariant applied to every instance of it
#
# One predicate per layer, in `crates/mnema-walk/src/rules.rs` beside the code
# it mirrors, and a case per way the listing can offer a folder the walk will
# not walk. Three of these mutate that file rather than `src-tauri`; they are
# here rather than in a `mnema-walk` case file because the tests that kill them
# are `mnema-desktop` ones and this harness selects a test by package — the
# same reason `pr8-exclusions.sh` keeps its own `validate_prefix` case.
# ---------------------------------------------------------------------------

# The blocking defect of fix round 1, put back: the built-in question asked of
# the LAST component only. `!**/{dir}` prunes the subtree, so `.git/hooks` is
# pruned — but with this mutant the listing offers it as an ordinary folder and
# `exclude_subfolder` writes a rule the walk ignores.
case_ "the built-in question is asked of every component, not the last one" \
  crates/mnema-walk/src/rules.rs \
  's~        if over\.matched\(&path, is_dir\)\.is_ignore\(\) \{~        if over.matched(\&path, is_dir).is_ignore()\n            \&\& path == root.join(relative_path)\n        { /* mutant: last component only */~' \
  '{ /* mutant: last component only */' \
  mnema-desktop 'everything_under_a_built_in_directory_is_built_in_too' --test commands

# The drift fix round 2 exists to make impossible, reintroduced from the
# PREDICATE's side: `builtin_layers` compiles only `BUILTIN_DIRS` while
# `builder()` still adds both lists. That is exactly the state that shipped a
# folder named `.DS_Store` as ordinary and excludable.
case_ "the predicate compiles the same built-in patterns the walker does" \
  crates/mnema-walk/src/rules.rs \
  's~        for pattern in Self::builtin_override_patterns\(\) \{\n            let _ = builder\.add\(&pattern\);\n        \}~        /* mutant: predicate reads BUILTIN_DIRS only */\n        for name in Self::BUILTIN_DIRS {\n            let _ = builder.add(\&format!("!**/{name}"));\n        }~' \
  '/* mutant: predicate reads BUILTIN_DIRS only */' \
  mnema-desktop 'a_directory_named_like_a_built_in_file_is_built_in_too' --test commands

# The same drift from the WALKER's side: `builder()` stops adding
# `BUILTIN_FILES` while the predicate still claims it prunes them. Killed by
# the drift guard rather than by a desktop test, because this half is a
# disagreement between the predicate and the walk and that guard is the only
# thing that runs both.
case_ "the walker compiles the same built-in patterns the predicate does" \
  crates/mnema-walk/src/rules.rs \
  's~            for pattern in Self::builtin_override_patterns\(\) \{\n                let _ = over\.add\(&pattern\);\n            \}~            /* mutant: walker reads BUILTIN_DIRS only */\n            for name in Self::BUILTIN_DIRS {\n                let _ = over.add(\&format!("!**/{name}"));\n            }~' \
  '/* mutant: walker reads BUILTIN_DIRS only */' \
  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules

# The anchored layer goes invisible: `target` beside its `Cargo.toml` lists as
# an ordinary folder, the exclusion succeeds, and the walk had already pruned
# it.
case_ "the anchored build-output layer is visible in the listing" \
  crates/mnema-walk/src/rules.rs \
  's~        let anchored = is_dir\n            && WalkRules::ANCHORED_DIRS\.iter\(\)\.any\(\|\(dir, markers\)\| \{\n                \*dir == component && markers\.iter\(\)\.any\(\|marker\| parent\.join\(marker\)\.is_file\(\)\)\n            \}\);~        let anchored = false; /* mutant: anchored layer invisible */~' \
  'let anchored = false; /* mutant: anchored layer invisible */' \
  mnema-desktop 'an_anchored_build_directory_is_built_in_only_beside_its_marker' --test commands

# The other direction of the same layer: the marker file stops being checked,
# so every folder called `target` is reported as pruned. That one is worse than
# it looks — the row would tell a person that `House/target/permits.pdf` is
# protected while the walk indexes it.
case_ "an anchored name is only pruned beside its marker file" \
  crates/mnema-walk/src/rules.rs \
  's~                \*dir == component && markers\.iter\(\)\.any\(\|marker\| parent\.join\(marker\)\.is_file\(\)\)~                *dir == component /* mutant: marker unchecked */~' \
  '*dir == component /* mutant: marker unchecked */' \
  mnema-desktop 'an_anchored_build_directory_is_built_in_only_beside_its_marker' --test commands

# The first-component rules applied at every depth: `weird/~` becomes a name no
# rule can express, when it is in fact a rule `exclude_subfolder` accepts. The
# mirror of the case below — one says the state must appear, this says it must
# not appear where the validator would not.
case_ "the drive-letter and \`~\` rules apply to the first component only" \
  crates/mnema-walk/src/rules.rs \
  's~        validate_component\(prefix, component, index == 0\)\?;~        validate_component(prefix, component, index >= 0)?; /* mutant: first-component rules everywhere */~' \
  '/* mutant: first-component rules everywhere */' \
  mnema-desktop 'a_folder_whose_path_no_rule_can_name_says_so_instead_of_offering_a_control' --test commands

# A folder whose name no rule can express is offered as an ordinary excludable
# one; pressing exclude answers with a message about path components for a path
# the person never typed.
case_ "a name the validator refuses says so instead of offering a control" \
  src-tauri/src/tree.rs \
  's~    if WalkRules::check_prefix\(relative_path\)\.is_err\(\) \{\n        return SubfolderState::UnusableName;\n    \}~    /* mutant: unusable names look ordinary */~' \
  '/* mutant: unusable names look ordinary */' \
  mnema-desktop 'a_folder_whose_path_no_rule_can_name_says_so_instead_of_offering_a_control' --test commands

# The asked-for path stops being validated, so `a/.` answers a page of rows
# whose every `relativePath` `exclude_subfolder` will refuse.
case_ "a relative_path that cannot be a rule is refused before any row is built" \
  src-tauri/src/tree.rs \
  's~    WalkRules::check_prefix\(&relative_path\)\?;~    /* mutant: the asked-for path is not validated */~' \
  '/* mutant: the asked-for path is not validated */' \
  mnema-desktop 'a_relative_path_that_cannot_be_a_rule_is_refused_with_the_validators_sentence' --test commands

# The classifier inside `refusal()`, disarmed: every filesystem error about the
# asked-for path becomes "there is no folder X". That is the sentence task 5
# turns into an offer to remove a rule as stale — for a folder that is merely
# on a volume that went away. Fix round 1 measured this branch protected by a
# correct line of code and nothing else.
case_ "an error about the observer is never answered as an absence" \
  src-tauri/src/tree.rs \
  's~    if crate::bridge::path_error_is_an_answer\(source\.kind\(\)\) \{~    if true { /* mutant: every error is an absence */~' \
  'if true { /* mutant: every error is an absence */' \
  mnema-desktop 'a_folder_that_cannot_be_read_is_refused_as_unreadable_not_as_absent' --test commands

# The same classifier collapsed the other way: no error is ever an answer, so a
# folder that genuinely is not there is reported as one this process could not
# read. Killed by the second half of the same test, which is what makes that
# test a claim about the SPLIT rather than about one string (fix round 2, N5:
# it used to end with an `assert_ne!` that could not fail).
case_ "a folder that is not there is never answered as unreadable" \
  src-tauri/src/tree.rs \
  's~    if crate::bridge::path_error_is_an_answer\(source\.kind\(\)\) \{~    if false { /* mutant: no error is ever an answer */~' \
  'if false { /* mutant: no error is ever an answer */' \
  mnema-desktop 'a_folder_that_cannot_be_read_is_refused_as_unreadable_not_as_absent' --test commands

# ---------------------------------------------------------------------------
# The listing's claim, asked of the command that has to honour it
# ---------------------------------------------------------------------------

# The guard removed: `exclude_subfolder` accepts a path the walk already
# prunes, writes a row, and `list_exclusions` renders it live. That is the
# state fix round 2 found — the listing said `builtIn` and the command
# disagreed.
case_ "excluding a folder the walk already prunes is refused" \
  src-tauri/src/bridge.rs \
  's~    if mnema_walk::WalkRules::builtin_layers\(root_path\)\.prunes\(&relative_path\) \{~    if false { /* mutant: the built-in layers do not refuse */~' \
  'if false { /* mutant: the built-in layers do not refuse */' \
  mnema-desktop 'excluding_a_folder_the_walk_already_prunes_is_refused_and_stores_nothing' --test commands

# The other direction, which is what stops the guard from being "refuse
# everything": a folder carrying a pruned NAME with no marker beside it, and an
# ordinary folder, must both still store.
case_ "the refusal is about the built-in layers, not about every path" \
  src-tauri/src/bridge.rs \
  's~    if mnema_walk::WalkRules::builtin_layers\(root_path\)\.prunes\(&relative_path\) \{~    if true { /* mutant: every exclusion is refused */~' \
  'if true { /* mutant: every exclusion is refused */' \
  mnema-desktop 'excluding_a_folder_the_walk_already_prunes_is_refused_and_stores_nothing' --test commands

# A pruning layer that is neither an override nor `ANCHORED_DIRS`, added by
# changing one word of a `WalkBuilder` SETTING: `hidden(true)` prunes every
# dot-directory in the walk while `BuiltinLayers` keeps answering `false` for
# them. Fix round 3 measured this slipping past the whole workspace, and past
# the grep the predicate's doc used to call complete. Killed by the drift
# guard, which is the only test that runs both sides — and only because its
# fixture now holds a dot-directory on neither built-in list.
case_ "a pruning layer added by a builder setting is caught by the drift guard" \
  crates/mnema-walk/src/rules.rs \
  's~            \.hidden\(false\)~            .hidden(true) /* mutant: dotfiles pruned by a setting */~' \
  '.hidden(true) /* mutant: dotfiles pruned by a setting */' \
  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules

# The membership half, from the other side: a name deleted from `BUILTIN_DIRS`
# takes the generated fixture and its expectation with it, so both derived
# halves of the guard still agree. Fix round 3 measured `.hg` doing exactly
# that with the whole workspace green; the hand-written list is what catches it
# now.
case_ "a name deleted from BUILTIN_DIRS is caught by the hand-pinned list" \
  crates/mnema-walk/src/rules.rs \
  's~        "\.hg",\n~        /* mutant: .hg dropped from the built-in list */\n~' \
  '/* mutant: .hg dropped from the built-in list */' \
  mnema-walk 'builtin_layers_agree_with_what_the_walk_enumerates' --test rules
