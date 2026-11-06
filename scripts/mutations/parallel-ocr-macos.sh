# Mutation cases for parallel OCR (D196) whose tests are macOS only. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/parallel-ocr-macos.sh
#
# not-in-matrix: every test named here is #[cfg(target_os = "macos")] (the worker starts helpers only where Vision is); on the ubuntu leg the baseline selects 0 tests and the whole file exits 1 — the same reason as pr8-exclusions-macos.sh

# P7. The sysctl is actually asked (macOS only: elsewhere the stub is None).
case_ "this Mac's P-cores are read from sysctl" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~    \(rc == 0 && len == std::mem::size_of::<i32>\(\) && value > 0\)\.then_some\(value as usize\)~    (rc == 0 \&\& len == 0).then_some(value as usize)~' \
  '(rc == 0 && len == 0)' \
  mnema-extract 'this_mac_names_its_performance_cores' --test ocr_parallel

# R1. An orphaned helper exits on its own. Without the watchdog it waits for
# EOF — which, inside a 46 s cold start, it does not read — and outlives the
# worker the pool just killed (found by the kill test, 2026-11-06).
case_ "an orphaned helper exits without waiting for EOF" \
  crates/mnema-extract/src/bin/worker.rs \
  's~                if std::os::unix::process::parent_id\(\) != parent \{~                if std::os::unix::process::parent_id() == u32::MAX {~' \
  'parent_id() == u32::MAX {' \
  mnema-extract 'helpers_do_not_outlive_a_killed_worker' --test parallel_ocr

# R2. The page a failed helper held is read in the worker, not dropped: as
# "nothing recognised" it would be journalled as a blank page of the scan.
case_ "a failed helper's page is read in the worker" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~                \(page_no, crate::recognise\(&image\.as_image\(\)\)\)~                (page_no, Ok(None))~' \
  '                (page_no, Ok(None))' \
  mnema-extract 'a_helper_that_crashes_loses_no_page' --test parallel_ocr

# R3. The worker does start helpers for a scan (macOS): with the condition
# never true the frames still match — only the slower path ran.
case_ "the worker starts helpers when it may" \
  crates/mnema-extract/src/bin/worker.rs \
  's~if cfg!\(target_os = "macos"\) && helpers > 1 =>~if cfg!(target_os = "macos") \&\& helpers > 100 =>~' \
  'helpers > 100 =>' \
  mnema-extract 'helpers_do_not_outlive_a_killed_worker' --test parallel_ocr
