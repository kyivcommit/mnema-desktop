#!/usr/bin/env bash
# A worker that reads PDFs with a library that is not in the bundle it shipped in.
#
# This is the exact state the `blocks` branch of verify-bundle.sh was written to catch,
# and it is not hypothetical: the first bundle built after the PDF reader landed loaded
# `vendor/pdfium/lib/libpdfium.dylib` out of the developer's checkout, because the third
# branch of the library search is an absolute path baked in at compile time. It read PDFs
# perfectly on the machine that built it and would have read none anywhere else. Only a
# code-signing refusal made it visible; with the entitlement in place it would have been
# green and wrong.
#
# So: blocks over the wire, and a probe that answers `loaded:true` while naming a
# directory outside the image. A check that stopped at "it reads PDFs", or at "a library
# is in the bundle", passes this. Control 16f is what has seen it fail.
if [ "${1:-}" = "--probe-pdfium" ]; then
  printf '{"loaded":true,"pages":1,"stage":"ok","library_dir":"/usr/lib"}\n'
  exit 0
fi
while IFS= read -r _request; do
  printf '{"frame":"header","sha256":"0","mime":"application/pdf","source_kind":"document","reader":"pdf","reader_version":1,"pages":1}\n'
  printf '{"frame":"page","page_no":1,"section_title":null}\n'
  printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n'
done
