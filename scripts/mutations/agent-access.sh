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

# Task 3 (the handlers): src-tauri/src/agent.rs, tested by tests/agent.rs.
A=src-tauri/src/agent.rs

case_ "agent: search must pass k on, not a fixed twenty" \
  $A \
  's~retrieve\(state, query, arms, i64::from\(k\), embed\)~retrieve(state, query, arms, 20, embed)~' \
  'arms, 20, embed' \
  mnema-desktop 'search_with_k_one_answers_one_passage' --test agent

case_ "agent: search must name the path the hit has" \
  $A \
  's~"path": h\.relative_path,~"path": Value::Null,~' \
  '"path": Value::Null,' \
  mnema-desktop 'search_answers_verbatim_passages_with_their_place' --test agent

case_ "agent: the absolute path must end in the relative one" \
  $A \
  's~Path::new\(root\)\.join\(relative\)~Path::new(root).join("")~' \
  'join("")' \
  mnema-desktop 'search_answers_verbatim_passages_with_their_place' --test agent

case_ "agent: an empty answer must say empty" \
  $A \
  's~"empty": passages\.is_empty\(\)~"empty": false~' \
  '"empty": false' \
  mnema-desktop 'search_on_an_empty_index_is_empty_not_an_error' --test agent

case_ "agent: search must stat the file, not claim current" \
  $A \
  's~"freshness": freshness_kind\(&freshness\)\?,(\s+\}\)\)\s+\}\))~"freshness": "current",$1~s' \
  '"freshness": "current",' \
  mnema-desktop 'a_file_changed_after_indexing_is_marked_in_search_and_passage' --test agent

case_ "agent: passage must stat the file, not claim current" \
  $A \
  's~"freshness": freshness_kind\(&freshness\)\?,(\s+"blocks")~"freshness": "current",$1~s' \
  '"freshness": "current",' \
  mnema-desktop 'a_file_changed_after_indexing_is_marked_in_search_and_passage' --test agent

case_ "agent: an id from before a reindex must be stale" \
  $A \
  's~citation\.document_id != id\.document_id \|\| citation\.ord != id\.ord~false~' \
  'if false {' \
  mnema-desktop 'passage_after_reindex_is_stale_not_another_place' --test agent

case_ "agent: an unknown chunk must be not_found" \
  $A \
  's~err\(ErrKind::NotFound, "no such passage"\)~err(ErrKind::Internal, "no such passage")~' \
  'ErrKind::Internal, "no such passage"' \
  mnema-desktop 'passage_of_an_unknown_chunk_is_not_found' --test agent

case_ "agent: the focus blocks must be marked" \
  $A \
  's~focus_ids\.contains\(&b\.block_id\)~false~' \
  '"focus": false' \
  mnema-desktop 'passage_of_a_fresh_id_gives_that_text' --test agent

case_ "agent: context 0 must not widen the window upward" \
  $A \
  's~first\.saturating_sub\(context\)~first.saturating_sub(context + 1)~' \
  'context + 1' \
  mnema-desktop 'passage_context_bounds_the_blocks' --test agent

case_ "agent: context must widen the window at all" \
  $A \
  's~first\.saturating_sub\(context\)~first~; s~\(last \+ context\)~last~' \
  'let hi = last' \
  mnema-desktop 'passage_context_bounds_the_blocks' --test agent

case_ "agent: context must not widen the window without bound" \
  $A \
  's~\(last \+ context\)\.min\(blocks\.len\(\) - 1\)~blocks.len() - 1~' \
  'let hi = blocks.len() - 1;' \
  mnema-desktop 'passage_context_bounds_the_blocks' --test agent

case_ "agent: status must carry the scan state" \
  $A \
  's~"scan": serde_json::to_value\(state\.scan_state\(\)\)\.map_err\(internal\)\?~"scan": json!({})~' \
  '"scan": json!({})' \
  mnema-desktop 'status_carries_the_scan_state_and_folders' --test agent

case_ "agent: status must stat each folder" \
  $A \
  's~Path::new\(&path\)\.is_dir\(\)~true~' \
  '"available": true' \
  mnema-desktop 'status_carries_the_scan_state_and_folders' --test agent

case_ "agent: a busy slot must say started false" \
  $A \
  's~Err\(Error::JobAlreadyRunning\) => false~Err(Error::JobAlreadyRunning) => true~' \
  'JobAlreadyRunning) => true' \
  mnema-desktop 'rescan_while_running_says_running_not_error' --test agent

case_ "agent: a busy slot must not be an internal error" \
  $A \
  's~Err\(Error::JobAlreadyRunning\) => false,~Err(Error::JobAlreadyRunning) => return Err(internal("busy")),~' \
  'return Err(internal("busy"))' \
  mnema-desktop 'rescan_while_running_says_running_not_error' --test agent

case_ "agent: rescan wait must wait" \
  $A \
  's~if !wait \{~if true {~' \
  'if true {' \
  mnema-desktop 'rescan_wait_returns_the_report' --test agent

case_ "agent: rescan wait must carry the report" \
  $A \
  's~"report": serde_json::to_value\(report\)\.map_err\(internal\)\?~"report": Value::Null~' \
  '"report": Value::Null,' \
  mnema-desktop 'rescan_wait_returns_the_report' --test agent
