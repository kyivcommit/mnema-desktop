# Mutation cases for Task 1: crates/mnema-extract/src/bin/worker.rs's
# --probe-pdfium diagnostic branch. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-1.sh
#
# Review round 1 found that `the_worker_reports_whether_pdfium_loaded`'s two
# assertions on a successful probe (`loaded` and `pages`) had never been
# observed red for the right reason: the only red run on record failed two
# lines earlier, on `serde_json::from_str`, because the CLI flag did not
# exist yet — the process printed nothing, and neither assertion ever
# executed. An assertion that has never failed for the reason it exists is
# not proven coverage. The same review added the `stage` field to the
# failure branch (library_dir/verify_build/bind, distinct from `error`'s free
# text) and asked the same question of it. These four cases are the missing
# red runs, one per field the two branches report.

case_ "worker: a successful probe still reports a page count" \
  crates/mnema-extract/src/bin/worker.rs \
  's{            Ok\(probes\) => format!\(\n                "\{\{\\"loaded\\":true,\\"pages\\":\{\},\\"stage\\":\\"ok\\"\}\}",\n                probes\.len\(\)\n            \),}{            Ok(probes) => { let _ = probes; format!("{{\\"loaded\\":true,\\"stage\\":\\"ok\\"}}") },}' \
  'let _ = probes; format!("{{\"loaded\":true,\"stage\":\"ok\"}}")' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

case_ "worker: a successful probe must not say loaded:false" \
  crates/mnema-extract/src/bin/worker.rs \
  's{"\{\{\\"loaded\\":true,\\"pages\\":\{\},\\"stage\\":\\"ok\\"\}\}"}{"{{\\"loaded\\":false,\\"pages\\":{},\\"stage\\":\\"ok\\"}}"}' \
  '"{{\"loaded\":false,\"pages\":{},\"stage\":\"ok\"}}"' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

case_ "worker: a successful probe's stage is ok, not some other word" \
  crates/mnema-extract/src/bin/worker.rs \
  's{"\{\{\\"loaded\\":true,\\"pages\\":\{\},\\"stage\\":\\"ok\\"\}\}"}{"{{\\"loaded\\":true,\\"pages\\":{},\\"stage\\":\\"loaded\\"}}"}' \
  '"{{\"loaded\":true,\"pages\":{},\"stage\":\"loaded\"}}"' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

# Not a text-matching stand-in: the failure branch must report the stage
# `Error::stage()` actually names, not a constant. This is also the mutation
# that a hypothetical `stage: "bind"` hard-coded for every failure would
# survive if only the happy-path test existed — it needs the real
# verify_build case (an_empty_library_directory_fails_at_the_verify_build_stage
# in the same file) to catch it.
case_ "worker: the failure branch reports the error's own stage, not a constant" \
  crates/mnema-extract/src/bin/worker.rs \
  's{serde_json::to_string\(e\.stage\(\)\)\.expect\("a string serialises"\),}{serde_json::to_string("bind").expect("a string serialises"),}' \
  'serde_json::to_string("bind").expect("a string serialises"),' \
  mnema-extract 'an_empty_library_directory_fails_at_the_verify_build_stage' --test pdfium_binding
