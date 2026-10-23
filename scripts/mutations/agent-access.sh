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
  's~"empty": passages\.is_empty\(\) && looked~"empty": false~' \
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
  's~err\(ErrKind::NotFound, "no such passage[^"]*"\)~err(ErrKind::Internal, "no such passage")~' \
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
  's~"started": false, "running": true \}~"started": true, "running": true }~' \
  '"started": true, "running": true }' \
  mnema-desktop 'rescan_while_running_says_running_not_error' --test agent

case_ "agent: a busy slot must not be an internal error" \
  $A \
  's~Err\(Error::JobAlreadyRunning\) => \{\}~Err(Error::JobAlreadyRunning) => return Err(internal("busy")),~' \
  'return Err(internal("busy"))' \
  mnema-desktop 'rescan_while_running_says_running_not_error' --test agent

case_ "agent: rescan wait must wait" \
  $A \
  's~if !wait \{\n        return Ok\(json!\(\{ "started": started~if true {\n        return Ok(json!({ "started": started~' \
  'if true {' \
  mnema-desktop 'rescan_wait_returns_the_report' --test agent

case_ "agent: rescan wait must carry the report" \
  $A \
  's~"report": serde_json::to_value\(report\)\.map_err\(internal\)\?~"report": Value::Null~' \
  '"report": Value::Null,' \
  mnema-desktop 'rescan_wait_returns_the_report' --test agent

# Task 4 (the socket, the switch, the log): src-tauri/src/agent.rs and
# src-tauri/src/prefs.rs, tested by tests/agent.rs.
P=src-tauri/src/prefs.rs

case_ "agent socket: the file must be mode 0600" \
  $A \
  's~from_mode\(0o600\)~from_mode(0o666)~' \
  'from_mode(0o666)' \
  mnema-desktop 'turning_access_on_creates_a_socket_only_the_user_can_open' --test agent

case_ "agent access: the switch must default to off" \
  $P \
  's~(\.get\("agent_access"\)[^;]*?)unwrap_or\(false\)~$1unwrap_or(true)~' \
  'unwrap_or(true)' \
  mnema-desktop 'access_is_off_by_default_and_no_socket_exists' --test agent

case_ "agent access: a boot must not open the socket whatever the switch says" \
  $A \
  's~if crate::prefs::agent_access\(&data_dir\) \{~if true {~' \
  'if true {' \
  mnema-desktop 'access_is_off_by_default_and_no_socket_exists' --test agent

case_ "agent access: a boot with the switch on must open the socket" \
  $A \
  's~if crate::prefs::agent_access\(&data_dir\) \{~if !crate::prefs::agent_access(\&data_dir) {~' \
  'if !crate::prefs::agent_access' \
  mnema-desktop 'a_boot_with_access_left_on_opens_the_socket' --test agent

case_ "agent access: a boot with the switch off must remove a crash's socket" \
  $A \
  's~let _ = std::fs::remove_file\(socket_path\(&data_dir\)\);~let _ = ();~' \
  'let _ = ();' \
  mnema-desktop 'a_boot_with_access_off_removes_a_socket_a_crash_left' --test agent

case_ "agent access: turning off must remove the socket file" \
  $A \
  's~(inner\.generation \+= 1;\n\s*)let _ = std::fs::remove_file\(&path\);~$1let _ = ();~' \
  'let _ = ();' \
  mnema-desktop 'turning_access_off_removes_the_socket_and_cuts_an_open_connection' --test agent

case_ "agent access: turning off must shut open connections down" \
  $A \
  's~let _ = stream\.shutdown\(std::net::Shutdown::Both\);~let _ = \&stream;~' \
  'let _ = &stream;' \
  mnema-desktop 'turning_access_off_removes_the_socket_and_cuts_an_open_connection' --test agent

case_ "agent access: a rescan waiting when access goes off must be cut" \
  $A \
  's~let _ = stream\.shutdown\(std::net::Shutdown::Both\);~let _ = \&stream;~' \
  'let _ = &stream;' \
  mnema-desktop 'turning_access_off_during_rescan_wait_cuts_the_client_but_not_the_scan' --test agent

case_ "agent access: the switch must close the socket when it goes off" \
  $P \
  's~(if !on \{\n\s*)crate::agent::stop\(&app\);~$1();~' \
  '();' \
  mnema-desktop 'turning_access_off_removes_the_socket_and_cuts_an_open_connection' --test agent

case_ "agent socket: a leftover temporary file must not block start" \
  $A \
  's~(make `bind` fail\.\n\s*)let _ = std::fs::remove_file\(&tmp\);~$1let _ = ();~' \
  'let _ = ();' \
  mnema-desktop 'a_socket_left_by_a_crash_is_replaced_on_start' --test agent

case_ "agent socket: a line of limit plus one bytes must be refused" \
  $A \
  's~\.take\(MAX_LINE as u64 \+ 1\)~.take(MAX_LINE as u64 + 2)~' \
  'MAX_LINE as u64 + 2' \
  mnema-desktop 'an_overlong_line_is_refused_and_the_connection_closed' --test agent

case_ "agent socket: a line of exactly the limit must be served" \
  $A \
  's~\.take\(MAX_LINE as u64 \+ 1\)~.take(MAX_LINE as u64)~' \
  'take(MAX_LINE as u64)' \
  mnema-desktop 'an_overlong_line_is_refused_and_the_connection_closed' --test agent

case_ "agent socket: the ninth connection must be refused" \
  $A \
  's~inner\.streams\.len\(\) >= MAX_CONNECTIONS~inner.streams.len() > MAX_CONNECTIONS~' \
  'streams.len() > MAX_CONNECTIONS' \
  mnema-desktop 'a_ninth_connection_is_refused' --test agent

case_ "agent socket: a path of 104 bytes must be refused with the limit named" \
  $A \
  's~len\(\) > MAX_SOCKET_PATH~len() > MAX_SOCKET_PATH + 1~' \
  '> MAX_SOCKET_PATH + 1' \
  mnema-desktop 'a_socket_path_too_long_is_an_error_not_a_panic' --test agent

case_ "agent socket: a path of 103 bytes must be accepted" \
  $A \
  's~len\(\) > MAX_SOCKET_PATH~len() >= MAX_SOCKET_PATH~' \
  '>= MAX_SOCKET_PATH' \
  mnema-desktop 'a_socket_path_too_long_is_an_error_not_a_panic' --test agent

case_ "agent access: a socket that cannot open must leave the switch off" \
  $P \
  's~(start\(&app\) \{\n\s*if let Err\(w\) = )key\(false\)~$1key(true)~' \
  'if let Err(w) = key(true)' \
  mnema-desktop 'a_socket_path_too_long_is_an_error_not_a_panic' --test agent

case_ "agent log: it must keep fifty entries" \
  $A \
  's~const LOG_CAP: usize = 50;~const LOG_CAP: usize = 51;~' \
  'LOG_CAP: usize = 51' \
  mnema-desktop 'the_log_keeps_the_last_fifty_newest_first' --test agent

case_ "agent log: the newest entry must come first" \
  $A \
  's~inner\.log\.push_front\(~inner.log.push_back(~' \
  'push_back(' \
  mnema-desktop 'the_log_keeps_the_last_fifty_newest_first' --test agent

case_ "agent socket: the reply must carry what handle answered" \
  $A \
  's~Ok\(v\) => Reply::Ok\(v\),~Ok(_) => Reply::Ok(Value::Null),~' \
  'Ok(_) => Reply::Ok(Value::Null)' \
  mnema-desktop 'a_request_round_trips_over_the_socket' --test agent

case_ "agent socket: a request that fails to parse must be a bad request" \
  $A \
  's~(let reply = match parsed \{\n\s*)Err\(e\) => Reply::Err\(e\),~$1Err(_) => Reply::Err(WireError { kind: ErrKind::Internal, message: String::new() }),~' \
  'kind: ErrKind::Internal, message: String::new()' \
  mnema-desktop 'a_malformed_request_over_the_socket_is_a_bad_request_and_the_connection_lives' --test agent

case_ "agent log: the date must carry the year past February" \
  $A \
  's~i64::from\(m <= 2\)~0~' \
  'yoe + era * 400 + 0' \
  mnema-desktop 'agent::tests::a_time_is_written_as_rfc_3339_utc' --lib

# Task 4 fix round: the switch is serialised, off stops first, a failed start
# switches the stored value off, and "enabled" reports the socket.

case_ "agent access: off must stop the socket before it writes the prefs" \
  $P \
  's~(crate::agent::stop\(&app\);)(\s*)(key\(false\)\?;)~$3$2$1~' \
  'key(false)?;' \
  mnema-desktop 'turning_access_off_cuts_the_socket_even_when_the_prefs_write_fails' --test agent

case_ "agent access: two changes of the switch must not interleave" \
  $P \
  's~let _toggle = server\.toggle_lock\(\);~let _toggle = ();~' \
  'let _toggle = ();' \
  mnema-desktop 'a_change_of_the_switch_waits_for_the_one_under_way' --test agent

case_ "agent access: a boot that cannot start must switch the stored value off" \
  $A \
  's~serde_json::Value::Bool\(false\)~serde_json::Value::Bool(true)~' \
  'Value::Bool(true))' \
  mnema-desktop 'a_boot_that_cannot_open_the_socket_switches_access_off' --test agent

case_ "agent access: enabled must say whether the socket is open" \
  $P \
  's~server\.is_running\(\)~true~' \
  '    true' \
  mnema-desktop 'the_switch_reports_the_socket_and_not_the_stored_value' --test agent

case_ "agent access: a failed write-back must not hide the start error" \
  $P \
  's~(if let Err\(e\) = crate::agent::start\(&app\) \{\n\s*)if let Err\(w\) = key\(false\) \{~$1key(false)?; if let Err(w) = Ok::<(), std::io::Error>(()) {~' \
  'key(false)?; if let Err(w)' \
  mnema-desktop 'a_failed_start_with_a_failed_write_back_reports_the_start' --test agent

# Task 5 (the `mnema` binary): CLI and stdio MCP. The tests are integration
# tests in crates/mnema-cli/tests/cli.rs, which spawn the built binary.

M=crates/mnema-cli/src/main.rs
C=crates/mnema-cli/src/client.rs
X=crates/mnema-cli/src/mcp.rs

case_ "mnema cli: the ok payload must be printed" \
  $M \
  's~println!\("\{v\}"\);~println!("null");~' \
  'println!("null")' \
  mnema-cli 'search_prints_the_ok_payload_and_exits_zero' --test cli

case_ "mnema cli: an err must exit one" \
  $M \
  's~(serialises"\)\n\s*\);\n\s*)ExitCode::FAILURE~$1ExitCode::from(0)~' \
  'ExitCode::from(0)' \
  mnema-cli 'an_err_reply_prints_the_error_and_exits_one' --test cli

case_ "mnema cli: an err must be printed as the wire error" \
  $M \
  's~serde_json::to_string\(&e\)\.expect~serde_json::to_string(&e.message).expect~' \
  'to_string(&e.message)' \
  mnema-cli 'an_err_reply_prints_the_error_and_exits_one' --test cli

case_ "mnema client: no socket with the override must not wait" \
  $C \
  's~(if std::env::var_os\(OVERRIDE\)\.is_some\(\) \{)~$1 std::thread::sleep(POLL * 10);~' \
  'POLL * 10' \
  mnema-cli 'no_socket_with_the_override_set_is_not_running_without_launching' --test cli

case_ "mnema client: no socket with the override must say not_running" \
  $C \
  's~map_err\(\|_\| not_running\(\)\)~map_err(|_| disabled())~' \
  'map_err(|_| disabled())' \
  mnema-cli 'no_socket_with_the_override_set_is_not_running_without_launching' --test cli

case_ "mnema client: a hang-up before any reply must read as disabled" \
  $C \
  's~return Err\(disabled\(\)\),~return Err(err(ErrKind::Internal, "closed")),~' \
  'ErrKind::Internal, "closed"' \
  mnema-cli 'a_dropped_connection_reads_as_disabled' --test cli

case_ "mnema cli: the request must state the protocol version" \
  $M \
  's~(Request \{\s*v: )VERSION~${1}2~' \
  'v: 2,' \
  mnema-cli 'the_request_carries_v_and_the_op' --test cli

case_ "mnema cli: --k must reach the request" \
  $M \
  's~(query: args\.pop\(\)\.ok_or_else\(usage\)\?,\s*)k,~${1}k: None,~' \
  'k: None,' \
  mnema-cli 'the_request_carries_v_and_the_op' --test cli

case_ "mnema cli: --wait must reach the request" \
  $M \
  's~\.then_some\(true\)~.then_some(false)~' \
  'then_some(false)' \
  mnema-cli 'the_request_carries_v_and_the_op' --test cli

case_ "mnema client: an oversized request must be refused before connecting" \
  $C \
  's~if line\.len\(\) > MAX_LINE \{~if line.len() > MAX_LINE * 2 {~' \
  'MAX_LINE * 2' \
  mnema-cli 'a_request_over_the_limit_is_refused_locally_without_connecting' --test cli

case_ "mnema mcp: the server must name itself mnema" \
  $X \
  's~Implementation::new\("mnema"~Implementation::new("mnemo"~' \
  'Implementation::new("mnemo"' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: all four tools must be listed" \
  $X \
  's~"rescan",(\s*)RESCAN~"rescan2",$1RESCAN~' \
  '"rescan2"' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the client name must come from clientInfo" \
  $X \
  's~\.map\(\|i\| i\.client_info\.name\.clone\(\)\)~.map(|_| "mcp".to_string())~' \
  '"mcp".to_string()' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the op must follow the tool name" \
  $X \
  's~Value::String\(name\.into\(\)\)~Value::String("status".into())~' \
  'Value::String("status".into())' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the payload text must reach the agent byte for byte" \
  $X \
  's~value\.to_string\(\)\)~value.to_string().replace("\\\\n", " "))~' \
  'replace("\\n", " ")' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: an err must be an error result" \
  $X \
  's~CallToolResult::error\(vec!\[text\]\)~CallToolResult::success(vec![text])~' \
  'CallToolResult::success(vec![text])' \
  mnema-cli 'mcp_turns_an_err_into_an_error_result' --test cli

case_ "mnema mcp: an err result must carry the wire error" \
  $X \
  's~&serde_json::to_value\(e\)\.expect\("a WireError serialises"\)~&Value::Null~' \
  '&Value::Null' \
  mnema-cli 'mcp_turns_an_err_into_an_error_result' --test cli

# Task 6 (the bundle): the application must not link the MCP stack. `rmcp` is a
# dependency of mnema-cli only; the mutant gives it to the shell as well.
case_ "bundle: the application must not link rmcp" \
  src-tauri/Cargo.toml \
  's~(mnema-index = \{ path = "\.\./crates/mnema-index" \}\n)~$1rmcp = { version = "3.7.0", default-features = false, features = ["server"] }\n~' \
  'rmcp = { version = "3.7.0"' \
  mnema-desktop 'the_application_does_not_link_rmcp' --test dependency_boundary

# Task 7 (the Agents section, vitest): one case per test in Agents.test.ts.
A=ui/src/settings/Agents.svelte
AT=src/settings/Agents.test.ts

case_ "Agents.svelte: the command shows although access is off" \
  $A \
  's~\{:else if on && path\}~{:else if true}~' \
  '{:else if true}' \
  $AT 'access is off by default and no command is shown' runner=vitest

case_ "Agents.svelte: turning access on asks for off" \
  $A \
  's~setAgentAccess\(want\)~setAgentAccess(false)~' \
  'setAgentAccess(false)' \
  $AT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: the command carries a placeholder instead of the real path" \
  $A \
  's~claude mcp add --scope user mnema -- \$\{quoted\} mcp~claude mcp add --scope user mnema -- mnema mcp~' \
  'claude mcp add --scope user mnema -- mnema mcp' \
  $AT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: the switch is not re-read after a change" \
  $A \
  's~    await readEnabled\(\);\n    busy = false;~    busy = false;~' \
  '    try { await setAgentAccess(want); } catch (err) { failure = String(err); }
    busy = false;' \
  $AT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: a refused start leaves the box ticked" \
  $A \
  's~    box\.checked = on;\n~~' \
  'what the socket says.
  }' \
  $AT 'a refused start shows the error and does not claim access is on' runner=vitest

case_ "Agents.svelte: a refused start is not shown" \
  $A \
  's~(setAgentAccess\(want\); \} catch \(err\) \{) failure = String\(err\);~$1 failure = null;~' \
  'catch (err) { failure = null; }' \
  $AT 'a refused start shows the error and does not claim access is on' runner=vitest

case_ "Agents.svelte: the fragment button copies the command" \
  $A \
  's~copy\(json\)\}~copy(cmd)}~' \
  'copy(cmd)}>{#key $locale}{t(' \
  $AT 'the copy buttons put the command and the fragment on the clipboard' runner=vitest

case_ "Agents.svelte: the log is listed oldest first" \
  $A \
  's~\{#each log as entry~{#each [...log].reverse() as entry~' \
  '[...log].reverse()' \
  $AT 'the log lists entries newest first with the claimed client name' runner=vitest

case_ "Agents.svelte: the claimed client name is replaced by the operation" \
  $A \
  's~client: entry\.client~client: entry.op~' \
  'client: entry.op' \
  $AT 'the log lists entries newest first with the claimed client name' runner=vitest

case_ "Agents.svelte: an empty log draws no sentence" \
  $A \
  's~log\.length === 0~log.length === 99~' \
  'log.length === 99' \
  $AT 'an empty log says no agent has asked yet' runner=vitest

case_ "Agents.svelte: the poll runs every five seconds" \
  $A \
  's~POLL_MS = 2000~POLL_MS = 5000~' \
  'POLL_MS = 5000' \
  $AT 'the log is polled every two seconds while mounted, and no longer after' runner=vitest

case_ "Agents.svelte: the poll outlives the section" \
  $A \
  's~return \(\) => clearInterval\(timer\);~return () => {};~' \
  'return () => {};' \
  $AT 'the log is polled every two seconds while mounted, and no longer after' runner=vitest

case_ "Settings.svelte: the Agents section is left out of the navigation" \
  ui/src/settings/Settings.svelte \
  "s~const SECTIONS: SectionId\[\] = \['models', 'folders', 'indexing', 'agents', 'application'\];~const SECTIONS: SectionId[] = ['models', 'folders', 'indexing', 'application'];~" \
  "'indexing', 'application'];" \
  src/settings/Settings.test.ts 'the agents section is reachable from the settings navigation' runner=vitest

case_ "Agents.svelte: the path is quoted without escaping the quote" \
  $A \
  's~path\.replaceAll\([^\n]*?\)\}~path}~' \
  "const quoted = \$derived(\`'\${path}'\`);" \
  $AT 'a path with a space, a quote and a dollar sign is quoted for the shell' runner=vitest

case_ "Agents.svelte: the command is drawn for an empty path" \
  $A \
  's~\{:else if on && path\}~{:else if on}~' \
  '{:else if on}' \
  $AT 'while the path is pending there is no command and no copy button' runner=vitest

case_ "Agents.svelte: a rejected path read is swallowed" \
  $A \
  's~\(\) => \(pathFailed = true\)~() => {}~' \
  'agentCommandPath().then((p) => (path = p), () => {});' \
  $AT 'a rejected path read shows an error line and no copyable command' runner=vitest

case_ "Agents.svelte: a failed read of the switch turns it off" \
  $A \
  's~catch \(err\) \{ failure = String\(err\); \}\n  \}\n\n  async function toggle~catch (err) { failure = String(err); on = false; }\n  }\n\n  async function toggle~' \
  'on = false; }' \
  $AT 'a failed read of the switch keeps the last known state and shows the error' runner=vitest

case_ "Agents.svelte: the switch stays enabled during a change" \
  $A \
  's~ disabled=\{busy\}~~' \
  'checked={on} onchange' \
  $AT 'the switch is disabled while a change is pending' runner=vitest

case_ "Agents.svelte: the log is polled while access is off" \
  $A \
  's~    if \(!on\) return;\n~~' \
  '  $effect(() => {
    void pollLog();' \
  $AT 'the log is not polled while access is off' runner=vitest

case_ "Agents.svelte: the box shows what was asked, not what the socket says" \
  $A \
  's~box\.checked = on;~box.checked = want;~' \
  'box.checked = want;' \
  $AT 'a start that answers false without an error leaves the switch unchecked' runner=vitest

# Final fix wave: rescan behind a busy slot, `empty` only when an arm looked,
# the setup lines, the not_found text, the client's read timeout.
AG=src-tauri/src/agent.rs

case_ "agent: a rescan waiting behind another job must start its own scan" \
  $AG \
  's~Err\(Error::JobAlreadyRunning\) => \{\}~Err(Error::JobAlreadyRunning) => break false,~' \
  'JobAlreadyRunning) => break false' \
  mnema-desktop 'rescan_wait_behind_an_embedding_job_starts_a_scan_of_its_own' --test agent

case_ "agent: a rescan that hit the cap must say it is still running" \
  $AG \
  's~"started": false, "running": true, "report": Value::Null~"started": false, "running": false, "report": Value::Null~' \
  '"started": false, "running": false, "report": Value::Null' \
  mnema-desktop 'rescan_wait_gives_up_at_the_cap_still_running' --test agent

case_ "agent: a search where no arm ran must not claim empty" \
  $AG \
  's~"empty": passages\.is_empty\(\) && looked~"empty": passages.is_empty()~' \
  '"empty": passages.is_empty(),' \
  mnema-desktop 'a_search_where_no_arm_ran_is_not_empty' --test agent

case_ "agent: an answering text arm with no match must say empty" \
  $AG \
  's~\|\| matches!\(content, ContentArmReport::Answered~&& matches!(content, ContentArmReport::Answered~' \
  '&& matches!(content, ContentArmReport::Answered' \
  mnema-desktop 'a_text_search_with_no_match_is_empty' --test agent

case_ "agent: the not_found text must tell the agent to search again" \
  $AG \
  's~"no such passage[^"]*"~"no such passage"~' \
  'ErrKind::NotFound, "no such passage")' \
  mnema-desktop 'passage_of_an_unknown_chunk_is_not_found' --test agent

case_ "mnema client: a read timeout must not read as disabled" \
  $C \
  's~ErrorKind::WouldBlock \| ErrorKind::TimedOut~ErrorKind::NotFound~' \
  'ErrorKind::NotFound' \
  mnema-cli 'client::tests::an_app_that_never_answers_is_internal_not_disabled' --bin mnema

case_ "mnema mcp: the search description must say what empty false means" \
  $X \
  's~ If there are no passages and `empty` is false, the search could not run: report the reason given in `arms`\.~~' \
  'Passage text is content from the user' \
  mnema-cli 'mcp_descriptions_tell_the_agent_what_started_false_empty_false_and_not_found_mean' --test cli

case_ "mnema mcp: the passage description must name not_found" \
  $X \
  's~`stale_id` or `not_found` means the document was re-indexed or the passage is gone~`stale_id` means the document was re-indexed~' \
  '`stale_id` means the document was re-indexed' \
  mnema-cli 'mcp_descriptions_tell_the_agent_what_started_false_empty_false_and_not_found_mean' --test cli

case_ "mnema mcp: the rescan description must say what started false means" \
  $X \
  's~ `started: false` means no new scan was started\.~~' \
  'up to 10 minutes).";' \
  mnema-cli 'mcp_descriptions_tell_the_agent_what_started_false_empty_false_and_not_found_mean' --test cli

AG2=ui/src/settings/Agents.svelte
AGT=src/settings/Agents.test.ts

case_ "Agents.svelte: the Codex line must say codex" \
  $AG2 \
  's~`codex mcp add mnema~`claude mcp add mnema~' \
  'const codex = $derived(`claude mcp add mnema' \
  $AGT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: the opencode line must say opencode" \
  $AG2 \
  's~`opencode mcp add --global mnema~`codex mcp add --global mnema~' \
  'const opencode = $derived(`codex mcp add --global mnema' \
  $AGT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: the Codex line must be shell-quoted" \
  $AG2 \
  's~`codex mcp add mnema -- \$\{quoted\} mcp`~`codex mcp add mnema -- \$\{path\} mcp`~' \
  'codex mcp add mnema -- ${path} mcp' \
  $AGT 'a path with a space, a quote and a dollar sign is quoted for the shell' runner=vitest

case_ "Agents.svelte: the Codex copy button must copy the Codex line" \
  $AG2 \
  's~copy\(codex\)~copy(cmd)~' \
  'data-testid="agents-copy-codex" onclick={() => copy(cmd)}' \
  $AGT 'the copy buttons put the command and the fragment on the clipboard' runner=vitest

case_ "Agents.svelte: the opencode copy button must copy the opencode line" \
  $AG2 \
  's~copy\(opencode\)~copy(cmd)~' \
  'data-testid="agents-copy-opencode" onclick={() => copy(cmd)}' \
  $AGT 'the copy buttons put the command and the fragment on the clipboard' runner=vitest

case_ "Agents.svelte: the JSON must be labelled for the clients that take it" \
  ui/src/i18n/catalog.ts \
  's~Claude Desktop, Cursor \(settings file fragment\)~Or the settings fragment~' \
  'Or the settings fragment:' \
  $AGT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

# ── Live-stand fixes ──────────────────────────────────────────────────────────

case_ "mnema client: a running app without a socket must not be launched" \
  $C \
  's~if running\(\) \{\n        return Err\(disabled\(\)\);~if false \&\& running() {\n        return Err(disabled());~' \
  'if false && running()' \
  mnema-cli 'client::tests::a_running_app_without_a_socket_is_disabled_at_once_and_is_not_launched' --bin mnema

case_ "mnema client: an app that is not running must be launched" \
  $C \
  's~\n    launch\(\);~\n    let _ = launch;~' \
  'let _ = launch;' \
  mnema-cli 'client::tests::an_app_that_is_not_running_is_launched_and_then_reported_not_running' --bin mnema

case_ "mnema client: a socket that appears after the launch must be used" \
  $C \
  's~while start\.elapsed\(\) < wait~while false \&\& start.elapsed() < wait~' \
  'while false && start.elapsed() < wait' \
  mnema-cli 'client::tests::a_launched_app_that_opens_its_socket_is_connected_to' --bin mnema

case_ "mnema client: a path outside a bundle must give no bundle" \
  $C \
  's~app\.extension\(\)\? == "app"~app.extension().is_some()~' \
  'app.extension().is_some()' \
  mnema-cli 'client::tests::the_bundle_is_derived_from_the_executable_path' --bin mnema

case_ "mnema client: a bundle path must name the .app" \
  $C \
  's~\.then\(\|\| app\.to_path_buf\(\)\)~.then(|| contents.to_path_buf())~' \
  '.then(|| contents.to_path_buf())' \
  mnema-cli 'client::tests::the_bundle_is_derived_from_the_executable_path' --bin mnema

case_ "mnema mcp: the server must carry its instructions" \
  $X \
  's~\.with_instructions\(INSTRUCTIONS\)~.with_instructions("")~' \
  '.with_instructions("")' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the instructions must tell the agent to search the library before the file system" \
  $X \
  's~ Mnema already indexes these documents: call `search_library` first, instead of searching the file system for them\.~~' \
  '«моя бібліотека»). Answer from' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the search tool must be named search_library" \
  $X \
  's~"search_library",(\s*)SEARCH~"search",$1SEARCH~' \
  '"search",' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: search_library must forward as the search op" \
  $X \
  's~if name == "search_library" \{~if name == "search_librar" {~' \
  'name == "search_librar"' \
  mnema-cli 'mcp_lists_four_tools_and_calls_search' --test cli

case_ "mnema mcp: the passage description must point at search_library" \
  $X \
  's~run `search_library` again~run `search` again~' \
  'run `search` again' \
  mnema-cli 'mcp_descriptions_tell_the_agent_what_started_false_empty_false_and_not_found_mean' --test cli

case_ "Agents.svelte: the Claude Code line must register for the user, not the directory" \
  $AG2 \
  's~claude mcp add --scope user mnema~claude mcp add mnema~' \
  'claude mcp add mnema -- ${quoted}' \
  $AGT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest

case_ "Agents.svelte: the opencode line must write the global config" \
  $AG2 \
  's~opencode mcp add --global mnema~opencode mcp add mnema~' \
  'opencode mcp add mnema -- ${quoted}' \
  $AGT 'turning access on calls set_agent_access with true and shows the command with the real path' runner=vitest
