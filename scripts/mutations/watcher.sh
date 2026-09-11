# Mutation cases for the folder watcher (spec 2026-09-10): the classifier,
# the pending cap, the look-before-claim Stop rule, the cover and the
# diff-based reconcile. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/watcher.sh

case_ "watch: MAX_WAIT cap removed, a steady stream starves the scan" \
  src-tauri/src/watch.rs \
  's~let fire_at = \(last \+ QUIET\)\.min\(first \+ MAX_WAIT\);~let fire_at = last + QUIET;~' \
  'let fire_at = last + QUIET;' \
  mnema-desktop 'watch::tests::a_wake_every_second_still_fires_by_max_wait' --lib

case_ "watch: Access events wake like any other" \
  src-tauri/src/watch.rs \
  's~if matches!\(event\.kind, notify::EventKind::Access\(_\)\) \{\n        return false;~if matches!(event.kind, notify::EventKind::Access(_)) {\n        return true;~' \
  'if matches!(event.kind, notify::EventKind::Access(_)) {
        return true;' \
  mnema-desktop 'watch::tests::a_read_is_not_a_change' --lib

case_ "watch: the private-directory filter drops an event with ANY private path" \
  src-tauri/src/watch.rs \
  's~!event\n        \.paths\n        \.iter\(\)\n        \.all\(\|p\| plain\(p\)\.starts_with\(private_dir\)\)~!event\n        .paths\n        .iter()\n        .any(|p| plain(p).starts_with(private_dir))~' \
  '.any(|p| plain(p).starts_with(private_dir))' \
  mnema-desktop 'watch::tests::the_private_directory_is_dropped_only_when_every_path_is_inside_it' --lib

case_ "watch: the trigger claims first and looks at the slot never" \
  src-tauri/src/watch.rs \
  's~ScanSnapshot::Ended \{ report \} if report\.reason == EndReason::Cancelled => \{~ScanSnapshot::Ended { report } if report.reason == EndReason::Cancelled \&\& false => {~' \
  'if report.reason == EndReason::Cancelled && false => {' \
  mnema-desktop 'watch::tests::a_wake_during_the_wait_that_postdates_stop_restarts' --lib

case_ "watch: the Stop rule ignores when Stop was pressed" \
  src-tauri/src/watch.rs \
  's~\(Some\(n\), Some\(s\)\) => n > s,~(Some(n), Some(s)) => true,~' \
  '(Some(n), Some(s)) => true,' \
  mnema-desktop 'watch::tests::a_change_before_stop_while_running_does_not_restart_once_cancelled' --lib

case_ "watch: newest forgotten between busy retries" \
  src-tauri/src/watch.rs \
  's~newest = newest\.max\(p\.last\);~newest = p.last;~' \
  'newest = p.last;' \
  mnema-desktop 'watch::tests::newest_survives_a_second_busy_that_restores_the_cancelled_report' --lib

case_ "watch: cover subscribes nested roots on their own" \
  src-tauri/src/watch.rs \
  's~\.filter\(\|r\| !roots\.iter\(\)\.any\(\|o\| o != \*r && r\.starts_with\(o\)\)\)~.filter(|_| true)~' \
  '.filter(|_| true)' \
  mnema-desktop 'watch::tests::cover_keeps_only_roots_without_a_watched_ancestor' --lib

case_ "watch: reconcile rebuilds instead of diffing" \
  src-tauri/src/watch.rs \
  's~let gone: Vec<PathBuf> = watched\.difference\(&want\)\.cloned\(\)\.collect\(\);~let gone: Vec<PathBuf> = watched.iter().cloned().collect();~' \
  'let gone: Vec<PathBuf> = watched.iter().cloned().collect();' \
  mnema-desktop 'watch::tests::reconcile_touches_only_what_changed' --lib

case_ "watch: a removed root stays in the watched set" \
  src-tauri/src/watch.rs \
  's~watched\.remove\(&p\);~if false { watched.remove(&p); }~' \
  'if false { watched.remove(&p); }' \
  mnema-desktop 'watch::tests::a_removed_root_leaves_the_watched_set' --lib
