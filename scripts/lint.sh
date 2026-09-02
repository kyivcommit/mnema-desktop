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
# **`ci.yml` deliberately does NOT use this**, and that is not an oversight.
# Every CI job starts from a bare checkout with a cold cache, so there is no
# second build to save there — the win is entirely local, in a tree that is
# already built. Changing the workflow would also break a mutation case that
# quotes its `- run: cargo clippy` line (`scripts/mutations/branch-review.sh`)
# and the test that case names, for no measured gain.
#
# ⚠️ Use this instead of calling `cargo clippy` directly. A bare `cargo clippy`
# still works and still lints correctly — it just silently throws away the test
# profile's artifacts, and the next `cargo test` pays for it.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/clippy"
exec cargo clippy --workspace --all-targets "$@" -- -D warnings
