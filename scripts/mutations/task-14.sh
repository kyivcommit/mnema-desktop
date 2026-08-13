# Mutation cases for Task 14: the randomised data-loss harness. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-14.sh
#
# **`task-14.sh`, and `scripts/mutations/task-13.sh` is a different file** —
# the previous cycle's walk-level cases for this same harness. Both are run for
# this task under D63, because this task edits the file that one measures.
#
# What is different about mutating a harness: every case below breaks the
# **product** and requires the harness to notice, except the last group, which
# breaks the **generator** and requires the corpus assertion to notice. Those two
# are not the same claim. A harness whose invariants are perfect and whose
# generator reaches nothing is green for ever, and that is exactly the state this
# task found: `Unsupported` had no generator for two cycles, and no invariant
# could have said so.
#
# Cases are anchored on code, never on the prose beside it.

# ------------------------------------------------- the per-page journal rows

# C1. The clean-up after a fresh index. A page that has text again keeps a row
# saying it has none — a journal line telling someone page 3 is missing while the
# index answers searches with page 3's text. Nothing else in the file notices:
# the document is complete and every marker is findable.
case_ "ingest: a re-read file forgets the page skips it used to have" \
  crates/mnema-ingest/src/lib.rs \
  's~(fn journal_skipped_pages\(.*?)    db\.forget_page_skips\(root_id, relative\)\?;~${1}    let _ = \&db;~s' \
  '    let _ = &db;' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C2. **The second entrance, and the whole reason this class is worth a case of
# its own.** `forget_page_skips` is called from two independent places and the
# plan that added the class saw one. Breaking either alone must redden.
case_ "ingest: the other path that forgets page skips forgets them too" \
  crates/mnema-ingest/src/lib.rs \
  's~            db\.forget_page_skips\(root_id, relative\)\?;~            let _ = \&db;~' \
  '            let _ = &db;' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ------------------------------------------------------- displaces, per rule

# C3. `NoTextLayer` moved to the unconditional side. A document of scans walked
# by a build whose reader recovers and then by one whose reader does not would
# lose its document with the bytes never having moved — and until this task the
# harness put this rule in an empty arm and asserted nothing about it.
case_ "ingest: a document with no text layer keeps a document built from the same bytes" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::NoTextLayer => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::NoTextLayer => true,~' \
  'SkipRule::NoTextLayer => true,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C4. The same for `Malformed`, which no generator reached before this task
# either: a reader that gives up on a damaged file must not delete the document
# an earlier build read out of the identical bytes.
case_ "ingest: a malformed file keeps a document built from the same bytes" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Malformed => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Malformed => true,~' \
  'SkipRule::Malformed => true,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C5. And `Encrypted`. "Cannot open this" becomes "ask for a key" the day a
# prompt is built, so the file has not changed when the verdict does.
case_ "ingest: an encrypted file keeps a document built from the same bytes" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Encrypted => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Encrypted => true,~' \
  'SkipRule::Encrypted => true,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C6. `Unsupported` on the other side: a rule that refuses to displace when the
# bytes really did change leaves the index answering under this name with a
# document that is gone.
case_ "ingest: an unsupported file whose bytes changed displaces the old document" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Unsupported => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Unsupported => false,~' \
  'SkipRule::Unsupported => false,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ------------------------------------------------------------ the generator
#
# These break the corpus rather than the product. They are the cases that would
# have caught `opaque_body` going stale, and they fail on an assertion about what
# the run *contained* rather than about what it did.

# C7. **The defect this task was called to fix, restored.** A `%PDF-` stub is
# `malformed` since the pdf reader landed, so `Unsupported` loses its only
# generator — and every invariant that judges it goes quiet while staying green.
case_ "harness: the unsupported generator really produces unsupported" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            bytes: zip_of\(&\[\("readme.nfo", b"nothing any reader here knows".to_vec\(\)\)\]\),~            bytes: b"%PDF-1.7\\n1 0 obj\\n<<>>\\nendobj\\n".to_vec(),~' \
  'bytes: b"%PDF-1.7' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C8. The four container readers dropped out of `create`, which is how a format
# stops being generated without anybody editing an invariant.
case_ "harness: the corpus really contains every format it claims" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            5 => format!\("docs/page-\{n\}.html"\),~            5 => format!("docs/file-{n}.txt"),~' \
  '5 => format!("docs/file-{n}.txt"),' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C9. The per-page class stops being drawn. The rows exist, the invariant that
# judges them is intact, and nothing produces one — which is the shape of every
# defect this file is meant to catch, turned on the file itself.
case_ "harness: the corpus really contains a document with an unreadable page" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            25 => self\.document_with_an_unreadable_page\(\),~            25 => self.run_walk(),~' \
  '25 => self.run_walk(),' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ------------------------------------------ fix round 1: the page NUMBER

# C10. **The half of Task 11's class that counting rows cannot reach.** The gap
# is generated first, so page 1 is the one that cannot be read; moving it to the
# end leaves the row count right and the number wrong, which is what a reader
# that renumbers what came back produces.
case_ "harness: the gap is reported at the number it is at" \
  crates/mnema-ingest/tests/randomised.rs \
  's~                let mut spine = vec!\[gap\];~                let mut spine: Vec<SpineEntry> = Vec::new();\n                let _ = \&gap;~' \
  'let mut spine: Vec<SpineEntry> = Vec::new();' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C11. The other arm of 3e: chapters written in reverse, so every page holds
# some *other* chapter's text. The count is right, every marker is findable,
# invariant 4 is satisfied — and every citation names the wrong chapter. This is
# the defect a "sensible" fix produced in Task 11.
case_ "harness: each page holds its own chapter's text" \
  crates/mnema-ingest/tests/randomised.rs \
  's~                for \(i, m\) in markers\.iter\(\)\.enumerate\(\) \{\n                    spine\.push\(SpineEntry::Chapter\(i\)\);~                for (i, m) in markers.iter().rev().enumerate() {\n                    spine.push(SpineEntry::Chapter(i));~' \
  'for (i, m) in markers.iter().rev().enumerate() {' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C12. The same member named twice must arrive as two pages. Dropping the second
# reference loses a page the spine declares.
case_ "harness: a chapter the spine names twice is two pages" \
  crates/mnema-ingest/tests/randomised.rs \
  's~                spine\.push\(SpineEntry::Repeat\(0\)\);~                let _ = SpineEntry::Repeat(0);~' \
  'let _ = SpineEntry::Repeat(0);' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C13. The corpus assertion compares **sets**, so a rule classified as
# unmodelled that the corpus does in fact reach fails just as loudly as one it
# does not. This is the direction the first version could not see, and the
# direction that corrected my own classification of `Unreadable` on its first
# run.
case_ "harness: the corpus assertion fails in both directions" \
  crates/mnema-ingest/tests/randomised.rs \
  's~        SkipRule::Memory => false,~        SkipRule::Memory => true,~' \
  'SkipRule::Memory => true,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised
