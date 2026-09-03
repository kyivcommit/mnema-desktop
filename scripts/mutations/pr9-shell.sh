# The preferences file — `src-tauri/src/prefs.rs`, the one place the app reads
# and writes `prefs.json` from PR 9 onward. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9-shell.sh
#
# What is here, and it is two different kinds of claim. Three cases pin
# behaviour that already existed while `locale.rs` owned the file and that the
# extraction had to carry across unchanged — the merge, and the temp-file-plus-
# rename. Their oracle is deliberately the locale's OWN tests, unedited: a
# refactor whose evidence is a test the refactor also wrote has no evidence.
# The rest pin what the extraction ADDED and nothing before it had: the
# process-wide lock, and the backup of a malformed file.
#
# No count of the cases here, deliberately — `pr8-exclusions.sh` explains what
# that number costs when it goes stale. Re-derive:
#
#   grep -c '^case_ ' scripts/mutations/pr9-shell.sh                     # cases
#   grep -oE "mnema-desktop '[^']+'" scripts/mutations/pr9-shell.sh | sort -u
#
# ⚠️ Every case here names a `--lib` test: `prefs.rs` and `locale.rs` both keep
# their tests in their own `mod tests`, so the names carry the `prefs::tests::`
# / `locale::tests::` prefix. A name without it selects nothing and the harness
# reports a BASELINE FAILURE for the whole file rather than a result.
#
# ⚠️ One case here (`a_write_into_a_read_only_directory…`'s sibling, the
# temp-plus-rename one) names a test that is `#[cfg(unix)]`. Both of this
# repository's CI legs are unix; a Windows leg would fail this file's baseline
# whole, the way `pr8-exclusions-macos.sh` had to be split off for.

# The merge itself, removed: `write_key` builds a fresh object instead of the
# one already in the file. Every key anybody else wrote is then dropped by the
# next write of any single key — the hotkey erases the locale and the locale
# erases the hotkey — while the file stays valid JSON holding exactly what was
# last written, which is why nothing but a two-key fixture can see it.
case_ "a key already in the file must survive a write of a different key" \
  src-tauri/src/prefs.rs \
  's~        Some\(Ok\(object\)\) => object,~        Some(Ok(_)) => serde_json::Map::new(),~' \
  'Some(Ok(_)) => serde_json::Map::new(),' \
  mnema-desktop 'prefs::tests::a_new_key_joins_the_one_already_in_the_file' --lib

# The same mutation, judged by the oracle the refactor did not write. This is
# the case that says the extraction is behaviour-preserving rather than merely
# self-consistent: `write_preserves_unknown_fields` was written against the
# code that used to live in `locale.rs`, has not been edited, and still dies
# when the merge it was written for goes away.
case_ "the locale's own forward-safety test must still see the merge disappear" \
  src-tauri/src/prefs.rs \
  's~        Some\(Ok\(object\)\) => object,~        Some(Ok(_)) => serde_json::Map::new(),~' \
  'Some(Ok(_)) => serde_json::Map::new(),' \
  mnema-desktop 'locale::tests::write_preserves_unknown_fields' --lib

# Temp-file-plus-rename collapsed to a write straight at the target. A write
# that fails part-way then leaves a truncated file where the preferences were:
# in a read-only directory the sibling temp file cannot be created at all, but
# the existing file is still writable through its own mode, so the mutant
# truncates it and reports success. Again judged by the locale's own unedited
# test, which is what that test was written for.
case_ "a failed write must not be able to destroy what was already persisted" \
  src-tauri/src/prefs.rs \
  's~    let tmp = path\.with_extension\(temp_extension\(\)\);\n    std::fs::write\(&tmp, &body\)\?;\n(?:    //[^\n]*\n)+    std::fs::rename\(&tmp, &path\)~    std::fs::write(\&path, \&body)~' \
  'std::fs::write(&path, &body)' \
  mnema-desktop 'locale::tests::failed_write_keeps_the_previous_choice' --lib

# `read_all`'s promise that nothing it meets is an error. A file that is not
# JSON, or is JSON of the wrong shape, then takes the process down — and the
# first caller of this function is start-up, before there is anywhere to report
# anything to. The named test's first assertion is what sees it.
case_ "a malformed file must read as no preferences, not as a panic" \
  src-tauri/src/prefs.rs \
  's~    serde_json::from_slice\(&bytes\)\.unwrap_or_default\(\)~    serde_json::from_slice(\&bytes).unwrap()~' \
  'serde_json::from_slice(&bytes).unwrap()' \
  mnema-desktop 'prefs::tests::a_malformed_file_is_kept_byte_for_byte_beside_the_one_that_replaces_it' --lib

# 🔴 The lock, gone. Everything still compiles, every other test in this file
# stays green, and two writers that interleave each write an object computed
# before the other's key was added — so the loser's key is simply not in the
# file, with nothing to say it ever was. This is the one fixture in the task
# that can see it: the named test parks the first writer INSIDE the critical
# section and watches the second one walk past. A "two threads, N writes each,
# then check nothing was lost" test would pass against this mutant whenever the
# interleaving happened not to occur.
case_ "the read-modify-write must be serialised, not merely usually uncontended" \
  src-tauri/src/prefs.rs \
  's~    let _guard = PREFS_LOCK\.lock\(\)\.unwrap_or_else\(\|e\| e\.into_inner\(\)\);~    let _guard = (); // mutant: the critical section is not serialised~' \
  'let _guard = (); // mutant: the critical section is not serialised' \
  mnema-desktop 'prefs::tests::a_second_writer_waits_for_the_first_to_leave_the_critical_section' --lib

# 🔴 The backup, skipped: a file that does not parse is simply overwritten. The
# malformed file may be the only copy of something a person hand-edited, or
# something a newer version wrote in a shape this build cannot read, and after
# this mutant it is gone with nothing recording that it existed. Nothing above
# can see this — the file it replaces it with is byte-identical either way; it
# is the `.corrupt` sibling's contents, asserted byte-for-byte, that dies.
case_ "a malformed file must be kept before it is replaced, not destroyed" \
  src-tauri/src/prefs.rs \
  's~        Some\(Err\(_\)\) => \{\n            std::fs::rename\(&path, path\.with_extension\("json\.corrupt"\)\)\?;\n            serde_json::Map::new\(\)\n        \}~        Some(Err(_)) => serde_json::Map::new(), // mutant: the malformed file is overwritten~' \
  'Some(Err(_)) => serde_json::Map::new(), // mutant: the malformed file is overwritten' \
  mnema-desktop 'prefs::tests::a_malformed_file_is_kept_byte_for_byte_beside_the_one_that_replaces_it' --lib

# ── Task review, concern 1 ────────────────────────────────────────────────────
#
# 🔴 The read reverted from bytes to text, which is exactly what the body moved
# out of `locale.rs` used to do. `read_to_string` fails on a file that is not
# valid UTF-8, so `existing` is `None` and the file lands on the same arm as a
# file that DOES NOT EXIST: no backup, and the only copy of whatever wrote it is
# gone. Nothing else in this file can see it — measured by running the whole
# `--lib prefs` filter against this exact reversion, where thirteen tests stay
# green and only the one named below dies:
#
#   a file that is not text was replaced with no backup beside it
#
# It is the arm, not the parse, that this case is about: both readings answer an
# empty map from `read_all` and both write the same replacement file.
case_ "a preferences file that is not valid UTF-8 must be malformed, not absent" \
  src-tauri/src/prefs.rs \
  's~    let existing = std::fs::read\(&path\)\.ok\(\);\n    let parsed = existing\n        \.as_deref\(\)\n        \.map\(serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>\);~    let existing = std::fs::read_to_string(\&path).ok(); // mutant: unreadable text, not malformed bytes\n    let parsed = existing\n        .as_deref()\n        .map(serde_json::from_str::<serde_json::Map<String, serde_json::Value>>);~' \
  'let existing = std::fs::read_to_string(&path).ok(); // mutant: unreadable text, not malformed bytes' \
  mnema-desktop 'prefs::tests::a_file_of_invalid_utf8_is_backed_up_rather_than_overwritten' --lib
