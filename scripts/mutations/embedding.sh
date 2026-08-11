# Mutation cases for the indexing-and-embedding cycle.
#
# Each `case` breaks exactly one thing and names the test that must go red.
# Cases are added by the task that adds the test they protect, so this file
# grows through the cycle rather than being written once at the end.

case_ "write: a rebuild takes its document's vectors with it (D88)" \
  crates/mnema-index/src/write.rs \
  's{        crate::space::delete_vectors_for_document_in\(tx, id\)\?;\n}{}' \
  'table is the target of no foreign key, so nothing else reaches it.
        tx.execute("DELETE FROM page' \
  mnema-index 'clearing_a_document_takes_its_vectors' --test citation
