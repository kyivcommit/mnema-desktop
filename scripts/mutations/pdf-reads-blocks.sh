#!/usr/bin/env bash
# A worker that reads everything it is handed, PDFs included: every request is answered
# with a header, a page and a block. No case statement, because there is no case — that
# is the point of the stand-in.
#
# This is the state verify-bundle.sh's PDF branch was written for and the only reason
# that branch exists: the day a real reader lands, the bundled worker answers a PDF with
# blocks, the library has to be inside the image, and nothing in the check proves it is.
# The branch must turn red that day rather than stay quietly green. Control 16d is what
# has seen it do so.
while IFS= read -r _request; do
  printf '{"frame":"header","sha256":"0","mime":"application/octet-stream","source_kind":"document","pages":1}\n'
  printf '{"frame":"page","page_no":1,"section_title":null}\n'
  printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n'
done
