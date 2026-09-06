# The §9.3 Scanning SECTION's own guards (called Indexing before Task 8) —
# `ui/src/settings/Scanning.svelte`,
# the file that says what the index holds — and, from Task 7, the §9.4
# Application section's own: `ui/src/settings/Application.svelte` and
# `ui/src/i18n/shortcut.ts`. From Task 11b, `ui/src/i18n/recency.ts` too — the
# fix is one function shared by every locale, and reached from here rather
# than given a case file of its own. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9-ui.sh
#
# Not to be confused with `pr9-index.sh`, which covers the busy-index line in
# the window's job strip (`JobStrip.svelte`) and the Rust that feeds it. This
# file is PR 9 Tasks 6, 7 and 11b.
#
# What is here, by name rather than by count, because a count in a comment is a
# definition and drifts:
#
#   the discriminant     — the Unreadable arm is told from the Read arm by
#                            `kind`, before anything is read out of either
#   the stated null      — `lastIndexedAt: null` is a sentence, never a default
#   the date line        — the date and the relative phrase are two lines, and
#                            the phrase does not stand in for the date
#   the two scopes       — the index's cumulative count and the run's own get
#                            two keys, because they name two subjects
#   the run's own scope   — and the run's sentence outlives a read that fails,
#                            because its subject is the pass, not the index
#   the refresh trigger  — an ENDING re-reads the index; an emission does not
#   the generation stamp — the older of two reads in flight writes nothing,
#                            whether it resolves or is refused
#   the cleared sentence — a read that succeeds takes the failure away with it
#   the teardown         — the subscription dies with the component
#   the resume act       — the pending-chunks button starts the embedding
#                            pass through the controller, never a scan
#   the empty-queue guard — the pending line and its button do not draw when
#                            pendingChunks is 0
#   the running-pass guard — the pending line and its button step aside
#                            while the strip above owns a run already
#                            under way
#   the trailing-stop strip — `formatIndexedDate` (`ui/src/i18n/recency.ts`,
#                            reached from here rather than from its own file)
#                            drops ICU's own stop, so a locale whose long-date
#                            form ends in one is not doubled by the sentence
#                            wrapped around it
#   the no-modifier guard — the recorder refuses a press that carries no
#                            modifier at all, same as `set_hotkey` step 3
#   the canonical order   — the modifiers are joined in ONE fixed order, on
#                            both the format side and the record side
#   the autostart re-render — the toggle draws the OS's REPLY, never the
#                            request it sent
#   the two shortcut sentences — `registered` and `unavailable` say two
#                            different things, never one drawn twice
#   the re-read on rejection — a refused `set_hotkey` carries no state at all,
#                            so what is drawn next can only come from a fresh
#                            `appPrefs()`, never the pre-call value
#   the platform source  — the shortcut's glyphs are drawn from the WIRE's
#                            `Platform`, never guessed from `navigator.userAgent`
#
# ⚠️ **Why the discriminant is mutated the way it is, and not the obvious way.**
# The obvious mutant is `const read = $derived(index)` — the section reading
# `IndexRead` off whichever arm arrived. It is not here, and the reason is the
# harness's own rule about crashed oracles (`mutation-check.sh`'s header):
# `read.lastIndexedAt` is then `undefined`, `formatIndexedDate` hands
# `new Date(NaN)` to `Intl.DateTimeFormat`, and the render throws `RangeError:
# Invalid time value` — vitest scores that `Errors 1 error` beside a passing
# count and exits 1, which the harness classifies BROKEN rather than red, and
# rightly: the test never saw the mutant. Measured, not assumed — it is exactly
# what eleven inline fixtures missing Task 3's three required fields did to this
# suite before they were annotated. The mutant below branches on the WRONG kind
# instead, which renders and is judged.
#
# ⚠️ **Read the discriminant case's title narrowly: it covers ONE of the two
# arms** (review, Minor 6). `Scanning.svelte`'s own `const read` line has no case
# here, and cannot have one for the same measured reason: pointing it at the
# `unreadable` arm is the crashing mutant described above. That line is defended
# by the `notOpen` test all the same (its `queryByTestId(...).toBeNull()`
# assertions fail, or the render throws and the test fails with it) — just not by
# anything this harness can score. Named so a reader does not take the summary
# line above for coverage of both arms.

# The one place the union is discriminated. Everything below it reads `read` or
# `unreadable`, each null on the other arm — so a section that picks the wrong
# arm has nothing to draw and says nothing at all, which is the state §9.3
# exists to replace ("секція показує «не вдалося прочитати індекс», а не
# порожні числа").
case_ "the Unreadable arm must be told from the Read arm by kind, before anything is read" \
  ui/src/settings/Scanning.svelte \
  "s~  const unreadable = \\\$derived\(index !== null && index\.kind === 'unreadable' \? index : null\);~  const unreadable = \\\$derived(index !== null \&\& index.kind === 'read' ? index : null); // mutant: the arms are not told apart~" \
  "const unreadable = \$derived(index !== null && index.kind === 'read' ? index : null); // mutant: the arms are not told apart" \
  src/settings/Scanning.test.ts 'an index that is not open says so, and shows the backend reason verbatim' runner=vitest

# 🔴 `lastIndexedAt: null` is the backend's own statement that nothing has ever
# finished indexing (`MAX(ingest_stage.updated_at)` over an empty set), and the
# section owes it a sentence. `?? 0` is the tidy-looking substitute and it draws
# 1 January 1970 with a relative phrase counting twenty thousand days beside it —
# two lines that look like measurements, on a screen whose whole job is to say
# what the index actually holds. Only a fixture in the empty state can tell the
# two apart; every filled-index assertion passes under this mutant.
case_ "an index nothing has ever finished indexing must not be given the epoch as its date" \
  ui/src/settings/Scanning.svelte \
  "s~  const lastIndexedAt = \\\$derived\(read === null \? null : read\.lastIndexedAt\);~  const lastIndexedAt = \\\$derived(read === null ? null : (read.lastIndexedAt ?? 0)); // mutant: null becomes the epoch~" \
  "const lastIndexedAt = \$derived(read === null ? null : (read.lastIndexedAt ?? 0)); // mutant: null becomes the epoch" \
  src/settings/Scanning.test.ts 'an index nothing has ever finished indexing says so, and draws no time at all' runner=vitest

# D-e: the date is not a duplicate of the phrase. «годину тому» is what a person
# feels; the date is what they compare against the file they edited this
# morning, and §9.3 asks for it by name («останнє оновлення з датою»). Dropping
# the line leaves a screen that still reads perfectly well and has quietly
# stopped answering the question the spec asked. The relative phrase survives
# this mutant, which is why an assertion on it cannot see the loss.
case_ "the date must be drawn beside the relative phrase, not replaced by it" \
  ui/src/settings/Scanning.svelte \
  's~\{#if dateLine\}<p data-testid="indexing-index-date">\{dateLine\}</p>\{/if\}\n~~' \
  '{#if filesLine}<p data-testid="indexing-index-files">{filesLine}</p>{/if}
{#if agoLine}<p data-testid="indexing-index-ago">{agoLine}</p>{/if}' \
  src/settings/Scanning.test.ts 'a filled index says how many files it holds, the date it last grew, and how long ago that was' runner=vitest

# 🔴 The PR 7 debt, and the mutant that makes it invisible again.
# `IndexRead::failed_chunks` is cumulative for the SPACE; `job::Progress::refused`
# is what the run that has just ended gave up on. `job.rs:38-44` holds them
# apart and says whichever surface shows them owes each its own words. One key
# for both draws two numbers under one subject — and every state with only ONE
# of the two on screen passes under this mutant, which is exactly why the suite
# needs the state that has both.
# Rewritten at Task 11b: the section no longer holds its own `phase`, it reads
# the snapshot from the controller and takes the refusals off the ENDING's
# embedding outcome. Same key, same two subjects, same test.
case_ "the run's refusals and the index's must not be drawn from one key" \
  ui/src/settings/Scanning.svelte \
  "s~    return t\('indexing_index_refused_run', \{ count: embedding\.refused \}\);~    return t('indexing_index_failed_chunks', { count: embedding.refused }); // mutant: one subject for two scopes~" \
  "return t('indexing_index_failed_chunks', { count: embedding.refused }); // mutant: one subject for two scopes" \
  src/settings/Scanning.test.ts 'a run that gave up on chunks and an index that already had some show two sentences, each about its own subject' runner=vitest

# An ENDING is the one moment the numbers on this screen can have changed. A
# subscriber that re-fetches on every store emission answers every "does an
# ending re-read the index" test correctly and issues an IPC call per progress
# report for the whole of a long pass. The mirror — a progress event and a call
# count that does not move — is the only thing that tells the two apart.
# Rewritten at Task 11b. Task 8 made `Settings.svelte` the window's SINGLE
# reader of `model_settings` and this section a prop of it, so the subscription
# that decides when to re-read is up there now — and it has grown two triggers
# beside the ending (`readSeq` growth, and `scan.files` moving, F1/F9). The
# property is the one it always was: something has to have CHANGED, and a
# subscriber that re-fetches on every emission issues an IPC call per progress
# report for the whole of a long pass while answering every "does an ending
# re-read" assertion correctly. The mirror is still the only thing that can see
# it, and it is the running tick that moves nothing.
# ⚠️ Re-quoted a second time in final fix round 2, and the reason is worth
# recording: the round that MOVED this condition (area C's fourth trigger, any
# transition out of `running`) gated on `npm test` alone, because nothing it
# touched was Rust. `mutation-staleness.sh` was not in that gate and did not run
# until the next commit, so two cases sat stale for one commit. Staleness is
# cheap and reads every case file regardless of language; it belongs in a UI
# gate too.
case_ "the re-read must follow an ending, not every emission of the job store" \
  ui/src/settings/Settings.svelte \
  "s~      if \(readSeqChanged \|\| filesChanged \|\| leftRunning \|\| scan\.snapshot\.kind === 'ended'\) \{\n        void refresh\(\);\n      \}~      void refresh(); // mutant: every emission re-reads~" \
  "      void refresh(); // mutant: every emission re-reads" \
  src/settings/Settings.test.ts 'a running tick with an unchanged files count does not re-read' runner=vitest

# 🔴 The decision that the two scope sentences do NOT share a fate. A pass ends,
# the ending triggers the re-read, and the re-read comes back `Unreadable` — an
# ordinary sequence, not a contrived pairing. Gated on `read`, the window would
# answer "the index could not be read" and delete, in the same breath, the only
# surviving report of what the pass just did. The mutant is the tidier-looking
# guard and the lossy one.
# Rewritten at Task 11b: same decision, same mutant, on the line the ending
# check now sits on. The test was renamed with the section.
case_ "the run's own report must outlive a read of the index that fails" \
  ui/src/settings/Scanning.svelte \
  "s~    if \(snapshot\.kind !== 'ended'\) return null;~    if (read === null || snapshot.kind !== 'ended') return null; // mutant: the run's report dies with the index~" \
  "if (read === null || snapshot.kind !== 'ended') return null; // mutant: the run's report dies with the index" \
  src/settings/Scanning.test.ts 'a pass that gave up on chunks says so even when the index cannot be read at all' runner=vitest

# 🔴 Two reads can be in flight here whenever endings arrive faster than the IPC
# answers, and they may settle in either order. Without the stamp the older
# reply repaints the screen with numbers taken before the pass that triggered
# the newer one — and nothing on the screen says so, because both answers are
# well-formed. Only a fixture that resolves them in reverse can see it.
# Rewritten at Task 11b: `refresh` and its generation stamp moved up to
# `Settings.svelte` with the single `model_settings` read (Task 8). The
# expression, the marker and the fixture shape are unchanged.
case_ "an older read that settles last must not write over the newer one" \
  ui/src/settings/Settings.svelte \
  "s~      if \(seq !== settingsSeq\) return; // a newer read has already spoken\n~~" \
  "      const s = await modelSettings();
      settings = s;" \
  src/settings/Settings.test.ts 'an older read that settles last does not repaint over the newer one' runner=vitest

# The same stamp's other half, on the exit nothing else reaches. An older read
# can REJECT after a newer one has already repainted the screen, and an
# unstamped catch then puts «не вдалося прочитати стан індексу» over numbers
# that were read successfully. Only a fixture that rejects the older of two
# deferred reads can see it — the reversed-order case above resolves both.
# Rewritten at Task 11b, same move as the case above.
case_ "an older read that is refused last must not put a failure over the newer numbers" \
  ui/src/settings/Settings.svelte \
  "s~      if \(seq !== settingsSeq\) return; // superseded before this rejection arrived\n~~" \
  "    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);" \
  src/settings/Settings.test.ts 'an older read that is refused last does not overwrite the newer numbers' runner=vitest

# 🔴 A sentence that outlives the state it describes — this project's own
# dominant late-PR class, in the smallest possible form. One refused re-read
# would otherwise leave "the state of the index could not be read" standing over
# numbers a later read confirmed, for the rest of the session. Every test that
# only ever fails, or only ever succeeds, passes under this mutant.
# Rewritten at Task 11b, same move as the two cases above. The one test that
# drove both directions is now two, and this case takes the half that can only
# be seen by a success FOLLOWING a failure.
case_ "a read that succeeds must take the failure sentence away with it" \
  ui/src/settings/Settings.svelte \
  "s~      settings = s;\n      loadError = null;~      settings = s; // mutant: the failure sentence outlives the failure~" \
  "      settings = s; // mutant: the failure sentence outlives the failure" \
  src/settings/Settings.test.ts 'a live success after a rejection takes the failure banner away and shows the new numbers' runner=vitest

# 🔴 Rewritten at Task 11b, and the ARGUMENT changed with the code rather than
# only the file name. The old sentence was "a nav change destroys this section,
# so its listener must die with it" — and since F10 (Task 10e) a nav change
# destroys nothing that holds this subscription: it is the WINDOW's, opened once
# in `Settings.svelte`'s own `onMount`. What it must not outlive is the window.
# An `onMount` that does not RETURN the unsubscriber leaves a live listener
# behind on every settings window a person opens and closes, each one re-reading
# `model_settings` on every ending for the rest of the process. Only a counted
# fixture that destroys the window and then delivers one more snapshot can see
# it, which is what the named test is.
case_ "the job subscription must die with the window, not outlive it" \
  ui/src/settings/Settings.svelte \
  "s~    return stop;~    void stop; // mutant: the subscription outlives the window~" \
  "    void stop; // mutant: the subscription outlives the window" \
  src/settings/Settings.jobs-teardown.test.ts "the window's own subscription to jobs.state is torn down on unmount, not just the channel jobs.mount guards separately" runner=vitest

# ---------------------------------------------------------------------------
# F4 (spec §9.3, amended 2026-09-04): the embedding queue.
# `IndexRead.pendingChunks` — a tray Stop mid-pass, then a restart, left
# thousands of chunks un-embedded with nothing on any screen saying so; the
# only resume was the Scan button beside the right folder happening to chain
# into an embed. The cases: the button must start the right pass, the line
# must not claim a queue that is empty, neither may show while the strip
# above already owns a running pass, and the `ended` phase alone must keep
# them visible.
# ---------------------------------------------------------------------------

# `scan` chains an embed only if the walk actually reads the folder and both
# preconditions hold — so a button that called it instead would sometimes
# still end up embedding, sometimes not, and always run a walk nobody asked
# for. Killed by asserting BOTH sides: `startEmbedJob` called and
# `startWalkJob` never.
# Rewritten at Task 11b. There is one `scan(entry)` on the controller now rather
# than a `scan()` and an `embed()`, so the mutant is the entry point hardcoded to
# `'full'` — the button that reads «Продовжити вбудовування» and walks every
# watched folder instead, over an archive whose reading pass already finished.
case_ "the resume button must start the embedding pass, not a folder scan" \
  ui/src/settings/Scanning.svelte \
  "s~    onclick={\(\) => jobs\.scan\(sectionAction\.entry\)}~    onclick={() => jobs.scan(/* mutant: resumes by scanning instead of embedding */ 'full')}~" \
  "onclick={() => jobs.scan(/* mutant: resumes by scanning instead of embedding */ 'full')}" \
  src/settings/Scanning.test.ts 'a waiting queue with no incomplete marker offers to resume embedding, not a full scan' runner=vitest

# `>= 0` is true of an empty queue too, so this is the mutant that draws
# "0 chunks not embedded yet" beside a button that would resume nothing. Every
# fixture with a real queue passes under it; only the empty-queue state can
# tell the two conditions apart.
# Rewritten at Task 11b: D-m moved the decision into `continueAction`
# (`jobs.ts`), so the section no longer answers it and this case follows it
# there. `>= 0` is true of an empty queue too, which is the mutant that draws
# «Продовжити вбудовування» beside a queue holding nothing; every fixture with a
# real queue passes under it.
case_ "the pending line must not draw when the queue is empty" \
  ui/src/settings/jobs.ts \
  "s~  if \(read\.pendingChunks > 0\) return \{ entry: 'embedOnly', where: 'section', label: 'resume' \};~  if (read.pendingChunks >= 0) return { entry: 'embedOnly', where: 'section', label: 'resume' }; // mutant: an empty queue still shows~" \
  "if (read.pendingChunks >= 0) return { entry: 'embedOnly', where: 'section', label: 'resume' }; // mutant: an empty queue still shows" \
  src/settings/Scanning.test.ts 'idle with neither marker nor queue shows the scan button alone' runner=vitest

# The phase half of the gate, dropped: the count on screen is a moment-old
# read that does not shrink as a resumed run works through the queue, so
# without this the line and the button would sit beside the strip's own
# progress, both claiming the same chunks. Every idle/ended fixture passes
# under this mutant; only a fixture that drives the controller into `running`
# can see the gate is gone.
# Rewritten at Task 11b, same move into `continueAction` as the case above. The
# count on screen is a moment-old read that does not shrink as a resumed run
# works through the queue, so without this the section's offer sits beside the
# strip's own progress, both claiming the same chunks — and the index's markers
# are still SET while a scan is under way, because the pass that finishes is
# what clears them, so a table reading them first would offer a second scan over
# the one already going.
case_ "the pending line and its button must step aside while a run is under way" \
  ui/src/settings/jobs.ts \
  "s~  if \(snapshot\.kind === 'running'\) return null;~  // mutant: shows regardless of the running pass~" \
  "  // mutant: shows regardless of the running pass" \
  src/settings/Scanning.test.ts 'a run under way hides the scan button and the continue row both' runner=vitest

# 🔴 RETIRED at Task 11b — «the pending line and its button must survive the
# phase reaching ended, not only idle». The `idle || ended` gate it quoted is
# gone: R2-2 found that an `Ended` snapshot naming no resumption, over an index
# still carrying a marker, was offered NOTHING by either surface, and D-m
# replaced the gate with one pure `continueAction` whose only phase condition is
# that a scan is not RUNNING. So «ended» is no longer an arm to be dropped — it
# is every state that is not running.
#
# The property survives in the wider form and is carried by
# `scripts/mutations/pr9b-scan.sh`'s «the index's own markers must be offered
# after an ending too, not only when idle», which puts the `idle` gate back and
# is killed by 'an ended report naming no resumption still offers to continue
# when the index marks a walk incomplete' (`Scanning.test.ts`).

# ---------------------------------------------------------------------------
# F1 (measured live, 2026-09-04): `formatIndexedDate`'s own trailing-stop
# strip. `ui/src/i18n/recency.ts`, not `Scanning.svelte` — the two other files
# this case file already reaches beyond its own header's list
# (`ui/src/i18n/shortcut.ts` below) — because the fix is one function used by
# every locale, and the fixture that tells "uk ends in «р.»" apart from "the
# rest of the sentence" lives beside that function.
# ---------------------------------------------------------------------------

case_ "the date must not carry its own trailing stop into the sentence around it" \
  ui/src/i18n/recency.ts \
  's~    \.format\(new Date\(indexedAt \* 1000\)\)\n    \.replace\(/\\\.\$/, \x27\x27\);~    .format(new Date(indexedAt * 1000)); // mutant: the trailing stop survives~' \
  '.format(new Date(indexedAt * 1000)); // mutant: the trailing stop survives' \
  src/i18n/recency.test.ts 'carries no trailing stop, so the sentence around it supplies the only one' runner=vitest

# ---------------------------------------------------------------------------
# PR 9 Task 7 — the Application section: the shortcut, autostart, the version.
# `ui/src/i18n/shortcut.ts` first (the two pure functions), then
# `ui/src/settings/Application.svelte`.
# ---------------------------------------------------------------------------

# D-b step 3, this side of it: `Space` alone parses on the Rust side and would
# take the space bar away system-wide, so the recorder refuses a press that
# carries no modifier before it ever reaches `set_hotkey`. Every fixture that
# presses a key WITH a modifier passes under this mutant; only the bare-key
# fixture can see the guard is gone.
case_ "the recorder must refuse a press that carries no modifier" \
  ui/src/i18n/shortcut.ts \
  's~if \(held\.length === 0\) return null;~if (false) return null; // mutant: a press with no modifier is accepted~' \
  'if (false) return null; // mutant: a press with no modifier is accepted' \
  src/i18n/shortcut.test.ts 'a press with no modifier at all builds nothing' runner=vitest

# The canonical order is shared by `formatShortcut` and `shortcutFromEvent`
# through one constant, and it is canonical rather than incidental (D-j): what
# it buys is that two people pressing the same keys store the same string and
# read the same label. A press event carries four booleans and no order of its
# own, so a wrong fixed order is indistinguishable from a right one on every
# single-modifier fixture — only a combination of two or more modifiers sees it.
case_ "the modifiers must be joined in the one canonical order, not some other fixed one" \
  ui/src/i18n/shortcut.ts \
  "s~const ORDER = \['Ctrl', 'Alt', 'Shift', 'Super'\] as const;~const ORDER = ['Alt', 'Ctrl', 'Shift', 'Super'] as const; // mutant: the canonical order is not the one the parser and the glyphs agree on~" \
  "const ORDER = ['Alt', 'Ctrl', 'Shift', 'Super'] as const; // mutant: the canonical order is not the one the parser and the glyphs agree on" \
  src/i18n/shortcut.test.ts 'the modifiers are emitted in the canonical order, whichever way the event states them' runner=vitest

# D-c: `set_autostart` re-reads the OS after the change and answers THAT,
# never the request. A mutant that draws the outgoing boolean instead is
# invisible on every fixture where the reply happens to agree with the
# request — which the "once" fixture's own reply does, on purpose, to test the
# toggle direction rather than this. Only the fixture whose reply DISAGREES
# with the request (a failed re-read reported as `unknown`) can tell them apart.
case_ "the autostart control must draw the OS's reply, not the request it sent" \
  ui/src/settings/Application.svelte \
  "s~if \(prefs !== null\) prefs = \{ \.\.\.prefs, autostart: reply \};~if (prefs !== null) prefs = { ...prefs, autostart: target ? { kind: 'enabled' } : { kind: 'disabled' } }; // mutant: renders the request instead of the reply~" \
  "// mutant: renders the request instead of the reply" \
  src/settings/Application.test.ts 'the autostart state drawn after a press is the reply, not the request' runner=vitest

# D128: `registered` and `unavailable` are two different facts about the
# shortcut and must read as two different sentences — never one worded as
# though it covered both. A mutant that always says "registered" is invisible
# on every fixture that only ever renders ONE of the two states; only the case
# that renders both and compares them can see the collapse.
case_ "unavailable must not be worded as registered" \
  ui/src/settings/Application.svelte \
  "s~return hotkey\.status\.kind === 'registered'~return true // mutant: unavailable reads as registered~" \
  "return true // mutant: unavailable reads as registered" \
  src/settings/Application.test.ts 'the two shortcut states get two sentences, not one drawn twice' runner=vitest

# 🔴 D-b's closing note: a rejected `set_hotkey` carries no `HotkeyState` at
# all — which of the table's seven rows produced it is not recoverable from the
# sentence alone, so the only honest source for what the screen draws next is a
# fresh `appPrefs()`, never the value the window held before the call. Every
# fixture whose fresh read happens to answer with the SAME shortcut the window
# already held would pass under a mutant that skips the re-read entirely; only
# the pair that changes the answer between the two reads can see it, and this is
# the first of that pair.
case_ "a rejected set_hotkey must trigger a fresh read, not keep the pre-call value" \
  ui/src/settings/Application.svelte \
  's~hotkeyError = err instanceof Error \? err\.message : String\(err\);.*?void refresh\(\)\.then\(.*?\}\);~hotkeyError = err instanceof Error ? err.message : String(err); // mutant: a rejected set_hotkey does not re-read appPrefs~s' \
  '// mutant: a rejected set_hotkey does not re-read appPrefs' \
  src/settings/Application.test.ts 'a refused change shows the sentence and then draws the NEW shortcut when a fresh read reports it' runner=vitest

# D-i: `platform` comes from the WIRE — `Platform::of_this_build`, chosen at
# compile time on the Rust side — and never from `navigator.userAgent`; that
# type's own doc records this project measuring a plausible proxy wrong twice,
# on two platforms. Every fixture whose reply platform happens to match the
# TEST RUNNER's own platform would pass under a mutant that reads the browser
# instead of the reply; jsdom's `navigator.userAgent` names neither `Mac OS X`
# nor `Windows`, so only a fixture that sends `platform: 'mac'` over the wire —
# while running in an environment that is not one — can tell the two apart.
case_ "the shortcut's platform must come from the wire, never guessed from the browser" \
  ui/src/settings/Application.svelte \
  "s~  const platform = \\\$derived\(prefs === null \? null : prefs\.platform\);~  const platform = \\\$derived(prefs === null ? null : (navigator.userAgent.includes('Mac') ? 'mac' : 'linux')); // mutant: platform read from navigator.userAgent instead of the wire~" \
  "const platform = \$derived(prefs === null ? null : (navigator.userAgent.includes('Mac') ? 'mac' : 'linux')); // mutant: platform read from navigator.userAgent instead of the wire" \
  src/settings/Application.test.ts 'a mac reply is drawn with mac glyphs even though this window is not running on a mac' runner=vitest

# 🔴 (review, Important 3) The push order into `held` (`shortcut.ts:150-158`)
# now reads Super/Shift/Alt/Ctrl — deliberately NOT `ORDER` — so this line's
# `.filter` is what produces the canonical string, not a coincidence with the
# push sequence. Before that reordering, `held` and `ORDER.filter(...)` were
# the identical array for every input, and this exact mutant survived the
# whole suite; it is written now because the fix that closes Important 3 is
# what makes it expressible.
case_ "the join must read the canonical ORDER, not the sequence the flags were pushed in" \
  ui/src/i18n/shortcut.ts \
  "s~return \[\.\.\.ORDER\.filter\(\(m\) => held\.includes\(m\)\), key\]\.join\('\+'\);~return [...held, key].join('+'); // mutant: joins the push order instead of the canonical one~" \
  "return [...held, key].join('+'); // mutant: joins the push order instead of the canonical one" \
  src/i18n/shortcut.test.ts 'the modifiers are emitted in the canonical order, whichever way the event states them' runner=vitest

# ── Task 11a, fix round 1 (review, Minor 4): the fixture binds BOTH halves ────
#
# `scripts/mutations/pr9-shell.sh` carries a case judged by the Rust loop over
# `ui/src/i18n/shortcut.fixtures.json`, on the argument that a case the
# fixture's own loop could not kill would say the sharing is decorative. That
# argument is symmetric and only one half had a case: nothing in the harness
# would have noticed the TypeScript loop quietly ceasing to bind. This is the
# mirror, and it is deliberately the SAME property the Rust case mutates — the
# command key's per-platform spelling — so the two cases say the one fixture row
# is load-bearing on both sides rather than each testing something of its own.
#
# The mac and linux fixture rows survive this mutant untouched, and so does
# every `shortcutFromEvent` test in the file: the recorder never reads
# `SUPER_WORD`.
#
# ⚠️ Anchored on the `const SUPER_WORD` line above it, because `windows: 'Win',`
# on its own is NOT unique in that file — `MODIFIER_KEY_NAME` at the bottom
# spells it the same way, for prose rather than for the formatter. An
# unanchored pattern matches twice and this harness refuses it.
case_ "the shared fixture kills a wrong per-platform spelling of the command key, this side too" \
  ui/src/i18n/shortcut.ts \
  "s~const SUPER_WORD: Record<Exclude<Platform, 'mac'>, string> = \{\n  windows: 'Win',~const SUPER_WORD: Record<Exclude<Platform, 'mac'>, string> = {\n  windows: 'Super', // mutant: the parsers spelling on a platform that prints Win~" \
  "windows: 'Super', // mutant: the parsers spelling on a platform that prints Win" \
  src/i18n/shortcut.test.ts 'the shared fixture is what this formatter produces' runner=vitest

# ── The final whole-branch review's two Application-section fixes ─────────────

# 🔴 D-I1: the per-field stamps collapsed back into one, which is the shape the
# review found. Three writers share this guard and they write DISJOINT fields,
# so a single generation is field-blind: a `setAutostart` that succeeds while
# the corrective `appPrefs()` a refused `setHotkey` started is still in flight
# discards that read whole, and the window goes on drawing the old shortcut
# beside "the shortcut was not changed" while the operating system holds the new
# one (D-b's persist-failure row).
#
# Every fixture that drives ONE writer survives this mutant, including the one
# that pins the stamp itself — hotkey against hotkey, where superseding IS
# correct. Only a fixture that drives both writers can see it, which is why
# there had to be a new one. The mirror case (`a successful shortcut change …`)
# goes red under the same mutant, from the other direction.
case_ "one stamp per field, because a write to one field says nothing about another" \
  ui/src/settings/Application.svelte \
  's~      const takeAutostart = myAutostart === autostartSeq;~      const takeAutostart = myHotkey === hotkeySeq; // mutant: one stamp decides both fields~' \
  'const takeAutostart = myHotkey === hotkeySeq; // mutant: one stamp decides both fields' \
  src/settings/Application.test.ts 'a successful autostart change must not discard the re-read a refused shortcut change started' runner=vitest

# 🔴 D-I2: the record control's in-flight guard, never armed — so the button
# stays enabled and focused across the whole of `setHotkey`, and a second
# recording sends a second call. The backend serialises the two behind
# `change_hotkey`'s critical section; the window paints whichever reply resolves
# last, which need not be the one the operating system kept. One assignment
# rather than the `disabled` attribute alone: this deletes both halves of the
# guard, which is what "no in-flight guard" was.
case_ "the record control must be busy while a shortcut change is in flight" \
  ui/src/settings/Application.svelte \
  's~    hotkeyBusy = true;~    hotkeyBusy = false; // mutant: the record control is never busy~' \
  'hotkeyBusy = false; // mutant: the record control is never busy' \
  src/settings/Application.test.ts 'the record control is busy while a change is in flight, so a second press sends one call and does not reopen the recorder' runner=vitest

# External review P2. The `Unknown` arm collapsed back into the single toggle:
# `autostartOffersBothDirections` answers `false`, so a state the operating
# system could not be READ for gets one button, and `autostartIsEnabled` is
# `false` there — so that button is Enable. Somebody whose machine really does
# start Mnema, and whose read merely failed, is offered no way to turn it off.
# Every `Enabled`/`Disabled` fixture in the file survives this mutant: none of
# them is in the state it is about.
case_ "an unreadable autostart offers both directions, not the one Enable" \
  ui/src/settings/Application.svelte \
  's~  const autostartOffersBothDirections = \$derived\(autostartUnknown !== null\);~  const autostartOffersBothDirections = \$derived(false); // mutant: the unknown arm renders one button~' \
  'const autostartOffersBothDirections = $derived(false); // mutant: the unknown arm renders one button' \
  src/settings/Application.test.ts 'an unreadable autostart offers both directions, not just the one' runner=vitest

# External review P3. The heading is the old one whatever the corrective re-read
# says: «Скорочення не змінено» drawn beside the new shortcut the operating
# system is holding (transition-table row 6, `prefs.rs`) — the persist failed,
# the registration did not. Every fixture whose re-read reports the OLD shortcut
# survives this mutant, which is every one that existed before the finding.
case_ "a shortcut the system kept but could not save is not called unchanged" \
  ui/src/settings/Application.svelte \
  's~    return t\(shortcutNotSaved \? .application_shortcut_not_saved. : .application_shortcut_failed.\);~    return t("application_shortcut_failed"); // mutant: always the old heading~' \
  'return t("application_shortcut_failed"); // mutant: always the old heading' \
  src/settings/Application.test.ts 'a shortcut the system kept but could not save is not reported as unchanged' runner=vitest

# 🔴 Fix round 2. The corrective re-read's own stamp ignored: a read that was
# superseded still answers with the shortcut it found, and the rejection that
# started it then compares that answer against the shortcut ITS call sent. With
# two rejections in flight the stale one wins the comparison and rewrites the
# heading of a rejection it knows nothing about — «Скорочення діє…» drawn over a
# refusal that changed nothing at all.
#
# The judging test had to be rebuilt before this case could exist. Its first
# version superseded the held-open read with a SUCCESSFUL recording, which sets
# `hotkeyError = null` and takes the heading off screen by itself, so it
# asserted an absence the write-side stamp was already producing and this mutant
# survived it. It now supersedes with a second REJECTION, so the heading stays
# on screen and the assertion is positive: the sentence is still the live
# rejection's own.
#
# One site, deliberately. The same expression stood twice until fix round 2, and
# the second copy was unobservable — reachable only behind a successful
# `setHotkey`, which nulls the very error the sentence is drawn under. Mutated,
# it survived all 665 tests. Collapsing the pair is what makes this mutant
# killable at all.
case_ "a discarded corrective read must not choose the sentence" \
  ui/src/settings/Application.svelte \
  's~      const appliedHotkey = takeHotkey \? p\.hotkey : null;~      const appliedHotkey = p.hotkey; // mutant: a superseded read answers anyway~' \
  '// mutant: a superseded read answers anyway' \
  src/settings/Application.test.ts 'a corrective read the stamp discarded does not get to choose the sentence' runner=vitest
