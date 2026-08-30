# The folder window's own guards — the first case file in this repository that
# runs something other than `cargo test`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-ui-folders.sh
#
# Why it exists. PR 8a's Tasks 5, 6 and 7 established thirty-odd UI guards and
# proved each by hand: delete the line, run the file, read the count, restore.
# The cases below are the subset whose reverts reproduce at c12fb9d — Tasks 5
# and 7's; Task 6's guards are the same classes at other sites and are named at
# the end of this header. Those reverts are recorded in
# `.superpowers/sdd/2026-08-29-desktop-pr8-folder-exclusions/task-{5,6,7}-report.md`
# and nowhere a later change can trip over them. Meanwhile the branch's gate
# printed `651 cases, stale 0`, which was true of `crates/` and `src-tauri/`
# and would have stayed green on the day one of these guards was weakened —
# this project's own stale-artifact class, sitting inside its own gate.
#
# What is here, by name rather than by count, because a count in a comment is a
# definition and drifts:
#
#   the wire pin        — a seventh variant in `tree.rs` must fail `ipc.test.ts`
#   the control branch  — the markup names its two controls, never `!== 'none'`
#   the shut-row guard  — `patch` writes nothing into a panel that is gone
#   the excluded toggle — an excluded folder can still be opened and looked into
#   the two locale anchors — `void $locale` on the expand and remove-rule labels
#
# ⚠️ **One case here mutates Rust and is killed by a TypeScript test**, which is
# the pairing the wire pin exists for: Rust and TypeScript share no compiler, so
# a variant added to `SubfolderState` and not mirrored in `ipc.ts` reaches
# `Folders.svelte`'s classifier as a state that file has never heard of. The
# review that found that rendered the result — a folder name, an empty sentence,
# and an unlabelled button that removed the person's exclusion rule.
#
# ⚠️ **Two things a `runner=vitest` case is subject to that a cargo one is not.**
# First, it selects its test with `-t`, which is a regular expression matched as
# a substring rather than cargo's `--exact`, so a short name runs several tests;
# the harness's baseline pass refuses in both directions — a name that selects
# nothing and a name that selects two. Second, a mutant that throws before any
# assertion runs kills the run instead of failing it, and vitest reports that as
# a passing count beside an `Errors` line while exiting 1. The harness scores
# that BROKEN, not red. It is not a limitation to work around: it is the reason
# the shut-row case below mutates its guard the way it does — see the case.
#
# Unlike `pr8-exclusions-macos.sh` and `pr8-subfolders-linux.sh`, this file needs
# no platform split. Its tests run in jsdom and none of them sits behind a
# platform condition, so every case here runs on either CI leg.
#
# ⚠️ **What is NOT here, named rather than left as a silence.** Task 6's thirteen
# reverts and Task 5's remaining two are guards of classes this file already
# covers at one site each — a `describe` arm, a catalogue key pinned per locale,
# a `$locale` anchor — and three of them are not case-shaped at all: one kills
# ten tests (`roots` instead of a fresh `listTree()`), one leaks across the
# file's fixtures and kills eight, and one is green by design. Every guard in
# `Indexing.svelte`, `Models.svelte` and the i18n catalogue has no case file at
# all. `stale: 0` still says nothing about any of them.

# Task 5, and the direction `Record<SubfolderState['kind'], …>` cannot see: that
# annotation checks the union against ITSELF, so it is satisfied by a union one
# variant behind Rust. This is the half that reads `tree.rs` and compares.
# Measured at c12fb9d: with `SomethingNewer` added to the enum, `ipc.test.ts`
# reports `Tests 1 failed | 26 passed (27)` — this test and nothing else. It is
# also the only test in `ui/` that reads `tree.rs` at all (`ipc.test.ts:331` is
# the sole `readFileSync` of it), so no other file could have answered.
case_ "a variant Rust gains and the window has never heard of must fail the pin" \
  src-tauri/src/tree.rs \
  "s{    UnusableName,\n\}}{    UnusableName,\n    SomethingNewer,\n\}}" \
  "    UnusableName,
    SomethingNewer,
}" \
  src/lib/ipc.test.ts 'SubfolderState is exactly what tree.rs defines, in the spelling serde sends' runner=vitest

# Task 5, review finding, and the run-time half of the case above. `describe`'s
# default arm returns the state object itself, so an undescribed state arrives
# in the markup with `control` undefined — which `!== 'none'` is satisfied by.
# That is the unlabelled button that removed a rule. The markup names the two
# controls it draws instead, and this is the case that keeps it named: the
# mutation is the exact line the code used to have.
case_ "the row's controls must be named, because undefined is not 'none'" \
  ui/src/settings/Folders.svelte \
  "s{\{#if row\.control === 'exclude' \|\| row\.control === 'include'\}}{\{#if row.control !== 'none'\}}" \
  "{#if row.control !== 'none'}" \
  src/settings/Folders.test.ts 'a state this build has never heard of offers no control, and the rule stays reachable' runner=vitest

# Task 5 review I2. Two obvious mutants of this guard are not results, which is
# why the one used here is neither of them.
#
# 🔴 **Deleting the line is one of them.** `{ ...panel, … }` on `undefined`
# throws inside `patch`, the throw reaches the `rows` derived before anything is
# drawn, and vitest files it as an unhandled error beside a full passing count.
# Measured at c12fb9d: the whole file reports `Tests 62 passed (62)` beside
# `Errors 1 error`, and this one test alone reports `Tests 1 passed | 61
# skipped (62)` beside the same `Errors 1 error` — both exit 1, and in neither
# did anything judge the mutant. The harness now reports that shape as BROKEN,
# which is why it is not the mutation used here.
#
# What is used is the reviewer's own type-checking form: a fresh panel where the
# missing one was. It renders, so the oracle survives to answer, and the answer
# is that the row the person closed comes back open showing the listing from
# before the action — `expected 'true' to be 'false'` on the row's own
# `aria-expanded`.
case_ "a write into a panel that is gone must be dropped, not turned into a new panel" \
  ui/src/settings/Folders.svelte \
  "s{    if \(panel === undefined\) return; // the row was shut while this was running\n    panels = \{ \.\.\.panels, \[rootId\]: \{ \.\.\.panel, \.\.\.fields \} \};}{    panels = \{ ...panels, [rootId]: \{ ...(panel ?? \{ tree: null, rules: null, loadError: null, actionError: null, alreadyGone: false, pending: null \}), ...fields \} \};}" \
  "    panels = { ...panels, [rootId]: { ...(panel ?? { tree: null, rules: null, loadError: null, actionError: null, alreadyGone: false, pending: null }), ...fields } };" \
  src/settings/Folders.test.ts 'a row shut while an exclude is in flight is not re-opened by the re-read behind it' runner=vitest

# Task 5. An excluded folder is still one a person can look inside, even though
# the walk will not walk it — `excluded` and `open` are the two states that
# offer a toggle, and the other four (`excludedByAncestor`, `builtIn`,
# `symlink`, `unusableName`) deliberately do not. `expandable: false` here
# compiles, renders, and looks like a tidier row; what it takes away is the only
# way to see what a stored rule is covering. The test dies on the absent toggle:
# `Unable to find an element by: [data-testid="subfolder-expand-1-Archive"]`.
case_ "an excluded folder must still open, or nothing inside it can be looked at" \
  ui/src/settings/Folders.svelte \
  "s{      case 'excluded':\n        return \{ sentence: t\('settings_subfolder_excluded'\), control: 'include', expandable: true \};}{      case 'excluded':\n        return \{ sentence: t('settings_subfolder_excluded'), control: 'include', expandable: false \};}" \
  "        return { sentence: t('settings_subfolder_excluded'), control: 'include', expandable: false };" \
  src/settings/Folders.test.ts 'an excluded folder opens, and what is inside it names the rule and offers nothing' runner=vitest

# Task 7. These two labels are read inside a `$derived.by` whose only reference
# to the language is the `void $locale` line, so dropping it freezes the label
# at whatever language was current when the row was first drawn. Nothing else
# notices: the sentence renders, the button works, and it is in the wrong
# language only for somebody who switched. One anchor per case, because a single
# case removing both would be killed by either.
case_ "the expand label must read the locale, or it freezes at the first render" \
  ui/src/settings/Folders.svelte \
  "s{const expandLabel = \\\$derived\.by\(\(\) => \{ void \\\$locale; return t\('settings_folders_expand'\); \}\);}{const expandLabel = \\\$derived.by(() => t('settings_folders_expand'));}" \
  "const expandLabel = \$derived.by(() => t('settings_folders_expand'));" \
  src/settings/Folders.test.ts 'the expanded panel switches language with everything else on screen' runner=vitest

case_ "the remove-rule label must read the locale, or it freezes at the first render" \
  ui/src/settings/Folders.svelte \
  "s{const removeRuleLabel = \\\$derived\.by\(\(\) => \{ void \\\$locale; return t\('settings_folders_rule_remove'\); \}\);}{const removeRuleLabel = \\\$derived.by(() => t('settings_folders_rule_remove'));}" \
  "const removeRuleLabel = \$derived.by(() => t('settings_folders_rule_remove'));" \
  src/settings/Folders.test.ts 'the expanded panel switches language with everything else on screen' runner=vitest
