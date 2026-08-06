#!/usr/bin/env bash
# A worker with no PDF reader in it — the build every bundle of this repository was,
# until the format-readers cycle landed one.
#
# It exists because that state stopped being producible by the real worker and the rule
# it produces did not stop mattering: a bundle that cannot read PDFs must not carry 7.7 MB
# of Pdfium. verify-bundle.sh still has the branch; without this stand-in the branch had
# nothing left that could reach it, and a branch nobody can reach is a claim rather than a
# check.
#
# Note what this stand-in shows about the branch it reaches. `bundle.resources` in
# src-tauri/tauri.conf.json packages the library unconditionally, so a bundle built here
# always carries it — which means the branch's green path ("refuses PDFs, and no library
# is bundled, so nothing is wasted") can no longer be produced from this repository's own
# configuration at all. Every bundle this repository builds and hands to a worker like
# this one reddens on dead weight. Control 15 is that run.
if [ "${1:-}" = "--probe-pdfium" ]; then
  printf '{"loaded":false,"stage":"library_dir","error":"no reader, so nothing looked"}\n'
  exit 0
fi
while IFS= read -r line; do
  case "${line}" in
    # Matches the whole request line rather than a path, true while a request carries
    # exactly one — the same trade pdf-says-nothing.sh beside it makes, for the reason
    # written there.
    *.pdf*) printf '{"frame":"refused","rule":"unsupported","reason":"no reader implemented yet for application/pdf (Pdf)"}\n' ;;
    *) printf '{"frame":"header","sha256":"0","mime":"text/plain","source_kind":"document","reader":"text","reader_version":1,"pages":1}\n'
       printf '{"frame":"page","page_no":1,"section_title":null}\n'
       printf '{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"x","line_start":1,"line_end":1}\n' ;;
  esac
done
