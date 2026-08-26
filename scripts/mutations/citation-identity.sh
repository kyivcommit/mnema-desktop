# Mutation cases for the `rootId` seam on `Hit`/`AskCitation` (PR 6a). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/citation-identity.sh
#
# `retrieve` (`bridge.rs`) builds one `Hit` per fused chunk and echoes
# `Citation::root_id` onto it; `ask` (`bridge.rs`) then echoes `Hit::root_id`
# again onto the `AskCitation` it resolves each cited anchor to. Two separate
# struct-literal assignments, two separate places a silent `None` can be
# smuggled in — and before this file, nothing asserted `rootId` on either
# wire shape, so replacing either assignment with a hardcoded `None` left the
# whole suite green (owner review, F1 on PR #23).
#
# Why this matters here specifically: `rootId` is what this PR trades a
# too-cautious `NoPath` verdict for (`tree.rs`'s `cited_occupant`) — a
# citation whose root goes quietly missing falls back to the old
# ambiguity-scan behaviour for two watched roots sharing a relative path,
# and no gate would show it. Each assertion below is deliberately positive
# (`rootId == Some(root)`), not merely "is not null": a bare non-null check
# is satisfied by any wrong root, and a bare `None` check is satisfied by
# the fixture's own zero case.
#
# ⚠️ Both patterns below were counted against the tree at b7c4eb0 and hit
# EXACTLY once each, checked with `grep -c` before writing the cases.

case_ "retrieve: Hit::root_id is dropped to None" \
  src-tauri/src/bridge.rs \
  's~root_id: c\.root_id,~root_id: None,~' \
  'root_id: None,' \
  mnema-desktop 'search_returns_citations_not_ids' --test commands

case_ "ask: AskCitation::root_id is dropped to None" \
  src-tauri/src/bridge.rs \
  's~root_id: h\.root_id,~root_id: None,~' \
  'root_id: None,' \
  mnema-desktop 'ask_maps_each_anchor_to_the_right_citation_and_generates' --test commands
