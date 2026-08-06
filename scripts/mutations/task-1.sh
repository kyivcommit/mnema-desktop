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

# The `pages` and `stage` fields are matched through the ONE place they are
# spelled — the format string — rather than through the surrounding `format!`
# call, which task 16 grew a third argument and a comment block. Three of these
# cases went BROKEN on that edit, having anchored on the whole expression; what
# survives an edit is the smallest fragment that still names the thing under
# test.
case_ "worker: a successful probe still reports a page count" \
  crates/mnema-extract/src/bin/worker.rs \
  's{\\"pages\\":\{\},}{};s{\n                probes\.len\(\),}{}' \
  '"{{\"loaded\":true,\"stage\":\"ok\",\"library_dir\":{}}}"' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

case_ "worker: a successful probe must not say loaded:false" \
  crates/mnema-extract/src/bin/worker.rs \
  's{\\"loaded\\":true,\\"pages\\"}{\\"loaded\\":false,\\"pages\\"}' \
  '\"loaded\":false,\"pages\"' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

case_ "worker: a successful probe's stage is ok, not some other word" \
  crates/mnema-extract/src/bin/worker.rs \
  's{\\"stage\\":\\"ok\\"}{\\"stage\\":\\"loaded\\"}' \
  '\"stage\":\"loaded\"' \
  mnema-extract 'the_worker_reports_whether_pdfium_loaded' --test pdfium_binding

# Task 16. The field that says WHICH library answered, and the two ways it stops
# meaning that: the consumer's name for it goes away, and the value stops being
# the place. Not a third case for "recorded at the load rather than re-derived",
# which is what `loaded_library_dir` is built as — no test here can see that
# difference, because `library_dir()` is deterministic and would agree with the
# recording every time it is asked. That is an argument for the shape, not a
# measured property, and it is left as one.
case_ "worker: a successful probe names the directory it loaded from" \
  crates/mnema-extract/src/bin/worker.rs \
  's{\\"library_dir\\":}{\\"lib_dir\\":}' \
  '\"lib_dir\":' \
  mnema-extract 'a_successful_probe_names_the_directory_it_loaded_from' --test pdfium_binding

case_ "extract: the reported directory is a place, not a plausible constant" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{    bound_pdfium\(\)\.map\(\|\(_, dir\)\| dir\.as_path\(\)\)}{    bound_pdfium()?;\n    Ok(Path::new("/nowhere"))}' \
  'Ok(Path::new("/nowhere"))' \
  mnema-extract 'a_successful_probe_names_the_directory_it_loaded_from' --test pdfium_binding

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
