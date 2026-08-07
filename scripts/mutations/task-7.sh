# Mutation cases for Task 7: the `Malformed` and `Encrypted` skip rules, and
# the one question that decides whether adding them costs documents — which
# side of `mnema_ingest`'s `displaces` they land on. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-7.sh
#
# Two variants added to a closed vocabulary is the cheap half. The expensive
# half is that both new rules must displace a document *only* when the digest
# says the file is not the one the index was built from — the condition
# `Unsupported` already carries, and the one it did not carry until a review
# measured what that cost. Every case below is a way for that to be silently
# wrong: a rule that always displaces (a folder loses a document per file the
# day a reader gets worse at damage), a rule that never does (the index goes on
# citing text a replaced file no longer contains), a predicate answering for the
# wrong column, or a wire string that never reaches the rule it names.
#
# Note what the compiler already covers and no case here duplicates: every
# `match` in the chain — `displaces`, `is_about_content`,
# `suggests_broken_environment`, `From<Failure> for SkipRule`, `journalled_as`
# and the randomised harness's invariant 3c — is exhaustive, so a variant added
# without a decision does not compile. What the compiler cannot check is whether
# the decision made is the right one, which is all this file is about.

# ------------------------------------------------------------- displaces

# C1. `Malformed` displacing unconditionally: the mistake `Unsupported` was
# shipped with and corrected for. A build whose reader gives up on damage that
# an earlier build recovered from would delete the document of every such file
# — with the bytes never having moved, and with nothing but a journal row to
# say so.
case_ "a damaged file whose bytes did not move keeps its document" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Malformed => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Malformed => true,~' \
  'SkipRule::Malformed => true,' \
  mnema-ingest 'a_damaged_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# C2. The other side of the same line, and the reason the test asserts both:
# "does not displace when the digest matches" is satisfied by a rule that never
# displaces at all. Here a note is overwritten by a broken PDF and the index
# goes on answering under that name with prose the file no longer contains.
case_ "a damaged file whose bytes are NOT the indexed ones stops answering" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Malformed => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Malformed => false,~' \
  'SkipRule::Malformed => false,' \
  mnema-ingest 'a_damaged_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# C3. `Encrypted` has its own arm rather than sharing `Malformed`'s, so it
# needs its own pair: an arm that is never exercised alone can be wrong alone.
case_ "a locked file whose bytes did not move keeps its document" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Encrypted => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Encrypted => true,~' \
  'SkipRule::Encrypted => true,' \
  mnema-ingest 'a_locked_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# C4. And its displacing side.
case_ "a locked file whose bytes are NOT the indexed ones stops answering" \
  crates/mnema-ingest/src/lib.rs \
  's~        SkipRule::Encrypted => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),~        SkipRule::Encrypted => false,~' \
  'SkipRule::Encrypted => false,' \
  mnema-ingest 'a_locked_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# ------------------------------------------------------- the two predicates

# C5. Both rules off the content side of `is_about_content`. Nothing breaks
# loudly: the journal simply stops remembering the bytes, every damaged and
# every locked file costs a worker process on every walk forever, and the debt
# the second cheap arm exists to pay comes back. `TooLarge` is the variant that
# proves this column can be got wrong in the plausible direction.
case_ "damage and a password are facts about the bytes" \
  crates/mnema-index/src/journal.rs \
  's~            \| SkipRule::BinaryTail\n            \| SkipRule::Malformed\n            \| SkipRule::Encrypted => true,~            | SkipRule::BinaryTail => true,\n            SkipRule::Malformed | SkipRule::Encrypted => false,~' \
  'SkipRule::Malformed | SkipRule::Encrypted => false,' \
  mnema-index 'a_damaged_file_is_not_the_same_verdict_as_an_unread_one' --test journal

# C6. The same two rules onto the *environment* side of the other predicate,
# which is the more expensive direction: a folder holding a run of interrupted
# downloads would read as a dying worker and end a walk that has nothing wrong
# with it, leaving the rest of the folder unindexed.
case_ "a folder of broken downloads is not a broken machine" \
  crates/mnema-index/src/journal.rs \
  's~            \| SkipRule::BinaryTail\n            \| SkipRule::Malformed\n            \| SkipRule::Encrypted => false,~            | SkipRule::BinaryTail => false,~; s~            SkipRule::Crash \| SkipRule::Timeout \| SkipRule::Memory \| SkipRule::Unreadable => true,~            SkipRule::Crash\n            | SkipRule::Timeout\n            | SkipRule::Memory\n            | SkipRule::Unreadable\n            | SkipRule::Malformed\n            | SkipRule::Encrypted => true,~' \
  '            | SkipRule::Unreadable
            | SkipRule::Malformed' \
  mnema-index 'a_damaged_file_is_not_the_same_verdict_as_an_unread_one' --test journal

# ------------------------------------------------------ the strings themselves

# C7. The two strings swapped in `as_str`. `parse` is untouched, so every rule
# still round-trips through *some* variant and a test that only checked
# "something came back" would pass — the journal would just answer "encrypted"
# for every damaged file and "malformed" for every locked one, which is the one
# distinction the two variants exist to draw.
case_ "each rule is written under its own string, through SQLite" \
  crates/mnema-index/src/journal.rs \
  's~            SkipRule::Malformed => "malformed",\n            SkipRule::Encrypted => "encrypted",~            SkipRule::Malformed => "encrypted",\n            SkipRule::Encrypted => "malformed",~' \
  'SkipRule::Malformed => "encrypted",' \
  mnema-index 'every_skip_rule_is_recorded_under_its_own_string' --test journal

# C8. `parse` forgetting one arm. The compiler cannot force this one at all —
# `parse` matches on strings — and the cost is a row written to the journal
# that cannot come back out of it: `skips_for_root` lists it as an unknown rule
# and every query grouping by rule misses it.
case_ "a rule written to the journal comes back out of it" \
  crates/mnema-index/src/journal.rs \
  's~            "malformed" => SkipRule::Malformed,\n~~' \
  '            "binary_tail" => SkipRule::BinaryTail,
            "encrypted" => SkipRule::Encrypted,' \
  mnema-index 'a_damaged_file_is_not_the_same_verdict_as_an_unread_one' --test journal

# --------------------------------------------------------------- the wire

# C9. The wire string landing on the wrong failure. This is the arm that makes
# the rules exist at all — `mnema-extract` may not depend on `mnema-index`
# (D26/D40), so a worker names its rule as a plain string and this `match` is
# the only translation. Folded onto `Unsupported`, a damaged file is journalled
# as "no reader for this format yet", which is a promise the product cannot
# keep and an answer that sends the user looking for the wrong fix.
case_ "the wire string \"malformed\" reaches the rule it names" \
  crates/mnema-pool/src/lib.rs \
  's~                    "malformed" => Failure::Malformed,~                    "malformed" => Failure::Unsupported,~' \
  '"malformed" => Failure::Unsupported,' \
  mnema-pool 'a_damaged_file_and_a_locked_one_cross_the_wire_apart' --test supervision

# C10. The mapping one step further on, where `Failure::BinaryTail` was
# measured being wrong with only another crate's test to catch it. Two failures
# sharing a rule are one rule to every query the journal can be asked.
case_ "two failures do not collapse onto one journal rule" \
  crates/mnema-pool/src/lib.rs \
  's~            Failure::Encrypted => SkipRule::Encrypted,~            Failure::Encrypted => SkipRule::Malformed,~' \
  'Failure::Encrypted => SkipRule::Malformed,' \
  mnema-pool 'every_failure_maps_onto_its_own_skip_rule' --test supervision
