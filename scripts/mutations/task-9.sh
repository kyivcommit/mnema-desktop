# Mutation cases for packaging. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-9.sh
#
# Two files are under test and neither is Rust: docs/BUILD.md, which a packager
# reads to learn which Pdfium release to fetch, and .github/workflows/ci.yml,
# whose two silent-pass settings are the point of packaging_workflow.rs.
#
# The checks over the .dmg itself are not here, because the harness runs
# `cargo test` and no Rust test can build a bundle. They live in
# scripts/verify-bundle.sh and their seven negative controls are reproducible
# with a build in hand; the task-9 report lists them.

# The three cases below match the version with a pattern and replace it with a
# fixed impossible value, rather than naming the current pin. A case that spells
# out today's number is one more place a bump has to be chased into — and one
# that goes BROKEN at exactly the wrong moment, since the harness reports a
# pattern that matched nothing as a broken case rather than as a result.

case_ "build doc: the crate version row is the pinned one" \
  docs/BUILD.md \
  's{\| `pdfium-render` \| `=[0-9][0-9.]*` \|}{| `pdfium-render` | `=0.0.0` |}' \
  '| `pdfium-render` | `=0.0.0` |' \
  mnema-extract 'the_build_document_names_the_same_pair' --test pdfium_binding

case_ "build doc: the binary row is the build the bindings declare" \
  docs/BUILD.md \
  's{\| Pdfium binary, non-V8 \| `chromium/[0-9]+` \|}{| Pdfium binary, non-V8 | `chromium/0` |}' \
  '| Pdfium binary, non-V8 | `chromium/0` |' \
  mnema-extract 'the_build_document_names_the_same_pair' --test pdfium_binding

# The number survives, the row does not. A substring search for the build number
# is satisfied by this and sees nothing — which is how the same defect passed
# three times in this repository before. The check matches whole lines, so it
# must go red here. The marker is a prefix for the same reason as above: it must
# not name a version either.
case_ "build doc: prose naming the build is not a row" \
  docs/BUILD.md \
  's{\| Pdfium binary, non-V8 \| `chromium/([0-9]+)` \|}{The Pdfium binary is chromium/$1, non-V8.}' \
  'The Pdfium binary is chromium/' \
  mnema-extract 'the_build_document_names_the_same_pair' --test pdfium_binding

case_ "workflow: the upload must not fall back to the default" \
  .github/workflows/ci.yml \
  's{if-no-files-found: error}{if-no-files-found: warn}' \
  'if-no-files-found: warn' \
  mnema-desktop 'the_artefact_upload_refuses_to_succeed_with_nothing_to_upload' --test packaging_workflow

# Commented out rather than deleted: the words stay in the file, the setting is
# gone. Same family as the case above it, and the reason `declares` strips
# everything after `#` before comparing.
case_ "workflow: a commented-out setting is not a setting" \
  .github/workflows/ci.yml \
  's{\n          if-no-files-found: error\n}{\n          # if-no-files-found: error\n}' \
  '# if-no-files-found: error' \
  mnema-desktop 'the_artefact_upload_refuses_to_succeed_with_nothing_to_upload' --test packaging_workflow

# The step that exists to fail is made unable to fail. This is the exact shape
# the brief proposed for the signature check —
# `otool -L … | grep -i pdfium || echo "pdfium is statically linked"` — and it
# is why nothing in verify-bundle.sh ends in `|| true`.
case_ "workflow: the bundle check must not be swallowed" \
  .github/workflows/ci.yml \
  's{run: scripts/verify-bundle\.sh}{run: scripts/verify-bundle.sh || true}' \
  'run: scripts/verify-bundle.sh || true' \
  mnema-desktop 'the_bundle_job_checks_what_it_built' --test packaging_workflow

case_ "workflow: naming the script is not running it" \
  .github/workflows/ci.yml \
  's{run: scripts/verify-bundle\.sh}{run: echo would run scripts/verify-bundle.sh}' \
  'run: echo would run scripts/verify-bundle.sh' \
  mnema-desktop 'the_bundle_job_checks_what_it_built' --test packaging_workflow

# The next two are the review's own attack: the setting leaves the job that has
# to carry it and reappears, verbatim, in a job that never runs. A whole-file
# search is satisfied by the copy and reports the gutted job as sound — which is
# what happened, on both lines at once, before `job()` existed. `bundle` is the
# last job in the file, so the impostor is appended after it.

case_ "workflow: the upload setting must be in the job that uploads" \
  .github/workflows/ci.yml \
  's{\n          if-no-files-found: error\n}{\n};s{\z}{\n  dead:\n    if: false\n    steps:\n      - uses: actions/upload-artifact\@v4\n        with:\n          if-no-files-found: error\n}' \
  '  dead:
    if: false' \
  mnema-desktop 'the_artefact_upload_refuses_to_succeed_with_nothing_to_upload' --test packaging_workflow

case_ "workflow: the check must be in the job that builds" \
  .github/workflows/ci.yml \
  's{\n        run: scripts/verify-bundle\.sh\n}{\n};s{\z}{\n  dead:\n    if: false\n    steps:\n      - name: not this one\n        run: scripts/verify-bundle.sh\n}' \
  '      - name: not this one
        run: scripts/verify-bundle.sh' \
  mnema-desktop 'the_bundle_job_checks_what_it_built' --test packaging_workflow

# Four cases on the slicer itself. Without them `opens_a_job` is decoration:
# `bundle` is the last job in ci.yml, so its body runs to the end of the file
# whether the end condition works or not, and every clause below could be
# deleted with the two checks above still green. They red against the two-job
# document inside the test instead.

case_ "slice: a body that never ends reaches into the next job" \
  src-tauri/tests/packaging_workflow.rs \
  's{    line\.starts_with\("  "\)\n}{    false\n}' \
  '    false
        && !line.starts_with("   ")' \
  mnema-desktop 'a_job_body_stops_where_the_next_job_starts' --test packaging_workflow

case_ "slice: a comment at job depth is not a job" \
  src-tauri/tests/packaging_workflow.rs \
  "s{\n        && !line\.trim_start\(\)\.starts_with\('#'\)}{}" \
  "is_empty()
}" \
  mnema-desktop 'a_job_body_stops_where_the_next_job_starts' --test packaging_workflow

case_ "slice: a line of nothing but two spaces is not a job" \
  src-tauri/tests/packaging_workflow.rs \
  's{\n        && !line\.trim\(\)\.is_empty\(\)}{}' \
  'starts_with("   ")
        && !line.trim_start()' \
  mnema-desktop 'a_job_body_stops_where_the_next_job_starts' --test packaging_workflow

# Without the `?`, `skip_while` running off the end yields an empty body rather
# than None, and every assertion downstream then reports a missing setting in a
# job that does not exist.
case_ "slice: a job that is not there is None, not an empty body" \
  src-tauri/tests/packaging_workflow.rs \
  's{    lines\.next\(\)\?;}{    let _ = lines.next();}' \
  'let _ = lines.next();' \
  mnema-desktop 'a_job_body_stops_where_the_next_job_starts' --test packaging_workflow
