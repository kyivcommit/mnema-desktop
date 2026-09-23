# Debt PR E — Rust guards whose mutant no test killed, and the tests that do
# now. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/debt-rust-mutants.sh
#
# Rust-only: no vitest case, so this file's matrix leg carries no `node:`.
# Re-derive rather than trust that sentence:
#
#   grep -c 'runner=vitest' scripts/mutations/debt-rust-mutants.sh
#
# ⚠️ The pool case's test is `#[cfg(unix)]`; the one CI leg that runs this file
# is ubuntu, and a local run is macOS or Linux.

# A worker the pool has only just started, and that cannot be written to, means
# the environment is broken — not that a worker aged out. Retrying it spawns a
# second process and ends in the same variant, so the test asserts the spawn
# count and the error kind rather than the variant.
case_ "pool: a freshly spawned worker that cannot be written to is not retried" \
  crates/mnema-pool/src/lib.rs \
  's{                    if fresh \{}{                    if false \{ // mutant: a fresh worker is retried}' \
  'if false { // mutant: a fresh worker is retried' \
  mnema-pool 'a_fresh_worker_that_cannot_be_handed_its_request_is_not_retried' --test unreachable
