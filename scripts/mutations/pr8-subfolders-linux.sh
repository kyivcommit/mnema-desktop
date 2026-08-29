# Linux-only sibling of `pr8-subfolders.sh`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-subfolders-linux.sh
#
# One case, and it lives here rather than in `pr8-subfolders.sh` because the
# test it names — `a_directory_whose_name_is_not_utf8_is_counted_and_never_
# named_lossily` (`src-tauri/tests/commands.rs`) — is
# `#[cfg(target_os = "linux")]`. The property under test genuinely cannot exist
# on the other leg: APFS refuses to create a directory whose name is not valid
# UTF-8 at all, `EILSEQ` / "Illegal byte sequence" (measured on macOS 26.6.2,
# twice — once as the failing fixture and once as a standalone `rustc` probe),
# so no macOS run can produce a non-zero `unnameable` through the IPC.
# `mutation-check.sh`'s baseline pass runs each named test once with `--exact`
# and requires `test result: ok. 1 passed`; off Linux this test does not exist
# (`#[cfg]`, not `#[ignore]`), the baseline reads `0 passed`, and the whole FILE
# exits 1 before any mutation runs — for every case in it, not only this one.
# Splitting is what keeps that from taking `pr8-subfolders.sh` down with it, the
# same shape and the same reason as `pr8-exclusions-macos.sh`.
#
# ⚠️ **What this file exists to pin, and why the `--lib` cases do not cover
# it.** `read_subfolders` is a pure function and cases in the sibling file pin
# its `unnameable` arithmetic on every unix. What they cannot reach is the
# WIRING — that the number the pure function computed is still the number the
# command answers with. Fix round 1 measured that gap through the harness: a
# mutant zeroing the count between the call and the reply came back
# `*** STILL GREEN ***`, because on macOS no IPC test can produce a non-zero
# count and on Linux the test that would catch it was named by no case. This is
# the seam limit `bridge::entry_named`'s doc records in the same words — the
# function is pinned, its call site is not — closed here rather than disclosed,
# because unlike that one it IS reachable, on one leg.
#
# ⚠️ **CI does not run this file, and does not run its sibling either.** The
# `mutations` job (`.github/workflows/ci.yml:379-382`) runs exactly
# `source.sh` and `embedding.sh`; every other case file is swept only by
# `scripts/mutation-staleness.sh`, which proves a case still APPLIES and never
# that it still kills. Whoever adds this cycle's files to that list should add
# both — this one is the leg that can run it, since the job is `ubuntu-24.04`.

# The count is computed correctly and then thrown away on the way out. The
# folder answers with the entries it could name and nothing saying that others
# exist, which is exactly the "a folder looks emptier than it is" defect the
# field was added for.
case_ "the unnameable count the pure function computed is the one the command answers with" \
  src-tauri/src/tree.rs \
  's~    Ok\(read_subfolders\(\n        &root_canonical,\n        &entries,\n        &relative_path,\n        &prefixes,\n    \)\)~    let mut listing = read_subfolders(\&root_canonical, \&entries, \&relative_path, \&prefixes);\n    listing.unnameable = 0; /* mutant: the count is dropped on the way to the wire */\n    Ok(listing)~' \
  '/* mutant: the count is dropped on the way to the wire */' \
  mnema-desktop 'a_directory_whose_name_is_not_utf8_is_counted_and_never_named_lossily' --test commands
