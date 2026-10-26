# The agent connect work — `src-tauri/src/agent_clients.rs`, its Tauri commands
# and the settings section that drives them. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/agent-connect.sh
#
# One case per mutant the four task reports list as killing a test:
# `docs/private/sdd/2026-10-26-agent-connect/task-{1,2,3,4}-report.md`, the
# round-fix sections included. A mutant whose code later rounds deleted is not
# here; `task-5-report.md` names each one and why:
#
#   - task 2 M8 (drop `preserve_order` from serde_json) — the mutant no longer
#     compiles: `shift_remove` exists only under that feature. The file order it
#     pins is still killed here, by the shift_remove case below;
#   - task 3 M6, M9 and M10 (`find_cli`, and a client's name interpolated into a
#     shell command) — no shell runs a command any more; the PATH is resolved in
#     Rust since round 1;
#   - task 3 R1 (a finished child's group is not ended) — round 2 made "no
#     signal on a normal exit" the intended behaviour, so the mutant has no
#     subject, and its test was replaced;
#   - task 3 S4 (an error for every client, wanted or not) — its `want` no
#     longer exists; Task 4's R1 pins the same "only listed clients" rule;
#   - task 4 M10 (a `want` field on the wire) — superseded by I1, the same
#     field-name mutant against the shape the command now takes;
#   - task 4 U2 (the stale clause dropped from `change`) — the same mutant as
#     M6, kept once.
#
# Every test named here is the one the report names, under its current name
# where a later round renamed it (`apply_removes_a_current_client_the_person_unticked`,
# `a_child_that_outlives_the_timeout_is_killed_with_its_group`).

# ── Task 1: each agent CLI's own answer, parsed ───────────────────────────────
W=src-tauri/src/agent_clients.rs
T='agent_clients::tests::'

case_ "agent_clients: a claude failure that is not the absent phrase must be an error" \
  $W \
  's~if out\.code == 1 && said\.starts_with\("No MCP server named"\) \{~if out.code == 1 {~' \
  'if out.code == 1 {' \
  mnema-desktop "${T}claude_a_missing_server_is_absent_and_another_failure_is_an_error" --lib

case_ "agent_clients: claude's scope must be the user one" \
  $W \
  's~    if !scope\.starts_with\("User config"\) \{~    if false {~' \
  'if false {' \
  mnema-desktop "${T}claude_a_server_in_another_scope_is_not_ours" --lib

case_ "agent_clients: claude's command runs to the end of its line, not to its first word" \
  $W \
  's~    let command = field\(&out\.stdout, "Command:"\)\.ok_or\(said\)\?;~    let command = field(&out.stdout, "Command:").map(|c| c.split_whitespace().next().unwrap_or_default()).ok_or(said)?;~' \
  '.map(|c| c.split_whitespace().next().unwrap_or_default())' \
  mnema-desktop "${T}claude_our_command_and_args_are_current" --lib

case_ "agent_clients: claude's args must reach the comparison" \
  $W \
  's~    let args: Vec<&str> = field\(&out\.stdout, "Args:"\)\n        \.unwrap_or_default\(\)\n        \.split_whitespace\(\)\n        \.collect\(\);~    let args: Vec<&str> = vec!["mcp"];~' \
  'let args: Vec<&str> = vec!["mcp"];' \
  mnema-desktop "${T}claude_another_args_is_stale" --lib

case_ "agent_clients: a claude record with no command line must be an error" \
  $W \
  's~    let command = field\(&out\.stdout, "Command:"\)\.ok_or\(said\)\?;~    let command = field(&out.stdout, "Command:").unwrap_or_default();~' \
  'let command = field(&out.stdout, "Command:").unwrap_or_default();' \
  mnema-desktop "${T}claude_without_a_command_line_is_an_error_carrying_the_output" --lib

case_ "agent_clients: claude's unexpected exit code must be an error" \
  $W \
  's~    if out\.code != 0 \{\n        return Err\(said\);\n    \}\n    // `get` answers~    if out.code != 0 {\n        return Ok(Link::Absent);\n    }\n    // `get` answers~' \
  'return Ok(Link::Absent);
    }
    // `get` answers' \
  mnema-desktop "${T}claude_an_unexpected_exit_code_is_an_error" --lib

case_ "agent_clients: codex's absent phrase is searched anywhere, not only at the start" \
  $W \
  's~said\.contains\("No MCP server named"\)~said.starts_with("No MCP server named")~' \
  'said.starts_with("No MCP server named")' \
  mnema-desktop "${T}codex_a_missing_server_is_absent" --lib

case_ "agent_clients: codex's command must be read, not assumed to be ours" \
  $W \
  's~    let command = transport\["command"\]\.as_str\(\)\.ok_or_else\(\|\| said\.clone\(\)\)\?;~    let command = ours;~' \
  'let command = ours;' \
  mnema-desktop "${T}codex_another_command_is_stale" --lib

case_ "agent_clients: codex's args must reach the comparison" \
  $W \
  's~    let args: Vec<&str> = transport\["args"\]\n        \.as_array\(\)\n        \.map\(\|args\| args\.iter\(\)\.filter_map\(Value::as_str\)\.collect\(\)\)\n        \.unwrap_or_default\(\);~    let args: Vec<&str> = vec!["mcp"];~' \
  'let args: Vec<&str> = vec!["mcp"];' \
  mnema-desktop "${T}codex_another_args_is_stale" --lib

case_ "agent_clients: codex's unexpected exit code must be an error" \
  $W \
  's~    if out\.code != 0 \{\n        return Err\(said\);\n    \}\n    let record: Value = serde_json::from_str\(&out\.stdout\)\.map_err\(\|_\| said\.clone\(\)\)\?;\n    let transport = &record\["transport"\];~    if out.code != 0 {\n        return Ok(Link::Absent);\n    }\n    let record: Value = serde_json::from_str(&out.stdout).map_err(|_| said.clone())?;\n    let transport = &record["transport"];~' \
  'return Ok(Link::Absent);
    }
    let record: Value' \
  mnema-desktop "${T}codex_an_unexpected_exit_code_is_an_error" --lib

case_ "agent_clients: pi's server must be in the global scope" \
  $W \
  's~server\["name"\]\.as_str\(\) == Some\("mnema"\) && server\["scope"\]\.as_str\(\) == Some\("global"\)~server["name"].as_str() == Some("mnema")~' \
  'server["name"].as_str() == Some("mnema")
        })' \
  mnema-desktop "${T}pi_a_server_in_another_scope_is_absent" --lib

case_ "agent_clients: pi compares the whole transport, the argument included" \
  $W \
  's~    Ok\(if transport == format!\("\{ours\} mcp"\) \{~    Ok(if transport == ours {~' \
  'Ok(if transport == ours {' \
  mnema-desktop "${T}pi_our_transport_without_the_argument_is_stale" --lib

case_ "agent_clients: pi strips the trailing mcp from a stale transport" \
  $W \
  's~            command: transport\n                \.strip_suffix\(" mcp"\)\n                \.unwrap_or\(transport\)\n                \.to_string\(\),~            command: transport.to_string(),~' \
  'command: transport.to_string(),' \
  mnema-desktop "${T}pi_another_transport_is_stale_without_the_trailing_mcp" --lib

case_ "agent_clients: pi's unexpected exit code must be an error" \
  $W \
  's~    if out\.code != 0 \{\n        return Err\(said\);\n    \}\n    let record: Value = serde_json::from_str\(&out\.stdout\)\.map_err\(\|_\| said\.clone\(\)\)\?;\n    let server = record\["servers"\]~    if out.code != 0 {\n        return Ok(Link::Absent);\n    }\n    let record: Value = serde_json::from_str(&out.stdout).map_err(|_| said.clone())?;\n    let server = record["servers"]~' \
  'return Ok(Link::Absent);
    }
    let record: Value = serde_json::from_str(&out.stdout).map_err(|_| said.clone())?;
    let server = record["servers"]' \
  mnema-desktop "${T}pi_an_unexpected_exit_code_is_an_error" --lib

case_ "agent_clients: said prefers stderr when the CLI said anything" \
  $W \
  's~    let text = if out\.stderr\.trim\(\)\.is_empty\(\) \{\n        &out\.stdout\n    \} else \{\n        &out\.stderr\n    \};~    let text = &out.stdout;~' \
  'let text = &out.stdout;' \
  mnema-desktop "${T}claude_an_unexpected_exit_code_is_an_error" --lib

case_ "agent_clients: said falls back to stdout when stderr is empty" \
  $W \
  's~    let text = if out\.stderr\.trim\(\)\.is_empty\(\) \{\n        &out\.stdout\n    \} else \{\n        &out\.stderr\n    \};~    let text = &out.stderr;~' \
  'let text = &out.stderr;' \
  mnema-desktop "${T}claude_without_a_command_line_is_an_error_carrying_the_output" --lib

case_ "agent_clients: Link is tagged kind on the wire, not type" \
  $W \
  's~#\[serde\(tag = "kind", rename_all = "lowercase"\)\]~#[serde(tag = "type", rename_all = "lowercase")]~' \
  'tag = "type"' \
  mnema-desktop "${T}a_link_reaches_the_window_as_a_kind_and_a_command" --lib

case_ "agent_clients: codex's unreadable JSON is an error, not an absent server" \
  $W \
  's~    let record: Value = serde_json::from_str\(&out\.stdout\)\.map_err\(\|_\| said\.clone\(\)\)\?;\n    let transport = &record\["transport"\];~    let Ok(record) = serde_json::from_str::<Value>(&out.stdout) else {\n        return Ok(Link::Absent);\n    };\n    let transport = &record["transport"];~' \
  'serde_json::from_str::<Value>(&out.stdout) else {' \
  mnema-desktop "${T}codex_output_that_is_not_json_is_an_error" --lib

case_ "agent_clients: pi's unreadable JSON is an error, not an absent server" \
  $W \
  's~    let record: Value = serde_json::from_str\(&out\.stdout\)\.map_err\(\|_\| said\.clone\(\)\)\?;\n    let server = record\["servers"\]~    let Ok(record) = serde_json::from_str::<Value>(&out.stdout) else {\n        return Ok(Link::Absent);\n    };\n    let server = record["servers"]~' \
  'serde_json::from_str::<Value>(&out.stdout) else {' \
  mnema-desktop "${T}pi_output_that_is_not_json_is_an_error" --lib
# ── Task 2: opencode's file, read and rewritten ──────────────────────────────

case_ "opencode: the new form wins when both forms are present" \
  $W \
  's~    let \(key, entry\) = if let Some\(entry\) = new_form \{\n        \("mcp\.servers\.mnema", entry\)\n    \} else if let Some\(entry\) = mcp\.and_then\(\|mcp\| mcp\.get\("mnema"\)\) \{\n        \("mcp\.mnema", entry\)\n    \} else \{\n        return Ok\(Link::Absent\);\n    \};~    let (key, entry) = if let Some(entry) = mcp.and_then(|mcp| mcp.get("mnema")) {\n        ("mcp.mnema", entry)\n    } else if let Some(entry) = new_form {\n        ("mcp.servers.mnema", entry)\n    } else {\n        return Ok(Link::Absent);\n    };~' \
  'if let Some(entry) = mcp.and_then(|mcp| mcp.get("mnema")) {
        ("mcp.mnema", entry)
    } else if let Some(entry) = new_form {' \
  mnema-desktop "${T}opencode_the_new_form_wins_when_both_are_present" --lib

case_ "opencode: the old form must still be read" \
  $W \
  's~    \} else if let Some\(entry\) = mcp\.and_then\(\|mcp\| mcp\.get\("mnema"\)\) \{~    } else if let Some(entry) = None::<&Value> {~' \
  'None::<&Value>' \
  mnema-desktop "${T}opencode_the_old_form_is_read" --lib

case_ "opencode: an entry with no command is an error, not a current record" \
  $W \
  's~        return Err\(format!\("\{key\} has no command"\)\);~        return Ok(Link::Current);~' \
  'return Ok(Link::Current);
    };' \
  mnema-desktop "${T}opencode_an_entry_without_a_command_is_an_error" --lib

case_ "opencode: text that is not JSON is an error, not an empty config" \
  $W \
  's~    let root: Value = serde_json::from_str\(text\)\.map_err\(\|error\| error\.to_string\(\)\)\?;~    let root: Value = serde_json::from_str(text).unwrap_or_default();~' \
  'let root: Value = serde_json::from_str(text).unwrap_or_default();' \
  mnema-desktop "${T}opencode_text_that_is_not_json_is_an_error" --lib

case_ "opencode: a server's arguments must reach the comparison" \
  $W \
  's~    Ok\(link_of\(command, args, ours\)\)~    Ok(link_of(command, &[], ours))~' \
  'link_of(command, &[], ours)' \
  mnema-desktop "${T}opencode_a_server_under_mcp_servers_is_read" --lib

case_ "opencode: the old form must be removed, not only the new one" \
  $W \
  's~    let mut removed = mcp\.shift_remove\("mnema"\)\.is_some\(\);~    let mut removed = false;~' \
  'let mut removed = false;' \
  mnema-desktop "${T}opencode_removing_mnema_leaves_the_rest_of_the_file_in_its_order" --lib

case_ "opencode: the new form must be removed, not only the old one" \
  $W \
  's~    if let Some\(servers\) = mcp\.get_mut\("servers"\)\.and_then\(Value::as_object_mut\) \{\n        removed \|= servers\.shift_remove\("mnema"\)\.is_some\(\);\n    \}\n~~' \
  'let mut removed = mcp.shift_remove("mnema").is_some();
    if !removed {' \
  mnema-desktop "${T}opencode_removing_mnema_leaves_the_rest_of_the_file_in_its_order" --lib

case_ "opencode: the servers key is read, not created on the way" \
  $W \
  's~    if let Some\(servers\) = mcp\.get_mut\("servers"\)\.and_then\(Value::as_object_mut\) \{~    if let Some(servers) = mcp.entry("servers".to_string()).or_insert(Value::Null).as_object_mut() {~' \
  '.or_insert(Value::Null)' \
  mnema-desktop "${T}opencode_the_old_form_is_removed_without_adding_anything" --lib

case_ "opencode: a file with nothing to remove is not rewritten" \
  $W \
  's~    if !removed \{~    if false {~' \
  'if false {' \
  mnema-desktop "${T}opencode_without_mnema_says_so_when_there_is_nothing_to_remove" --lib

case_ "opencode: the write is a rename, not a copy that leaves the temporary behind" \
  $W \
  's~    std::fs::rename\(tmp, path\)~    std::fs::copy(tmp, path).map(|_| ())~' \
  'std::fs::copy(tmp, path)' \
  mnema-desktop "${T}opencode_writing_atomically_leaves_only_the_target_in_the_directory" --lib

case_ "opencode: the target's permissions must be copied to the replacement" \
  $W \
  's~        if let Ok\(meta\) = std::fs::metadata\(path\) \{\n            file\.set_permissions\(meta\.permissions\(\)\)\?;\n        \}\n~~' \
  '.open(tmp)?;
        file.write_all(text.as_bytes())?;' \
  mnema-desktop "${T}opencode_a_rewritten_file_keeps_its_permissions" --lib

case_ "opencode: a write that cannot use a temporary leaves the target alone" \
  $W \
  's~    let written = write_then_rename\(&tmp, &target, text\);~    let written = std::fs::write(&target, text);~' \
  'let written = std::fs::write(&target, text);' \
  mnema-desktop "${T}opencode_a_failed_write_leaves_the_target_as_it_was" --lib

case_ "opencode: JSONC is refused, not accepted by stripping its comments" \
  $W \
  's~serde_json::from_str\(text\)\.map_err\(\|error\| error\.to_string\(\)\)\?~serde_json::from_str(&text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\\n")).map_err(|error| error.to_string())?~' \
  'starts_with("//")' \
  mnema-desktop "${T}opencode_text_that_is_not_json_is_an_error" --lib

case_ "opencode: an XDG config home must win over the home directory" \
  $W \
  's~    match xdg\.filter\(\|dir\| !dir\.as_os_str\(\)\.is_empty\(\)\) \{~    match None::<&Path> {~' \
  'match None::<&Path> {' \
  mnema-desktop "${T}opencode_config_is_xdg_then_home" --lib

case_ "opencode: a config without a mnema entry is absent, not current" \
  $W \
  's~    \} else \{\n        return Ok\(Link::Absent\);\n    \};~    } else {\n        return Ok(Link::Current);\n    };~' \
  'return Ok(Link::Current);
    };' \
  mnema-desktop "${T}opencode_without_a_mnema_entry_is_absent" --lib

case_ "opencode: removing the first key must shift, not swap the last one into its place" \
  $W \
  's~mcp\.shift_remove\("mnema"\)~mcp.remove("mnema")~; s~servers\.shift_remove\("mnema"\)~servers.remove("mnema")~' \
  'mcp.remove("mnema")' \
  mnema-desktop "${T}opencode_mnema_first_in_the_object_leaves_the_other_keys_in_order" --lib

case_ "opencode: a file that cannot be handed back unchanged is refused" \
  $W \
  's~    if unmodified != text \{~    if false {~' \
  'if false {' \
  mnema-desktop "${T}opencode_a_file_it_cannot_hand_back_unchanged_is_refused_and_left_alone" --lib

case_ "opencode: a file without a trailing newline must not gain one" \
  $W \
  's~    Ok\(if newline \{ format!\("\{text\}\\n"\) \} else \{ text \}\)~    Ok(format!("{text}\\n"))~' \
  'Ok(format!("{text}\n"))' \
  mnema-desktop "${T}opencode_a_two_space_file_keeps_the_newline_it_had" --lib

case_ "opencode: a file with a trailing newline must keep it" \
  $W \
  's~    Ok\(if newline \{ format!\("\{text\}\\n"\) \} else \{ text \}\)~    Ok(text)~' \
  'Ok(text)
}' \
  mnema-desktop "${T}opencode_a_two_space_file_keeps_the_newline_it_had" --lib

case_ "opencode: a symlinked config is followed, not replaced by a regular file" \
  $W \
  's~    let target = std::fs::canonicalize\(path\)\.unwrap_or_else\(\|_\| path\.to_path_buf\(\)\);~    let target = path.to_path_buf();~' \
  'let target = path.to_path_buf();' \
  mnema-desktop "${T}opencode_a_symlinked_file_stays_a_link_and_its_target_is_rewritten" --lib

case_ "opencode: an emptied servers object is left in place" \
  $W \
  's~        removed \|= servers\.shift_remove\("mnema"\)\.is_some\(\);\n    \}\n    if !removed \{~        removed |= servers.shift_remove("mnema").is_some();\n    }\n    if mcp.get("servers").and_then(Value::as_object).is_some_and(|servers| servers.is_empty()) {\n        mcp.shift_remove("servers");\n    }\n    if !removed {~' \
  'mcp.shift_remove("servers");' \
  mnema-desktop "${T}opencode_an_emptied_servers_object_is_left_in_place" --lib

# ── Task 3: finding the CLIs, connecting and disconnecting them ──────────────

case_ "agent_clients: claude on a stale path is removed before it is added" \
  $W \
  's~            remove\(env, id, cli, timeout\)\?;\n            add\(env, id, cli, timeout\)~            add(env, id, cli, timeout)~' \
  'ClientId::Claude => {
            add(env, id, cli, timeout)' \
  mnema-desktop "${T}claude_on_a_stale_path_is_removed_before_it_is_added" --lib

case_ "agent_clients: codex on a stale path is only added, never removed first" \
  $W \
  's~        \(true, Link::Stale \{ \.\. \}\) if id == ClientId::Claude => \{~        (true, Link::Stale { .. }) => {~' \
  '(true, Link::Stale { .. }) => {' \
  mnema-desktop "${T}codex_on_a_stale_path_is_only_added" --lib

case_ "agent_clients: the write re-reads the file before replacing it" \
  $W \
  's~    if now != expected \{~    if false {~' \
  'if false {' \
  mnema-desktop "${T}a_file_another_writer_changes_mid_disconnect_is_left_to_that_writer" --lib

case_ "agent_clients: a CLI runs with the login shell's PATH" \
  $W \
  's~    if let Some\(path\) = path \{\n        command\.env\("PATH", path\);\n    \}~    if let Some(path) = path {\n        let _ = path;\n    }~' \
  'let _ = path;' \
  mnema-desktop "${T}the_login_path_reaches_every_cli_run" --lib

case_ "agent_clients: a timed-out child is killed and waited for" \
  $W \
  's~            Ok\(None\) if Instant::now\(\) >= deadline => \{\n                kill_group\(&mut child\);\n                let _ = child\.wait\(\);~            Ok(None) if Instant::now() >= deadline => {\n                kill_group(&mut child);~' \
  'kill_group(&mut child);
                return Err(format!("timed out after' \
  mnema-desktop "${T}a_child_that_outlives_the_timeout_is_killed_with_its_group" --lib

case_ "agent_clients: apply keeps going after a client that fails" \
  $W \
  's~            Ok\(Some\(cli\)\) => applied\.push\(apply_one\(\n                &env,\n                change\.id,\n                &cli,\n                change\.connect,\n                run_timeout,\n            \)+\),~            Ok(Some(cli)) => {\n                let one = apply_one(&env, change.id, &cli, change.connect, run_timeout);\n                let failed = one.error.is_some();\n                applied.push(one);\n                if failed {\n                    return applied;\n                }\n            }~' \
  'if failed {' \
  mnema-desktop "${T}a_client_that_fails_does_not_stop_the_one_after_it" --lib

case_ "agent_clients: states answers in ALL order, not in the order the threads finish" \
  $W \
  's~\Q        let mut threads = Vec::with_capacity(ALL.len());
        for id in ALL {
            threads.push((id, scope.spawn(move || state_of(machine, id, run_timeout))));
        }
        threads
            .into_iter()
            .map(|(id, thread)| {
                thread.join().unwrap_or_else(|_| ClientState {
                    id,
                    installed: false,
                    link: None,
                    error: Some("the thread reading this client panicked".to_string()),
                })
            })
            .collect()\E~        let (send, receive) = std::sync::mpsc::channel();
        let mut threads = Vec::with_capacity(ALL.len());
        for id in ALL {
            let send = send.clone();
            threads.push(scope.spawn(move || {
                let state = state_of(machine, id, run_timeout);
                let _ = send.send(state);
            }));
        }
        drop(send);
        for thread in threads {
            let _ = thread.join();
        }
        std::iter::from_fn(|| receive.recv().ok()).collect()~' \
  'std::iter::from_fn(|| receive.recv().ok()).collect()' \
  mnema-desktop "${T}states_keeps_all_order_whatever_thread_finishes_first" --lib

case_ "agent_clients: no opencode file is an absent client, not an error" \
  $W \
  's~        Err\(error\) if error\.kind\(\) == std::io::ErrorKind::NotFound => Ok\(Link::Absent\),~        Err(error) => Err(format!("{}: {error}", path.display())),~' \
  'Err(format!("{}: {error}", path.display())),
        Err(error)' \
  mnema-desktop "${T}an_opencode_without_a_file_is_absent_and_connect_goes_through_its_cli" --lib

case_ "agent_clients: a state that cannot be read is neither added nor removed" \
  $W \
  's~    let outcome = read_link\(env, id, cli, timeout\)\.and_then\(\|link\| match \(connect, link\) \{~    let outcome = read_link(env, id, cli, timeout)\n        .or(Ok(Link::Absent))\n        .and_then(|link| match (connect, link) {~' \
  '.or(Ok(Link::Absent))' \
  mnema-desktop "${T}a_client_whose_state_cannot_be_read_is_left_untouched" --lib

case_ "agent_clients: a current client the person unticked is removed" \
  $W \
  's~        \(false, _\) => remove\(env, id, cli, timeout\),~        (false, _) => Ok(()),~' \
  '(false, _) => Ok(()),' \
  mnema-desktop "${T}apply_removes_a_current_client_the_person_unticked" --lib

case_ "agent_clients: a CLI that is not there is not reported as installed" \
  $W \
  's~        Ok\(None\) => \{\n            return ClientState \{\n                id,\n                installed: false,~        Ok(None) => {\n            return ClientState {\n                id,\n                installed: true,~' \
  'Ok(None) => {
            return ClientState {
                id,
                installed: true,' \
  mnema-desktop "${T}states_reports_an_installed_client_and_leaves_the_others_alone" --lib

case_ "agent_clients: opencode's disconnect rewrites its file" \
  $W \
  's~fn remove_opencode\(env: &Env\) -> Result<\(\), String> \{\n    let path = env\.opencode_file\(\)\?;~fn remove_opencode(env: &Env) -> Result<(), String> {\n    return Ok(());\n    let path = env.opencode_file()?;~' \
  'return Ok(());
    let path = env.opencode_file()?;' \
  mnema-desktop "${T}disconnecting_opencode_removes_the_key_from_its_file" --lib

case_ "agent_clients: a shell that will not answer is replaced by the second one" \
  $W \
  's~    match ask\(fallback, timeout\) \{~    match Err::<String, String>(first.clone()) {~' \
  'match Err::<String, String>(first.clone()) {' \
  mnema-desktop "${T}a_shell_that_will_not_answer_falls_back_to_the_second_one" --lib

case_ "agent_clients: the PATH is the text after the last mark, not the whole output" \
  $W \
  's~    stdout\n        \.lines\(\)\n        \.filter_map\(\|line\| line\.trim_end\(\)\.strip_prefix\(PATH_MARK\)\)\n        \.rfind\(\|path\| !path\.is_empty\(\)\)\n        \.map\(str::to_string\)~    Some(stdout.trim().to_string())~' \
  'Some(stdout.trim().to_string())' \
  mnema-desktop "${T}a_chatty_shell_does_not_corrupt_the_path" --lib

case_ "agent_clients: a failed discovery answers for every listed client" \
  $W \
  's~            Err\(error\) => applied\.push\(Applied \{\n                id: change\.id,\n                error: Some\(error\),\n            \}\),~            Err(error) => {\n                let _ = error;\n            }~' \
  'Err(error) => {
                let _ = error;
            }' \
  mnema-desktop "${T}a_shell_that_will_not_say_its_path_is_an_error_for_every_client" --lib

case_ "agent_clients: remove_opencode writes through the re-read, not around it" \
  $W \
  's~    write_if_unchanged\(&path, &text, &text_without\)\n        \.map_err\(\|error\| format!\("\{\}: \{error\}", path\.display\(\)\)\)~    write_atomically(&path, &text_without).map_err(|error| format!("{}: {error}", path.display()))~' \
  'write_atomically(&path, &text_without)' \
  mnema-desktop "${T}a_file_another_writer_changes_mid_disconnect_is_left_to_that_writer" --lib

case_ "agent_clients: a timed-out child is killed with its whole group" \
  $W \
  's~    let _ = Command::new\("kill"\)\n        \.arg\("-KILL"\)\n        \.arg\(format!\("-\{\}", child\.id\(\)\)\)\n        \.stdin\(Stdio::null\(\)\)\n        \.stdout\(Stdio::null\(\)\)\n        \.stderr\(Stdio::null\(\)\)\n        \.status\(\);\n~~' \
  'nothing to report.
    let _ = child.kill();' \
  mnema-desktop "${T}a_child_that_outlives_the_timeout_is_killed_with_its_group" --lib

case_ "agent_clients: no home directory is an error, not a relative config path" \
  $W \
  's~            \(None, None\) => Err\("could not find your home directory"\.to_string\(\)\),~            (None, None) => Ok(opencode_config(None, Path::new(""))),~' \
  'Ok(opencode_config(None, Path::new(""))),' \
  mnema-desktop "${T}an_unset_home_is_an_error_and_not_a_relative_path" --lib

case_ "agent_clients: a reader waits for the answer but not past its deadline" \
  $W \
  's~            \.recv_timeout\(deadline\.saturating_duration_since\(Instant::now\(\)\)\)\n            \.ok\(\),~            .recv()\n            .ok(),~' \
  '.recv()
            .ok(),' \
  mnema-desktop "${T}a_reader_that_answers_late_does_not_hold_the_command_past_its_deadline" --lib

case_ "agent_clients: a reader that missed its deadline is an error, not an empty answer" \
  $W \
  's~            \.recv_timeout\(deadline\.saturating_duration_since\(Instant::now\(\)\)\)\n            \.ok\(\),~            .recv_timeout(deadline.saturating_duration_since(Instant::now()))\n            .ok().or(Some(String::new())),~' \
  '.ok().or(Some(String::new())),' \
  mnema-desktop "${T}a_command_whose_output_stays_open_is_an_error_not_an_empty_answer" --lib

case_ "agent_clients: a normal exit signals no group, however the child left the pipes" \
  $W \
  's~                return Ok\(Output \{~                kill_group(&mut child);\n                return Ok(Output {~' \
  'kill_group(&mut child);
                return Ok(Output {' \
  mnema-desktop "${T}a_finished_command_leaves_a_background_job_that_holds_no_pipe_alone" --lib

case_ "agent_clients: only absolute PATH entries are searched for a CLI" \
  $W \
  's~    std::env::split_paths\(path\)\.filter\(\|dir\| dir\.is_absolute\(\)\)~    std::env::split_paths(path)~' \
  '    std::env::split_paths(path)
}' \
  mnema-desktop "${T}a_path_entry_that_is_relative_is_not_searched" --lib

case_ "agent_clients: the readers get their own grace, not what is left of the child's clock" \
  $W \
  's~                let end = Instant::now\(\) \+ grace;~                let end = deadline;~' \
  'let end = deadline;' \
  mnema-desktop "${T}a_command_whose_output_stays_open_is_an_error_not_an_empty_answer" --lib

# ── Task 4: the checkboxes and one Apply button ──────────────────────────────
# The section itself (`ui/src/settings/Agents.svelte`), the wire wrappers in
# `ui/src/lib/ipc.ts`, and the two Rust structs the window mirrors.

A=ui/src/settings/Agents.svelte
AT=src/settings/Agents.test.ts

case_ "Agents.svelte: a state that could not be read shows the error, not the note" \
  $A \
  's~      \{#if row\.state\?\.error\}\n        <details data-testid=\{`agents-error-\$\{row\.id\}`\}>\n          <summary>\{detailsLabel\}</summary>\n          <p>\{row\.state\.error\}</p>\n        </details>\n      \{:else if row\.note\}\n        <p class="fc" data-testid=\{`agents-note-\$\{row\.id\}`\}>\{row\.note\}</p>\n      \{/if\}~      {#if row.note}\n        <p class="fc" data-testid={`agents-note-\${row.id}`}>{row.note}</p>\n      {:else if row.state?.error}\n        <details data-testid={`agents-error-\${row.id}`}>\n          <summary>{detailsLabel}</summary>\n          <p>{row.state.error}</p>\n        </details>\n      {/if}~; s~state === undefined \|\| state\.error !== null \? null~state === undefined ? null~' \
  'data-testid={`agents-note-${row.id}`}>{row.note}</p>' \
  $AT 'a client whose state could not be read shows the error, not "not installed"' runner=vitest

case_ "Agents.svelte: Apply re-reads the record instead of drawing the request" \
  $A \
  's~    await readClients\(\);\n    applying = false;~    applying = false;~' \
  'applyRejected = message(err);
    }
    applying = false;' \
  $AT 'after Apply the boxes show the re-read state, not what was asked for' runner=vitest

case_ "Agents.svelte: Apply is disabled while nothing would change" \
  $A \
  's~const canApply = \$derived\(states !== null && !applying && rows\.some\(\(row\) => row\.change\)\);~const canApply = \$derived(states !== null && !applying);~' \
  'const canApply = $derived(states !== null && !applying);' \
  $AT 'Apply is disabled until the person changes a box' runner=vitest

case_ "Agents.svelte: every client's failure is kept, not only the first" \
  $A \
  's~error: one\.error \}\]\);~error: one.error }]).slice(0, 1);~' \
  '.slice(0, 1);' \
  $AT 'one client failing is named with its direction and does not hide another' runner=vitest

case_ "Agents.svelte: no copy button, command line or JSON fragment is drawn" \
  $A \
  's~    <h3 id="agents-clients-heading">\{clientsHeading\}</h3>~    <h3 id="agents-clients-heading">{clientsHeading}</h3>\n    <button type="button" data-testid="agents-copy-cmd">mcp add</button>~' \
  'data-testid="agents-copy-cmd"' \
  $AT 'no command line, JSON fragment or copy button is left in the section' runner=vitest

case_ "Agents.svelte: a stale record is a change an add corrects" \
  $A \
  's~        change: !disabled\n          && \(checked !== connected\(state\) \|\| \(checked && state\.link\?\.kind === '"'"'stale'"'"'\)\),~        change: !disabled\n          && (checked !== connected(state)),~' \
  'change: !disabled
          && (checked !== connected(state)),' \
  $AT 'an outdated path alone enables Apply' runner=vitest

case_ "Agents.svelte: the failure sentence names the direction the box moved" \
  $A \
  's~      one\.connecting \? '"'"'agents_apply_failed'"'"' : '"'"'agents_disconnect_failed'"'"',~      '"'"'agents_apply_failed'"'"',~' \
  "'agents_apply_failed',
      { name:" \
  $AT 'one client failing is named with its direction and does not hide another' runner=vitest

case_ "Agents.svelte: a CLI that is not installed cannot be ticked" \
  $A \
  's~const disabled = applying \|\| state === undefined \|\| !state\.installed \|\| state\.error !== null;~const disabled = applying || state === undefined || state.error !== null;~' \
  'const disabled = applying || state === undefined || state.error !== null;' \
  $AT 'a client whose CLI is not installed is disabled and says so' runner=vitest

case_ "Agents.svelte: a state that could not be read is never sent with a press" \
  $A \
  's~      \.filter\(\(row\) => row\.change\)~      .filter((row) => row.change || row.disabled)~' \
  'row.change || row.disabled' \
  $AT 'a client whose state could not be read is never sent, even when Apply is about another' runner=vitest

case_ "Agents.svelte: every box is a change, moved or not" \
  $A \
  's~const changes: Change\[\] = rows\n      \.filter\(\(row\) => row\.change\)\n      \.map~const changes: Change[] = rows\n      .map~' \
  'const changes: Change[] = rows
      .map(' \
  $AT 'Apply sends a change per box the person moved, and nothing for the rest' runner=vitest

case_ "Agents.svelte: a rejected press is its own sentence, not a failed read" \
  $A \
  's~      applyRejected = message\(err\);~      readFailure = message(err);~' \
  'readFailure = message(err);
    }
    await readClients();' \
  $AT 'a press the command did not answer is its own sentence, not a failed read' runner=vitest

case_ "Agents.svelte: a rejected press is reported before the re-read, not after" \
  $A \
  's~    applying = true;~    applying = true;\n    let held: string | null = null;~; s~      applyRejected = message\(err\);~      held = message(err);~; s~    await readClients\(\);\n    applying = false;~    await readClients();\n    applyRejected = held;\n    applying = false;~' \
  'applyRejected = held;' \
  $AT 'a rejected press is reported at once, without waiting for the re-read' runner=vitest

case_ "Agents.svelte: a read that answers drops the pick it was made against" \
  $A \
  's~      choice = null;\n~~' \
  "the boxes are still the person's.
    } catch (err) {" \
  $AT 'a re-read that disables a box drops the pick that box was carrying' runner=vitest

case_ "Agents.svelte: a read that fails keeps the pick it cannot replace" \
  $A \
  's~      readFailure = message\(err\);\n    \}\n  \}\n\n  async function toggle~      readFailure = message(err);\n      choice = null;\n    }\n  }\n\n  async function toggle~' \
  'readFailure = message(err);
      choice = null;' \
  $AT 'a failed re-read keeps the pick and says the state is unknown' runner=vitest

I=ui/src/lib/ipc.ts
IT=src/lib/ipc.test.ts

case_ "ipc: agentClients invokes agent_clients, not a longer name" \
  $I \
  's~invoke<ClientState\[\]>\(\x27agent_clients\x27\)~invoke<ClientState[]>(\x27agent_clients_all\x27)~' \
  'agent_clients_all' \
  $IT 'agentClients invokes agent_clients with no arguments at all' runner=vitest

case_ "ipc: applyAgentClients sends the changes under the name the command declares" \
  $I \
  's~invoke<Applied\[\]>\('"'"'apply_agent_clients'"'"', \{ changes \}\)~invoke<Applied[]>('"'"'apply_agent_clients'"'"', { want: changes })~' \
  '{ want: changes }' \
  $IT 'applyAgentClients sends the changes under the name the command declares' runner=vitest

case_ "ipc: ClientState's link field is the name agent_clients.rs defines" \
  src-tauri/src/agent_clients.rs \
  's~    pub link: Option<Link>,~    pub server: Option<Link>,~' \
  'pub server: Option<Link>,' \
  $IT 'ClientState is exactly what agent_clients.rs defines, field for field' runner=vitest

case_ "ipc: Link's stale arm names its command" \
  src-tauri/src/agent_clients.rs \
  's~    Stale \{ command: String \},~    Stale { path: String },~' \
  'Stale { path: String }' \
  $IT 'Link is exactly what agent_clients.rs defines, and a stale record names its command' runner=vitest

case_ "ipc: ClientId's variants are lowercase on the wire" \
  src-tauri/src/agent_clients.rs \
  's~#\[serde\(rename_all = "lowercase"\)\]\npub enum ClientId~#[serde(rename_all = "camelCase")]\npub enum ClientId~' \
  '#[serde(rename_all = "camelCase")]
pub enum ClientId' \
  $IT 'ClientId is exactly what agent_clients.rs defines, in the spelling serde sends' runner=vitest

case_ "agent_clients: apply reads only the clients the person touched" \
  $W \
  's~    for change in changes \{\n        match locate\(&env, change\.id\) \{~    for id in ALL {\n        let change = changes.iter().find(|change| change.id == id).copied().unwrap_or(Change { id, connect: false });\n        match locate(&env, change.id) {~' \
  "unwrap_or(Change { id, connect: false });" \
  mnema-desktop "${T}a_client_the_person_did_not_touch_is_never_run" --lib

case_ "agent_clients: a change's direction travels on the wire" \
  $W \
  's~    pub id: ClientId,\n    pub connect: bool,~    pub id: ClientId,\n    #[serde(skip)]\n    pub connect: bool,~' \
  '#[serde(skip)]
    pub connect: bool,' \
  mnema-desktop "${T}a_change_reaches_the_backend_with_the_fields_it_names" --lib

case_ "ipc: Change's field is connect, not wanted" \
  src-tauri/src/agent_clients.rs \
  's~    pub connect: bool,~    pub wanted: bool,~' \
  'pub wanted: bool,' \
  $IT 'Change is exactly what agent_clients.rs defines, field for field' runner=vitest
