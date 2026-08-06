#!/usr/bin/env bash
# A worker that reads a PDF over the wire and then cannot run its own probe.
#
# The two answers come from one library and one process, so a build where they disagree
# is a build nothing here understands — and the `blocks` branch must call that
# UNANSWERED rather than take the half it likes. It has just seen blocks; the temptation
# is to treat the probe as a formality and read a failed run as "well, it read the PDF".
# That is how a check comes to report on the run it wanted rather than the run it got.
#
# Exits 3 rather than 1, so that the status in the message is a number this file put
# there. Control 16h is what has seen it fail.
if [ "${1:-}" = "--probe-pdfium" ]; then
  exit 3
fi
while IFS= read -r _request; do
  printf '{"frame":"header","sha256":"0","mime":"application/pdf","source_kind":"document","reader":"pdf","reader_version":1,"pages":1}\n'
  printf '{"frame":"page","page_no":1,"section_title":null}\n'
  printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n'
done
