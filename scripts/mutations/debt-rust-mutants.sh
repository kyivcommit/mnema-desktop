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

# The DROP of a space's vector table must share the DELETE's transaction. Run
# ahead of it, a DELETE that then fails leaves a row naming a table that is gone
# — and the model-change loop, which reads an Err as "nothing was retired",
# keeps the old model saved over it.
case_ "space: drop_space drops the vector table inside the transaction that deletes the row" \
  crates/mnema-index/src/space.rs \
  's{        let tx = Transaction::new_unchecked\(self\.conn\(\), TransactionBehavior::Immediate\)\?;\n        // DROP takes the four shadow tables with it, so the id becomes reusable\.\n        tx\.execute_batch\(&format!\("DROP TABLE IF EXISTS \{table\};"\)\)\?;\n}{        self.conn().execute_batch(&format!("DROP TABLE IF EXISTS {table};"))?; // mutant: the drop runs outside the transaction\n        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;\n}' \
  '// mutant: the drop runs outside the transaction' \
  mnema-index 'a_drop_that_fails_after_the_table_went_leaves_the_space_whole' --test space
