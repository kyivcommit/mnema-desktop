# The agent API — `crates/mnema-core/src/agent_wire.rs` and what later tasks
# add on top of it. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/agent-access.sh
#
# Task 1 (the wire protocol): one case per test, naming the mutant that
# reddens it. Cases for the later tasks are appended below.

W=crates/mnema-core/src/agent_wire.rs
T='agent_wire::tests::'

case_ "agent_wire: a search without k must default to eight" \
  $W \
  's~unwrap_or\(DEFAULT_K\)~unwrap_or(1)~' \
  'unwrap_or(1)' \
  mnema-core "${T}a_search_without_k_gets_eight" --lib

case_ "agent_wire: k = 0 must be refused" \
  $W \
  's~\(1\.\.=MAX_K\)~(0..=MAX_K)~' \
  '(0..=MAX_K)' \
  mnema-core "${T}k_outside_one_to_twenty_is_a_bad_request" --lib

case_ "agent_wire: k = 21 must be refused" \
  $W \
  's~\(1\.\.=MAX_K\)~(1..=MAX_K + 1)~' \
  '(1..=MAX_K + 1)' \
  mnema-core "${T}k_outside_one_to_twenty_is_a_bad_request" --lib

case_ "agent_wire: context = 10 must be accepted" \
  $W \
  's~if context > MAX_CONTEXT~if context >= MAX_CONTEXT~' \
  'if context >= MAX_CONTEXT' \
  mnema-core "${T}context_outside_zero_to_ten_is_a_bad_request" --lib

case_ "agent_wire: context = 11 must be refused" \
  $W \
  's~if context > MAX_CONTEXT~if context > MAX_CONTEXT + 1~' \
  'if context > MAX_CONTEXT + 1' \
  mnema-core "${T}context_outside_zero_to_ten_is_a_bad_request" --lib

case_ "agent_wire: a missing context must default to two" \
  $W \
  's~DEFAULT_CONTEXT: u32 = 2~DEFAULT_CONTEXT: u32 = 3~' \
  'DEFAULT_CONTEXT: u32 = 3' \
  mnema-core "${T}context_outside_zero_to_ten_is_a_bad_request" --lib

case_ "agent_wire: a whitespace-only query must be refused" \
  $W \
  's~query\.trim\(\)\.is_empty\(\)~query.is_empty()~' \
  'query.is_empty()' \
  mnema-core "${T}a_blank_query_is_a_bad_request" --lib

case_ "agent_wire: version 2 must be refused" \
  $W \
  's~!= Some\(u64::from\(VERSION\)\)~> Some(u64::from(VERSION) + 1)~' \
  '> Some(u64::from(VERSION) + 1)' \
  mnema-core "${T}an_unknown_version_is_refused_as_version" --lib

case_ "agent_wire: a version refusal must say version, not bad_request" \
  $W \
  's~kind: ErrKind::Version,~kind: ErrKind::BadRequest,~' \
  'kind: ErrKind::BadRequest,' \
  mnema-core "${T}an_unknown_version_is_refused_as_version" --lib

case_ "agent_wire: an unknown op must be a bad_request" \
  $W \
  's~bad\(format!\("bad request: \{e\}"\)\)~WireError { kind: ErrKind::Internal, message: e.to_string() }~' \
  'WireError { kind: ErrKind::Internal, message: e.to_string() }' \
  mnema-core "${T}an_unknown_op_or_non_json_is_a_bad_request" --lib

case_ "agent_wire: non-JSON must be a bad_request" \
  $W \
  's~bad\(format!\("not JSON: \{e\}"\)\)~WireError { kind: ErrKind::Internal, message: e.to_string() }~' \
  'WireError { kind: ErrKind::Internal, message: e.to_string() }' \
  mnema-core "${T}an_unknown_op_or_non_json_is_a_bad_request" --lib

case_ "agent_wire: a line over MAX_LINE must be refused for its length" \
  $W \
  's~line\.len\(\) > MAX_LINE~line.len() > MAX_LINE * 2~' \
  'line.len() > MAX_LINE * 2' \
  mnema-core "${T}a_line_longer_than_max_line_is_a_bad_request" --lib

case_ "agent_wire: a line of exactly MAX_LINE must still be read" \
  $W \
  's~line\.len\(\) > MAX_LINE~line.len() >= MAX_LINE~' \
  'line.len() >= MAX_LINE' \
  mnema-core "${T}a_line_longer_than_max_line_is_a_bad_request" --lib

case_ "agent_wire: a passage id keeps its fields in order" \
  $W \
  's~self\.chunk_id, self\.document_id, self\.ord\)~self.document_id, self.chunk_id, self.ord)~' \
  'self.document_id, self.chunk_id, self.ord)' \
  mnema-core "${T}passage_id_round_trips_and_rejects_garbage" --lib

case_ "agent_wire: a passage id with a fourth part must be refused" \
  $W \
  's~parts\.next\(\)\.is_some\(\) \|\| ~~' \
  'if document_id.is_empty()' \
  mnema-core "${T}passage_id_round_trips_and_rejects_garbage" --lib

case_ "agent_wire: a passage id with an empty document must be refused" \
  $W \
  's~ \|\| document_id\.is_empty\(\)~~' \
  'if parts.next().is_some() {' \
  mnema-core "${T}passage_id_round_trips_and_rejects_garbage" --lib

case_ "agent_wire: a non-numeric chunk id must be refused" \
  $W \
  's~chunk_id: chunk_id\.parse\(\)\.ok\(\)\?~chunk_id: chunk_id.parse().unwrap_or(0)~' \
  'chunk_id.parse().unwrap_or(0)' \
  mnema-core "${T}passage_id_round_trips_and_rejects_garbage" --lib

case_ "agent_wire: a reply must not be pretty-printed over several lines" \
  $W \
  's~serde_json::to_string\(env\)~serde_json::to_string_pretty(env)~' \
  'serde_json::to_string_pretty(env)' \
  mnema-core "${T}a_multiline_cyrillic_text_stays_one_line" --lib

case_ "agent_wire: a reply ends in exactly one newline" \
  $W \
  's~s\.push\(.\\n.\)~s.push_str("\\n\\n")~' \
  's.push_str("\n\n")' \
  mnema-core "${T}a_multiline_cyrillic_text_stays_one_line" --lib

case_ "agent_wire: the ok key is spelled ok" \
  $W \
  's~\n    Ok\(Value\),~\n    #[serde(rename = "okay")]\n    Ok(Value),~' \
  '#[serde(rename = "okay")]' \
  mnema-core "${T}the_wire_shape_is_pinned" --lib

case_ "agent_wire: the error kind is spelled in snake_case" \
  $W \
  's~\n    StaleId,~\n    #[serde(rename = "staleId")]\n    StaleId,~' \
  '#[serde(rename = "staleId")]' \
  mnema-core "${T}the_wire_shape_is_pinned" --lib

# Task 2 (the &AppState seams).
V=src-tauri/src/tree.rs
TT='tree::tests::'

case_ "tree: freshness_of must stat the file, not answer a constant" \
  $V \
  's~Ok\(decide_freshness\(occupant\.as_ref\(\), document_id\)\)~Ok(Freshness::Current)~' \
  'Ok(Freshness::Current)' \
  mnema-desktop "${TT}freshness_of_agrees_with_open_document" --lib

case_ "tree: freshness_of must read the cited occupant" \
  $V \
  's~Ok\(decide_freshness\(occupant\.as_ref\(\), document_id\)\)~Ok(decide_freshness(None, document_id))~' \
  'decide_freshness(None, document_id)' \
  mnema-desktop "${TT}freshness_of_agrees_with_open_document" --lib
