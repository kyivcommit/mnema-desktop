# Mutation cases for Task 6: crates/mnema-extract/src/zip_part.rs's cap on
# one archive member. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-6.sh
#
# `read_member` exists for one reason: a zip member's declared uncompressed
# size is written by whoever crafted the archive, and the `zip` crate does
# not check a Deflated member's real output against it — decoding runs until
# the compressed stream's own end marker, however much that turns out to be.
# Every case here breaks that guarantee in a way nothing crashes on: a
# silently truncated read that returns Ok with the wrong bytes, a comparison
# that never fires, or the wrong one of the three error variants — each of
# which reads as a perfectly plausible `ZipPartError` to a caller that only
# checks `is_err()`.

# C1. The property the whole module exists for: the cap has to be decided
# from what the stream actually produced, never from `member.size()`, which
# is the central directory's declared (and forgeable) number. This is the
# same mistake the server's `_read_zip_member_capped` comment names directly
# (app/textdoc/office.py:45) — trusting `ZipInfo.file_size` instead of a
# capped read.
case_ "the cap is decided from the stream, not the member's declared size" \
  crates/mnema-extract/src/zip_part.rs \
  's~    let member = archive\.by_name\(name\)\.map_err\(\|e\| match e \{\n        zip::result::ZipError::FileNotFound => ZipPartError::Missing,\n        _ => ZipPartError::Malformed,\n    \}\)\?;\n\n    let mut out = Vec::new\(\);\n    member\n        \.take\(cap as u64 \+ 1\)\n        \.read_to_end\(&mut out\)\n        \.map_err\(\|_\| ZipPartError::Malformed\)\?;\n    if out\.len\(\) > cap \{~    let mut member = archive.by_name(name).map_err(|e| match e {\n        zip::result::ZipError::FileNotFound => ZipPartError::Missing,\n        _ => ZipPartError::Malformed,\n    })?;\n\n    if member.size() > cap as u64 {\n        return Err(ZipPartError::TooLarge);\n    }\n    let mut out = Vec::new();\n    member\n        .read_to_end(&mut out)\n        .map_err(|_| ZipPartError::Malformed)?;\n    if false {~' \
  'if member.size() > cap as u64 {' \
  mnema-extract 'a_declared_size_does_not_decide_anything' --test zip_part

# C2. The `+ 1` on the capped read is what makes "more than cap came out"
# observable at all: without it, `take` clamps the read to exactly `cap`
# bytes, `out.len()` can never exceed `cap`, and an oversized member is
# silently truncated and returned as `Ok` rather than refused.
case_ "the capped read asks for one byte past the cap, not exactly the cap" \
  crates/mnema-extract/src/zip_part.rs \
  's~\.take\(cap as u64 \+ 1\)~.take(cap as u64)~' \
  '.take(cap as u64)' \
  mnema-extract 'a_declared_size_does_not_decide_anything' --test zip_part

# C3. The same disabling, reached from the comparison instead of the read
# limit: `out.len()` can be at most `cap + 1` (C2's read), so `> cap + 1`
# never fires no matter how far over the cap the real member is.
case_ "the length check compares against the cap, not the cap plus one" \
  crates/mnema-extract/src/zip_part.rs \
  's~if out\.len\(\) > cap \{~if out.len() > cap + 1 {~' \
  'if out.len() > cap + 1 {' \
  mnema-extract 'a_declared_size_does_not_decide_anything' --test zip_part

# C4. The other edge of the same comparison: `>=` refuses a member sized
# exactly at the cap, which the module doc promises comes back whole. Nothing
# in C1-C3 exercises this boundary — they are all far enough over the cap
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
