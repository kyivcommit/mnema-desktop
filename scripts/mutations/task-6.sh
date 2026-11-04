# Mutation cases for Task 6: crates/mnema-extract/src/zip_part.rs's cap on
# one archive member. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-6.sh
#
# `read_member` exists for one reason: a zip member's declared uncompressed
# size is written by whoever crafted the archive, and up to `zip` 8 the crate
# did not check a Deflated member's real output against it. `zip` 9 does (the
# stream errors past the declared size), which leaves the cap one job: the
# bomb that declares its true size. Every case here breaks that guarantee in a way nothing crashes on: a
# silently truncated read that returns Ok with the wrong bytes, a comparison
# that never fires, or the wrong one of the three error variants — each of
# which reads as a perfectly plausible `ZipPartError` to a caller that only
# checks `is_err()`.

# C1, retired. It swapped the stream cap for a check on `member.size()`.
# Under `zip` 9 that mutant is equivalent: the declared size is enforced by the
# stream itself, so a forged-small member is `Malformed` either way and an
# honest one is refused by its declared size. What still guards the property
# is `a_declared_size_forged_small_is_refused`, which goes red if `zip` stops
# enforcing (the member would inflate to the cap and answer `TooLarge`).

# C2. The `+ 1` on the capped read is what makes "more than cap came out"
# observable at all: without it, `take` clamps the read to exactly `cap`
# bytes, `out.len()` can never exceed `cap`, and an oversized member is
# silently truncated and returned as `Ok` rather than refused.
case_ "the capped read asks for one byte past the cap, not exactly the cap" \
  crates/mnema-extract/src/zip_part.rs \
  's~\.take\(\(cap as u64\)\.saturating_add\(1\)\)~.take(cap as u64)~' \
  '.take(cap as u64)' \
  mnema-extract 'a_member_honestly_past_the_cap_is_too_large' --test zip_part

# C3. The same disabling, reached from the comparison instead of the read
# limit: `out.len()` can be at most `cap + 1` (C2's read), so `> cap + 1`
# never fires no matter how far over the cap the real member is.
case_ "the length check compares against the cap, not the cap plus one" \
  crates/mnema-extract/src/zip_part.rs \
  's~if out\.len\(\) > cap \{~if out.len() > cap + 1 {~' \
  'if out.len() > cap + 1 {' \
  mnema-extract 'a_member_honestly_past_the_cap_is_too_large' --test zip_part

# C4. The other edge of the same comparison: `>=` refuses a member sized
# exactly at the cap, which the module doc promises comes back whole. Nothing
# in C2-C3 exercises this boundary — they are all far enough over the cap
# that `>` and `>=` agree.
case_ "a member exactly at the cap is not refused" \
  crates/mnema-extract/src/zip_part.rs \
  's~if out\.len\(\) > cap \{~if out.len() >= cap {~' \
  'if out.len() >= cap {' \
  mnema-extract 'a_member_exactly_at_the_cap_comes_back_whole' --test zip_part

# C5. `Missing` and `Malformed` swapped at the one place that decides between
# them. A caller cannot tell "this archive is fine but does not have the part
# I asked for" from "this archive is broken" — the distinction Task 7's skip
# rules, and any docx/xlsx/epub reader built on this, are meant to carry.
case_ "a member not found in a valid archive is Missing, not Malformed" \
  crates/mnema-extract/src/zip_part.rs \
  's~        zip::result::ZipError::FileNotFound => ZipPartError::Missing,\n        _ => ZipPartError::Malformed,~        zip::result::ZipError::FileNotFound => ZipPartError::Malformed,\n        _ => ZipPartError::Missing,~' \
  'zip::result::ZipError::FileNotFound => ZipPartError::Malformed,' \
  mnema-extract 'a_missing_member_is_reported_as_missing_not_malformed' --test zip_part

# C6. The other error path into the same two variants: bytes that do not
# parse as a zip at all must be Malformed, not Missing — the archive itself
# is the thing that is broken, not a lookup inside it.
case_ "bytes that are not a zip archive at all are Malformed, not Missing" \
  crates/mnema-extract/src/zip_part.rs \
  's~zip::ZipArchive::new\(Cursor::new\(bytes\)\)\.map_err\(\|_\| ZipPartError::Malformed\)~zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| ZipPartError::Missing)~' \
  '.map_err(|_| ZipPartError::Missing)' \
  mnema-extract 'bytes_that_are_not_a_zip_at_all_are_malformed' --test zip_part
