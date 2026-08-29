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
# function is pinned, its call site is not — **named here rather than only
# disclosed, and measured on the Linux leg rather than on the machine that
# wrote it** (fix round 2, N3: the sentence that stood here said "closed", and
# nothing had ever observed the case kill).
#
# ⚠️ **CI runs this file, and that is the whole point of it.** The `mutations`
# job is `runs-on: ubuntu-24.04`, so this is the one leg where the case can
# execute at all; fix round 2 named it there, together with `pr8-exclusions.sh`
# and `pr8-subfolders.sh`. Its macOS-only counterpart `pr8-exclusions-macos.sh`
# still cannot be: there is no macOS mutations leg, and adding one is a
# decision about runner minutes rather than about this file. Until then that
# one file remains local evidence — `mutation-staleness.sh` proves its case
# still applies, and nothing proves it still kills.

# The count is computed correctly and then thrown away on the way out. The
# folder answers with the entries it could name and nothing saying that others
# exist, which is exactly the "a folder looks emptier than it is" defect the
# field was added for.
case_ "the unnameable count the pure function computed is the one the command answers with" \
  src-tauri/src/tree.rs \
  's~    Ok\(read_subfolders\(\n        &layers,\n        &entries,\n        &relative_path,\n        &prefixes,\n    \)\)~    let mut listing = read_subfolders(\&layers, \&entries, \&relative_path, \&prefixes);\n    listing.unnameable = 0; /* mutant: the count is dropped on the way to the wire */\n    Ok(listing)~' \
  '/* mutant: the count is dropped on the way to the wire */' \
  mnema-desktop 'a_directory_whose_name_is_not_utf8_is_counted_and_never_named_lossily' --test commands
