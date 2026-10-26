//! Each agent CLI's own answer about the `mnema` MCP server, read back as one
//! of three states. Four CLIs, four shapes — Claude prints labelled lines, Codex
//! and pi JSON — so the shapes are pinned here against output recorded
//! 2026-10-26 rather than guessed at.
//!
//! Pure functions over [`Output`]: no process runs and no file is read, which is
//! what lets a test reproduce any shape without an agent installed.

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OURS: &str = "/tmp/My App's/mnema";

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
