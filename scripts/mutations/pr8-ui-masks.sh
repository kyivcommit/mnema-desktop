# The mask editor's own guards. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-ui-masks.sh
#
# Why it exists, and why it is a second file rather than lines added to
# `pr8-ui-folders.sh`: that file's header records what its cases reproduce at
# `c12fb9d`, and mixing a later task's cases into it would make that statement
# false about half its contents. This file's cases were measured at the task 11
# working tree, one mutation at a time, each restored from a copy taken before
# the run — never `git checkout --`, which has destroyed uncommitted work here.
#
# 🔴 **Every case below was run before it was written down.** Thirty-two
# mutations of `Masks.svelte`, `catalog.ts`, `ipc.ts` and `Settings.svelte` were
# applied one at a time; all of them went red, none survived, none crashed its oracle
# — every run reported at least one FAILED test and no `Errors` line, which is
# the shape this harness scores as a kill rather than as BROKEN. The subset here
# is the one whose reverts are case-shaped: a single
# unique marker, one test selected by a name that selects exactly it. The rest —
# the swapped pair of numbers, the dropped `at least`, the trimmed mask on the
# wire, the frozen `$locale` — are recorded in
# `.superpowers/sdd/2026-08-31-desktop-pr8b-masks/task-11-report.md` with the
# test each one killed.
#
# What is here, by name rather than by count:
#
#   the preview gate      — nothing is stored before the question is answered
#   the cancel            — the second control dismisses, it does not answer
#   the verbatim sentence — a rejection reaches the person as the shell wrote it
#   the typed mask        — the frame names what was typed, not what was folded
#   the zero's own words  — a preview of zero has a sentence of its own
#   the re-read           — the list after an answer is a read, not an edit
#   the already-gone      — a row another window removed first says so
#   the list generation   — the older of two overlapping reads writes nothing
#   the unknown list      — "no mask is stored" is not printed before it is known
#   the blank row         — the empty string is not pressable; whitespace is
#   the announced wait    — the press is answered before the numbers arrive
#   the mounted editor    — the folders panel really holds `Masks.svelte`
#   the add outcome       — "already stored" reaches the screen, not the floor
#   the case note         — decided by the mask, not by any uppercase in the answer
#
# ⚠️ Every case here is `runner=vitest`, so both of that runner's traps apply
# (see `pr8-ui-folders.sh`'s header): `-t` matches as a substring, and a mutant
# that throws before any assertion runs is scored BROKEN rather than red. The
# test names below were checked to select exactly one test each.

# The rule the whole section exists for (D-d): the loss is named BEFORE it
# happens. This mutant stores while the question is still on screen, which is
# the shape the exclusion side's revert #6 had. Measured: it takes four tests
# with it, and the one named here is the one whose subject it is.
case_ "nothing may be stored before the question is answered" \
  ui/src/settings/Masks.svelte \
  "s{      pending = \{ kind: 'add', mask, paths: preview\.paths, documents: preview\.documents \};}{      await addMask(mask);\n      pending = \{ kind: 'add', mask, paths: preview.paths, documents: preview.documents \};}" \
  "      await addMask(mask);" \
  src/settings/Masks.test.ts 'adding asks the preview first and shows both its numbers before anything is stored' runner=vitest

# The other half of the same rule, and the one a list fixture cannot see: an
# empty list looks identical whether the cancel stored something or not, which
# is why the test asserts the COMMAND was never called.
case_ "the second control must dismiss the question, not answer it" \
  ui/src/settings/Masks.svelte \
  "s{onclick=\{dismiss\}>\{q\.cancelLabel\}}{onclick=\{answer\}>\{q.cancelLabel\}}" \
  "onclick={answer}>{q.cancelLabel}" \
  src/settings/Masks.test.ts 'cancelling stores nothing, and the question goes with the press' runner=vitest

# 🔴 A rejection is a sentence, never a kind. Nothing in this component branches
# on an error kind, so this one line is the only thing that can say which rule
# was refused and why; replacing it with a catalogue sentence of ours loses the
# whole of the shell's answer while leaving a paragraph in its place.
case_ "the shell's rejection must reach the person in the shell's own words" \
  ui/src/settings/Masks.svelte \
  "s{<p data-testid=\"mask-refused-reason\">\{actionError\}</p>}{<p data-testid=\"mask-refused-reason\">\{loadFailedLabel\}</p>}" \
  '<p data-testid="mask-refused-reason">{loadFailedLabel}</p>' \
  src/settings/Masks.test.ts 'a refused mask shows the backend sentence verbatim, names the mask as typed, and stores nothing' runner=vitest

# 🔴 The booked decision. `RulesError::InvalidMask`'s `reason` is `globset`'s own
# text and it quotes the FOLDED pattern: someone who typed `[A-_]x.txt` reads
# about `[a-_]x.txt`. The frame naming the mask as typed is what keeps the
# quoted form from being mistaken for what they wrote, and dropping it leaves a
# screen that quotes a pattern the person never entered.
case_ "the refusal must name the mask as it was typed, beside the folded one" \
  ui/src/settings/Masks.svelte \
  "s{<p data-testid=\"mask-refused-heading\">\{refusal\.heading\}</p>}{<p data-testid=\"mask-refused-heading\"></p>}" \
  '<p data-testid="mask-refused-heading"></p>' \
  src/settings/Masks.test.ts 'a refused mask shows the backend sentence verbatim, names the mask as typed, and stores nothing' runner=vitest

# A preview of two zeros is not "this rule removes nothing": it is "the indexed
# set holds nothing that matches today", and it gets a sentence of its own.
#
# 🔴 Fix round 7. This comment used to argue from the shared sentence's `=0` arm
# ("each one is also indexed under another path"), a wording that had already
# been rewritten twice and that F1 has now deleted outright — so the argument
# named a string that no longer exists. What actually separates the two strings
# is the HEDGE: the shared one hedges both ways because its number can be too
# high, and this number is zero and cannot be, so `_none` warns one way only.
# The mutant sends `paths === 0` down the shared branch and the named test's
# whole-sentence `toBe` is what dies.
case_ "a preview of zero must not borrow the sentence written for a non-zero one" \
  ui/src/settings/Masks.svelte \
  "s{          \? p\.paths === 0}{          ? p.paths < 0}" \
  "          ? p.paths < 0" \
  src/settings/Masks.test.ts 'a preview of zero asks the question anyway, in a sentence of its own' runner=vitest

# The list on screen after an answer must be what the index holds, not an array
# this window edited. Measured: it takes five tests with it, including the two
# that watch the count of reads.
case_ "the list after an answer must be a read, never an edit" \
  ui/src/settings/Masks.svelte \
  "s{    await refresh\(\);}{    await Promise.resolve();}" \
  "    await Promise.resolve();" \
  src/settings/Masks.test.ts 'removing re-reads the list rather than editing the array on screen' runner=vitest

# `remove_mask` answers whether a row actually went, and the two answers are
# different sentences: a second window may have removed the same mask first.
# The mutant is the one that always says "removed", which is the direction that
# tells a person they did something they did not.
case_ "a row another window removed first must say so" \
  ui/src/settings/Masks.svelte \
  "s{        alreadyGone = !\(await removeMask\(p\.mask\)\);}{        await removeMask(p.mask);\n        alreadyGone = false;}" \
  "        alreadyGone = false;" \
  src/settings/Masks.test.ts 'a mask another window removed first says so, and one this window removed does not' runner=vitest

# Three call sites read this list — mount, an add and a remove — so two
# `list_masks` calls can be on the wire at once and the one asked for first is
# not the one that has to answer first. The same class as `Folders.svelte`'s
# `refreshes`, at a screen that has its own.
case_ "the older of two overlapping list reads must write nothing" \
  ui/src/settings/Masks.svelte \
  "s{      const list = await listMasks\(\);\n      if \(n !== reads\) return;}{      const list = await listMasks();}" \
  "      const list = await listMasks();
      masks = list;" \
  src/settings/Masks.test.ts 'the older of two overlapping list reads writes nothing' runner=vitest

# 🔴 "No mask is stored" and "nobody has answered yet" are opposite claims about
# the person's protection, and the first one printed on a screen that does not
# know it yet is the defect class this repository names "a screen stating what
# the data contradicts".
case_ "the section must not claim the list is empty before it has been read" \
  ui/src/settings/Masks.svelte \
  "s{\{:else if masks !== null && rows\.length === 0\}}{\{:else if rows.length === 0\}}" \
  "{:else if rows.length === 0}" \
  src/settings/Masks.test.ts 'the section does not claim the list is empty while the first read is still on the wire' runner=vitest

# The empty string is `validate_mask`'s one deliberate non-error and `add_mask`
# is where it is refused; the blank row simply cannot be pressed. Whitespace is
# NOT blank: `"   "` has a refusal sentence of its own, and trimming here would
# hand the person the wrong one of the two.
case_ "whitespace is not the blank row, and must reach the shell as typed" \
  ui/src/settings/Masks.svelte \
  "s{    if \(mask === ''\) return;}{    if (mask.trim() === '') return;}" \
  "    if (mask.trim() === '') return;" \
  src/settings/Masks.test.ts 'the blank row cannot be pressed, and whitespace is not blank' runner=vitest

# `mask_preview` holds the index mutex across a scan of every indexed path of
# every root, so a press made while a walk is running waits on that lock. A
# screen that answers a press with nothing is a screen that invites a second
# press — and the second one queues behind the first on the same mutex.
case_ "the wait must be announced, not answered with an empty screen" \
  ui/src/settings/Masks.svelte \
  "s{    pending = \{ kind: 'checking', mask \};}{    pending = null;}" \
  "    const n = ++previews;
    pending = null;" \
  src/settings/Masks.test.ts 'the press says it is checking, and the numbers replace that sentence rather than joining it' runner=vitest

# The <h2> in `Settings.svelte` proves only that `Settings.svelte` rendered. A
# mask is global (D-c), so the editor is drawn beside the folder list rather
# than inside a folder row, and this is the case that keeps it drawn at all.
case_ "the folders panel must really hold the mask editor, not just its heading" \
  ui/src/settings/Settings.svelte \
  "s{        <Masks />}{        <!-- Masks -->}" \
  "        <!-- Masks -->" \
  src/settings/Settings.test.ts 'clicking Folders shows the Folders heading and removes the Models heading' runner=vitest

# ── Task 11 fix round 1 additions ────────────────────────────────────────────
#
# Eight more cases, one per guard the fix round added or changed. Measured the
# same way as the twelve above: each mutated alone in a copy of
# `Masks.svelte`, the copy restored between runs, never `git checkout --`. All
# eight went red, none crashed its oracle. See
# `docs/private/sdd/2026-08-31-desktop-pr8b-masks/task-11-fix-round-1-report.md`
# for the failure text each one produced.

# F1, blocking: `askAdd` had no generation guard on its OWN reply, though
# `reads` above already has one for the list read. This drops the guard on the
# success arm; the older of two overlapping previews then wins if it answers
# last, exactly the shape `the list generation` case above reproduces for
# `listMasks`.
case_ "the older of two overlapping mask previews must write nothing" \
  ui/src/settings/Masks.svelte \
  "s{      const preview = await maskPreview\(mask\);\n      if \(n !== previews\) return; // a newer question has replaced this one\n}{      const preview = await maskPreview(mask);\n}" \
  "      const preview = await maskPreview(mask);
      pending = { kind: 'add', mask, paths: preview.paths, documents: preview.documents };" \
  src/settings/Masks.test.ts 'the older of two overlapping mask previews writes nothing' runner=vitest

# F5. `refresh` writes `masks = list; loadError = null;` together. Dropping the
# second half leaves a read that once failed stuck forever: `{#if loadError}`
# wins over the list branch, so a later successful read never reaches the
# screen.
case_ "a read that succeeds must clear an earlier read's failure" \
  ui/src/settings/Masks.svelte \
  "s{      masks = list;\n      loadError = null;}{      masks = list;}" \
  "      masks = list;
    } catch (e) {" \
  src/settings/Masks.test.ts 'a read that fails is not the last word: a later successful read replaces it' runner=vitest

# F5. `<label for="mask-draft-input">` names the field's accessible name;
# `screen.getByRole('textbox')` finds the one input on the page whether or not
# the id actually matches, so no test before round 1 would have noticed this
# going stale.
case_ "the mask input's label must really point at the field" \
  ui/src/settings/Masks.svelte \
  's{<label class="fl" for="mask-draft-input">}{<label class="fl" for="mask-draft-inpuXX">}' \
  '<label class="fl" for="mask-draft-inpuXX">' \
  src/settings/Masks.test.ts 'the mask input is really named by its own label, not just found by being the only textbox' runner=vitest

# F3. `refusal.heading` picks one of three catalogue keys from `r.of`; nothing
# forced the REMOVE case off the ADD key it shares no test with before round 1.
case_ "the refusal heading must not default to the add key" \
  ui/src/settings/Masks.svelte \
  "s{    const heading = t\(\n      r\.of === 'add-check' \? 'settings_masks_refused_add'\n        : r\.of === 'add-store' \? 'settings_masks_refused_store'\n        : 'settings_masks_refused_remove',\n      \{ mask: r\.mask \},\n    \);}{    const heading = t('settings_masks_refused_add', { mask: r.mask });}" \
  "    const heading = t('settings_masks_refused_add', { mask: r.mask });" \
  src/settings/Masks.test.ts 'a refusal from removing a mask names the removal, not the add, and carries no case note' runner=vitest

# F3's other half. The case note belongs to the check only — it explains a
# folded pattern quoted inside a compile refusal, and neither other path folds
# anything. This forces it onto every refusal; the same test above catches it.
#
# ⚠️ **Re-quoted in fix round 2**, which rewrote the very line this case
# substitutes (the note is now asked of the answer as well as of the path).
# Left as it was, the pattern matched nothing and the harness reported BROKEN
# CASE — caught by a run, not by reading. The mutation it produces is
# byte-identical to the one it always produced; only the text it is cut out of
# has moved.
case_ "the case note must not follow a removal refusal" \
  ui/src/settings/Masks.svelte \
  "s{      note:\n        r\.of === 'add-check' && answerSpellsItDifferently\(actionError, r\.mask\)\n          \? t\('settings_masks_refused_case_note'\)\n          : null,}{      note: t('settings_masks_refused_case_note'),}" \
  "      note: t('settings_masks_refused_case_note')," \
  src/settings/Masks.test.ts 'a refusal from removing a mask names the removal, not the add, and carries no case note' runner=vitest

# F4. A failure from `add_mask` inside `answer` is the STORE's, never the
# check's — the check already passed, or there would be no question to
# confirm. This files it under the check's key instead, reusing the frame that
# says "This is what the check answered" for a call the check never made.
case_ "a store refusal on the add path must not be filed as a check refusal" \
  ui/src/settings/Masks.svelte \
  "s{      refused = \{ mask: p\.mask, of: p\.kind === 'add' \? 'add-store' : 'remove' \};}{      refused = { mask: p.mask, of: p.kind === 'add' ? 'add-check' : 'remove' };}" \
  "      refused = { mask: p.mask, of: p.kind === 'add' ? 'add-check' : 'remove' };" \
  src/settings/Masks.test.ts 'a refusal from the store itself is shown too, and the list is read again' runner=vitest

# F5. `void $locale` dropped from `alreadyGoneLabel`: the string stops
# following a language switch, the precise failure D130's rule exists for.
case_ "the already-gone note must follow a language switch" \
  ui/src/settings/Masks.svelte \
  "s{    void \\\$locale;\n    return alreadyGone \? t\('settings_masks_already_gone'\) : null;}{    return alreadyGone ? t('settings_masks_already_gone') : null;}" \
  "    return alreadyGone ? t('settings_masks_already_gone') : null;" \
  src/settings/Masks.test.ts 'the refusal frame and the already-gone note also follow a language switch' runner=vitest

# F5, separately from the one above: `void $locale` dropped from `refusal`
# itself. Two different derived values, the same failure. Delimited `s{}[]`
# rather than `s{}{}` because the replacement's own unmatched `{` (the arrow
# function body `Masks.svelte` never closes within this snippet) would
# otherwise be read as nested delimiter, not literal text.
case_ "the refusal frame must follow a language switch" \
  ui/src/settings/Masks.svelte \
  "s{  const refusal = \\\$derived\.by\(\(\) => \{\n    void \\\$locale;\n    const r = refused;}[  const refusal = \\\$derived.by(() => {\n    const r = refused;]" \
  "  const refusal = \$derived.by(() => {
    const r = refused;" \
  src/settings/Masks.test.ts 'the refusal frame and the already-gone note also follow a language switch' runner=vitest

# ── Task 11 fix round 2 additions ────────────────────────────────────────────
#
# FOUR cases now: three from fix round 2, and one from the review of round 3
# that found the case-note guard could be replaced by a locator ignoring the
# mask entirely with the whole suite still green. The count is corrected here
# rather than left standing, because an inventory that no longer matches its
# section is read as "the last one was never measured". Measured the same way as
# everything above: each mutated alone in a copy of `Masks.svelte`, the copy
# restored between runs, never `git checkout --`. All four went red, none
# crashed its oracle. See
# `docs/private/sdd/2026-08-31-desktop-pr8b-masks/task-11-fix-round-2-report.md`
# and `…-fix-round-3-review.md` for the failure text each one produced.

# 🔴 F3. The outcome discarded — which is exactly the state the live run found,
# with `add_mask` still returning `()`: a person types `*.PDF` over a stored
# `*.pdf`, answers the question, and the screen says nothing at all. The
# discarded value is the ONE thing that can tell them the rule they just typed
# is already there under a spelling they are not looking for.
case_ "the add outcome must reach the screen, not be discarded" \
  ui/src/settings/Masks.svelte \
  "s{        const added = await addMask\(p\.mask\);\n        alreadyStored = added\.kind === 'alreadyStored' \? added\.stored : null;}{        await addMask(p.mask);}" \
  "        await addMask(p.mask);
        draft = '';" \
  src/settings/Masks.test.ts 'a rule that is already stored under another spelling is not added, and the sentence names the stored spelling' runner=vitest

# 🔴 F5. The caveat back to unconditional — the state the live run saw, where it
# stood under `sub/*.txt` and `!notes.txt` and explained a re-spelling that had
# not happened. Most of the check's refusals quote the mask exactly as typed;
# only the compile refusal carries `globset`'s folded pattern.
case_ "the case note must not stand where the answer spells the mask as it was typed" \
  ui/src/settings/Masks.svelte \
  "s{      note:\n        r\.of === 'add-check' && answerSpellsItDifferently\(actionError, r\.mask\)\n          \? t\('settings_masks_refused_case_note'\)\n          : null,}{      note: r.of === 'add-check' ? t('settings_masks_refused_case_note') : null,}" \
  "      note: r.of === 'add-check' ? t('settings_masks_refused_case_note') : null," \
  src/settings/Masks.test.ts 'the case note stands only where the answer really spells the mask differently' runner=vitest

# F3, D130's rule at the third derived value of this shape: `void $locale`
# dropped from `alreadyStoredLabel`, so the sentence keeps the language it was
# first read in. The siblings above cover `alreadyGoneLabel` and `refusal`.
case_ "the already-stored sentence must follow a language switch" \
  ui/src/settings/Masks.svelte \
  "s{  const alreadyStoredLabel = \\\$derived\.by\(\(\) => \{\n    void \\\$locale;\n    const stored = alreadyStored;}[  const alreadyStoredLabel = \\\$derived.by(() => {\n    const stored = alreadyStored;]" \
  "  const alreadyStoredLabel = \$derived.by(() => {
    const stored = alreadyStored;" \
  src/settings/Masks.test.ts 'the refusal frame and the already-gone note also follow a language switch' runner=vitest

# 🔴 F5, the guard the round-1 fixtures did not have. Independent review replaced
# the whole locator with a version that never looks at the mask — "does this
# sentence contain any uppercase character" — and the entire UI suite stayed
# green, 570 of 570, because both existing fixtures moved two variables at once:
# `[A-_]x.txt` has an uppercase letter AND a respelling, `sub/*.txt` has neither.
# The discriminating state is an UPPERCASE mask the answer quotes exactly as
# typed, and it is now in the test above as `SUB/*.txt`.
case_ "the case note must be decided by the mask, not by any uppercase in the answer" \
  ui/src/settings/Masks.svelte \
  "s{    const inAnswer = answer\.toLowerCase\(\);\n    const wanted = mask\.toLowerCase\(\);\n    for \(let at = inAnswer\.indexOf\(wanted\); at !== -1; at = inAnswer\.indexOf\(wanted, at \+ 1\)\) \{\n      if \(answer\.slice\(at, at \+ mask\.length\) !== mask\) return true;\n    \}\n    return false;}{    void mask;\n    return answer.toLowerCase() !== answer;}" \
  "    void mask;
    return answer.toLowerCase() !== answer;" \
  src/settings/Masks.test.ts 'the case note stands only where the answer really spells the mask differently' runner=vitest

# ── Task 11 fix round 4 additions ────────────────────────────────────────────
#
# Two catalogue cases rather than component ones, because both findings ARE the
# wording: the component renders whatever string it is given, so the guard that
# matters is the one that reads the rendered sentence.

# 🔴 F3. The removal sentence, put back to the promise it used to make. With
# `*.pdf` and `report.*` both stored, removing either leaves `report.pdf`
# excluded — so "the files this mask was holding back are indexed again" is
# false about the files it names. It cannot be fixed by counting: those files
# are NOT in the index, which is why this side has no preview at all. The mutant
# is the exact string that shipped.
case_ "the removal sentence must not promise that every held-back file comes back" \
  ui/src/i18n/catalog.ts \
  "s{    settings_masks_remove_cost: 'From the next scan of each folder on, this mask stops holding anything back: each file it was excluding is indexed again — unless another of your rules still excludes it — and its text is sent to the model provider.',}{    settings_masks_remove_cost: 'From the next scan of each folder on, the files this mask was holding back are indexed again, and their text is sent to the model provider.',}" \
  "the files this mask was holding back are indexed again" \
  src/settings/Masks.test.ts 'removing states the inverse cost before it removes anything, in both locales' runner=vitest

# 🔴 F2's disclosed half, dropped from the explainer. `?` is deliberately NOT
# refused — its breakage is a property of the NAME, not of the mask, so refusing
# every `?` would cut through the healthy case — which makes this clause the
# only place a person is told. Measured: `?.txt` does not match `й.txt`,
# `??.txt` does.
case_ "the explainer must disclose that \`?\` counts bytes rather than letters" \
  ui/src/i18n/catalog.ts \
  "s{ And \\? stands for a single byte rather than a single letter, so a letter outside the basic Latin alphabet needs more than one of them: \\?\\.txt does not match й\\.txt, and \\?\\?\\.txt does\\.'}{'}" \
  "neither does the way a name happens to store its accents.'," \
  src/settings/Masks.test.ts 'the whole section reads as one screen' runner=vitest

# 🔴 Fix round 5, F4. The same clause, dropped from the UKRAINIAN half.
# The case above mutates the English string and is killed by a component test
# that reads English only, so until this one existed the Ukrainian half of the
# disclosure had a test and no mutant — and it is the half most of this
# product's people read. Killed by the catalogue test that asserts the clause
# in both locales — `the mask explainer discloses that \`?\` counts bytes, in
# both locales`, selected here by a prefix of its name because `-t` is a REGEX
# and the name itself carries a `?`.
case_ "the explainer must disclose that \`?\` counts bytes in Ukrainian too" \
  ui/src/i18n/catalog.ts \
  "s{ А «\\?» замінює один байт, а не одну літеру, тож для літер поза латиницею його треба ставити кілька: «\\?\\.txt» не збігається з «й\\.txt», а «\\?\\?\\.txt» збігається\\.'}{'}" \
  "записані на диску літери з діакритичними знаками.'," \
  src/i18n/i18n.test.ts 'the mask explainer discloses that' runner=vitest

# 🔴 Fix round 5, F3, owner's ruling. The floor word put back, in English.
# `mask_preview.paths` is a bound in NEITHER direction: it understates, because
# only `status = 'indexed'` rows are counted while the walk's reconcile set is
# status-agnostic; and it overstates, because the in-tree `.gitignore` stack is
# in neither rule set (so a path it already covers counts as surviving and this
# press is charged for it), and because a rule set that does not compile answers
# as an empty override, after which `walk_root` stops before phase 2 and removes
# nothing at all. "At least N" would then be said about a scan that removes
# zero. Killed by the `not.toContain('at least')` on the screen test — probed
# in fix round 6 the way its Ukrainian sibling below had to be: with that one
# assertion deleted the mutant SURVIVES, so the guard is the sole killer and the
# insertion point really is past every positive assertion here.
#
# 🔴 Fix round 7 moved the insertion point by one clause, because F1 deleted the
# `documents == 0` arm and moved the `, and ` INSIDE the non-zero arms — so
# `already take, and {documents` no longer exists in the catalogue and the old
# expression would have matched nothing. The word now goes between
# `already take` and the `{documents` plural. That is still the one spot the
# named test's positive assertions do not span: its
# `toContain('takes 4 files beyond what your rules already take')` ends exactly
# at the insertion point, and `can remove more than that or fewer` and the
# `.gitignore` clause both start after it.
case_ "the add-cost sentence must not claim a floor, in English" \
  ui/src/i18n/catalog.ts \
  "s~already take\\{documents~already take, at least\\{documents~" \
  "already take, at least{documents" \
  src/settings/Masks.test.ts 'the file count is not stated as a floor' runner=vitest

# The same ruling, the Ukrainian half — the half most of this product's people
# read, and the one round 4's own regression was written in first. Killed by the
# language-switch test, which is where the Ukrainian sentence is read off the
# screen rather than out of the catalogue.
#
# 🔴 **The insertion point is the case, and only a probe finds it.** Fix round 5
# put the floor word at `ця маска забирає {paths`, inside the region the
# language-switch test's own POSITIVE assertion
# (`toContain('Станом на зараз ця маска забирає 4 файли')`) reads — so the case
# died there, on a line that fires for ANY edit to that region, and
# `not.toContain('щонайменше')` below it never ran. Deleting that guard changed
# no mutation number: it was unpinned.
#
# The word now goes between the plural's closing `}}` and `понад те`, which is
# the ONE spot in this sentence no positive assertion in that test covers:
# `забирає 4 файли` ends before it, `може прибрати і більше, і менше` and the
# `.gitignore` clause both start after it. Measured rather than reasoned — with
# the mutation applied and `not.toContain('щонайменше')` deleted, the test
# PASSES, and with the guard back it fails on that line alone. An insertion at
# `може прибрати і більше` was tried first and rejected for exactly the round-5
# defect one clause over: the neighbouring `toContain('може прибрати і більше,
# і менше')` kills it and the guard never runs (fix round 6, F3).
case_ "the add-cost sentence must not claim a floor, in Ukrainian" \
  ui/src/i18n/catalog.ts \
  "s~\\}\\} понад те, що вже забирають~\\}\\} щонайменше понад те, що вже забирають~" \
  "}} щонайменше понад те, що вже забирають" \
  src/settings/Masks.test.ts 'every sentence on the section follows a language switch' runner=vitest
