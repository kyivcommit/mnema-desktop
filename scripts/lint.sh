#!/usr/bin/env bash
#
# `cargo clippy --workspace --all-targets -- -D warnings`, into a target
# directory of its own.
#
# 🔴 **Why this script exists, measured rather than assumed.** `cargo clippy`
# and `cargo test` compile the same crates with different rustc invocations, so
# sharing one `CARGO_TARGET_DIR` makes each of them invalidate what the other
# just built. The local gate runs both, one after the other, and so was
# compiling the whole workspace **twice** every time.
#
# Measured on an Apple M2 Max, 2026-09-02, on a tree already fully built by
# `cargo test --workspace --no-run` immediately beforehand:
#
#   shared target dir      clippy 172 s   then  cargo test --workspace  1783 s
#   separate target dirs   clippy   2 s   then  cargo test --workspace   492 s
#
# The whole Rust half of the gate went from about 33 minutes to about 8. The
# `cargo test` figure still contains ~180 s of tests actually running: the
# workspace has 74 integration test files, and what the gate had been paying for
# was linking them a second time, not running them.
#
# `target/clippy` sits inside `target/`, which `.gitignore` already covers, so
# this costs one more build cache on disk and nothing in the repository.
#
# ⚠️ **`ci.yml` does not use this yet, and the reason first written here was
# WRONG.** It said every CI job starts from a bare checkout with a cold cache,
# so there was no second build to save. `.github/workflows/ci.yml` runs
# `Swatinem/rust-cache`, which restores `target/` between runs — so the `check`
# job pays the same double build this script exists to remove. Independent
# review caught the false premise; it is left here rather than quietly deleted
# because a reason that turned out to be wrong is the thing a later session
# would otherwise re-derive.
#
# What is true is only the cost: the workflow's `- run: cargo clippy` line is
# quoted by a mutation case (`scripts/mutations/branch-review.sh`) and asserted
# by the test that case names, so moving CI onto this script means moving those
# too. Whether that trade is worth it is an open decision, not a settled no.
#
# ⚠️ Use this instead of calling `cargo clippy` directly. A bare `cargo clippy`
# still works and still lints correctly — it just silently throws away the test
# profile's artifacts, and the next `cargo test` pays for it.
set -euo pipefail
cd "$(dirname "$0")/.."
# The two checks that read comments rather than code, first because they cost
# about a second and compile nothing: an obligation written into a comment
# (`check-booked.sh`, its own self-test first — it writes failures to stderr,
# so silencing its stdout hides only the success line) and a citation whose
# line is past the end of its file (`check-citations.sh`). `ci.yml` runs the
# same in its `mutations` job.
scripts/check-booked.sh --self-test > /dev/null
scripts/check-booked.sh
# The citation sweep prints every citation it checked (2 700 lines) and its
# problem list last; only that list is worth a screen, and only on failure.
# On failure the list is printed from its own heading to the end, whatever
# its length; should that heading ever be reworded, the tail of the output is
# printed instead of nothing.
# The file is removed by hand before `exec`, which replaces this shell and so
# never runs an EXIT trap (review of PR #28, fix round 1, Minor 2).
cit="$(mktemp)"; trap 'rm -f "$cit"' EXIT
scripts/check-citations.sh > "$cit" \
  || { awk '/^--- [0-9]+ mechanical problem/ {p = 1} p' "$cit" | grep . || tail -n 20 "$cit"; exit 1; }
rm -f "$cit"
# Formatting, before clippy: it compiles nothing and `ci.yml`'s `check` job
# runs the same line first. PR #29 reached CI with eight rustfmt differences in
# a file every local gate had passed, because nothing here asked.
cargo fmt --all -- --check
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/clippy"
exec cargo clippy --workspace --all-targets "$@" -- -D warnings
