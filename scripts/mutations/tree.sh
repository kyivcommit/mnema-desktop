# Mutation cases for the list_tree read functions (PR 4). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/tree.sh
#
# Three queries back the left card and the recents list (spec §7, §12): which
# root a file lives under, which of a root's files are actually searchable,
# and which documents most recently finished indexing. Each rule below is
# silent when broken — a stale-sorted list, or a pending/failed document
# quietly counted as citable, still returns *something*, so each case turns a
# named test red rather than trusting the query to fail loudly.
#
# Every mutation is checked twice: once against the `mnema-index` unit test
# that calls the function directly (`crates/mnema-index/tests/tree.rs`), and
# once against the src-tauri integration test that reaches the same query
# through the `list_tree` command and the wire format
# (`list_tree_enumerates_roots_indexed_files_and_recents`,
# src-tauri/tests/commands.rs) — the second case per mutation is the "does
# this actually reach the command a person calls" half.
#
# ── Not covered here, and why ─────────────────────────────────────────────
#
# `indexed_files_under_root`'s `ORDER BY p.relative_path` has no case.
# `path`'s primary key is `(watched_root_id, relative_path)`, so a query
# scoped to one root is already walking that index in `relative_path` order
# before any ORDER BY is applied — removing the clause is therefore a
# likely-equivalent mutant (STILL GREEN for a reason that says nothing about
# the code), not a case that proves the sort is load-bearing.
# `recent_indexed_documents`'s own `ORDER BY s.updated_at DESC, d.id` has no
# such shortcut — it orders by a column no primary key walks in order — which is
# exactly why that one gets a case below and this one does not.

# ─────────────────────────────────────────────────────────────────────────────
# recent_indexed_documents: newest-first order

case_ "recents: newest-first flips to oldest-first" \
  crates/mnema-index/src/write.rs \
  's~              ORDER BY s\.updated_at DESC, d\.id~              ORDER BY s.updated_at ASC, d.id~' \
  'ORDER BY s.updated_at ASC, d.id' \
  mnema-index 'recent_indexed_documents_orders_by_completion_desc_indexed_only' --test tree

case_ "recents: newest-first flips to oldest-first, end to end" \
  crates/mnema-index/src/write.rs \
  's~              ORDER BY s\.updated_at DESC, d\.id~              ORDER BY s.updated_at ASC, d.id~' \
  'ORDER BY s.updated_at ASC, d.id' \
  mnema-desktop 'list_tree_enumerates_roots_indexed_files_and_recents' --test commands

# ─────────────────────────────────────────────────────────────────────────────
# indexed_files_under_root: only status = 'indexed' is a file

case_ "indexed_files_under_root: the indexed-only filter is dropped" \
  crates/mnema-index/src/write.rs \
  "s~              WHERE p\.watched_root_id = \?1 AND d\.status = 'indexed'~              WHERE p.watched_root_id = ?1~" \
  'WHERE p.watched_root_id = ?1
              ORDER BY p.relative_path",' \
  mnema-index 'indexed_files_under_root_lists_only_indexed_paths_sorted' --test tree

case_ "indexed_files_under_root: the indexed-only filter is dropped, end to end" \
  crates/mnema-index/src/write.rs \
  "s~              WHERE p\.watched_root_id = \?1 AND d\.status = 'indexed'~              WHERE p.watched_root_id = ?1~" \
  'WHERE p.watched_root_id = ?1
              ORDER BY p.relative_path",' \
  mnema-desktop 'list_tree_enumerates_roots_indexed_files_and_recents' --test commands

# ─────────────────────────────────────────────────────────────────────────────
# recent_indexed_documents: only status = 'indexed' is recent

case_ "recent_indexed_documents: the indexed-only filter is dropped" \
  crates/mnema-index/src/write.rs \
  "s~              WHERE d\.status = 'indexed'\n~~" \
  "               JOIN ingest_stage s ON s.content_hash = d.id AND s.stage = 'chunk' AND s.status = 'done'
              GROUP BY d.id" \
  mnema-index 'recent_indexed_documents_orders_by_completion_desc_indexed_only' --test tree

case_ "recent_indexed_documents: the indexed-only filter is dropped, end to end" \
  crates/mnema-index/src/write.rs \
  "s~              WHERE d\.status = 'indexed'\n~~" \
  "               JOIN ingest_stage s ON s.content_hash = d.id AND s.stage = 'chunk' AND s.status = 'done'
              GROUP BY d.id" \
  mnema-desktop 'list_tree_enumerates_roots_indexed_files_and_recents' --test commands
