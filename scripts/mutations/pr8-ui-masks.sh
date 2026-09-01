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
# set holds nothing that matches today". The shared sentence's `=0` arm says
# "each one is also indexed under another path", which has nobody to be about
# when no path matched at all.
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
# Three more cases, for the two guards this round added to the screen. Measured
# the same way as everything above: each mutated alone in a copy of
# `Masks.svelte`, the copy restored between runs, never `git checkout --`. All
# three went red, none crashed its oracle. See
# `docs/private/sdd/2026-08-31-desktop-pr8b-masks/task-11-fix-round-2-report.md`
# for the failure text each one produced.

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
