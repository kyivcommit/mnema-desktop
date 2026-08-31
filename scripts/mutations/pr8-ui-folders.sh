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
#   the panel re-read   — a finished scan re-reads the panel, not only the row
#   the panel loop      — every open panel, by the paths that panel has open
#   the withdrawal      — a question standing when a job ends is taken back
#   the withdrawal's bump — and the check already on the wire is stopped with it
#   the withdrawal's pass — only a WALK's ending takes a question back, never
#                            an embedding pass's (fix round 2, review I1)
#   the panel's identity — a kept panel is one whose PATH is still the same, in
#                            BOTH directions: kept when it is, dropped when the
#                            id has been handed to another folder (round 4, I1)
#   the list's own generation — the older `list_tree` of two overlapping ones
#                            writes nothing, resolved or rejected (round 4, I2)
#   the question's own identity — the SAME rule at the two sites that act on it:
#                            a question is asked, and a rule is removed, only
#                            while the id and the path still agree, and only the
#                            press whose folder moved is refused (round 5)
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
# ⚠️ **What is NOT here, named rather than left as a silence.** Apart from
# reverts 5, 6 and 7 (next paragraph), Task 6's thirteen reverts and Task 5's
# remaining two are guards of classes this file already covers at one site
# each — a `describe` arm, a `$locale` anchor — and three of them are not
# case-shaped at all: one kills ten tests (`roots` instead of a fresh
# `listTree()`), one leaks across the file's fixtures and kills eight, and one
# is green by design. Task 6's catalogue-key reverts (#9-12) are pins on a
# locale string, not a `$locale` anchor — no case here covers that class.
# Every guard in `Indexing.svelte`, `Models.svelte` and the i18n catalogue has
# no case file at all. `stale: 0` still says nothing about any of them.
#
# ⚠️ **Task 6's reverts 5, 6 and 7 are not covered by any class above.** The
# `paths === 0` shortcut, storing immediately instead of raising the question,
# and storing anyway after a rejected `list_tree` are the confirm-question
# flow — `askExclude`/`askInclude` at `Folders.svelte:294-358`, guarded at
# `:321` by `if (cost.paths === 0)`. That flow lives in the same file this
# case file covers. Task 8's cases reach one edge of it — a question WITHDRAWN
# by a job ending, and the generation bump that goes with it — and pin none of
# the three reverts named above: the shortcut itself, storing immediately
# instead of asking, and storing anyway after a rejected `list_tree`.

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
  "s{    if \(panel === undefined\) return; // the row was shut while this was running\n    panels = \{ \.\.\.panels, \[rootId\]: \{ \.\.\.panel, \.\.\.fields \} \};}{    panels = \{ ...panels, [rootId]: \{ ...(panel ?? \{ tree: null, rules: null, loadError: null, actionError: null, alreadyGone: false, withdrawn: null, pending: null \}), ...fields \} \};}" \
  "    panels = { ...panels, [rootId]: { ...(panel ?? { tree: null, rules: null, loadError: null, actionError: null, alreadyGone: false, withdrawn: null, pending: null }), ...fields } };" \
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

# ── Task 8: what a finished scan re-reads ────────────────────────────────────
#
# The live run's finding 1. `refresh` re-read the ROW and nothing under it, so a
# panel open across a scan went on showing the listing and the rules from before
# it: measured on a case-only rename, the row read "archive — Виключено вашим
# правилом" while that same folder's text was being sent to the model provider,
# because the rule names `archive` and the folder had become `Archive`. This is
# the exact line the file used to have, and the whole-window test is the one
# that dies on it.
# Fix round 2 moved this call behind an arrow (`.then(() => rereadPanels(pass))`)
# so the ending's `pass` could reach the withdrawal below — same guard, updated
# shape. Fix round 1 then took `pass` off `rereadPanels` again, because the
# withdrawal moved out of it entirely (see the four cases below), so the arrow
# is gone and the callback is the function itself.
case_ "a finished scan must re-read the panel, not only the row above it" \
  ui/src/settings/Folders.svelte \
  "s{    refresh\(\)\.then\(rereadPanels\)\.catch\(\(e\) => \{}{    refresh().catch((e) => \{}" \
  "    refresh().catch((e) => {" \
  src/settings/Indexing.test.ts 'a finished scan re-reads the open panel, so a renamed folder stops reading as excluded' runner=vitest

# The half of `rereadPanels` that reads. Deleting it leaves the withdrawal
# behind, which is why the withdrawal test is NOT the one named here: measured,
# that test still passes against this mutation. The panel test dies on the
# subfolder that never arrives — `Unable to find an element by:
# [data-testid="subfolder-1-first-after"]`.
#
# Fix round 2 gave `reread` a `pass` parameter, so the marker below now names
# that signature rather than the old parameterless one. Fix round 1 then split
# `rereadPanels` in two, so the body it empties is the whole of that function
# and the marker is the empty loop it leaves behind.
case_ "every open panel must be re-read, and by the set of paths it has open" \
  ui/src/settings/Folders.svelte \
  "s{      void read\(Number\(key\), openPathsOf\(panel\.tree\)\);\n}{}" \
  "  function rereadPanels() {
    for (const [key, panel] of Object.entries(panels)) {
    }
  }" \
  src/settings/Folders.test.ts 'a job ending re-reads every expanded panel, not only the root whose Scan was pressed' runner=vitest

# The other half, and the mirror of the case above: deleting the withdrawal
# leaves the read, and the panel test passes against it. A question left
# standing is a question whose two numbers were read from a `list_tree` taken
# before this scan — and the sentence a person answers must not be one the
# ending has already falsified.
#
# Fix round 2, review I1: the guard also requires `pass === 'walk'` — and fix
# round 1 then moved that clause and the whole withdrawal OUT of `rereadPanels`
# and into `reread`, ahead of any I/O. So this case now removes the CALL rather
# than the block: it is about the withdrawal happening at all, not about which
# pass it fires on (the case after next) nor about where it sits relative to the
# refresh (the last case in this group).
case_ "a question standing when a job ends must be withdrawn, not left over the new listing" \
  ui/src/settings/Folders.svelte \
  "s{    if \(pass === 'walk'\) withdrawQuestions\(\);}{    /* mutant: no withdrawal */}" \
  "/* mutant: no withdrawal */" \
  src/settings/Folders.test.ts 'a question standing when a job ends is withdrawn by name, and nothing is stored' runner=vitest

# And the generation bump inside it, which clearing `pending` alone does not do.
# Without it the press whose `list_tree` was still on the wire when the ending
# landed comes back as a question — over the listing that has just replaced the
# one it was asked about. Anchored on the two lines together because
# `ask(rootId);` occurs eight times in this file.
case_ "withdrawing a question must also stop the check already on the wire" \
  ui/src/settings/Folders.svelte \
  "s{      ask\(rootId\);\n      patch\(rootId, \{ pending: null, withdrawn: panel\.pending\.path \}\);}{      patch(rootId, \{ pending: null, withdrawn: panel.pending.path \});}" \
  "      const rootId = Number(key);
      patch(rootId, { pending: null, withdrawn: panel.pending.path });" \
  src/settings/Folders.test.ts 'a check still in flight when a job ends raises no question when its reply lands' runner=vitest

# ── Fix round 2, review I1 ────────────────────────────────────────────────────
#
# The withdrawal used to fire on ANY ending, including a chained embedding
# pass — which takes no root and moves nothing about a question raised AFTER
# the walk that chained it had already landed. Reverting to the unconditional
# guard is the exact bug the review reproduced: a press made while the
# embedding pass runs is discarded, and the panel claims a scan ended when
# none had at that moment. The re-read stays unconditional on purpose — see
# `reread`'s own comment — so this case targets only the `pass === 'walk'`
# clause, not the `refresh().then(rereadPanels)` beside it.
case_ "the withdrawal must not fire on a pass that changed nothing about the question" \
  ui/src/settings/Folders.svelte \
  "s{    if \(pass === 'walk'\) withdrawQuestions\(\);}{    withdrawQuestions(); /* mutant: any pass withdraws */}" \
  "withdrawQuestions(); /* mutant: any pass withdraws */" \
  src/settings/Folders.test.ts 'an embedding pass ending does not withdraw a question raised after the walk that chained it' runner=vitest

# ── Fix round 1, I1 ───────────────────────────────────────────────────────────
#
# WHERE the withdrawal sits, which the three cases above cannot see: all of
# them leave it inside `reread`, and moving it back behind `refresh().then(...)`
# — the shape this branch shipped until fix round 1 — keeps every one of them
# red for the right reason while the defect returns. A rejected `list_tree` at a
# walk's ending then loses the withdrawal with the re-read, and the ending is
# consumed once (`seen = phase` advances first), so it never comes back: the
# next successful refresh redraws the panel with the question still standing,
# stating pre-scan numbers as current.
#
# One expression across the comment between the two lines, because `-0` slurps
# the file and this is one edit: the call leaves its own line and reappears
# inside the `then`.
case_ "the withdrawal must not depend on a call that can fail" \
  ui/src/settings/Folders.svelte \
  "s{    if \(pass === 'walk'\) withdrawQuestions\(\);\n(    //[^\n]*\n)+    refresh\(\)\.then\(rereadPanels\)\.catch\(\(e\) => \{}{    refresh().then(() => \{ if (pass === 'walk') withdrawQuestions(); rereadPanels(); \}).catch((e) => \{ /* mutant: withdrawal downstream of the refresh */}" \
  "/* mutant: withdrawal downstream of the refresh */" \
  src/settings/Folders.test.ts 'a walk ending withdraws the question even when the re-read that follows it fails' runner=vitest

# ── Fix round 4, item 1: the panel's identity ─────────────────────────────────
#
# `watched_root.id` is `INTEGER PRIMARY KEY` with no `AUTOINCREMENT`
# (`schema.sql:11-15`), so SQLite hands a deleted id out again in the ordinary
# case. The prune in `refresh` therefore has TWO cases and not one, and the
# branch shipped the first alone with a comment that named the hazard — the
# class this branch has paid for three times. The two cases below are a PAIR:
# each mutant is green under the other's test, which is what says neither half
# is standing on the other's defence.
#
# This one is the shape the branch had before the fix: the id is present, so
# the panel is kept, and the old folder's subfolders are drawn under the new
# folder's path.
#
# ⚠️ Fix round 5 moved the comparison itself into `namesFolder`, which all three
# sites now call, so these two mutate the CALL and not the comparison: a mutant
# inside the predicate would change what all three sites decide at once and say
# nothing about any of them.
case_ "a panel must not be kept by an id that now names a different folder" \
  ui/src/settings/Folders.svelte \
  "s{      if \(namesFolder\(listing, rootId, panel\.rootPath\)\) \{}{      if (listing.roots.some((r) => r.rootId === rootId)) \{ /* mutant: the id alone */}" \
  "if (listing.roots.some((r) => r.rootId === rootId)) { /* mutant: the id alone */" \
  src/settings/Folders.test.ts 'a root id handed to a different folder does not keep the old' runner=vitest

# And its mirror, which is the reason the pair exists at all: a prune that drops
# EVERYTHING satisfies every assertion about a panel disappearing. This mutant
# is broad — it takes seven tests in this file down, six of them about a job
# ending re-reading panels that are no longer there — and the harness runs only
# the one named below, which is the case written for this direction.
case_ "a panel whose folder is unchanged must survive the refresh" \
  ui/src/settings/Folders.svelte \
  "s{      if \(namesFolder\(listing, rootId, panel\.rootPath\)\) \{\n        kept\[rootId\] = panel;}{      if (false) \{ /* mutant: nothing survives */\n        kept[rootId] = panel;}" \
  "if (false) { /* mutant: nothing survives */" \
  src/settings/Folders.test.ts 'a refresh that finds the same folder under the same id keeps its expansion open' runner=vitest

# ── Fix round 4, item 2: the list's own generation ────────────────────────────
#
# A panel's read has had a generation guard since Task 5; the read of the LIST
# had none, and `refresh` is reachable from four places — mount, an add, a
# remove and a job ending — so two `list_tree` calls can be on the wire at once.
# Without this line the older answer lands behind the newer one and both
# replaces `roots` and prunes the panels against it.
#
# Anchored on the line after it, because the identical guard also appears in the
# `catch` above (the next case) and a bare pattern would match twice.
case_ "the older of two overlapping list_tree answers must not replace the list" \
  ui/src/settings/Folders.svelte \
  "s{\n    if \(refreshes !== generation\) return;\n    roots = listing\.roots;}{\n    /* mutant: no guard on the list's own read */\n    roots = listing.roots;}" \
  "/* mutant: no guard on the list's own read */" \
  src/settings/Folders.test.ts 'an older list_tree that lands after a newer one replaces neither the list nor the panels' runner=vitest

# The rejection half of the same guard, and its own case for the reason item 1's
# two cases are separate: every caller of `refresh` turns a rejection into
# `loadError`, which replaces the whole list with a failure banner — so a guard
# written for the resolved path alone leaves an older failure free to print "the
# list could not be read" over a list that has just been read.
case_ "a stale list_tree REJECTION must not print a failure over a newer answer" \
  ui/src/settings/Folders.svelte \
  "s{\n      if \(refreshes !== generation\) return;\n      throw e;}{\n      /* mutant: a stale rejection is not stale */\n      throw e;}" \
  "/* mutant: a stale rejection is not stale */" \
  src/settings/Folders.test.ts 'a list_tree rejection that lands after a newer answer prints no failure over it' runner=vitest

# ── Fix round 5: the second and third sites of round 4's own rule ─────────────
#
# Round 4 taught `refresh` that a panel belongs to a folder rather than to a
# number, and left the same question being asked of the id alone at the two
# places that ACT on it. `askExclude` matched a fresh `list_tree` on the id, so
# with the id handed to another folder the question quoted that folder's file
# and document counts under this folder's row (measured: the DOM under the
# first mutant below carries `Exclude Work?`, `loses 2 files from this folder`
# and `2 documents stop being findable` inside `folder-confirm-9`, whose row
# reads `/synthetic/beta`). `askInclude` read no listing at all and went
# straight to a control that removes a rule — under D29, protection taken off a
# folder the person never pointed at.
#
# Four cases, in two pairs, for the reason round 4's two are a pair: a guard
# that refuses everything satisfies every assertion about a refusal, and a guard
# that is not there satisfies every assertion about a question being asked. Each
# "refuse everything" mutant is broad — 17 and 16 tests in this file
# respectively — and the harness runs only the one named on the case, which is
# the test written for that direction.
#
# The predicate itself is deliberately not mutated anywhere: `namesFolder` is
# called from all three sites, so a mutant inside it changes what all three
# decide at once and pins none of them.
case_ "an exclude question must not be built from a listing that no longer names this folder" \
  ui/src/settings/Folders.svelte \
  "s{    if \(!namesFolder\(listing, rootId, panel\.rootPath\)\) \{\n      abandonChangedFolder\(rootId\);\n      return;\n    \}\n    const cost}{    /* mutant: the id alone decides the count */\n    const cost}" \
  "/* mutant: the id alone decides the count */" \
  src/settings/Folders.test.ts 'an exclude question is not asked when the id now names another folder' runner=vitest

case_ "an exclude question must still be asked when the id and the path agree" \
  ui/src/settings/Folders.svelte \
  "s{    if \(!namesFolder\(listing, rootId, panel\.rootPath\)\) \{\n      abandonChangedFolder\(rootId\);\n      return;\n    \}\n    const cost}{    if (true) \{ /* mutant: every question refused */\n      abandonChangedFolder(rootId);\n      return;\n    \}\n    const cost}" \
  "if (true) { /* mutant: every question refused */" \
  src/settings/Folders.test.ts 'an exclude question is asked as before when this folder' runner=vitest

# The mirror, and the half that removes protection rather than over-warning.
case_ "a rule must not be removed on a folder the listing no longer puts at this id" \
  ui/src/settings/Folders.svelte \
  "s{    if \(!namesFolder\(listing, rootId, panel\.rootPath\)\) \{\n      abandonChangedFolder\(rootId\);\n      return;\n    \}\n    patch\(rootId, \{\n      // Read from}{    /* mutant: no listing is consulted before the removal */\n    patch(rootId, \{\n      // Read from}" \
  "/* mutant: no listing is consulted before the removal */" \
  src/settings/Folders.test.ts 'a rule is not removed when the id now names another folder' runner=vitest

case_ "a rule removal must still be asked about when the id and the path agree" \
  ui/src/settings/Folders.svelte \
  "s{    if \(!namesFolder\(listing, rootId, panel\.rootPath\)\) \{\n      abandonChangedFolder\(rootId\);\n      return;\n    \}\n    patch\(rootId, \{\n      // Read from}{    if (true) \{ /* mutant: every removal refused */\n      abandonChangedFolder(rootId);\n      return;\n    \}\n    patch(rootId, \{\n      // Read from}" \
  "if (true) { /* mutant: every removal refused */" \
  src/settings/Folders.test.ts 'a rule removal is asked about and goes through when this folder' runner=vitest

# And the sentence the new wait says. Not a locale-string pin — the string is
# pinned by an ordinary test — but a BRANCH: `confirmView` picks between two
# `checking` sentences on `Pending.of`, and a press routed to the other one
# tells a person their removal is being costed up when nothing is being counted.
case_ "the wait before a removal must not say it is costing an exclusion" \
  ui/src/settings/Folders.svelte \
  "s{      pending: \{ kind: 'checking', path, of: 'include' \},}{      pending: \{ kind: 'checking', path, of: 'exclude' \}, /* mutant: one sentence over both waits */}" \
  "of: 'exclude' }, /* mutant: one sentence over both waits */" \
  src/settings/Folders.test.ts 'the wait before a rule removal says what it is checking' runner=vitest
