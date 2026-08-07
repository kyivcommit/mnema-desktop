#!/usr/bin/env bash
# A worker that satisfies every check except the one control 16 exists for: it
# answers a text file the way the real worker does, and answers a PDF with a frame
# that is neither blocks nor rule=unsupported. Lives beside the other deliberate
# mutations rather than inside the controls script, so that what it does is
# readable on its own.
while IFS= read -r line; do
  case "${line}" in
    *.pdf*) printf '{"frame":"summary","skipped_pages":[],"text_source":"native:pdf"}\n' ;;
    *) printf '{"frame":"header","sha256":"0","mime":"text/plain","source_kind":"document","reader":"text","reader_version":1,"pages":1}\n'
       printf '{"frame":"page","page_no":1,"section_title":null}\n'
       printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n' ;;
  esac
done
