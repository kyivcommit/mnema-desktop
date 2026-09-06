# PR 9b — the scan: one job over every watched folder, the reading pass, the
# embedding phase behind it, the slot that outlives both, and the two surfaces
# that draw them. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9b-scan.sh
#
# The files this reaches, and none of them by accident:
#
#   src-tauri/src/scan_job.rs   the pass itself — order, cancellation, counters
#   src-tauri/src/state.rs      the slot: the claim, the ending, the drop policy
#   src-tauri/src/bridge.rs     the removal that races the pass (D-k)
#   src-tauri/src/tray.rs       «Продовжити сканування» and what enables it
#   src-tauri/src/lib.rs        the menu arm that must not start a scan inline
#   ui/src/settings/jobs.ts     `apply` and `continueAction` (D-m)
#   ui/src/settings/JobStrip.svelte    `readingKind` (D-e)
#   ui/src/settings/Folders.svelte     the withdrawal (D-1) and the removal
#   ui/src/settings/Settings.svelte    the window's own re-read triggers
#
# No count of the cases here, deliberately — `pr8-exclusions.sh` explains what
# that number costs when it goes stale. Re-derive:
#
#   grep -c '^case_ ' scripts/mutations/pr9b-scan.sh
#   grep -oE "^  mnema-desktop '[^']+'" scripts/mutations/pr9b-scan.sh | sort -u
#
# ⚠️ `^  ` is load-bearing in that second line, not tidiness. Unanchored, the
# pattern also matches the copy of ITSELF written a few lines below (`[^']+`
# matches `[^`), so the recipe answered 20 where the file holds 19 — a count
# from a query that read its own instructions. A case invocation's package
# always sits at the start of a continuation line under two spaces; a comment
# starts with `#`.
#
# ⚠️ **Two test shapes on the cargo side, and the selector is what tells them
# apart** — the same trap `pr9-shell.sh` documents. The `--lib` cases name unit
# tests inside a module's own `mod tests`, so their names carry the
# `scan_job::tests::` / `state::tests::` / `tray::tests::` / `bridge::tests::`
# prefix (and `tests::` alone for `lib.rs`, whose test module is the crate
# root's). The `--test commands` cases name integration tests in
# `src-tauri/tests/commands.rs`, which carry NO prefix. Getting the two the
# wrong way round makes `--exact` select nothing, and the harness reports a
# BASELINE FAILURE for the whole file rather than a result.
#
# ⚠️ **Some named tests are `#[cfg(unix)]`**, so a Windows leg would fail this
# file's baseline WHOLE — every case in it, before any mutation runs — the way
# `pr8-exclusions-macos.sh` had to be split off for. Both of this repository's
# CI legs are unix. Which tests those are is taken from the code rather than
# from this sentence, which would go stale:
#
#   for t in $(grep -oE "^  mnema-desktop '[^']+'" scripts/mutations/pr9b-scan.sh \
#               | sed "s/.*'\(.*\)'/\1/" | sed 's/.*:://' | sort -u); do
#     grep -rn -B4 "fn ${t}(" src-tauri | grep -q 'cfg(unix)' && echo "$t"
#   done
#
# 🔴 **Three guards the plan asked for are NOT here, and their absence is the
# claim rather than an omission.** The count is three because it was four until
# fix round 1: the header used to say «two» while listing three, and a fourth —
# `revision` not bumped on `finish` — was missing from the list altogether,
# which is the shape this file exists to catch. All of it is written up in
# `task-11b-report.md`; in short:
#
#   • `Pool::new` hoisted above the per-folder loop — the poison-across-roots
#     observation does not exist (Task 2's open acceptance item, carried by
#     name into the review). A case with no oracle is not a case.
#   • `namesFolder`'s reconciliation moved below the next `await` in
#     `Folders.svelte`'s `refresh()` — there is no next `await`: everything
#     after `listTree()` returns is synchronous, so the mutation cannot be
#     constructed at all.
#   • the removal's path re-derived from the id before the claim — written,
#     measured GREEN, removed. `remove_hook` fires AFTER the claim, which is
#     the window the swap fixture models, so a pre-claim re-derivation still
#     reads the folder's real path and the compare below still refuses. Not an
#     equivalent mutant; simply one no fixture here reaches.
#
# CLOSED, and named so the list reads as a history rather than as a standing
# gap. Fix round 1: `revision` not bumped on `finish` had no oracle when this
# file was written; `state::tests::an_ending_moves_the_revision_once_and_is_
# announced_after_it_is_written` is that oracle now. Final fix round 1: the
# three REMAINING writes to `ScanState` — `drop`, `update` and
# `mark_reading_done` — had the same hole, and `drop`'s is the one no later
# write can repair. All four cases are below, together.

# ── The job slot: what a surface is told, and when (Task 1) ──────────────────

# 🔴 The announcement moved in front of the write it announces. An observer is
# handed nothing and READS `AppState::scan_state` for itself (`JobObserver`'s
# own doc), so an announcement fired before the ending is written finds the job
# still running — a tray that goes on offering Stop for a job that has already
# gone, until the next event happens to arrive. `JobSlot::drop`'s own ordering
# is `pr9-shell.sh`'s case; this is the same rule on the path every job that
# ends NORMALLY takes, which that case cannot reach.
#
# Two substitutions in one expression: the first puts the announcement at the
# top of `finish`, the second removes the real one, so the recorded log holds
# the phase twice instead of the phase and then the ending.
case_ "the ending is written before the observer is told, on the ordinary path too" \
  src-tauri/src/state.rs \
  's~    pub fn finish\(mut self, terminal: crate::scan_state::Terminal, files: Option<i64>\) \{\n~    pub fn finish(mut self, terminal: crate::scan_state::Terminal, files: Option<i64>) \{\n        self.announce(); // mutant: the observer is told before the ending is written\n~; s~        self\.finished = true;\n        self\.announce\(\);~        self.finished = true;~' \
  '// mutant: the observer is told before the ending is written' \
  mnema-desktop 'state::tests::the_observer_hears_a_job_start_and_finish' --lib

# 🔴 The ending's own `revision` bump, deleted — and every surface goes blind to
# the ending while the state itself stays perfectly correct. `revision` is the
# only thing that says one read of `ScanState` is newer than another
# (`scan_state.rs`'s own field doc), and `ui/src/settings/jobs.ts`'s `apply`
# keeps the higher one and DROPS everything else: an ending carrying the number
# the last progress tick already had is an ending the window throws away, so the
# strip goes on drawing the run, Stop live, over a slot that is free. Nothing
# arrives later to correct it, because the job that would have announced again
# has gone.
#
# Every assertion phrased on the SNAPSHOT passes under this mutant — the
# snapshot really does change — which is why this guard was missing until fix
# round 1 and why the case is anchored on the `if let Some(files)` block above
# it: `scan.revision += 1;` occurs six times in this file, and five of them are
# other writes that keep their own bump.
case_ "an ending must move the revision, or no surface can tell it happened" \
  src-tauri/src/state.rs \
  's~            if let Some\(files\) = files \{\n                scan\.files = files;\n            \}\n            scan\.revision \+= 1;\n        \}~            if let Some(files) = files \{\n                scan.files = files;\n            \}\n            // mutant: the ending carries the revision the last tick already had\n        \}~' \
  '// mutant: the ending carries the revision the last tick already had' \
  mnema-desktop 'state::tests::an_ending_moves_the_revision_once_and_is_announced_after_it_is_written' --lib

# 🔴 **The same defect at the three remaining writes, and `Drop`'s is the one
# that never recovers.** `revision` is the only thing that makes a write visible
# — `apply` keeps the strictly greater state and throws the rest away — and the
# state itself is left perfectly correct by every one of these mutants, so an
# assertion phrased on the snapshot passes against all three. `finish` had this
# guard from fix round 1; the final review found the other three writes with no
# test that could tell them from a version that skipped the bump.
#
# Each is anchored on the lines above the bump rather than on the bump itself:
# `scan.revision += 1;` occurs six times in this file and nothing else about
# them differs.

# 🔴 The worst of the three. A scan that panics or returns through `?` inside the
# reading pass leaves its ending to `Drop` — and there is NO LATER WRITE, because
# the job that would have announced again has gone. The strip goes on drawing a
# reading pass with Stop live over a slot that is free, for the life of the
# window; only a reload recovers, since `mount()` reads `jobStatus` against
# `apply`'s revision-0 default. The tray is unaffected — it re-reads
# `scan_state()` — so the two surfaces disagree and only the window is wrong.
case_ "a job that vanished must move the revision, or the window never hears it ended" \
  src-tauri/src/state.rs \
  's~            \} else \{\n                crate::scan_state::ScanSnapshot::Idle\n            \};\n            scan\.revision \+= 1;~            } else \{\n                crate::scan_state::ScanSnapshot::Idle\n            \};\n            // mutant: a job that vanished leaves the counter where it was~' \
  '// mutant: a job that vanished leaves the counter where it was' \
  mnema-desktop 'state::tests::a_reading_job_that_vanished_ends_with_the_report_nobody_wrote' --lib

# 🔴 `update` is the ONLY write during a running scan: the claim publishes
# `root_count: 0` and an empty path, and everything after it — the real folder
# count, every throttled progress tick, every folder change, and the Reading →
# Embedding phase change — is this function. With no bump `apply` discards all of
# it, so the strip draws «0 з 0» over an empty folder name for the whole scan and
# then jumps straight to the report, with the phase never changing on screen.
case_ "a progress tick must move the revision, or the whole scan is invisible" \
  src-tauri/src/state.rs \
  's~                phase,\n                cancellable: self\.cancellable,\n            \};\n            scan\.revision \+= 1;~                phase,\n                cancellable: self.cancellable,\n            \};\n            // mutant: a progress tick leaves the counter where it was~' \
  '// mutant: a progress tick leaves the counter where it was' \
  mnema-desktop 'state::tests::only_a_finished_reading_pass_moves_read_seq' --lib

# The milder of the three, and the guard is owed anyway. `mark_reading_done`
# moves `read_seq` and `last_reading` with NO snapshot change at all, so
# `revision` is the only thing that can carry it: unbumped, the window never
# learns the reading pass ended, does not re-read the index, and the partial-read
# warning never appears. It is transient rather than permanent — the next write
# carries the same fields with a higher revision — but what makes it transient is
# a caller's behaviour, not this function's contract.
#
# ⚠️ This case and the one above name the SAME test, and that is the point: until
# the final review that test asserted `done.revision > claimed.revision` end to
# end, which either bump satisfies alone. It now reads the revision between the
# two calls and asserts each step is exactly one, so each mutant dies on its own
# assertion rather than on its neighbour's.
case_ "a reading pass ending must move the revision on its own, not on the tick before it" \
  src-tauri/src/state.rs \
  's~            scan\.last_reading = Some\(outcome\);\n            scan\.read_seq \+= 1;\n            scan\.revision \+= 1;~            scan.last_reading = Some(outcome);\n            scan.read_seq += 1;\n            // mutant: the pass ends without moving the counter~' \
  '// mutant: the pass ends without moving the counter' \
  mnema-desktop 'state::tests::only_a_finished_reading_pass_moves_read_seq' --lib

# 🔴 A reading job that vanished, sent back to `Idle`. The window then says the
# folder was read to the end when it was not — an idle application over an index
# missing whatever the job never reached, with nothing left to correct it. Every
# fixture whose job ENDS properly passes under this mutant; only a slot dropped
# without `finish` can see it.
case_ "a reading slot dropped without a report ends Failed, never Idle" \
  src-tauri/src/state.rs \
  's~                    phase: crate::scan_state::Phase::Reading \{ \.\. \},\n                    \.\.\n                \} => Some\(crate::scan_state::EndedIn::Reading\),~                    phase: crate::scan_state::Phase::Reading { .. },\n                    ..\n                } => None, // mutant: a vanished reading job looks like a clean idle~' \
  '} => None, // mutant: a vanished reading job looks like a clean idle' \
  mnema-desktop 'state::tests::a_reading_job_that_vanished_ends_with_the_report_nobody_wrote' --lib

# 🔴 The claim blanks what the last pass concluded. `last_reading` is the only
# account a reopened window has of the scan that just ran, and a job starting is
# not a job having answered — so a person who presses «Продовжити вбудовування»
# would watch the partial-read warning disappear at the moment the resumption
# begins, with nothing that ever says it was true.
case_ "claiming the slot must not un-read the pass before it" \
  src-tauri/src/state.rs \
  's~            scan\.revision \+= 1;\n            // `files`, `read_seq` and `last_reading` are deliberately untouched:~            scan.revision += 1;\n            scan.last_reading = None; // mutant: the claim blanks the pass before it\n            // `files`, `read_seq` and `last_reading` are deliberately untouched:~' \
  'scan.last_reading = None; // mutant: the claim blanks the pass before it' \
  mnema-desktop 'state::tests::only_a_finished_reading_pass_moves_read_seq' --lib

# ── The reading pass: order, cancellation, counters (Tasks 2 and 3) ──────────

# 🔴 D-f, inverted: the list of watched folders is read BEFORE the slot is
# claimed. That is the ordering the deleted `start_walk_job` used and the one
# this file exists to reject — between an unclaimed read and the walk that acts
# on it, a folder can be removed and another added, SQLite hands the new row the
# id the old one gave up, and the walk writes the first folder's files into the
# second folder's `path` rows. The scan still reports `completed`.
#
# 🔴 **Production order only.** The hook the named test drives is called from
# inside `read_roots` (D-l), so it travels with the call: this expression moves
# one production line and touches no test code at all. A hook installed at the
# call site would have stayed behind while the call moved, and the mutant would
# then have been killed by the fixture rather than by the defect.
# ⚠️ Rewritten in the final fix round, because B-I1 changed the shape of the
# line this quotes: `read_roots` is a `match` now, so that the refusal it can
# answer with gets its own ending instead of falling to the drop policy. The
# mutant is therefore the whole pre-fix line — `read_roots(state)?` ahead of the
# claim — rather than a pure move of the match, which would not compile: the
# `Err` arm calls `slot.finish`, and there is no slot yet above the claim. That
# is the ordering this case has always been about, and the named test does not
# reach the rules path at all, so the second half of the revert changes nothing
# about why it dies.
case_ "the job slot must be taken before the list of folders is read, not after" \
  src-tauri/src/scan_job.rs \
  's{    let slot = state\.claim_job\(\n        Phase::Reading \{\n            root_index: 0,\n            root_count: 0,\n            root_path: String::new\(\),\n            counts: Progress::default\(\),\n        \},\n        true,\n    \)\?;\n\n(.*?)    let roots = match read_roots\(state\) \{\n        Ok\(roots\) => roots,\n.*?\n    \};\n}{    let roots = read_roots(state)?; // mutant: the folder list is read before the slot is claimed\n    let slot = state.claim_job(\n        Phase::Reading \{\n            root_index: 0,\n            root_count: 0,\n            root_path: String::new(),\n            counts: Progress::default(),\n        \},\n        true,\n    )?;\n\n$1}s' \
  'let roots = read_roots(state)?; // mutant: the folder list is read before the slot is claimed' \
  mnema-desktop 'scan_job::tests::a_root_swapped_between_the_read_and_the_walk_is_not_walked_under_its_successors_id' --lib

# The pass ends and says nothing about itself. `read_seq` never moves, so every
# consumer watching for "a reading pass has ended" — the folder list's
# withdrawal, the window's re-read — waits for ever, and `last_reading` stays
# whatever the run before it left. The job still ends correctly, the index is
# still written, and only an assertion about the PASS can see it.
case_ "a reading pass that ended must record that it did" \
  src-tauri/src/scan_job.rs \
  's~    slot\.mark_reading_done\(outcome\.clone\(\)\);~    let _ = \&outcome; // mutant: the pass never records that it ended~' \
  'let _ = &outcome; // mutant: the pass never records that it ended' \
  mnema-desktop 'a_scan_reads_every_watched_folder_under_one_pass_and_keeps_what_each_said' --test commands

# 🔴 A resumption that records a reading pass of its own. `Entry::EmbedOnly`
# reads no folder, so it has nothing to say about one — and the pass it
# overwrites is the one carrying «частково прочитано». R2-1's whole finding: a
# person presses «Продовжити», the embedding finishes, and the warning that
# their archive is only partly indexed disappears without anything ever having
# read the archive again.
case_ "a resumption must not record a reading pass it never ran" \
  src-tauri/src/scan_job.rs \
  's~        std::thread::spawn\(move \|\| embed_after\(slot, job_db, deps, base\)\);\n        return Ok\(\(\)\);~        slot.mark_reading_done(crate::scan_state::ReadingOutcome::default()); // mutant: a resumption claims a reading pass\n        std::thread::spawn(move || embed_after(slot, job_db, deps, base));\n        return Ok(());~' \
  'slot.mark_reading_done(crate::scan_state::ReadingOutcome::default()); // mutant: a resumption claims a reading pass' \
  mnema-desktop 'scan_job::tests::an_embed_only_run_keeps_the_last_readings_warning' --lib

# 🔴 D-g step 3, moved below the classification. A Stop raised while a keychain
# dialog was on screen then loses to the store's answer: «no key» and «the store
# would not answer» both end `Completed` on their own, so the scan reports that
# it FINISHED to a person who stopped it. Only the arm where a key really was
# returned would fail an implementation like this, which is why the named test
# drives all three answers.
case_ "a Stop during the key read must win whatever the store answered" \
  src-tauri/src/scan_job.rs \
  's~    let answer = \(deps\.key\)\(\);\n\n    if slot\.cancel_flag\(\)\.load\(Ordering::SeqCst\) \{\n        finish_embedding\(\n            slot,\n            &job_db,\n            EmbedOutcome::NotReached,\n            EndReason::Cancelled,\n            None,\n        \);\n        return;\n    \}\n\n    let key = match answer \{~    let answer = (deps.key)();\n\n    let key = match answer \{ // mutant: the store answers first and the Stop is asked afterwards~' \
  'let key = match answer { // mutant: the store answers first and the Stop is asked afterwards' \
  mnema-desktop 'scan_job::tests::a_stop_during_the_key_read_wins_whatever_the_store_answers' --lib

# 🔴 D-g step 3 removed outright rather than moved. The Stop is never asked
# about at all, so a person who pressed it while the dialog was up has their
# text sent to the provider anyway. Judged through `embed_calls` — the D-n spy —
# and not through "no request was made": `mnema_embed::run` marks the space
# `Building` before it asks `cancel`, so an HTTP-shaped assertion would answer
# this question with silence whether or not the guard survived.
case_ "the Stop check between the key read and the pass must exist at all" \
  src-tauri/src/scan_job.rs \
  's~    if slot\.cancel_flag\(\)\.load\(Ordering::SeqCst\) \{\n        finish_embedding\(\n            slot,\n            &job_db,\n            EmbedOutcome::NotReached,\n            EndReason::Cancelled,\n            None,\n        \);\n        return;\n    \}~    // mutant: nothing asks whether a Stop landed during the key read~' \
  '// mutant: nothing asks whether a Stop landed during the key read' \
  mnema-desktop 'scan_job::tests::a_stop_during_the_key_read_wins_whatever_the_store_answers' --lib

# 🔴 D-g step 1, inverted: the phase is announced AFTER the key is read. On
# macOS the credential store can put an authorisation dialog on screen and wait,
# and every surface then goes on drawing `Reading` — a folder name and a
# progress bar — for the whole of that wait, over a pass that had finished
# reading folders. The ending is identical either way; only the sequence of
# announcements can see it.
case_ "the embedding phase must be announced before the store is asked, not after" \
  src-tauri/src/scan_job.rs \
  's~    slot\.update\(Phase::Embedding \{\n        counts: index\.opening\(\),\n    \}\);\n\n    let answer = \(deps\.key\)\(\);~    let answer = (deps.key)(); // mutant: the wait happens under the reading phase\n\n    slot.update(Phase::Embedding \{\n        counts: index.opening(),\n    \});~' \
  'let answer = (deps.key)(); // mutant: the wait happens under the reading phase' \
  mnema-desktop 'scan_job::tests::a_stop_during_the_key_read_wins_whatever_the_store_answers' --lib

# A Stop that lands BETWEEN two folders is a Stop. `walk_root` reads the flag
# only while it is running, so without this check the pass walks on into the
# next folder and reads a whole archive somebody asked it to stop reading. D-h
# below still rewrites the reason at the boundary, so the ENDING looks right —
# which is exactly why the named test asserts `roots_read`, not the reason.
case_ "a Stop landing between two folders must stop the pass before the next one" \
  src-tauri/src/scan_job.rs \
  's~        if slot\.cancel_flag\(\)\.load\(Ordering::SeqCst\) \{\n            outcome\.reason = EndReason::Cancelled;\n            break;\n        \}~        // mutant: a Stop between two folders is not noticed until the next walk reads it~' \
  '// mutant: a Stop between two folders is not noticed until the next walk reads it' \
  mnema-desktop 'scan_job::tests::a_reading_phase_stopped_at_the_first_of_two_folders_leaves_the_marker_set' --lib

# 🔴 D-h, removed. The loop's own check is at the TOP of an iteration, so for
# the LAST folder there is no next iteration to ask in: every folder answers
# `Completed`, the person presses Stop while the pass is absorbing the last
# report, and the pass carries on into the embedding phase and hands their text
# to the provider they had just told it not to.
case_ "a Stop landing after the last folder's report must still end the scan" \
  src-tauri/src/scan_job.rs \
  's~    if outcome\.reason == EndReason::Completed && slot\.cancel_flag\(\)\.load\(Ordering::SeqCst\) \{\n        outcome\.reason = EndReason::Cancelled;\n    \}~    // mutant: a Stop at the pass boundary is lost~' \
  '// mutant: a Stop at the pass boundary is lost' \
  mnema-desktop 'scan_job::tests::a_stop_after_the_last_root_report_still_ends_cancelled_with_resume_full' --lib

# 🔴 D-i, inverted: the folder that ENDED the pass is absorbed after the
# decision to stop, which is to say never. Everything it wrote — how many files
# it indexed before the Stop, how far it got before the worker broke — is thrown
# away, and the report a person reads says the scan stopped and gives them
# zeroes. The folder that ended the scan is the one they most want the counts
# for.
#
# ⚠️ The oracle has to be a folder whose OWN report carries a stopping reason,
# and that is narrower than it sounds. `a_cancelled_root_still_counts_what_it_
# wrote` — the obvious name, and the one the plan reached for — raises its Stop
# in the announcement for the last file of the first folder, so that folder
# still answers `Completed`, `after_root` says nothing, and the pass breaks at
# the TOP of the next iteration with the counters already absorbed. Measured:
# this mutant leaves that test green. The broken-worker fixture is the one where
# the folder itself ends `BrokenWorker`, which is what takes the `break` on the
# same iteration the counters would have been added on.
case_ "the folder that ended the pass must still be counted into it" \
  src-tauri/src/scan_job.rs \
  's~        outcome\.absorb\(done\);\n        if let Some\(reason\) = stop \{\n            outcome\.reason = reason;\n            message = stop_message;\n            break;\n        \}~        if let Some(reason) = stop \{ // mutant: the folder that stopped the pass is never counted\n            outcome.reason = reason;\n            message = stop_message;\n            break;\n        \}\n        outcome.absorb(done);~' \
  'if let Some(reason) = stop { // mutant: the folder that stopped the pass is never counted' \
  mnema-desktop 'a_worker_that_reads_nothing_stops_the_scan_at_the_folder_that_broke' --test commands

# 🔴 The contention count moved BEHIND the throttle. `walk_root` announces
# contention once, in the callback for the file whose last busy retry was
# refused, and that callback is as likely to be dropped as any other — so stored
# after the check the number is whatever the last PUBLISHED report happened to
# carry, which for any folder read inside one report interval is nothing. It
# cannot be recovered afterwards either: `WalkReport` has no such field.
case_ "contention must be recorded from every callback, not only the published ones" \
  src-tauri/src/scan_job.rs \
  's~        contended_seen\.store\(progress\.contended, Ordering::Relaxed\);\n\n        // `0` refused~        // mutant: contention is only recorded when the throttle lets a report through\n\n        // `0` refused~; s~        self\.last_report = Some\(now\);\n\n        Some\(Progress \{~        self.last_report = Some(now);\n        contended_seen.store(progress.contended, Ordering::Relaxed);\n\n        Some(Progress \{~' \
  '// mutant: contention is only recorded when the throttle lets a report through' \
  mnema-desktop 'scan_job::tests::a_contended_report_the_throttle_drops_is_still_counted' --lib

# 🔴 D-j: the marker is never cleared. `scan.incomplete` is written before the
# first walk and cleared by the pass that gets all the way round the folders, so
# a pass that never clears it leaves every settings window reporting an
# unfinished scan for ever — and `continueAction` offers a full scan over an
# index that has just been fully read.
case_ "a pass that visited every folder must clear the incomplete marker" \
  src-tauri/src/scan_job.rs \
  's~    if outcome\.roots_read == outcome\.root_count && outcome\.reason == EndReason::Completed \{\n        let _ = job_db\.meta_set\(SCAN_INCOMPLETE, "0"\);\n    \}~    // mutant: the marker survives a scan that finished~' \
  '// mutant: the marker survives a scan that finished' \
  mnema-desktop 'scan_job::tests::a_reading_phase_that_visited_every_root_clears_the_marker_even_without_a_key' --lib

# F7 (Task 10b): the scan's own clock written AFTER the pass has announced that
# it ended. An observer woken by that announcement reads the index for itself
# and finds no fresher moment than whatever an earlier scan left behind — the
# section then draws «1 годину тому» beside a scan that has this second
# finished. The value ends up correct either way, which is why the named test
# reads the meta key AT the announcement rather than after it.
case_ "the scan's own clock must be set before the pass announces that it ended" \
  src-tauri/src/scan_job.rs \
  's~    if outcome\.roots_read > 0\n        && let Ok\(duration\) = std::time::SystemTime::now\(\)\.duration_since\(std::time::UNIX_EPOCH\)\n    \{\n        let _ = job_db\.meta_set\(LAST_READING_AT, &duration\.as_secs\(\)\.to_string\(\)\);\n    \}\n\n    slot\.mark_reading_done\(outcome\.clone\(\)\);~    slot.mark_reading_done(outcome.clone()); // mutant: the clock is set after the announcement\n\n    if outcome.roots_read > 0\n        && let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)\n    \{\n        let _ = job_db.meta_set(LAST_READING_AT, \&duration.as_secs().to_string());\n    \}~' \
  'slot.mark_reading_done(outcome.clone()); // mutant: the clock is set after the announcement' \
  mnema-desktop 'scan_job::tests::a_reading_phase_announces_last_reading_at_already_set_at_the_ending' --lib

# 🔴 «Completed» folded back into «saw everything». `RootOutcome::complete` is
# about phase 1 — what the walk SAW — and the reason is about whether phase 3
# ran; a folder that met an unreadable subdirectory still ends `Completed`. With
# the AND gone the pass claims it saw the whole archive, and the sentence that
# warns a person their exclusion rules and their deleted files are not being
# accounted for never appears.
case_ "a folder read only in part must make the pass not fully seen" \
  src-tauri/src/scan_job.rs \
  's~        self\.complete &= root\.complete && root\.reason == EndReason::Completed;~        self.complete \&= root.reason == EndReason::Completed; // mutant: how much was SEEN stops counting~' \
  'self.complete &= root.reason == EndReason::Completed; // mutant: how much was SEEN stops counting' \
  mnema-desktop 'a_folder_with_an_unreadable_subdirectory_completes_without_claiming_it_saw_everything' --test commands

# 🔴 `RulesNotApplied` given a resumption. A stored exclusion prefix that
# `WalkRules::new` refuses is fixed by the PERSON, not by running the same scan
# again — so this is a button that spends the whole time and fails the same way,
# offered under D29 beside an archive the person believes is being held back.
case_ "an ending a person has to act on must not offer to run itself again" \
  src-tauri/src/scan_job.rs \
  's~        EndReason::Completed\n        \| EndReason::RulesNotApplied\n        \| EndReason::RootUnavailable\n        \| EndReason::VolumeMissing => None,\n        EndReason::Cancelled \| EndReason::Failed \| EndReason::BrokenWorker => \{~        EndReason::Completed | EndReason::RootUnavailable | EndReason::VolumeMissing => None,\n        EndReason::RulesNotApplied // mutant: a rule nobody fixed is offered a retry\n        | EndReason::Cancelled\n        | EndReason::Failed\n        | EndReason::BrokenWorker => \{~' \
  'EndReason::RulesNotApplied // mutant: a rule nobody fixed is offered a retry' \
  mnema-desktop 'scan_job::tests::every_ending_names_what_the_next_scan_should_be_or_that_there_is_nothing' --lib

# F5 (Task 10d): the published counts drop the base. Every resumption then sends
# the tray back to «Вбудовування 0 %» over an archive that is almost entirely
# embedded, and the numbers a person watches stop being about their index. The
# ENDING is offset separately, so an assertion on the ending alone passes under
# this mutant — only the head of the published sequence can see it.
case_ "a published embedding report must be on the index's scale, not the pass's" \
  src-tauri/src/scan_job.rs \
  's~            done: self\.embedded\.saturating_add\(counts\.done\),\n            total: self\.embedded\.saturating_add\(counts\.total\),~            done: counts.done, // mutant: the bar restarts at this pass\n            total: self.embedded.saturating_add(counts.total),~' \
  'done: counts.done, // mutant: the bar restarts at this pass' \
  mnema-desktop 'scan_job::tests::the_embedding_counts_measure_the_whole_index_and_not_only_this_pass' --lib

# ── The removal that races the pass (Task 4, D-k) ────────────────────────────

# 🔴 The compare gone: the delete asks for an id and takes whatever now sits at
# it. `watched_root.id` is a rowid alias SQLite hands out again the moment a row
# is gone, so a second window that removes this folder and adds another in
# between has the newcomer deleted instead — every document under it out of the
# index, and the sentence a person read named a different folder.
case_ "a removal must delete the folder it was asked about, not whatever holds its id" \
  src-tauri/src/bridge.rs \
  's~        Ok\(match db\.delete_watched_root_if_path\(root_id, path\)\? \{~        Ok(match db.delete_watched_root(root_id).map(Some)? \{ // mutant: the path is never compared~' \
  'Ok(match db.delete_watched_root(root_id).map(Some)? { // mutant: the path is never compared' \
  mnema-desktop 'bridge::tests::a_root_swapped_before_the_delete_is_refused_and_the_newcomer_survives' --lib

# 🔴 **Not written, and this is the record of why.** The plan asks for a second
# removal case — the path re-derived from the id BEFORE the claim rather than
# taken from the caller — killed by the same swap test. Measured: it is not.
# `remove_hook` fires AFTER the claim, which is the window that fixture models,
# so a pre-claim re-derivation reads the folder's real path, the compare below
# still refuses, and the test stays green. The mutant is not equivalent — a
# caller sending a path that does NOT name the row would have its removal go
# through instead of being refused — but nothing in this repository calls the
# command that way, so there is no oracle. Carried into `task-11b-report.md`
# as a coverage gap rather than faked with a marker.

# ── The tray (Task 10c, F4) ──────────────────────────────────────────────────

# 🔴 The obvious wrong predicate: every ending offers to carry on. A completed
# scan, a scan whose embedding was skipped for want of a key, a folder that
# refused its rules — all of them would draw a live «Продовжити сканування» that
# starts nothing, because `resume_scan` reads the report's own `resume` and
# finds `None`. A control that does nothing is what `resume_enabled` exists to
# prevent.
case_ "the tray's resume is live only when the report carries one" \
  src-tauri/src/tray.rs \
  's~pub fn resume_enabled\(state: &crate::scan_state::ScanState\) -> bool \{\n    resume_entry\(state\)\.is_some\(\)\n\}~pub fn resume_enabled(state: \&crate::scan_state::ScanState) -> bool \{\n    // mutant: every ending offers to carry on\n    matches!(state.snapshot, crate::scan_state::ScanSnapshot::Ended \{ .. \})\n\}~' \
  '// mutant: every ending offers to carry on' \
  mnema-desktop 'tray::tests::resume_is_enabled_only_when_the_ended_report_carries_its_own_resume' --lib

# 🔴 The resume arm run inline on the main thread. `scan_job::start` reaches
# `start_inner`, which claims the slot, opens a job index and runs `read_roots`
# — a `with_index` call that blocks for as long as another job holds the
# connection (a folder removal alone, on the order of twenty seconds). A press
# would freeze every window redraw and every other menu click for that time.
# Nothing about the SCAN changes, which is why the guard reads the source.
case_ "the menu's resume must start its scan off the main thread" \
  src-tauri/src/lib.rs \
  's~            tray::RESUME_ID => \{\n                let app = app\.clone\(\);\n                std::thread::spawn\(move \|\| \{\n                    scan_job::resume_scan\(&app\.state::<state::AppState>\(\), scan_job::start\);\n                \}\);\n            \}~            tray::RESUME_ID => \{\n                // mutant: the start runs inline on the main thread\n                scan_job::resume_scan(\&app.state::<state::AppState>(), scan_job::start);\n            \}~' \
  '// mutant: the start runs inline on the main thread' \
  mnema-desktop 'tests::the_menu_handler_starts_a_scan_only_off_the_main_thread' --lib

# ── The window (Tasks 6, 7, 8, 9, 10) ───────────────────────────────────────

# `apply` is the whole of the window's ordering: the events and the polled
# snapshot race, and `revision` is the only thing that says which of two answers
# is newer. `>=` lets an event that carries the SAME revision replace the state
# already held — a new object every time, so `safe_not_equal` wakes every
# subscriber and the window repaints on a state nothing changed.
case_ "an equal revision must leave the state already held exactly where it is" \
  ui/src/settings/jobs.ts \
  's~  return incoming\.revision > current\.revision \? incoming : current;~  return incoming.revision >= current.revision ? incoming : current; // mutant: an equal revision replaces~' \
  'return incoming.revision >= current.revision ? incoming : current; // mutant: an equal revision replaces' \
  src/settings/jobs.test.ts 'an equal revision returns the state that was already held, by identity' runner=vitest

# The other half, and the one that loses data rather than costing a repaint: a
# `job_status` reply that settles after the event it raced puts the OLDER
# snapshot on screen and leaves it there until the next event happens to arrive.
case_ "an older snapshot must never replace a newer one" \
  ui/src/settings/jobs.ts \
  's~  return incoming\.revision > current\.revision \? incoming : current;~  return incoming; // mutant: whatever arrived last wins~' \
  'return incoming; // mutant: whatever arrived last wins' \
  src/settings/jobs.test.ts 'a higher revision wins and a lower one is ignored' runner=vitest

# 🔴 R2-2's finding put back: the section's offer gated on `idle`. An `Ended`
# snapshot over an index still carrying `scan.incomplete` then offers NOTHING —
# the strip has no resumption to draw (the report named none) and the section
# refuses to draw one because the phase is not idle. A person is left with an
# index that says a scan is half-done and no way to carry it on.
case_ "the index's own markers must be offered after an ending too, not only when idle" \
  ui/src/settings/jobs.ts \
  "s~  if \(read === null\) return null;~  if (read === null || snapshot.kind !== 'idle') return null; // mutant: only an idle window may offer the markers~" \
  "if (read === null || snapshot.kind !== 'idle') return null; // mutant: only an idle window may offer the markers" \
  src/settings/Scanning.test.ts 'an ended report naming no resumption still offers to continue when the index marks a walk incomplete' runner=vitest

# 🔴 The order of the two marker arms swapped. `scanIncomplete` outranks the
# queue because `embedOnly` over a half-read archive embeds what is there and
# leaves the unread half invisible — while the window says the work is done. Any
# state carrying only one of the two markers passes under this mutant.
case_ "an unfinished reading must outrank a waiting queue, not the other way round" \
  ui/src/settings/jobs.ts \
  "s~  if \(read\.scanIncomplete\) return \{ entry: 'full', where: 'section', label: 'resume' \};\n  if \(read\.pendingChunks > 0\) return \{ entry: 'embedOnly', where: 'section', label: 'resume' \};~  if (read.pendingChunks > 0) return { entry: 'embedOnly', where: 'section', label: 'resume' }; // mutant: the queue outranks the half-read archive\n  if (read.scanIncomplete) return { entry: 'full', where: 'section', label: 'resume' };~" \
  "if (read.pendingChunks > 0) return { entry: 'embedOnly', where: 'section', label: 'resume' }; // mutant: the queue outranks the half-read archive" \
  src/settings/jobs.test.ts 'an unfinished reading outranks a queue when the index carries both' runner=vitest

# A report that named no resumption is given one anyway. `scan_job::resume_for`
# answers `None` for exactly the endings running again cannot change — a
# completed scan, a folder whose rules would not apply — and this draws
# «Повторити» over every one of them.
case_ "a report naming no resumption must not have one invented for it" \
  ui/src/settings/jobs.ts \
  "s~  if \(snapshot\.kind === 'ended' && snapshot\.report\.resume !== null\) \{\n    return \{\n      entry: snapshot\.report\.resume,~  if (snapshot.kind === 'ended') { // mutant: every ending offers a button\n    return {\n      entry: snapshot.report.resume ?? 'full',~" \
  "if (snapshot.kind === 'ended') { // mutant: every ending offers a button" \
  src/settings/JobStrip.test.ts 'the label follows the reason, and the entry follows what the report named to resume from' runner=vitest

# 🔴 D-e, folded away: `completed` stops splitting on whether phase 1 saw the
# whole tree. A scan that met an unreadable subfolder then reads as a clean
# finish, and the sentence that tells a person their exclusion rules and their
# deleted files are unaccounted for in that folder is never drawn.
case_ "a completed reading that did not see the whole tree must not read as a clean finish" \
  ui/src/settings/JobStrip.svelte \
  "s~    return r\.reason === 'completed' \? \(r\.complete \? 'completed' : 'partlyRead'\) : r\.reason;~    return r.reason; // mutant: how much was seen stops changing the sentence~" \
  "return r.reason; // mutant: how much was seen stops changing the sentence" \
  src/settings/JobStrip.test.ts 'a completed reading that did not see the whole tree reads as partly read, not as completed' runner=vitest

# 🔴 Task 9's withdrawal keyed on the snapshot reaching `ended` instead of on
# `readSeq`. An `embedOnly` run ends like any other and reads no folder, so
# nothing it did made a frozen number wrong — the mutant discards a press a
# person is still looking at and prints "indexing has finished" over a run that
# read nothing.
case_ "the withdrawal must follow a reading pass ending, not any job ending" \
  ui/src/settings/Folders.svelte \
  "s~      if \(scan\.readSeq > seenReadSeq\) \{~      if (scan.snapshot.kind === 'ended') { // mutant: any ending withdraws~" \
  "if (scan.snapshot.kind === 'ended') { // mutant: any ending withdraws" \
  src/settings/Folders.test.ts 'an embedding-only run withdraws nothing, and re-reads the counts once' runner=vitest

# 🔴 Task 9's whole finding: the press removes rather than asks. Before Task 9
# «Видалити» took the documents out of the index on the press itself, with no
# question and nothing to cancel — the states "the press has been made" and "the
# removal has been asked for" were one state, and the second is the one that
# deletes.
case_ "the Remove press must ask a question, not remove" \
  ui/src/settings/Folders.svelte \
  "s~    removeQuestion = \{ rootId: root\.rootId, path: root\.absolutePath, files: root\.files\.length \};~    void removeWatchedFolder(root.rootId, root.absolutePath); // mutant: the press removes\n    removeQuestion = { rootId: root.rootId, path: root.absolutePath, files: root.files.length };~" \
  "void removeWatchedFolder(root.rootId, root.absolutePath); // mutant: the press removes" \
  src/settings/Folders.test.ts '«Remove» asks a question naming the folder and counting its files, and calls nothing' runner=vitest

# The path dropped from the call. `bridge.rs` compares it against the row the id
# names NOW, and that compare is what refuses a removal whose folder has been
# swapped underneath it — sending an empty path makes every removal a refusal on
# a correct backend and, on any backend that treated it as "no opinion", makes
# the whole D-k check unreachable from this window.
case_ "the removal must send the path the question was asked about" \
  ui/src/settings/Folders.svelte \
  "s~      await removeWatchedFolder\(question\.rootId, question\.path\);~      await removeWatchedFolder(question.rootId, ''); // mutant: the identity check is given nothing to check~" \
  "await removeWatchedFolder(question.rootId, ''); // mutant: the identity check is given nothing to check" \
  src/settings/Folders.test.ts '«Confirm» sends the frozen id and path while the list re-read behind it is still on the wire' runner=vitest

# 🔴 F1/F9 (Task 10b). A folder removal ends the slot with `finish(Idle,
# Some(files))` — straight to a bare `idle` snapshot, never `ended`, and it is
# not a reading pass so `readSeq` never moves either. Both of the other two
# triggers are blind to it, and the section went on saying «В індексі 668
# файлів» over an empty list.
# ⚠️ Re-quoted in final fix round 2 against the condition area C widened, and
# the mutant now KEEPS `leftRunning` while dropping `filesChanged` — otherwise
# it would be two deletions in one case, and the named test would no longer say
# which of them killed it.
#
# What separates them is the FIXTURE, not production: a real removal claims
# `Running { Removing }` first (`bridge.rs`), so in the application a removal's
# `idle` does follow a `running` and `leftRunning` would fire for it. The named
# test delivers an `ended` and then the removal's `idle` with no running tick
# between, so the kept arm is never true there and only `filesChanged` can
# answer. An earlier draft of this comment said the opposite about production
# and would have sent the next reader looking for a transition that is there.
case_ "the window must re-read when the index's own file count moves" \
  ui/src/settings/Settings.svelte \
  "s~      if \(readSeqChanged \|\| filesChanged \|\| leftRunning \|\| scan\.snapshot\.kind === 'ended'\) \{\n        void refresh\(\);\n      \}~      if (readSeqChanged || leftRunning || scan.snapshot.kind === 'ended') \{\n        void refresh(); // mutant: a removal moves nothing this window watches\n      \}~" \
  "void refresh(); // mutant: a removal moves nothing this window watches" \
  src/settings/Settings.test.ts 'a files count that changed on an idle snapshot re-reads, revealing the queue the vanished report offered' runner=vitest
