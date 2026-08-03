# Mutation cases for refusing a file by its content (D51). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/refuse-by-content.sh
#
# Named after the subsystem rather than a task number: the two rules this pins
# arrived in different tasks, and a reader asking "what proves the refusal
# works" should not have to know which.
#
# The subsystem has two refusals that look alike and are not. `not_text` says
# the bytes are a photo, and the index must stop answering under that path;
# `binary_tail` says the bytes opened as text and stopped, and the index must
# NOT — the prose is still on disk in front of the damage. Every case below
# that touches one of them names the other in passing, because collapsing the
# two is the mistake that costs a document.

# ------------------------------------------------------ what counts as text

# Without the mark branch a UTF-16 file's own NUL bytes read as corruption, and
# a file the product decodes correctly today is refused — and, through
# `displaces`, deleted.
case_ "typing: a UTF-16 byte-order mark still changes how the question is asked" \
  crates/mnema-extract/src/typing.rs \
  's{if bytes\.starts_with\(&\[0xFF, 0xFE\]\) \|\| bytes\.starts_with\(&\[0xFE, 0xFF\]\)}{if false}' \
  'if false {' \
  mnema-extract 'typing::tests::a_utf16_byte_order_mark_is_text_despite_its_nul_bytes' --lib

# The scan covers the whole slice, not a prefix. A prefix check passes a file
# that is text for a long stretch and binary afterwards — which is the exact
# shape of the interrupted note the `binary_tail` rule exists for, so this
# would not merely miss a verdict, it would index the damage as prose.
case_ "typing: the NUL scan does not stop at a prefix" \
  crates/mnema-extract/src/typing.rs \
  's{bytes\.iter\(\)\.position\(\|b\| \*b == 0\)}{bytes.iter().take(8192).position(|b| *b == 0)}' \
  'take(8192)' \
  mnema-extract 'typing::tests::the_scan_does_not_stop_at_a_prefix' --lib

# 512 is measured — every binary sample carries its first NUL at offset 0, 4,
# 5, 8, 15 or 254 — and a threshold nothing pins moves in a refactor. Measured
# before the assertion existed: moving it to 4096 left the whole workspace
# green, 51 test binaries and no failure.
case_ "typing: the head window is 512 bytes, and that number is measured" \
  crates/mnema-extract/src/typing.rs \
  's{pub\(crate\) const HEAD_BYTES: usize = 512;}{pub(crate) const HEAD_BYTES: usize = 4096;}' \
  'HEAD_BYTES: usize = 4096;' \
  mnema-extract 'typing::tests::the_head_window_ends_where_the_constant_says_and_the_constant_is_512' --lib

# The window is exclusive at HEAD_BYTES. An off-by-one here moves exactly one
# byte from "binary from the start" to "text that stopped", which is the line
# deciding whether the index keeps the document — and nothing else in the
# crate notices it.
case_ "typing: the head window's edge is exclusive, not inclusive" \
  crates/mnema-extract/src/typing.rs \
  's{Some\(at\) if at < HEAD_BYTES}{Some(at) if at <= HEAD_BYTES}' \
  'Some(at) if at <= HEAD_BYTES' \
  mnema-extract 'typing::tests::the_head_window_ends_where_the_constant_says_and_the_constant_is_512' --lib

# --------------------------------------------- the rule name crossing the wire

# The worker reports its rule as a plain string because `mnema-extract` may not
# link `mnema-index` (D26/D40), so nothing but a test compares the two spellings.
case_ "wire: the worker spells not_text the way the pool reads it" \
  crates/mnema-extract/src/bin/worker.rs \
  's{rule: "not_text"\.to_string\(\),}{rule: "nottext".to_string(),}' \
  'rule: "nottext".to_string(),' \
  mnema-extract 'a_photo_is_refused_by_the_real_worker' --test worker_cli

case_ "wire: the worker spells binary_tail the way the pool reads it" \
  crates/mnema-extract/src/bin/worker.rs \
  's{rule: "binary_tail"\.to_string\(\),}{rule: "binarytail".to_string(),}' \
  'rule: "binarytail".to_string(),' \
  mnema-ingest 'an_interrupted_append_does_not_delete_what_the_note_still_says' --test slice

# The pool's own side of the same seam. This is NOT an exhaustive `match` —
# there is an `other => Err` arm — so the compiler catches a forgotten enum
# variant here and never a forgotten string. Measured during task 4: with this
# arm deleted, all 22 tests of the crate stayed green, and the consequence was
# not cosmetic — every non-text file would have become a protocol error instead
# of a skip, stopping a walk over a folder of photos.
case_ "wire: the pool knows the string not_text" \
  crates/mnema-pool/src/lib.rs \
  's{                    "not_text" => Failure::NotText,\n}{}' \
  '"unsupported" => Failure::Unsupported,
                    "binary_tail" => Failure::BinaryTail,' \
  mnema-pool 'a_refusal_by_content_crosses_the_wire' --test supervision

case_ "wire: the pool knows the string binary_tail" \
  crates/mnema-pool/src/lib.rs \
  's{                    "binary_tail" => Failure::BinaryTail,\n}{}' \
  '"not_text" => Failure::NotText,
                    "unreadable" => Failure::Unreadable,' \
  mnema-ingest 'an_interrupted_append_does_not_delete_what_the_note_still_says' --test slice

# ------------------------------------------- which refusal deletes, and which not

# A `.txt` overwritten by a photo must stop answering under its own name. This
# is the citation the displacement exists to prevent: text the file no longer
# contains, under a filename that still exists.
case_ "displaces: a photo replacing a note removes what the note used to say" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::NotText => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{SkipRule::NotText => false,}' \
  'SkipRule::NotText => false,' \
  mnema-ingest 'a_text_file_overwritten_by_a_photo_stops_answering' --test slice

# The condition itself, which task 10 added after the data-loss harness found
# what its absence costs: a file whose bytes never moved losing its document
# because a later release classifies those same bytes differently. The rule
# changed, the file did not, and the text is still on disk.
case_ "displaces: a refusal on unchanged bytes deletes nothing" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::NotText => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{SkipRule::NotText => true,}' \
  'SkipRule::NotText => true,' \
  mnema-ingest 'a_file_whose_bytes_did_not_change_keeps_its_document' --test slice

# The comparison's direction. Inverted, it keeps exactly the documents it must
# remove and removes exactly those it must keep — and one test alone would not
# say which way round the condition is written.
case_ "displaces: the digest comparison is not inverted" \
  crates/mnema-ingest/src/lib.rs \
  's{sha != recorded\.document_id}{sha == recorded.document_id}' \
  'sha == recorded.document_id' \
  mnema-ingest 'a_text_file_overwritten_by_a_photo_stops_answering' --test slice

# The real worker's own end of it. Asserted at the worker's boundary rather
# than through `displaces`, because within one release a file that classifies
# as not-text was never indexed as text — the case the digest exists for needs
# two classifier versions, which no test in one binary can stage.
case_ "wire: the worker sends the digest it refused on" \
  crates/mnema-extract/src/bin/worker.rs \
  's{// whether the file changed or only the rule did\.\n                sha256: Some\(sha256\),}{// whether the file changed or only the rule did.\n                sha256: None,}' \
  'only the rule did.
                sha256: None,' \
  mnema-extract 'a_photo_is_refused_by_the_real_worker' --test worker_cli

# The digest has to survive two hops inside the pool, and each is a separate
# field that can be dropped on its own. Dropping either is a silent reversion
# to deleting on every refusal — `is_none_or` reads a missing digest as "the
# bytes are unknown, so displace".
#
# Both cases name the same test, and that is the point: it is the only one that
# reaches `displaces` with a digest that MATCHES what the index holds, so it is
# the only one either hop can be seen from.
case_ "wire: the pool carries the digest out of the frame" \
  crates/mnema-pool/src/lib.rs \
  's{                return Ok\(Answer::Skipped \{\n                    failure,\n                    reason,\n                    sha256,\n                \}\);}{                return Ok(Answer::Skipped \{\n                    failure,\n                    reason,\n                    sha256: None,\n                \});}' \
  'reason,
                    sha256: None,' \
  mnema-ingest 'a_file_whose_bytes_did_not_change_keeps_its_document' --test slice

case_ "wire: the pool carries the digest into the Skip it returns" \
  crates/mnema-pool/src/lib.rs \
  's{                    return Ok\(Outcome::Skipped\(Skip \{\n                        failure,\n                        reason,\n                        sha256,\n                    \}\)\);}{                    return Ok(Outcome::Skipped(Skip \{\n                        failure,\n                        reason,\n                        sha256: None,\n                    \}));}' \
  'reason,
                        sha256: None,' \
  mnema-ingest 'a_file_whose_bytes_did_not_change_keeps_its_document' --test slice

# And the other side of that same line, which is the whole of task 9: a note
# whose append was interrupted comes back with a zeroed tail and is refused,
# but its prose is still on disk and readable nowhere else.
case_ "displaces: an interrupted append does NOT remove what the note still says" \
  crates/mnema-ingest/src/lib.rs \
  's{        SkipRule::BinaryTail => false,\n}{        SkipRule::BinaryTail => true,\n}' \
  'SkipRule::BinaryTail => true,' \
  mnema-ingest 'an_interrupted_append_does_not_delete_what_the_note_still_says' --test slice

# ------------------------------------------------ when the journal may be trusted

# The second cheap arm answers from the journal without spending a worker, and
# `INDEX_FORMAT_VERSION` is the only lever that makes a content refusal
# re-examined. Measured during task 5: rolling the constant back from 2 to 1
# left all 51 groups green, so the condition had nothing guarding it.
case_ "journal: a verdict from an older format version is not honoured" \
  crates/mnema-ingest/src/lib.rs \
  's{        && skip\.format_version == INDEX_FORMAT_VERSION\n}{}' \
  '&& let Some(skip) = db.skip_entry(root_id, relative)?
        && skip.size_bytes == Some(disk.size_bytes)' \
  mnema-ingest 'a_stale_format_version_is_not_honoured_by_the_second_cheap_arm' --test slice
