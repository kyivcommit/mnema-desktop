# The §9.3 Indexing SECTION's own guards — `ui/src/settings/Indexing.svelte`,
# the file that says what the index holds. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9-ui.sh
#
# Not to be confused with `pr9-index.sh`, which covers the busy-index line in
# the window's job strip (`JobStrip.svelte`) and the Rust that feeds it. This
# file is PR 9 Task 6 only.
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
# arms** (review, Minor 6). `Indexing.svelte:105` — `const read` — has no case
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
  ui/src/settings/Indexing.svelte \
  "s~  const unreadable = \\\$derived\(index !== null && index\.kind === 'unreadable' \? index : null\);~  const unreadable = \\\$derived(index !== null \&\& index.kind === 'read' ? index : null); // mutant: the arms are not told apart~" \
  "const unreadable = \$derived(index !== null && index.kind === 'read' ? index : null); // mutant: the arms are not told apart" \
  src/settings/Indexing.test.ts 'an index that is not open says so, and shows the backend reason verbatim' runner=vitest

# 🔴 `lastIndexedAt: null` is the backend's own statement that nothing has ever
# finished indexing (`MAX(ingest_stage.updated_at)` over an empty set), and the
# section owes it a sentence. `?? 0` is the tidy-looking substitute and it draws
# 1 January 1970 with a relative phrase counting twenty thousand days beside it —
# two lines that look like measurements, on a screen whose whole job is to say
# what the index actually holds. Only a fixture in the empty state can tell the
# two apart; every filled-index assertion passes under this mutant.
case_ "an index nothing has ever finished indexing must not be given the epoch as its date" \
  ui/src/settings/Indexing.svelte \
  "s~  const lastIndexedAt = \\\$derived\(read === null \? null : read\.lastIndexedAt\);~  const lastIndexedAt = \\\$derived(read === null ? null : (read.lastIndexedAt ?? 0)); // mutant: null becomes the epoch~" \
  "const lastIndexedAt = \$derived(read === null ? null : (read.lastIndexedAt ?? 0)); // mutant: null becomes the epoch" \
  src/settings/Indexing.test.ts 'an index nothing has ever finished indexing says so, and draws no time at all' runner=vitest

# D-e: the date is not a duplicate of the phrase. «годину тому» is what a person
# feels; the date is what they compare against the file they edited this
# morning, and §9.3 asks for it by name («останнє оновлення з датою»). Dropping
# the line leaves a screen that still reads perfectly well and has quietly
# stopped answering the question the spec asked. The relative phrase survives
# this mutant, which is why an assertion on it cannot see the loss.
case_ "the date must be drawn beside the relative phrase, not replaced by it" \
  ui/src/settings/Indexing.svelte \
  's~\{#if dateLine\}<p data-testid="indexing-index-date">\{dateLine\}</p>\{/if\}\n~~' \
  '{#if filesLine}<p data-testid="indexing-index-files">{filesLine}</p>{/if}
{#if agoLine}<p data-testid="indexing-index-ago">{agoLine}</p>{/if}' \
  src/settings/Indexing.test.ts 'a filled index says how many files it holds, the date it last grew, and how long ago that was' runner=vitest

# 🔴 The PR 7 debt, and the mutant that makes it invisible again.
# `IndexRead::failed_chunks` is cumulative for the SPACE; `job::Progress::refused`
# is what the run that has just ended gave up on. `job.rs:38-44` holds them
# apart and says whichever surface shows them owes each its own words. One key
# for both draws two numbers under one subject — and every state with only ONE
# of the two on screen passes under this mutant, which is exactly why the suite
# needs the state that has both.
case_ "the run's refusals and the index's must not be drawn from one key" \
  ui/src/settings/Indexing.svelte \
  "s~    return t\('indexing_index_refused_run', \{ count: phase\.ending\.refused \}\);~    return t('indexing_index_failed_chunks', { count: phase.ending.refused }); // mutant: one subject for two scopes~" \
  "return t('indexing_index_failed_chunks', { count: phase.ending.refused }); // mutant: one subject for two scopes" \
  src/settings/Indexing.test.ts 'a run that gave up on chunks and an index that already had some show two sentences, each about its own subject' runner=vitest

# An ENDING is the one moment the numbers on this screen can have changed. A
# subscriber that re-fetches on every store emission answers every "does an
# ending re-read the index" test correctly and issues an IPC call per progress
# report for the whole of a long pass. The mirror — a progress event and a call
# count that does not move — is the only thing that tells the two apart.
case_ "the re-read must follow an ending, not every emission of the job store" \
  ui/src/settings/Indexing.svelte \
  "s~      if \(phase\.kind === 'ended'\) void refresh\(\);~      void refresh(); // mutant: every emission re-reads~" \
  "      void refresh(); // mutant: every emission re-reads" \
  src/settings/Indexing.test.ts 'a progress report is not an ending and re-reads nothing' runner=vitest

# 🔴 The decision that the two scope sentences do NOT share a fate. A pass ends,
# the ending triggers the re-read, and the re-read comes back `Unreadable` — an
# ordinary sequence, not a contrived pairing. Gated on `read`, the window would
# answer "the index could not be read" and delete, in the same breath, the only
# surviving report of what the pass just did. The mutant is the tidier-looking
# guard and the lossy one.
case_ "the run's own report must outlive a read of the index that fails" \
  ui/src/settings/Indexing.svelte \
  "s~    if \(phase\.kind !== 'ended' \|\| phase\.ending\.refused === 0\) return null;~    if (read === null || phase.kind !== 'ended' || phase.ending.refused === 0) return null; // mutant: the run's report dies with the index~" \
  "if (read === null || phase.kind !== 'ended' || phase.ending.refused === 0) return null; // mutant: the run's report dies with the index" \
  src/settings/Indexing.test.ts 'an index that stops being readable still says what the pass that just ended gave up on' runner=vitest

# 🔴 Two reads can be in flight here whenever endings arrive faster than the IPC
# answers, and they may settle in either order. Without the stamp the older
# reply repaints the screen with numbers taken before the pass that triggered
# the newer one — and nothing on the screen says so, because both answers are
# well-formed. Only a fixture that resolves them in reverse can see it.
case_ "an older read that settles last must not write over the newer one" \
  ui/src/settings/Indexing.svelte \
  "s~      if \(seq !== settingsSeq\) return; // a newer read has already spoken\n~~" \
  "      const s = await modelSettings();
      settings = s;" \
  src/settings/Indexing.test.ts 'an older read that settles last does not repaint over the newer one' runner=vitest

# The same stamp's other half, on the exit nothing else reaches. An older read
# can REJECT after a newer one has already repainted the screen, and an
# unstamped catch then puts «не вдалося прочитати стан індексу» over numbers
# that were read successfully. Only a fixture that rejects the older of two
# deferred reads can see it — the reversed-order case above resolves both.
case_ "an older read that is refused last must not put a failure over the newer numbers" \
  ui/src/settings/Indexing.svelte \
  "s~      if \(seq !== settingsSeq\) return; // superseded before this rejection arrived\n~~" \
  "    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);" \
  src/settings/Indexing.test.ts 'an older read that is refused last does not put a failure over the newer numbers' runner=vitest

# 🔴 A sentence that outlives the state it describes — this project's own
# dominant late-PR class, in the smallest possible form. One refused re-read
# would otherwise leave "the state of the index could not be read" standing over
# numbers a later read confirmed, for the rest of the session. Every test that
# only ever fails, or only ever succeeds, passes under this mutant.
case_ "a read that succeeds must take the failure sentence away with it" \
  ui/src/settings/Indexing.svelte \
  "s~      settings = s;\n      loadError = null;~      settings = s; // mutant: the failure sentence outlives the failure~" \
  "      settings = s; // mutant: the failure sentence outlives the failure" \
  src/settings/Indexing.test.ts 'a re-read that is refused says so beside the numbers it could not confirm, and stops saying it once one succeeds' runner=vitest

# 🔴 This section sits inside `Settings.svelte`'s `{#if section === …}` chain, so
# every nav change destroys and rebuilds it. An `onMount` that does not RETURN
# the unsubscriber leaves a live listener behind on each visit, and one ending
# then fans out to all of them. It is invisible to every assertion phrased as
# "at least once": only a counted fixture — three mounts, one ending, one
# re-read — can see it.
case_ "the job subscription must die with the component, not outlive it" \
  ui/src/settings/Indexing.svelte \
  "s~    return stop;~    void stop; // mutant: the subscription outlives the component~" \
  "    void stop; // mutant: the subscription outlives the component" \
  src/settings/Indexing.test.ts 'a section left behind by a nav change stops listening — three mounts, one ending, one re-read' runner=vitest
