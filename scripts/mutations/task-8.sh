# Mutation cases for the Tauri shell. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-8.sh
#
# Each case names the test it must make red. Two tests here have no case and
# say why in their own comments: `an_unknown_command_is_rejected` is a control
# over Tauri's behaviour, not this crate's, and nothing in the repository can
# make it red.

case_ "paths: the index sits directly in the local data directory" \
  src-tauri/src/paths.rs \
  's{app_local_data_dir\.join\("index\.sqlite"\)}{app_local_data_dir.join("cache").join("index.sqlite")}' \
  'join("cache")' \
  mnema-desktop 'paths::tests::the_index_sits_directly_in_the_local_data_directory' --lib

case_ "startup: the index goes to the LOCAL data directory, not the cache" \
  src-tauri/src/lib.rs \
  's{app\.path\(\)\.app_local_data_dir\(\)\?}{app.path().app_cache_dir()?}' \
  'app_cache_dir()?' \
  mnema-desktop 'the_application_puts_the_index_in_the_local_data_directory' --test commands

case_ "startup: the state is actually managed" \
  src-tauri/src/lib.rs \
  's{    app\.manage\(state::AppState::new\(dir, worker\)\);\n}{    let _ = (dir, worker);\n}' \
  'let _ = (dir, worker);' \
  mnema-desktop 'the_application_puts_the_index_in_the_local_data_directory' --test commands

case_ "job: nothing measured yet means no estimate" \
  src-tauri/src/job.rs \
  's{if done == 0 \{\n        return None;}{if done == 0 \{\n        return Some(0);}' \
  'return Some(0);' \
  mnema-desktop 'job::tests::nothing_measured_yet_means_no_estimate' --lib

case_ "job: the estimate comes from the measured rate" \
  src-tauri/src/job.rs \
  's{\* remaining as f64}{* total as f64}' \
  '* total as f64' \
  mnema-desktop 'job::tests::the_estimate_comes_from_the_measured_rate' --lib

case_ "job: a finished job has nothing left" \
  src-tauri/src/job.rs \
  's{total\.saturating_sub\(done\)}{done.saturating_sub(total)}' \
  'done.saturating_sub(total)' \
  mnema-desktop 'job::tests::a_finished_job_has_nothing_left' --lib

case_ "job: every unit is reported when the interval is zero" \
  src-tauri/src/job.rs \
  's{done == total \|\| last\.is_none_or\(\|last\| now\.duration_since\(last\) >= interval\)}{done == total}' \
  ') -> bool {
    done == total
}' \
  mnema-desktop 'job::tests::every_unit_is_reported_when_the_interval_is_zero' --lib

case_ "job: progress is throttled" \
  src-tauri/src/job.rs \
  's{done == total \|\| last\.is_none_or\(\|last\| now\.duration_since\(last\) >= interval\)}{true}' \
  ') -> bool {
    true
}' \
  mnema-desktop 'job::tests::progress_is_throttled_to_the_report_interval' --lib

case_ "job: cancellation is checked before the unit, not after" \
  src-tauri/src/job.rs \
  's{        if cancel\.load\(Ordering::SeqCst\) \{\n            return Outcome::Cancelled \{ done \};\n        \}\n        if !unit\.is_zero\(\) \{\n            std::thread::sleep\(unit\);\n        \}\n        done \+= 1;}{        if !unit.is_zero() \{\n            std::thread::sleep(unit);\n        \}\n        done += 1;\n        if cancel.load(Ordering::SeqCst) \{\n            return Outcome::Cancelled \{ done \};\n        \}}' \
  'done += 1;
        if cancel.load' \
  mnema-desktop 'job::tests::a_job_cancelled_before_it_starts_does_no_work' --lib

case_ "job: a cancelled job reports Cancelled" \
  src-tauri/src/job.rs \
  's{return Outcome::Cancelled \{ done \};}{return Outcome::Completed;}' \
  'return Outcome::Completed;' \
  mnema-desktop 'job::tests::cancelling_part_way_through_stops_the_loop' --lib

case_ "job: the cancelled count is the count, not a placeholder" \
  src-tauri/src/job.rs \
  's{return Outcome::Cancelled \{ done \};}{return Outcome::Cancelled \{ done: 0 \};}' \
  'Outcome::Cancelled { done: 0 }' \
  mnema-desktop 'job::tests::cancelling_part_way_through_stops_the_loop' --lib

case_ "job: a completed ending is at the total" \
  src-tauri/src/job.rs \
  's{                reason: EndReason::Completed,\n                done: total,}{                reason: EndReason::Completed,\n                done: 0,}' \
  'reason: EndReason::Completed,
                done: 0,' \
  mnema-desktop 'job::tests::a_completed_job_ends_at_the_total' --lib

case_ "job: a cancelled ending keeps where it stopped" \
  src-tauri/src/job.rs \
  's{                reason: EndReason::Cancelled,\n                done,}{                reason: EndReason::Cancelled,\n                done: total,}' \
  'reason: EndReason::Cancelled,
                done: total,' \
  mnema-desktop 'job::tests::a_cancelled_job_ends_where_it_stopped' --lib

case_ "commands: open_index reports its wire shape" \
  src-tauri/src/bridge.rs \
  's{#\[serde\(rename_all = "camelCase"\)\]\npub struct IndexInfo}{pub struct IndexInfo}' \
  'Serialize)]
pub struct IndexInfo' \
  mnema-desktop 'opening_the_index_creates_it_in_the_data_directory_and_reports_its_version' --test commands

case_ "commands: open_index is registered" \
  src-tauri/src/lib.rs \
  's{        bridge::open_index,\n}{}' \
  'generate_handler![
        bridge::add_watched_folder,' \
  mnema-desktop 'opening_the_index_creates_it_in_the_data_directory_and_reports_its_version' --test commands

case_ "commands: searching before open says which reason" \
  src-tauri/src/state.rs \
  's{\.ok_or\(Error::IndexNotOpen\)}{.ok_or(Error::StatePoisoned)}' \
  'ok_or(Error::StatePoisoned)' \
  mnema-desktop 'searching_before_the_index_is_open_says_so' --test commands

case_ "commands: a search finds what the other connection wrote" \
  src-tauri/src/bridge.rs \
  's{const SEARCH_LIMIT: i64 = 20;}{const SEARCH_LIMIT: i64 = 0;}' \
  'SEARCH_LIMIT: i64 = 0;' \
  mnema-desktop 'a_search_through_the_ipc_finds_what_another_connection_wrote' --test commands

case_ "commands: open_index leaves the main thread" \
  src-tauri/src/bridge.rs \
  's{#\[tauri::command\(async\)\]\npub fn open_index}{#[tauri::command]\npub fn open_index}' \
  '#[tauri::command]
pub fn open_index' \
  mnema-desktop 'the_commands_that_touch_the_database_leave_the_main_thread' --test commands

case_ "commands: search leaves the main thread" \
  src-tauri/src/bridge.rs \
  's{#\[tauri::command\(async\)\]\npub fn search}{#[tauri::command]\npub fn search}' \
  '#[tauri::command]
pub fn search' \
  mnema-desktop 'the_commands_that_touch_the_database_leave_the_main_thread' --test commands

case_ "commands: cancelling stops the stream" \
  src-tauri/src/state.rs \
  's{        self\.cancel\.store\(true, Ordering::SeqCst\);}{        let _ = \&self.cancel;}' \
  'let _ = &self.cancel;' \
  mnema-desktop 'a_started_job_reports_progress_and_a_cancelled_one_stops' --test commands

case_ "commands: the job says how it ended" \
  src-tauri/src/bridge.rs \
  's{        let _ = on_progress\.send\(JobEvent::Ended\(ending\)\);}{        let _ = \&ending;}' \
  'let _ = &ending;' \
  mnema-desktop 'a_started_job_reports_progress_and_a_cancelled_one_stops' --test commands

# Removes the unwind protection while still compiling: the loop is called
# directly and its result wrapped in the `Ok` the match below expects, so a panic
# escapes the thread exactly as it did before this was fixed.
case_ "commands: the ending survives a panic in the job" \
  src-tauri/src/bridge.rs \
  's{let caught = catch_unwind\(AssertUnwindSafe\(\|\| \{}{let caught = Ok::<_, Box<dyn std::any::Any + Send>>(\{};s{\n        \}\)\);\n}{\n        \});\n}' \
  'let caught = Ok::<_, Box<dyn std::any::Any + Send>>({' \
  mnema-desktop 'a_job_that_panics_still_tells_the_window_it_ended' --test commands

case_ "commands: a panic is not reported as a cancellation" \
  src-tauri/src/job.rs \
  's{            reason: EndReason::Failed,}{            reason: EndReason::Cancelled,}' \
  'reason: EndReason::Cancelled,
            done,
            total,' \
  mnema-desktop 'a_job_that_panics_still_tells_the_window_it_ended' --test commands

case_ "job: a failure is distinguishable from a cancellation" \
  src-tauri/src/job.rs \
  's{            reason: EndReason::Failed,}{            reason: EndReason::Cancelled,}' \
  'reason: EndReason::Cancelled,
            done,
            total,' \
  mnema-desktop 'job::tests::a_failure_is_not_reported_as_a_cancellation' --lib

case_ "commands: the failure count is what the window was shown, not what was attempted" \
  src-tauri/src/bridge.rs \
  's{                    if on_progress\.send\(JobEvent::Progress\(progress\)\)\.is_ok\(\) \{\n                        reported\.store\(done, Ordering::Relaxed\);\n                    \}}{                    reported.store(done, Ordering::Relaxed);\n                    let _ = on_progress.send(JobEvent::Progress(progress));}' \
  'reported.store(done, Ordering::Relaxed);
                    let _ = on_progress.send' \
  mnema-desktop 'a_job_that_panics_still_tells_the_window_it_ended' --test commands

case_ "build: the release profile must unwind" \
  Cargo.toml \
  's{panic = "unwind"}{panic = "abort"}' \
  'panic = "abort"' \
  mnema-desktop 'the_release_profile_unwinds_because_the_shell_catches_unwinds' --test unwind_profile

# The directive flips while the words stay in the file. A `contains` over the raw
# section is satisfied by the comment and never sees the change — which is how
# this passed before the check moved to whole lines.
case_ "build: a comment saying unwind does not count as declaring it" \
  Cargo.toml \
  's{\npanic = "unwind"\n}{\n# this used to be panic = "unwind"\npanic = "abort"\n}' \
  '# this used to be panic = "unwind"' \
  mnema-desktop 'the_release_profile_unwinds_because_the_shell_catches_unwinds' --test unwind_profile

case_ "build: the profile section must exist at all" \
  Cargo.toml \
  's{\[profile\.release\]}{[profile.release-was-here]}' \
  '[profile.release-was-here]' \
  mnema-desktop 'the_release_profile_unwinds_because_the_shell_catches_unwinds' --test unwind_profile

case_ "commands: job_status reports the running job" \
  src-tauri/src/bridge.rs \
  's{        running: state\.job_is_running\(\),}{        running: false,}' \
  'running: false,' \
  mnema-desktop 'the_window_can_ask_whether_a_job_is_running' --test commands

case_ "commands: job_status is registered" \
  src-tauri/src/lib.rs \
  's{        bridge::job_status,\n}{}' \
  'bridge::cancel_job,
        walk_job::start_walk_job,' \
  mnema-desktop 'the_window_can_ask_whether_a_job_is_running' --test commands

case_ "commands: a new job does not inherit the last cancellation" \
  src-tauri/src/state.rs \
  's{        self\.cancel\.store\(false, Ordering::SeqCst\);}{}' \
  'Ok(JobSlot {' \
  mnema-desktop 'a_job_started_after_a_cancelled_one_is_not_born_cancelled' --test commands

case_ "commands: only one job at a time" \
  src-tauri/src/state.rs \
  's{        self\.running\n            \.compare_exchange\(false, true, Ordering::AcqRel, Ordering::Acquire\)\n            \.map_err\(\|_\| Error::JobAlreadyRunning\)\?;}{        self.running.store(true, Ordering::Release);}' \
  'self.running.store(true, Ordering::Release);' \
  mnema-desktop 'only_one_job_runs_at_a_time' --test commands

case_ "commands: start_probe_job is registered" \
  src-tauri/src/lib.rs \
  's{        bridge::start_probe_job,\n}{}' \
  'bridge::skips,
        bridge::cancel_job,' \
  mnema-desktop 'the_probe_job_is_reachable_through_the_ipc' --test commands

case_ "index: a second writer waits rather than failing" \
  crates/mnema-index/src/open.rs \
  's{Duration::from_secs\(5\);}{Duration::ZERO;}' \
  'Duration::ZERO;' \
  mnema-index 'a_writer_that_arrives_second_waits_for_the_first_rather_than_failing' --test contention
