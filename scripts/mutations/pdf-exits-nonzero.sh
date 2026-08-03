#!/usr/bin/env bash
# A worker that answers a text file the way the real one does and then cannot answer for
# a PDF at all: it exits non-zero and says nothing about why. That is the state control
# 16c produces, and the one verify-bundle.sh must not read as the answer "no reader" —
# the bundle's Pdfium obligation is UNANSWERED, which is the distinction the whole PDF
# section exists to keep.
#
# Distinct from pdf-says-nothing.sh beside it: that one answers a PDF with a frame the
# check does not recognise, which is a different branch. This one never answers.
while IFS= read -r line; do
  case "${line}" in
    # Matches the whole request line rather than a path, true while a request carries
    # exactly one — the same trade pdf-says-nothing.sh makes and for the same reason.
    *.pdf*) exit 1 ;;
    *) printf '{"frame":"header","sha256":"0","mime":"text/plain","source_kind":"document","reader":"text","reader_version":1,"pages":1}\n'
       printf '{"frame":"page","page_no":1,"section_title":null}\n'
       printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n' ;;
  esac
done
