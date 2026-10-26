//! Each agent CLI's own answer about the `mnema` MCP server, read back as one
//! of three states. Four CLIs, four shapes — Claude prints labelled lines, Codex
//! and pi JSON, opencode keeps it in `opencode.json` — so the shapes are pinned
//! here against output recorded 2026-10-26 rather than guessed at.
//!
//! Pure functions over [`Output`] and text: no process runs and no file is read,
//! which is what lets a test reproduce any shape without an agent installed.
//! [`write_atomically`] is the one place this module writes — opencode has no
//! command that removes a server, so its config file is the one file the
//! application edits, and the surface for that is kept as small as it can be.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One of the agents the settings window offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientId {
    Claude,
    Codex,
    Pi,
    Opencode,
}

pub const ALL: [ClientId; 4] = [
    ClientId::Claude,
    ClientId::Codex,
    ClientId::Pi,
    ClientId::Opencode,
];

/// What one client's own record says about the `mnema` server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Link {
    Absent,
    Current,
    Stale { command: String },
}

/// What a CLI printed: exit code, stdout, stderr.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The CLI's own words for the "Details" fold: stderr when it said anything,
/// stdout otherwise, trimmed.
fn said(out: &Output) -> String {
    let text = if out.stderr.trim().is_empty() {
        &out.stdout
    } else {
        &out.stderr
    };
    text.trim().to_string()
}

/// The value of a `Label:` line, to the end of that line.
///
/// To the end of the line rather than to the next space because a command's
/// path can contain one — `/tmp/My App's/mnema` does.
fn field<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(label))
        .map(str::trim_start)
}

/// Current only when both halves are ours: the right path with other arguments
/// is a record "Застосувати" must rewrite, not one it may leave alone.
fn link_of(command: &str, args: &[&str], ours: &str) -> Link {
    if command == ours && args == ["mcp"] {
        Link::Current
    } else {
        Link::Stale {
            command: command.to_string(),
        }
    }
}

/// `claude mcp get mnema`: exit 0 prints labelled lines, exit 1 says the server
/// is not there.
pub fn parse_claude(out: &Output, ours: &str) -> Result<Link, String> {
    let said = said(out);
    if out.code == 1 && said.starts_with("No MCP server named") {
        return Ok(Link::Absent);
    }
    if out.code != 0 {
        return Err(said);
    }
    // `get` answers for whichever scope holds the name; only the user one is
    // ours to read and to remove, so a `local`/`project` record is "not
    // connected" rather than a second entry we would add beside it.
    let scope = field(&out.stdout, "Scope:").unwrap_or_default();
    if !scope.starts_with("User config") {
        return Ok(Link::Absent);
    }
    let command = field(&out.stdout, "Command:").ok_or(said)?;
    let args: Vec<&str> = field(&out.stdout, "Args:")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    Ok(link_of(command, &args, ours))
}

/// `codex mcp get mnema --json`: a JSON record, or exit 1 saying it is missing.
pub fn parse_codex(out: &Output, ours: &str) -> Result<Link, String> {
    let said = said(out);
    // `contains`, not `starts_with`: codex prefixes its message with `Error: `.
    if out.code == 1 && said.contains("No MCP server named") {
        return Ok(Link::Absent);
    }
    if out.code != 0 {
        return Err(said);
    }
    let record: Value = serde_json::from_str(&out.stdout).map_err(|_| said.clone())?;
    let transport = &record["transport"];
    let command = transport["command"].as_str().ok_or_else(|| said.clone())?;
    let args: Vec<&str> = transport["args"]
        .as_array()
        .map(|args| args.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Ok(link_of(command, &args, ours))
}

/// `pi mcp list --json`: every scope in one list, and the transport is a single
/// string with the arguments already appended to the command.
pub fn parse_pi(out: &Output, ours: &str) -> Result<Link, String> {
    let said = said(out);
    if out.code != 0 {
        return Err(said);
    }
    let record: Value = serde_json::from_str(&out.stdout).map_err(|_| said.clone())?;
    let server = record["servers"].as_array().and_then(|servers| {
        servers.iter().find(|server| {
            server["name"].as_str() == Some("mnema") && server["scope"].as_str() == Some("global")
        })
    });
    let Some(server) = server else {
        return Ok(Link::Absent);
    };
    let transport = server["transport"].as_str().ok_or(said)?;
    Ok(if transport == format!("{ours} mcp") {
        Link::Current
    } else {
        Link::Stale {
            command: transport
                .strip_suffix(" mcp")
                .unwrap_or(transport)
                .to_string(),
        }
    })
}

/// `$XDG_CONFIG_HOME/opencode/opencode.json`, else `~/.config/opencode/opencode.json`.
///
/// An empty `$XDG_CONFIG_HOME` counts as unset, as the XDG spec says; taken
/// literally it would make every path here a relative one.
pub fn opencode_config(xdg: Option<&Path>, home: &Path) -> PathBuf {
    match xdg.filter(|dir| !dir.as_os_str().is_empty()) {
        Some(xdg) => xdg.join("opencode").join("opencode.json"),
        None => home.join(".config").join("opencode").join("opencode.json"),
    }
}

/// `mcp.servers.mnema` is the form `opencode mcp add --global` writes;
/// `mcp.mnema` is the older one, and opencode still reads servers there — the
/// owner's own file keeps them that way.
pub fn parse_opencode(text: &str, ours: &str) -> Result<Link, String> {
    let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let mcp = root.get("mcp");
    let new_form = mcp
        .and_then(|mcp| mcp.get("servers"))
        .and_then(|servers| servers.get("mnema"));
    let (key, entry) = if let Some(entry) = new_form {
        ("mcp.servers.mnema", entry)
    } else if let Some(entry) = mcp.and_then(|mcp| mcp.get("mnema")) {
        ("mcp.mnema", entry)
    } else {
        return Ok(Link::Absent);
    };
    let command: Vec<&str> = entry
        .get("command")
        .and_then(Value::as_array)
        .map(|command| command.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some((command, args)) = command.split_first() else {
        return Err(format!("{key} has no command"));
    };
    Ok(link_of(command, args, ours))
}

/// What both refusals answer: this application removes one key from a file it
/// does not own, so a file it cannot hand back otherwise untouched is left
/// exactly as it is.
const UNTOUCHED: &str = "the file is not in the form opencode writes, so it is left as it is";

/// The file's text with `mcp.servers.mnema` and `mcp.mnema` gone — `Ok(None)`
/// when neither was there, so the caller has nothing to write.
///
/// A file this function cannot hand back otherwise byte-for-byte is refused,
/// not repaired. Comments and trailing commas do not parse at all; indentation,
/// `\u` escapes, `1e3`, an integer too wide for 64 bits and a key written twice
/// all parse to a value whose own rendering is a different file. One key is not
/// worth re-exporting a person's config to guess at.
pub fn opencode_without_mnema(text: &str) -> Result<Option<String>, String> {
    let mut root: Value =
        serde_json::from_str(text).map_err(|error| format!("{UNTOUCHED}: {error}"))?;
    let newline = text.ends_with('\n');
    // Taken before anything is removed: this is the shape check.
    let unmodified = written_form(&root, newline)?;
    let Some(mcp) = root.get_mut("mcp").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    // `shift_remove`, not `remove`: under `preserve_order`, `remove` is
    // `swap_remove`, which pulls the last key into the hole.
    let mut removed = mcp.shift_remove("mnema").is_some();
    if let Some(servers) = mcp.get_mut("servers").and_then(Value::as_object_mut) {
        removed |= servers.shift_remove("mnema").is_some();
    }
    if !removed {
        return Ok(None);
    }
    if unmodified != text {
        return Err(UNTOUCHED.to_string());
    }
    Ok(Some(written_form(&root, newline)?))
}

/// The text `opencode` itself writes — `to_string_pretty`, plus the trailing
/// newline the file had. Its `mcp add` leaves none; the owner's file has one,
/// and neither is ours to change.
fn written_form(value: &Value, newline: bool) -> Result<String, String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    Ok(if newline { format!("{text}\n") } else { text })
}

/// Replaces the file with `text` through a temporary sibling and a `rename`, so
/// an interrupted write leaves the old file whole.
///
/// The target's permissions are copied before the text is written: a `0600`
/// file must not become readable to anyone else even while its replacement is
/// being filled.
#[allow(dead_code)] // no caller in the shipped build until Task 3's `apply`
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    // A dotfiles `opencode.json` is a symlink: renaming onto the link replaces
    // it and leaves the file it points at untouched, so the temporary file and
    // the rename belong beside the real one.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = target.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(temp_name(&target));
    let written = write_then_rename(&tmp, &target, text);
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// A name no other writer takes — the pid separates processes, the counter
/// threads — so two writes never fill the same temporary file.
#[allow(dead_code)] // no caller in the shipped build until Task 3's `apply`
fn temp_name(path: &Path) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

#[allow(dead_code)] // no caller in the shipped build until Task 3's `apply`
fn write_then_rename(tmp: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    {
        // `create_new`, not `create`: a file already carrying this name is
        // never truncated and filled with bytes it did not have.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)?;
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    // Closed first: Windows will not rename a file another handle holds open.
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    const OURS: &str = "/tmp/My App's/mnema";

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn keys(value: &Value) -> Vec<String> {
        value.as_object().unwrap().keys().cloned().collect()
    }

    /// `claude mcp get mnema`, exit 0 — recorded 2026-10-26.
    const CLAUDE_GET: &str = "\
mnema:
  Scope: User config (available in all your projects)
  Status: ✘ Failed to connect
  Issue: ENOENT: ENOENT: no such file or directory, posix_spawn '/tmp/My App's/mnema'
  Type: stdio
  Command: /tmp/My App's/mnema
  Args: mcp
  Environment:

To remove this server, run: claude mcp remove mnema -s user
";

    /// `codex mcp get mnema --json`, exit 0 — recorded 2026-10-26.
    const CODEX_GET: &str = r#"{"name":"mnema","enabled":true,"transport":{"type":"stdio","command":"/tmp/My App's/mnema","args":["mcp"]}}"#;

    /// `pi mcp list --json`, exit 0 — recorded 2026-10-26.
    const PI_LIST: &str = r#"{"servers":[{"name":"mnema","scope":"global","enabled":true,"transport":"/tmp/My App's/mnema mcp","state":"failed","tools":[],"error":"spawn /tmp/My App's/mnema ENOENT"}],"errors":[]}"#;

    /// A whole `opencode.json` of the kind the owner keeps, in exactly the
    /// bytes `opencode` writes them: 2-space `to_string_pretty` with a trailing
    /// newline, keys in no alphabetical order, `mnema` first in both places it
    /// can sit, and nested objects under each.
    const OPENCODE_JSON: &str = r#"{
  "theme": "opencode",
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "mnema": {
      "type": "local",
      "command": [
        "/tmp/My App's/mnema",
        "mcp"
      ]
    },
    "zeta": {
      "type": "local",
      "command": [
        "zeta",
        "mcp"
      ],
      "environment": {
        "Z_LAST": "1",
        "A_FIRST": "2"
      }
    },
    "servers": {
      "mnema": {
        "type": "local",
        "command": [
          "/Applications/Mnema.app/Contents/MacOS/mnema",
          "mcp"
        ]
      },
      "omega": {
        "type": "local",
        "command": [
          "omega",
          "serve"
        ]
      },
      "alpha": {
        "type": "remote",
        "url": "https://example.com/mcp"
      }
    }
  },
  "agent": {
    "build": {
      "model": "anthropic/claude",
      "temperature": 0.1
    }
  }
}
"#;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn claude_a_missing_server_is_absent_and_another_failure_is_an_error() {
        let missing = out(
            1,
            "",
            "No MCP server named \"mnema\". Configured servers: …",
        );
        assert_eq!(parse_claude(&missing, OURS), Ok(Link::Absent));
        let failed = out(1, "", "Error: could not read the user config");
        assert_eq!(
            parse_claude(&failed, OURS),
            Err("Error: could not read the user config".to_string())
        );
    }

    #[test]
    fn claude_a_server_in_another_scope_is_not_ours() {
        let text = CLAUDE_GET.replace("User config", "Local config");
        assert_eq!(parse_claude(&out(0, &text, ""), OURS), Ok(Link::Absent));
    }

    #[test]
    fn claude_our_command_and_args_are_current() {
        assert_eq!(
            parse_claude(&out(0, CLAUDE_GET, ""), OURS),
            Ok(Link::Current)
        );
    }

    #[test]
    fn claude_another_command_is_stale() {
        let text = CLAUDE_GET.replace(
            "Command: /tmp/My App's/mnema",
            "Command: /Applications/Mnema.app/Contents/MacOS/mnema",
        );
        assert_eq!(
            parse_claude(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: "/Applications/Mnema.app/Contents/MacOS/mnema".to_string()
            })
        );
    }

    #[test]
    fn claude_another_args_is_stale() {
        let text = CLAUDE_GET.replace("Args: mcp", "Args: mcp --verbose");
        assert_eq!(
            parse_claude(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: OURS.to_string()
            })
        );
    }

    #[test]
    fn claude_without_a_command_line_is_an_error_carrying_the_output() {
        let text = CLAUDE_GET.replace("  Command: /tmp/My App's/mnema\n", "");
        assert_eq!(
            parse_claude(&out(0, &text, ""), OURS),
            Err(text.trim().to_string())
        );
    }

    #[test]
    fn claude_an_unexpected_exit_code_is_an_error() {
        assert_eq!(
            parse_claude(&out(2, "noise", "  boom  "), OURS),
            Err("boom".to_string())
        );
    }

    #[test]
    fn codex_a_missing_server_is_absent() {
        let missing = out(1, "", "Error: No MCP server named 'mnema' found.");
        assert_eq!(parse_codex(&missing, OURS), Ok(Link::Absent));
    }

    #[test]
    fn codex_our_command_and_args_are_current() {
        assert_eq!(parse_codex(&out(0, CODEX_GET, ""), OURS), Ok(Link::Current));
    }

    #[test]
    fn codex_another_command_is_stale() {
        let text = CODEX_GET.replace("/tmp/My App's/mnema", "/usr/local/bin/mnema");
        assert_eq!(
            parse_codex(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: "/usr/local/bin/mnema".to_string()
            })
        );
    }

    #[test]
    fn codex_another_args_is_stale() {
        let text = CODEX_GET.replace(r#""args":["mcp"]"#, r#""args":["serve"]"#);
        assert_eq!(
            parse_codex(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: OURS.to_string()
            })
        );
    }

    #[test]
    fn codex_output_that_is_not_json_is_an_error() {
        assert_eq!(
            parse_codex(&out(0, "not json", ""), OURS),
            Err("not json".to_string())
        );
    }

    #[test]
    fn codex_an_unexpected_exit_code_is_an_error() {
        assert_eq!(
            parse_codex(&out(2, "noise", "  boom  "), OURS),
            Err("boom".to_string())
        );
    }

    #[test]
    fn pi_a_global_server_with_our_transport_is_current() {
        assert_eq!(parse_pi(&out(0, PI_LIST, ""), OURS), Ok(Link::Current));
    }

    #[test]
    fn pi_another_transport_is_stale_without_the_trailing_mcp() {
        let text = PI_LIST.replace("/tmp/My App's/mnema mcp", "/usr/local/bin/mnema mcp");
        assert_eq!(
            parse_pi(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: "/usr/local/bin/mnema".to_string()
            })
        );
    }

    #[test]
    fn pi_our_transport_without_the_argument_is_stale() {
        let text = PI_LIST.replace("/tmp/My App's/mnema mcp", "/tmp/My App's/mnema");
        assert_eq!(
            parse_pi(&out(0, &text, ""), OURS),
            Ok(Link::Stale {
                command: OURS.to_string()
            })
        );
    }

    #[test]
    fn pi_a_server_in_another_scope_is_absent() {
        let text = PI_LIST.replace(r#""scope":"global""#, r#""scope":"project""#);
        assert_eq!(parse_pi(&out(0, &text, ""), OURS), Ok(Link::Absent));
    }

    #[test]
    fn pi_output_that_is_not_json_is_an_error() {
        assert_eq!(
            parse_pi(&out(0, "not json", ""), OURS),
            Err("not json".to_string())
        );
    }

    #[test]
    fn pi_an_unexpected_exit_code_is_an_error() {
        assert_eq!(
            parse_pi(&out(2, "noise", "  boom  "), OURS),
            Err("boom".to_string())
        );
    }

    #[test]
    fn opencode_config_is_xdg_then_home() {
        assert_eq!(
            opencode_config(Some(Path::new("/xdg")), Path::new("/home/u")),
            PathBuf::from("/xdg/opencode/opencode.json")
        );
        assert_eq!(
            opencode_config(None, Path::new("/home/u")),
            PathBuf::from("/home/u/.config/opencode/opencode.json")
        );
        // XDG says an empty value is as good as unset.
        assert_eq!(
            opencode_config(Some(Path::new("")), Path::new("/home/u")),
            PathBuf::from("/home/u/.config/opencode/opencode.json")
        );
    }

    #[test]
    fn opencode_a_server_under_mcp_servers_is_read() {
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"servers":{"mnema":{"type":"local","command":["/tmp/My App's/mnema","mcp"]}}}}"#,
                OURS
            ),
            Ok(Link::Current)
        );
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"servers":{"mnema":{"type":"local","command":["/usr/local/bin/mnema","mcp"]}}}}"#,
                OURS
            ),
            Ok(Link::Stale {
                command: "/usr/local/bin/mnema".to_string()
            })
        );
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"servers":{"mnema":{"type":"local","command":["/tmp/My App's/mnema","mcp","--x"]}}}}"#,
                OURS
            ),
            Ok(Link::Stale {
                command: OURS.to_string()
            })
        );
    }

    #[test]
    fn opencode_the_new_form_wins_when_both_are_present() {
        assert_eq!(
            parse_opencode(OPENCODE_JSON, OURS),
            Ok(Link::Stale {
                command: "/Applications/Mnema.app/Contents/MacOS/mnema".to_string()
            })
        );
    }

    #[test]
    fn opencode_the_old_form_is_read() {
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"context7":{"type":"remote","url":"https://x"},"mnema":{"type":"local","command":["/tmp/My App's/mnema","mcp"]}}}"#,
                OURS
            ),
            Ok(Link::Current)
        );
    }

    #[test]
    fn opencode_without_a_mnema_entry_is_absent() {
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"servers":{"zeta":{"type":"remote","url":"https://x"}}}}"#,
                OURS
            ),
            Ok(Link::Absent)
        );
        assert_eq!(parse_opencode("{}", OURS), Ok(Link::Absent));
        assert_eq!(parse_opencode(r#"{"mcp":null}"#, OURS), Ok(Link::Absent));
    }

    #[test]
    fn opencode_an_entry_without_a_command_is_an_error() {
        assert_eq!(
            parse_opencode(
                r#"{"mcp":{"servers":{"mnema":{"type":"remote","url":"https://x"}}}}"#,
                OURS
            ),
            Err("mcp.servers.mnema has no command".to_string())
        );
        assert_eq!(
            parse_opencode(r#"{"mcp":{"mnema":{}}}"#, OURS),
            Err("mcp.mnema has no command".to_string())
        );
    }

    #[test]
    fn opencode_text_that_is_not_json_is_an_error() {
        let jsonc = "{\n  // mine\n  \"mcp\": {}\n}";
        assert!(parse_opencode(jsonc, OURS).is_err());
        assert!(opencode_without_mnema(jsonc).is_err());
        assert!(opencode_without_mnema("not json").is_err());
    }

    #[test]
    fn opencode_removing_mnema_leaves_the_rest_of_the_file_in_its_order() {
        let text = opencode_without_mnema(OPENCODE_JSON).unwrap().unwrap();
        let file: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            file,
            json!({
                "theme": "opencode",
                "$schema": "https://opencode.ai/config.json",
                "mcp": {
                    "zeta": {
                        "type": "local",
                        "command": ["zeta", "mcp"],
                        "environment": {"Z_LAST": "1", "A_FIRST": "2"}
                    },
                    "servers": {
                        "omega": {"type": "local", "command": ["omega", "serve"]},
                        "alpha": {"type": "remote", "url": "https://example.com/mcp"}
                    }
                },
                "agent": {"build": {"model": "anthropic/claude", "temperature": 0.1}}
            })
        );
        assert_eq!(keys(&file), ["theme", "$schema", "mcp", "agent"]);
        assert_eq!(keys(&file["mcp"]), ["zeta", "servers"]);
        assert_eq!(keys(&file["mcp"]["servers"]), ["omega", "alpha"]);
        assert_eq!(
            keys(&file["mcp"]["zeta"]["environment"]),
            ["Z_LAST", "A_FIRST"]
        );
        assert_eq!(keys(&file["agent"]["build"]), ["model", "temperature"]);
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn opencode_the_old_form_is_removed_without_adding_anything() {
        let text = "{\n  \"mcp\": {\n    \"context7\": {\n      \"type\": \"remote\",\n      \"url\": \"https://x\"\n    },\n    \"mnema\": {}\n  }\n}\n";
        let new = opencode_without_mnema(text).unwrap().unwrap();
        assert_eq!(
            new,
            "{\n  \"mcp\": {\n    \"context7\": {\n      \"type\": \"remote\",\n      \"url\": \"https://x\"\n    }\n  }\n}\n"
        );
    }

    #[test]
    fn opencode_an_emptied_servers_object_is_left_in_place() {
        let text = "{\n  \"mcp\": {\n    \"servers\": {\n      \"mnema\": {}\n    }\n  }\n}\n";
        let new = opencode_without_mnema(text).unwrap().unwrap();
        assert_eq!(new, "{\n  \"mcp\": {\n    \"servers\": {}\n  }\n}\n");
    }

    #[test]
    fn opencode_mnema_first_in_the_object_leaves_the_other_keys_in_order() {
        // `mnema` first of four: `swap_remove` and `shift_remove` give
        // different files here, and every other key must keep its place.
        let text = r#"{
  "mcp": {
    "mnema": {},
    "zzz": {},
    "aaa": {},
    "servers": {
      "mnema": {},
      "zzz": {},
      "aaa": {}
    }
  }
}
"#;
        assert_eq!(
            opencode_without_mnema(text),
            Ok(Some(
                r#"{
  "mcp": {
    "zzz": {},
    "aaa": {},
    "servers": {
      "zzz": {},
      "aaa": {}
    }
  }
}
"#
                .to_string()
            ))
        );
    }

    #[test]
    fn opencode_a_two_space_file_keeps_the_newline_it_had() {
        let text = "{\n  \"mcp\": {\n    \"servers\": {\n      \"mnema\": {}\n    }\n  }\n}\n";
        let without = text.strip_suffix('\n').unwrap();
        let removed = "{\n  \"mcp\": {\n    \"servers\": {}\n  }\n}";
        assert_eq!(
            opencode_without_mnema(text),
            Ok(Some(format!("{removed}\n")))
        );
        assert_eq!(
            opencode_without_mnema(without),
            Ok(Some(removed.to_string()))
        );
    }

    #[test]
    fn opencode_without_mnema_says_so_when_there_is_nothing_to_remove() {
        assert_eq!(opencode_without_mnema("{}"), Ok(None));
        // Nothing to remove is decided before the layout check: a file nothing
        // is written to is never refused for its shape.
        assert_eq!(
            opencode_without_mnema(r#"{"mcp":{"servers":{}}}"#),
            Ok(None)
        );
        assert_eq!(opencode_without_mnema("{\n  \"mcp\": null\n}"), Ok(None));
    }

    #[test]
    fn opencode_a_file_it_cannot_hand_back_unchanged_is_refused_and_left_alone() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("opencode.json");
        for text in [
            // a comment and a trailing comma: not plain JSON at all
            "{\n  // the person's own comment\n  \"mcp\": {\"mnema\": {}}\n}\n",
            "{\n  \"mcp\": {\"mnema\": {}},\n}\n",
            // each of these parses, and each comes back a different file
            "{\n    \"mcp\": {\n        \"mnema\": {}\n    }\n}\n",
            "{\"mcp\":{\"mnema\":{}}}\n",
            "{\n  \"mcp\": {\n    \"mnema\": {}\n  },\n  \"caf\\u00e9\": \"\\u00e9\"\n}\n",
            "{\n  \"mcp\": {\n    \"mnema\": {}\n  },\n  \"x\": 1e3\n}\n",
            "{\n  \"mcp\": {\n    \"servers\": {}\n  },\n  \"mcp\": {\n    \"mnema\": {}\n  }\n}\n",
        ] {
            fs::write(&path, text).unwrap();
            // The exact call sequence the caller in the next task uses.
            let change = fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|text| opencode_without_mnema(&text))
                .and_then(|change| match change {
                    Some(text) => write_atomically(&path, &text).map_err(|error| error.to_string()),
                    None => Ok(()),
                });
            assert!(change.is_err(), "{text}");
            let error = change.unwrap_err();
            assert!(error.starts_with(UNTOUCHED), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
            assert_eq!(names(dir.path()), ["opencode.json"]);
        }
    }

    #[test]
    fn opencode_writing_atomically_leaves_only_the_target_in_the_directory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("opencode.json");
        write_atomically(&path, "{\n  \"a\": 1\n}\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"a\": 1\n}\n");
        assert_eq!(names(dir.path()), ["opencode.json"]);
    }

    /// A dotfiles `opencode.json` is a link into another directory: writing
    /// over the link would leave the person's real file with the old text.
    #[cfg(unix)]
    #[test]
    fn opencode_a_symlinked_file_stays_a_link_and_its_target_is_rewritten() {
        let dir = TempDir::new().unwrap();
        let real_dir = dir.path().join("dotfiles");
        fs::create_dir(&real_dir).unwrap();
        let real = real_dir.join("opencode.json");
        fs::write(&real, "{}\n").unwrap();
        let link = dir.path().join("opencode.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomically(&link, "{\n  \"a\": 1\n}\n").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "{\n  \"a\": 1\n}\n");
        assert_eq!(names(&real_dir), ["opencode.json"]);
        assert_eq!(names(dir.path()), ["dotfiles", "opencode.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn opencode_a_rewritten_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("opencode.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(&path, "{}\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// A directory nobody may write to forbids the sibling temporary file and
    /// still allows writing the target itself, which is the one thing that
    /// tells the two implementations apart.
    #[cfg(unix)]
    #[test]
    fn opencode_a_failed_write_leaves_the_target_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("opencode.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = write_atomically(&path, "{\"mcp\": {}}\n");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(names(dir.path()), ["opencode.json"]);
    }

    #[test]
    fn a_link_reaches_the_window_as_a_kind_and_a_command() {
        assert_eq!(
            serde_json::to_value(Link::Absent).unwrap(),
            json!({ "kind": "absent" })
        );
        assert_eq!(
            serde_json::to_value(Link::Current).unwrap(),
            json!({ "kind": "current" })
        );
        assert_eq!(
            serde_json::to_value(Link::Stale {
                command: "/x/mnema".to_string()
            })
            .unwrap(),
            json!({ "kind": "stale", "command": "/x/mnema" })
        );
        assert_eq!(
            serde_json::to_value(ClientId::Opencode).unwrap(),
            json!("opencode")
        );
    }
}
