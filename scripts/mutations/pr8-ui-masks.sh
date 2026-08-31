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
  "    forget();
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
