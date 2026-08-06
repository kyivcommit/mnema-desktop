#!/usr/bin/env bash
# A worker that reads PDFs and will not say where its library came from.
#
# The shape a regression takes rather than a shape anyone would write: delete the
# `library_dir` field from `--probe-pdfium` (crates/mnema-extract/src/bin/worker.rs) and
# this is what the bundled worker becomes. Everything else still answers — blocks over
# the wire, `loaded:true` from the probe — and the one question the `blocks` branch of
# verify-bundle.sh exists to ask has no answer at all.
#
# Which must not read as yes. "The check could not find out" and "the library is inside
# the bundle" are the two states this whole section keeps apart, and they collapse if an
# absent field is allowed to mean the good one. Control 16g is what has seen it fail.
if [ "${1:-}" = "--probe-pdfium" ]; then
  printf '{"loaded":true,"pages":1,"stage":"ok"}\n'
  exit 0
fi
while IFS= read -r _request; do
  printf '{"frame":"header","sha256":"0","mime":"application/pdf","source_kind":"document","reader":"pdf","reader_version":1,"pages":1}\n'
  printf '{"frame":"page","page_no":1,"section_title":null}\n'
  printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n'
done
