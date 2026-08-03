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

# The tail arm behind the byte-order mark — the whole output of one branch,
# which nothing reached. Measured before the test named below existed: this
# mutation left all eight targets of mnema-extract green, `mnema-ingest --test
# slice` at 35 passed, and mnema-pool green. The randomised harness cannot find
# it either: `interrupted_append_body` writes UTF-8 prose only.
case_ "typing: a UTF-16 note that stops being text is a tail, not a photo" \
  crates/mnema-extract/src/typing.rs \
  's{            Some\(_\) => Verdict::BinaryTail,\n        \};}{            Some(_) => Verdict::NotText,\n        \};}' \
  'Some(_) => Verdict::NotText,
        };' \
  mnema-extract 'typing::tests::an_interrupted_utf16_note_is_a_tail_and_not_a_photo' --lib

# The same mutation named against what it costs, rather than against what it
# classifies: `NotText` on changed bytes displaces, so the note's prose — still
# on disk in front of the damage — is deleted from the index.
case_ "displaces: an interrupted UTF-16 note keeps the prose it still has" \
  crates/mnema-extract/src/typing.rs \
  's{            Some\(_\) => Verdict::BinaryTail,\n        \};}{            Some(_) => Verdict::NotText,\n        \};}' \
  'Some(_) => Verdict::NotText,
        };' \
  mnema-ingest 'an_interrupted_utf16_note_does_not_delete_what_it_still_says' --test slice

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

# Reading the string is only half of the seam; the other half is the rule the
# journal is handed, and until now nothing here touched it. Four cases on
# writing the strings, none on the mapping — while the file's own header says
# collapsing the two refusals "is the mistake that costs a document", and that
# mistake lives in exactly one line of `impl From<Failure> for SkipRule`.
#
# Measured before `Failure::every` replaced the hand-written list in
# `every_failure_maps_onto_its_own_skip_rule`: this mutation left all of
# mnema-pool green (7/23/1 passed) and reddened only `mnema-ingest/tests/slice.rs`
# — the crate that owns the line was not the crate that caught it.
case_ "pool: the failure mapping does not collapse binary_tail into not_text" \
  crates/mnema-pool/src/lib.rs \
  's{            Failure::BinaryTail => SkipRule::BinaryTail,}{            Failure::BinaryTail => SkipRule::NotText,}' \
  'Failure::BinaryTail => SkipRule::NotText,' \
  mnema-pool 'every_failure_maps_onto_its_own_skip_rule' --test supervision

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

# The same pair for `Unsupported`, which kept displacing unconditionally for a
# release after `NotText` stopped. The inversion is worth naming: the rule made
# conditional first was the STABLE one — `not_text` promises no release will
# read those bytes as prose — while `unsupported` says "no reader implemented
# yet", which is what a release changes by definition. A folder of PDFs indexed
# by a build that has the reader and walked by one that does not lost a document
# per file, with the bytes never having moved.
case_ "displaces: a format with no reader replacing a note removes what the note said" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::Unsupported => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{SkipRule::Unsupported => false,}' \
  'SkipRule::Unsupported => false,' \
  mnema-ingest 'a_text_file_overwritten_by_a_format_with_no_reader_stops_answering' --test slice

case_ "displaces: a build that lost a reader deletes nothing" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::Unsupported => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{SkipRule::Unsupported => true,}' \
  'SkipRule::Unsupported => true,' \
  mnema-ingest 'a_file_no_reader_can_take_keeps_its_document_when_only_the_rule_changed' --test slice

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

# …and the two branches beside it, which had nothing. The blindness was
# structural: both of the parent's deterministic witnesses for this field stand
# a shell script in for the worker and have it print a digest they chose, so
# they pin that `displaces` CONSUMES it and nothing pinned that anything
# PRODUCES it. Measured before the test named below existed — dropping the field
# from the `unsupported` branch left the whole workspace green, 0 failed under
# `--no-fail-fast`, and `cargo test` without that flag stops at the first failing
# target and would have hidden even a real one.
#
# What it costs is the defect `e345491` closed, arriving from the other end: a
# missing digest reads as "the bytes are unknown, so displace", so a folder of
# PDFs indexed by a build with the reader and walked by a build without it loses
# a document per file with the bytes never having moved.
case_ "wire: the worker sends the digest for a format it has no reader for" \
  crates/mnema-extract/src/bin/worker.rs \
  's{("unsupported".*?)sha256: Some\(sha256\)}{$1sha256: None}s' \
  'file_type.mime, file_type.reader
                ),
                sha256: None,' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# `binary_tail` never displaces, so this field cannot cost a document through
# `displaces` today — which is exactly why it needs a case at the boundary
# rather than through the parent. It is the evidence a future rule about that
# path would be decided on, and nothing downstream would notice it going
# missing.
case_ "wire: the worker sends the digest for a note that stopped being text" \
  crates/mnema-extract/src/bin/worker.rs \
  's{("binary_tail".*?)sha256: Some\(sha256\)}{$1sha256: None}s' \
  '                sha256: None,
            }]
        }
        // None of these five formats' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

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

# ------------------------------------------------ the size ceiling's own arm

# The ceiling's arm had two cases named in prose and one of them measured. This
# pair states both, and the pair matters because either mutation alone looks
# like a plausible simplification of the other.
case_ "displaces: a file that grew past the ceiling stops answering" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::TooLarge => on_disk\.is_some_and\(\|disk\| \{\n            disk\.size_bytes != recorded\.size_bytes \|\| disk\.mtime != recorded\.mtime\n        \}\),}{SkipRule::TooLarge => false,}' \
  'SkipRule::TooLarge => false,' \
  mnema-ingest 'a_file_grown_past_the_ceiling_loses_what_the_index_held' --test slice

case_ "displaces: the ceiling does not delete a file it still recognises" \
  crates/mnema-ingest/src/lib.rs \
  's{SkipRule::TooLarge => on_disk\.is_some_and\(\|disk\| \{\n            disk\.size_bytes != recorded\.size_bytes \|\| disk\.mtime != recorded\.mtime\n        \}\),}{SkipRule::TooLarge => true,}' \
  'SkipRule::TooLarge => true,' \
  mnema-ingest 'a_lowered_ceiling_keeps_what_it_still_recognises' --test slice

# The Critical this arm was widened for. The size alone cannot see a file
# rewritten in place at the same length, and the argument that it could —
# "a file of that length that is over the ceiling now was over the ceiling
# then" — assumed the ceiling had not moved, inside the one rule that exists
# because it can.
case_ "displaces: the ceiling reads the modification time, not the size alone" \
  crates/mnema-ingest/src/lib.rs \
  's{ \|\| disk\.mtime != recorded\.mtime\n}{\n}' \
  'disk.size_bytes != recorded.size_bytes
        }),' \
  mnema-ingest 'a_file_rewritten_in_place_under_a_lowered_ceiling_stops_answering' --test slice

# And the other half of the same condition, which a `cp -p` of a different file
# is blind to the clock for. Without its own case the size comparison could be
# deleted outright and every ceiling test would stay green, because each of the
# others moves the modification time as well.
case_ "displaces: the ceiling reads the size, not the modification time alone" \
  crates/mnema-ingest/src/lib.rs \
  's{disk\.size_bytes != recorded\.size_bytes \|\| }{}' \
  'disk.mtime != recorded.mtime
        }),' \
  mnema-ingest 'a_replacement_of_a_different_length_carrying_the_old_time_stops_answering' --test slice

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

# The arm answered before anything decided whether the document under that path
# had to go, so a remembered refusal never displaced anything at all. Measured
# at `walk_root`'s own level: three walks, and the third came back
# `{ found: 1, indexed: 0, skipped: 1, removed: 0, stopped: Completed }` with
# the note still answering under a name whose file is a photo.
case_ "journal: a remembered refusal does not answer for a document it would remove" \
  crates/mnema-ingest/src/lib.rs \
  's{\n        && !recorded\n            \.as_ref\(\)\n            \.is_some_and\(\|entry\| displaces\(skip\.rule, entry, on_disk, None\)\)}{}' \
  '&& skip.mtime == Some(disk.mtime)
    {' \
  mnema-ingest 'a_remembered_refusal_does_not_answer_for_a_document_it_would_remove' --test slice

# The same defect through three walks of one folder — and this case removes
# BOTH guards, which is a finding rather than a convenience. Measured: with only
# the clause above deleted, the walk test stays green, because clearing the
# journal row on a successful index already stops the third walk finding
# anything to short-circuit on; and with only the clearing deleted it stays
# green too, because the clause then falls through to a worker. The measured
# reproduction is closed twice over. Each guard is pinned on its own by the case
# before this one and the case after it; this one is what says the walk-level
# behaviour depends on their union and nothing weaker.
#
# The marker checks the first substitution only — the two edits are far apart in
# the file and `contains` takes one contiguous string. `git diff --quiet`, the
# guard that actually matters, still covers both.
case_ "journal: neither guard alone is what the three-walk reproduction rests on" \
  crates/mnema-ingest/src/lib.rs \
  's{\n        && !recorded\n            \.as_ref\(\)\n            \.is_some_and\(\|entry\| displaces\(skip\.rule, entry, on_disk, None\)\)}{}; s{    db\.forget_skip\(root_id, relative\)\?;\n}{}' \
  '&& skip.mtime == Some(disk.mtime)
    {' \
  mnema-ingest 'a_photo_restored_with_its_own_time_stops_the_note_answering' --test walk

# The other direction, which is what the arm is paid for: a rule that KEEPS must
# still short-circuit, or a folder of interrupted files spends a worker process
# each on every walk forever.
case_ "journal: a rule that keeps still answers without a worker" \
  crates/mnema-ingest/src/lib.rs \
  's{\.is_some_and\(\|entry\| displaces\(skip\.rule, entry, on_disk, None\)\)}{.is_some_and(|_| true)}' \
  '.is_some_and(|_| true)' \
  mnema-ingest 'a_second_walk_over_an_interrupted_note_asks_no_worker' --test walk

# A file indexed after a refusal kept that refusal for the life of the index —
# listed in the window as "not indexed" while it was, and left standing as a
# live verdict for the arm above.
case_ "journal: indexing a file forgets the refusal that kept it out" \
  crates/mnema-ingest/src/lib.rs \
  's{    db\.forget_skip\(root_id, relative\)\?;\n}{}' \
  'db.insert_path(root_id, relative, id, disk.size_bytes, disk.mtime)?;
    if let Some(displaced) = displaced {' \
  mnema-ingest 'indexing_a_file_forgets_the_refusal_that_kept_it_out' --test slice
