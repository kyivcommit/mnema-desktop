//! Each agent CLI's own answer about the `mnema` MCP server, read back as one
//! of three states. Four CLIs, four shapes — Claude prints labelled lines, Codex
//! and pi JSON, opencode keeps it in `opencode.json` — so the shapes are pinned
//! here against output recorded 2026-10-26 rather than guessed at.
//!
//! The parsers are pure functions over [`Output`] and text, which is what lets a
//! test reproduce any shape without an agent installed. Everything that touches
//! the machine sits below them: [`Env`] is what the two commands take from it,
//! [`run`] is the one place a process starts, and [`write_atomically`] the one
//! place this module writes — opencode has no command that removes a server, so
//! its config file is the one file the application edits, and the surface for
//! that is kept as small as it can be.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

impl ClientId {
    /// The name the person's login shell knows the CLI by.
    fn name(self) -> &'static str {
        match self {
            ClientId::Claude => "claude",
            ClientId::Codex => "codex",
            ClientId::Pi => "pi",
            ClientId::Opencode => "opencode",
        }
    }
}

/// What one client's own record says about the `mnema` server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Link {
    Absent,
    Current,
    Stale { command: String },
}

/// What a CLI printed: exit code, stdout, stderr.
#[derive(Debug)]
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

/// How long one CLI may take. `claude mcp get` launches the server to check its
/// health and answers in about a second; `pi mcp list` starts every server it
/// knows.
const CLI_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the login shell may take. It is asked where a CLI is once per
/// client, and it reads the person's own start-up files before it answers.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// How often [`run`] asks a child whether it has finished.
const POLL: Duration = Duration::from_millis(50);

/// Everything the two commands take from the machine, so a test can point them
/// at a temporary directory instead.
#[derive(Clone, Debug)]
pub struct Env {
    /// The person's login shell: `$SHELL`, or `/bin/zsh` when it is unset.
    pub shell: PathBuf,
    pub home: PathBuf,
    pub xdg_config: Option<PathBuf>,
    /// The `mnema` binary an agent is pointed at.
    pub ours: String,
    /// The login shell's `PATH`, filled in once per command by
    /// [`with_login_path`] — `None` until then, and when the shell would not
    /// say.
    pub path: Option<String>,
}

impl Env {
    /// The machine this process is running on.
    pub fn from_process() -> Env {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Env {
            shell: var("SHELL").unwrap_or_else(|| PathBuf::from("/bin/zsh")),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default(),
            xdg_config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            ours: crate::agent::agent_command_path(),
            path: None,
        }
    }

    /// The file opencode keeps its servers in — see [`opencode_config`].
    fn opencode_file(&self) -> PathBuf {
        opencode_config(self.xdg_config.as_deref(), &self.home)
    }
}

/// What one checkbox draws.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientState {
    pub id: ClientId,
    pub installed: bool,
    pub link: Option<Link>,
    pub error: Option<String>,
}

/// What one client's change came to: `error` is the CLI's own words.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub id: ClientId,
    pub error: Option<String>,
}

/// The state of all four clients, in [`ALL`] order.
///
/// One thread per client, each finding its CLI and asking it: `claude mcp get`
/// takes about a second and `pi mcp list` starts every server it knows, so four
/// in a row would be a window that has not drawn yet. The threads are joined in
/// [`ALL`] order, so which one finishes first changes nothing that is drawn.
pub fn states(env: &Env, run_timeout: Duration, shell_timeout: Duration) -> Vec<ClientState> {
    let env = with_login_path(env, shell_timeout);
    let machine = &env;
    std::thread::scope(|scope| {
        let mut threads = Vec::with_capacity(ALL.len());
        for id in ALL {
            threads.push((
                id,
                scope.spawn(move || state_of(machine, id, run_timeout, shell_timeout)),
            ));
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
            .collect()
    })
}

fn state_of(
    env: &Env,
    id: ClientId,
    run_timeout: Duration,
    shell_timeout: Duration,
) -> ClientState {
    let Some(cli) = find_cli(&env.shell, id.name(), shell_timeout) else {
        return ClientState {
            id,
            installed: false,
            link: None,
            error: None,
        };
    };
    match read_link(env, id, &cli, run_timeout) {
        Ok(link) => ClientState {
            id,
            installed: true,
            link: Some(link),
            error: None,
        },
        Err(error) => ClientState {
            id,
            installed: true,
            link: None,
            error: Some(error),
        },
    }
}

/// Connects the clients in `want` and disconnects every other installed one,
/// one client at a time, with one answer per installed client.
///
/// Sequential, and without an early return: the four CLIs write the person's
/// configuration files, and a client that fails is a sentence under its own
/// checkbox rather than a reason to leave the rest as they were.
pub fn apply(
    env: &Env,
    want: &[ClientId],
    run_timeout: Duration,
    shell_timeout: Duration,
) -> Vec<Applied> {
    let env = with_login_path(env, shell_timeout);
    ALL.iter()
        .filter_map(|&id| {
            let cli = find_cli(&env.shell, id.name(), shell_timeout)?;
            Some(apply_one(&env, id, &cli, want.contains(&id), run_timeout))
        })
        .collect()
}

fn apply_one(env: &Env, id: ClientId, cli: &Path, wanted: bool, timeout: Duration) -> Applied {
    let outcome = read_link(env, id, cli, timeout).and_then(|link| match (wanted, link) {
        // The record is already the one this build would write.
        (true, Link::Current) => Ok(()),
        // `claude mcp add` refuses a name it already holds, so the stale record
        // goes first — measured 2026-10-26, and the only client like this:
        // codex, pi and opencode replace what is there.
        (true, Link::Stale { .. }) if id == ClientId::Claude => {
            remove(env, id, cli, timeout)?;
            add(env, id, cli, timeout)
        }
        (true, _) => add(env, id, cli, timeout),
        (false, Link::Absent) => Ok(()),
        (false, _) => remove(env, id, cli, timeout),
    });
    Applied {
        id,
        error: outcome.err(),
    }
}

fn read_link(env: &Env, id: ClientId, cli: &Path, timeout: Duration) -> Result<Link, String> {
    let read = |args: &[&str]| run(cli, args, timeout, env.path.as_deref());
    match id {
        ClientId::Claude => {
            read(&["mcp", "get", "mnema"]).and_then(|out| parse_claude(&out, &env.ours))
        }
        ClientId::Codex => {
            read(&["mcp", "get", "mnema", "--json"]).and_then(|out| parse_codex(&out, &env.ours))
        }
        ClientId::Pi => read(&["mcp", "list", "--json"]).and_then(|out| parse_pi(&out, &env.ours)),
        ClientId::Opencode => read_opencode(env),
    }
}

fn add(env: &Env, id: ClientId, cli: &Path, timeout: Duration) -> Result<(), String> {
    let ours = env.ours.as_str();
    let args: &[&str] = match id {
        ClientId::Claude => &["mcp", "add", "--scope", "user", "mnema", "--", ours, "mcp"],
        ClientId::Codex | ClientId::Pi => &["mcp", "add", "mnema", "--", ours, "mcp"],
        ClientId::Opencode => &["mcp", "add", "--global", "mnema", "--", ours, "mcp"],
    };
    ran(cli, args, timeout, env)
}

fn remove(env: &Env, id: ClientId, cli: &Path, timeout: Duration) -> Result<(), String> {
    match id {
        ClientId::Claude => ran(cli, &["mcp", "remove", "-s", "user", "mnema"], timeout, env),
        ClientId::Codex | ClientId::Pi => ran(cli, &["mcp", "remove", "mnema"], timeout, env),
        // opencode has no `mcp remove` at all: its record is the file.
        ClientId::Opencode => remove_opencode(env),
    }
}

/// Runs a command that is only interesting when it fails, and answers with the
/// CLI's own words when it does.
fn ran(program: &Path, args: &[&str], timeout: Duration, env: &Env) -> Result<(), String> {
    let out = run(program, args, timeout, env.path.as_deref())?;
    if out.code == 0 {
        return Ok(());
    }
    let said = said(&out);
    Err(if said.is_empty() {
        format!("{} exited with code {}", program.display(), out.code)
    } else {
        said
    })
}

/// opencode has no command that answers for one of its servers — `mcp list`
/// goes through a managed service and did not answer in an isolated `HOME` — so
/// its record is the file `mcp add --global` writes.
fn read_opencode(env: &Env) -> Result<Link, String> {
    let path = env.opencode_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            parse_opencode(&text, &env.ours).map_err(|error| format!("{}: {error}", path.display()))
        }
        // No file is a client that has never been configured, which is not an
        // error: `mcp add --global` is what would create it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Link::Absent),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// The one place this module writes a person's file.
fn remove_opencode(env: &Env) -> Result<(), String> {
    let path = env.opencode_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Gone since the state was read: nothing left to remove.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let Some(text_without) =
        opencode_without_mnema(&text).map_err(|error| format!("{}: {error}", path.display()))?
    else {
        return Ok(());
    };
    write_if_unchanged(&path, &text, &text_without)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Replaces the file only while it still holds what was read from it.
///
/// `opencode` itself may write `opencode.json` between our read and our rename —
/// it is running on this machine while the settings window is open — and that
/// write is not ours to drop. A file that moved on is left to the writer that
/// won, and the window is told the change did not happen.
fn write_if_unchanged(path: &Path, expected: &str, text: &str) -> Result<(), String> {
    let now = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    if now != expected {
        return Err("opencode.json changed while being updated, try again".to_string());
    }
    write_atomically(path, text).map_err(|error| error.to_string())
}

/// Runs a program with no shell between us and it, and answers what it printed.
///
/// `path` is the login shell's `PATH`, put into the child's environment: the
/// npm-installed CLIs are scripts whose `#!/usr/bin/env node` needs a directory
/// an application started from Finder does not have.
///
/// A child that outlives `timeout` is killed **and waited for** — an unreaped
/// child is a zombie holding its pid for as long as this application runs.
fn run(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    path: Option<&str>,
) -> Result<Output, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    // Draining on threads of their own: a child that fills one pipe while we
    // wait on the other would deadlock against a reader that is not there.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(Output {
                    // A signal has no exit code, and -1 is not a code any CLI
                    // answers with.
                    code: status.code().unwrap_or(-1),
                    stdout: joined(stdout),
                    stderr: joined(stderr),
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn drain(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stream.read_to_string(&mut text);
        text
    })
}

fn joined(reader: Option<std::thread::JoinHandle<String>>) -> String {
    reader
        .and_then(|thread| thread.join().ok())
        .unwrap_or_default()
}

/// Where the person's login shell finds a CLI, or `None`.
///
/// Through the shell because an application started from Finder has no
/// `~/.local/bin` and no `/opt/homebrew/bin` in `PATH`. The name is a positional
/// argument and never part of the script: a name is a name, not a command.
fn find_cli(shell: &Path, name: &str, timeout: Duration) -> Option<PathBuf> {
    let out = run(
        shell,
        &["-lc", "command -v \"$1\"", "mnema", name],
        timeout,
        None,
    )
    .ok()?;
    if out.code != 0 {
        return None;
    }
    // The last non-empty line, because a login shell prints its greeting first.
    let line = out
        .stdout
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    let path = PathBuf::from(line);
    // `command -v` also answers a builtin or an alias with something that is not
    // a path at all, and an absolute path to a file is the only thing this can
    // run.
    (path.is_absolute() && path.is_file()).then_some(path)
}

/// The login shell's `PATH`, or `None` when it would not answer.
fn login_path(shell: &Path, timeout: Duration) -> Option<String> {
    let out = run(shell, &["-lc", "printf \"%s\" \"$PATH\""], timeout, None).ok()?;
    let path = out.stdout.trim();
    (out.code == 0 && !path.is_empty()).then(|| path.to_string())
}

/// [`Env`] with the login `PATH`, read once so that every CLI run of one
/// command shares it.
fn with_login_path(env: &Env, timeout: Duration) -> Env {
    if env.path.is_some() {
        return env.clone();
    }
    Env {
        path: login_path(&env.shell, timeout),
        ..env.clone()
    }
}

/// What the four checkboxes show.
#[tauri::command(async)]
pub fn agent_clients() -> Vec<ClientState> {
    states(&Env::from_process(), CLI_TIMEOUT, SHELL_TIMEOUT)
}

/// Connects the clients in `want` and disconnects every other installed one.
#[tauri::command(async)]
pub fn apply_agent_clients(want: Vec<ClientId>) -> Vec<Applied> {
    apply(&Env::from_process(), &want, CLI_TIMEOUT, SHELL_TIMEOUT)
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

    /// No test waits fifteen seconds for anything.
    #[cfg(unix)]
    const TIMEOUT: Duration = Duration::from_secs(5);

    #[cfg(unix)]
    fn executable(path: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_path_buf()
    }

    /// A fake agent CLI, shared by all four names: it records its arguments and
    /// answers from the fixture files beside it. `$1-$2` picks the fixture, so
    /// `claude.mcp-get.out` is what `claude mcp get` prints and
    /// `claude.mcp-add.code` is what `claude mcp add` exits with.
    #[cfg(unix)]
    const CLI_BODY: &str = r#"dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
name=$(basename -- "$0")
{ printf '%s' "$name"; for arg in "$@"; do printf ' [%s]' "$arg"; done; printf '\n'; } >> "$dir/calls.log"
key="$1-$2"
for base in "$name.$key" "$name"; do
  if [ -f "$dir/$base.out" ]; then cat "$dir/$base.out"; fi
  if [ -f "$dir/$base.err" ]; then cat "$dir/$base.err" >&2; fi
  if [ -f "$dir/$base.code" ]; then exit "$(cat "$dir/$base.code")"; fi
done
exit 0
"#;

    /// The person's login shell, as this application asks it two questions: the
    /// `PATH` to run CLIs with, and where a CLI is. It answers the first from a
    /// literal and the second by running the command string the way `sh -c`
    /// would, with `mnema` as `$0` and the CLI name as `$1` — which is what
    /// makes a name interpolated into the script instead of passed beside it
    /// visible as a command that ran.
    #[cfg(unix)]
    fn shell_body(login: &str, noisy: bool) -> String {
        let mut body =
            String::from("#!/bin/sh\ndir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\n");
        if noisy {
            body.push_str("printf '%s\\n' 'Last login: Mon Oct 26 09:00:00 on ttys001'\n");
        }
        body.push_str("case \"$*\" in\n  *'$PATH'*) printf '%s' '");
        body.push_str(login);
        body.push_str(
            "'; exit 0 ;;\nesac\ncd \"$dir\" || exit 1\nPATH=\"$dir:/usr/bin:/bin\"\n\
             script=$2\nshift 2\nshift\neval \"$script\"\n",
        );
        body
    }

    /// A temporary machine: a fake login shell, four fake CLIs and the
    /// fixtures they answer from, all in one `TempDir`. Nothing here reads the
    /// real `~`, the real `$SHELL` or a real `claude`.
    #[cfg(unix)]
    struct Stand {
        dir: TempDir,
        bin: PathBuf,
        login: String,
    }

    #[cfg(unix)]
    impl Stand {
        fn new() -> Stand {
            let dir = TempDir::new().unwrap();
            let bin = dir.path().join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(dir.path().join("pathbin")).unwrap();
            let login = format!("{}:/usr/bin:/bin", dir.path().join("pathbin").display());
            let stand = Stand { dir, bin, login };
            stand.shell(false);
            stand
        }

        fn shell(&self, noisy: bool) {
            executable(&self.bin.join("shell"), &shell_body(&self.login, noisy));
        }

        fn env(&self) -> Env {
            Env {
                shell: self.bin.join("shell"),
                home: self.dir.path().join("home"),
                xdg_config: Some(self.dir.path().join("xdg")),
                ours: OURS.to_string(),
                path: None,
            }
        }

        fn install(&self, name: &str) {
            executable(&self.bin.join(name), &format!("#!/bin/sh\n{CLI_BODY}"));
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            executable(&self.bin.join(name), body)
        }

        /// The `node` an npm-installed CLI's `#!/usr/bin/env node` needs: only
        /// the login `PATH` knows it.
        fn fakenode(&self, body: &str) -> PathBuf {
            executable(&self.dir.path().join("pathbin").join("fakenode"), body)
        }

        fn fixture(&self, name: &str, text: &str) {
            fs::write(self.bin.join(name), text).unwrap();
        }

        fn calls(&self) -> Vec<String> {
            fs::read_to_string(self.bin.join("calls.log"))
                .map(|text| text.lines().map(str::to_string).collect())
                .unwrap_or_default()
        }

        fn opencode_path(&self) -> PathBuf {
            self.dir
                .path()
                .join("xdg")
                .join("opencode")
                .join("opencode.json")
        }

        fn opencode_file(&self) -> PathBuf {
            let path = self.opencode_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            path
        }
    }

    /// A missing CLI is a checkbox that is not drawn, and nothing was run for
    /// it: `installed: false`, no link, no error.
    #[cfg(unix)]
    #[test]
    fn states_reports_an_installed_client_and_leaves_the_others_alone() {
        let stand = Stand::new();
        stand.install("claude");
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        let states = states(&stand.env(), TIMEOUT, TIMEOUT);
        assert_eq!(states.iter().map(|state| state.id).collect::<Vec<_>>(), ALL);
        assert!(states[0].installed);
        assert_eq!(states[0].link, Some(Link::Current));
        assert_eq!(states[0].error, None);
        for state in &states[1..] {
            assert!(!state.installed, "{:?}", state.id);
            assert_eq!(state.link, None);
            assert_eq!(state.error, None, "{:?}", state.id);
        }
        // The three CLIs that are not there were never run.
        assert_eq!(stand.calls(), ["claude [mcp] [get] [mnema]"]);
    }

    /// The order the window draws is [`ALL`], not the order the threads
    /// happened to finish in.
    #[cfg(unix)]
    #[test]
    fn states_keeps_all_order_whatever_thread_finishes_first() {
        let stand = Stand::new();
        // Claude answers last, so a `states` that collected completion order
        // would put it fourth.
        stand.script(
            "claude",
            "#!/bin/sh\ndir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nsleep 0.3\ncat \"$dir/claude.out\"\n",
        );
        stand.fixture("claude.out", CLAUDE_GET);
        for name in ["codex", "pi", "opencode"] {
            stand.install(name);
        }
        stand.fixture("codex.mcp-get.out", CODEX_GET);
        stand.fixture("pi.mcp-list.out", PI_LIST);
        let states = states(&stand.env(), TIMEOUT, TIMEOUT);
        assert_eq!(
            states.iter().map(|state| state.id).collect::<Vec<_>>(),
            [
                ClientId::Claude,
                ClientId::Codex,
                ClientId::Pi,
                ClientId::Opencode
            ]
        );
        assert_eq!(states[0].link, Some(Link::Current));
        assert_eq!(states[1].link, Some(Link::Current));
        assert_eq!(states[2].link, Some(Link::Current));
    }

    /// The `add` table's first row: an absent client is added with exactly the
    /// arguments measured on 2026-10-26 — the path one argument, not two — and
    /// a wanted client that is already current is written nothing for.
    #[cfg(unix)]
    #[test]
    fn apply_adds_an_absent_client_and_writes_nothing_for_a_current_one() {
        let stand = Stand::new();
        stand.install("claude");
        stand.install("codex");
        stand.fixture("claude.mcp-get.code", "1");
        stand.fixture(
            "claude.mcp-get.err",
            "No MCP server named \"mnema\". Configured servers: …",
        );
        stand.fixture("codex.mcp-get.out", CODEX_GET);
        let applied = apply(
            &stand.env(),
            &[ClientId::Claude, ClientId::Codex],
            TIMEOUT,
            TIMEOUT,
        );
        assert_eq!(
            applied.iter().map(|applied| applied.id).collect::<Vec<_>>(),
            [ClientId::Claude, ClientId::Codex]
        );
        assert_eq!(applied[0].error, None);
        assert_eq!(applied[1].error, None);
        assert_eq!(
            stand.calls(),
            [
                "claude [mcp] [get] [mnema]",
                "claude [mcp] [add] [--scope] [user] [mnema] [--] [/tmp/My App's/mnema] [mcp]",
                "codex [mcp] [get] [mnema] [--json]",
            ]
        );
    }

    /// Unchecked and current is a `remove`, with the arguments measured on the
    /// same day.
    #[cfg(unix)]
    #[test]
    fn apply_removes_a_current_client_that_is_not_wanted() {
        let stand = Stand::new();
        stand.install("claude");
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        let applied = apply(&stand.env(), &[], TIMEOUT, TIMEOUT);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].error, None);
        assert_eq!(
            stand.calls(),
            [
                "claude [mcp] [get] [mnema]",
                "claude [mcp] [remove] [-s] [user] [mnema]",
            ]
        );
    }

    /// Claude refuses a name it already holds, so a stale record is removed
    /// before it is added. Measured 2026-10-26: codex, pi and opencode replace.
    #[cfg(unix)]
    #[test]
    fn claude_on_a_stale_path_is_removed_before_it_is_added() {
        let stand = Stand::new();
        stand.install("claude");
        stand.fixture(
            "claude.mcp-get.out",
            &CLAUDE_GET.replace(
                "Command: /tmp/My App's/mnema",
                "Command: /Applications/Mnema.app/Contents/MacOS/mnema",
            ),
        );
        let applied = apply(&stand.env(), &[ClientId::Claude], TIMEOUT, TIMEOUT);
        assert_eq!(applied[0].error, None);
        assert_eq!(
            stand.calls(),
            [
                "claude [mcp] [get] [mnema]",
                "claude [mcp] [remove] [-s] [user] [mnema]",
                "claude [mcp] [add] [--scope] [user] [mnema] [--] [/tmp/My App's/mnema] [mcp]",
            ]
        );
    }

    /// Codex replaces on `add`, so a stale record is only added.
    #[cfg(unix)]
    #[test]
    fn codex_on_a_stale_path_is_only_added() {
        let stand = Stand::new();
        stand.install("codex");
        stand.fixture(
            "codex.mcp-get.out",
            &CODEX_GET.replace("/tmp/My App's/mnema", "/usr/local/bin/mnema"),
        );
        let applied = apply(&stand.env(), &[ClientId::Codex], TIMEOUT, TIMEOUT);
        assert_eq!(applied[0].error, None);
        assert_eq!(
            stand.calls(),
            [
                "codex [mcp] [get] [mnema] [--json]",
                "codex [mcp] [add] [mnema] [--] [/tmp/My App's/mnema] [mcp]",
            ]
        );
    }

    /// opencode's removal is the file, and only the `mnema` keys go.
    #[cfg(unix)]
    #[test]
    fn disconnecting_opencode_removes_the_key_from_its_file() {
        let stand = Stand::new();
        stand.install("opencode");
        let file = stand.opencode_file();
        fs::write(&file, OPENCODE_JSON).unwrap();
        let applied = apply(&stand.env(), &[], TIMEOUT, TIMEOUT);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].error, None);
        // Neither the state nor the change ran the CLI.
        assert!(stand.calls().is_empty(), "{:?}", stand.calls());
        let text = fs::read_to_string(&file).unwrap();
        let written: Value = serde_json::from_str(&text).unwrap();
        assert!(written["mcp"]["mnema"].is_null(), "{text}");
        assert!(written["mcp"]["servers"]["mnema"].is_null(), "{text}");
        assert_eq!(
            written["mcp"]["servers"]["omega"]["command"],
            json!(["omega", "serve"])
        );
        assert_eq!(
            written["mcp"]["zeta"]["environment"],
            json!({"Z_LAST": "1", "A_FIRST": "2"})
        );
        assert_eq!(
            written["agent"]["build"]["model"],
            json!("anthropic/claude")
        );
        // No temporary file left behind.
        assert_eq!(names(file.parent().unwrap()), ["opencode.json"]);
    }

    /// A failing CLI is one checkbox's sentence, not the end of the list.
    #[cfg(unix)]
    #[test]
    fn a_client_that_fails_does_not_stop_the_one_after_it() {
        let stand = Stand::new();
        stand.install("claude");
        stand.install("codex");
        stand.fixture("claude.mcp-get.code", "1");
        stand.fixture(
            "claude.mcp-get.err",
            "No MCP server named \"mnema\". Configured servers: …",
        );
        stand.fixture("claude.mcp-add.err", "boom");
        stand.fixture("claude.mcp-add.code", "1");
        stand.fixture("codex.mcp-get.code", "1");
        stand.fixture(
            "codex.mcp-get.err",
            "Error: No MCP server named 'mnema' found.",
        );
        let applied = apply(
            &stand.env(),
            &[ClientId::Claude, ClientId::Codex],
            TIMEOUT,
            TIMEOUT,
        );
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].error.as_deref(), Some("boom"));
        assert_eq!(applied[1].error, None);
        assert!(
            stand
                .calls()
                .iter()
                .any(|call| call.starts_with("codex [mcp] [add]")),
            "{:?}",
            stand.calls()
        );
    }

    /// A state that could not be read is a client nothing is done to.
    #[cfg(unix)]
    #[test]
    fn a_client_whose_state_cannot_be_read_is_left_untouched() {
        let stand = Stand::new();
        stand.install("claude");
        stand.fixture("claude.mcp-get.code", "2");
        stand.fixture("claude.mcp-get.err", "could not read the user config");
        let state = &states(&stand.env(), TIMEOUT, TIMEOUT)[0];
        assert!(state.installed);
        assert_eq!(state.link, None);
        assert_eq!(
            state.error.as_deref(),
            Some("could not read the user config")
        );
        let applied = apply(&stand.env(), &[ClientId::Claude], TIMEOUT, TIMEOUT);
        assert_eq!(
            applied[0].error.as_deref(),
            Some("could not read the user config")
        );
        assert_eq!(
            stand.calls(),
            ["claude [mcp] [get] [mnema]", "claude [mcp] [get] [mnema]"]
        );
    }

    /// No `opencode.json` is a client that was never configured, and connecting
    /// it is the CLI's `add` — this build never creates the file.
    #[cfg(unix)]
    #[test]
    fn an_opencode_without_a_file_is_absent_and_connect_goes_through_its_cli() {
        let stand = Stand::new();
        stand.install("opencode");
        let states = states(&stand.env(), TIMEOUT, TIMEOUT);
        assert!(states[3].installed);
        assert_eq!(states[3].link, Some(Link::Absent));
        let applied = apply(&stand.env(), &[ClientId::Opencode], TIMEOUT, TIMEOUT);
        assert_eq!(applied[0].error, None);
        assert_eq!(
            stand.calls(),
            ["opencode [mcp] [add] [--global] [mnema] [--] [/tmp/My App's/mnema] [mcp]"]
        );
        assert!(!stand.opencode_path().exists(), "connecting wrote the file");
    }

    /// An npm-installed CLI is a script for an interpreter only the login
    /// `PATH` knows, as `pi` is a script for `node`: a run without that `PATH`
    /// cannot even start, and the child would see the application's own.
    #[cfg(unix)]
    #[test]
    fn the_login_path_reaches_every_cli_run() {
        let stand = Stand::new();
        let path_log = stand.dir.path().join("path.log");
        stand.fakenode(&format!(
            "#!/bin/sh\nprintf '%s\\n' \"$PATH\" >> '{}'\nexec /bin/sh \"$@\"\n",
            path_log.display()
        ));
        stand.script("claude", &format!("#!/usr/bin/env fakenode\n{CLI_BODY}"));
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        let state = &states(&stand.env(), TIMEOUT, TIMEOUT)[0];
        assert_eq!(state.error, None);
        assert_eq!(state.link, Some(Link::Current));
        assert_eq!(
            fs::read_to_string(&path_log).unwrap_or_default().trim(),
            stand.login,
            "the child did not run with the login shell's PATH"
        );
    }

    /// A login shell greets before it answers, and the path is still found.
    #[cfg(unix)]
    #[test]
    fn a_login_shell_that_greets_before_answering_is_still_read() {
        let stand = Stand::new();
        stand.shell(true);
        stand.install("claude");
        assert_eq!(
            find_cli(&stand.env().shell, "claude", TIMEOUT),
            Some(stand.bin.join("claude"))
        );
    }

    /// The name is an argument and never part of the script: a name whose shell
    /// meaning is "run this too" is answered with "no such CLI" and runs
    /// nothing.
    #[cfg(unix)]
    #[test]
    fn a_cli_name_is_never_run_as_part_of_the_script() {
        let stand = Stand::new();
        assert_eq!(find_cli(&stand.env().shell, "a;touch PWNED", TIMEOUT), None);
        assert!(
            !stand.bin.join("PWNED").exists(),
            "the name was run as a command"
        );
    }

    /// Only an absolute path to a file is a CLI: `command -v` also answers a
    /// builtin with a bare name, and a path that is not there runs nothing.
    #[cfg(unix)]
    #[test]
    fn find_cli_answers_only_an_absolute_path_to_a_file() {
        let dir = TempDir::new().unwrap();
        let shell = executable(
            &dir.path().join("shell"),
            "#!/bin/sh\nprintf '%s\\n' 'motd'\nprintf '%s\\n' 'claude'\n",
        );
        assert_eq!(
            find_cli(&shell, "claude", TIMEOUT),
            None,
            "a bare name is not a path"
        );
        let shell = executable(
            &dir.path().join("shell"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' '{}'\n",
                dir.path().join("gone").display()
            ),
        );
        assert_eq!(
            find_cli(&shell, "claude", TIMEOUT),
            None,
            "a file that is not there is not a CLI"
        );
        let shell = executable(
            &dir.path().join("shell"),
            &format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", dir.path().display()),
        );
        assert_eq!(
            find_cli(&shell, "claude", TIMEOUT),
            None,
            "a directory is not an executable"
        );
    }

    /// A child that outstays its timeout is killed **and waited for**: a kill
    /// without the wait leaves a zombie whose pid answers `kill -0` until this
    /// process exits.
    #[cfg(unix)]
    #[test]
    fn a_child_that_outlives_the_timeout_is_killed_and_waited_for() {
        let dir = TempDir::new().unwrap();
        let pid_file = dir.path().join("pid");
        let script = executable(
            &dir.path().join("slow"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nexec sleep 5\n",
                pid_file.display()
            ),
        );
        let started = Instant::now();
        let error = run(&script, &[], Duration::from_millis(300), None).unwrap_err();
        assert!(error.contains("timed out after"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout did not cut the child short"
        );
        let pid = fs::read_to_string(&pid_file).expect("the fake child never said its pid");
        let gone = Command::new("kill")
            .arg("-0")
            .arg(pid.trim())
            .output()
            .is_ok_and(|output| !output.status.success());
        assert!(gone, "the child is still there: {}", pid.trim());
    }

    /// The window between our read and our rename belongs to whichever writer
    /// got there first — opencode itself can be running.
    #[cfg(unix)]
    #[test]
    fn a_file_another_writer_changed_is_left_to_that_writer() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("opencode.json");
        let seen = "{\n  \"mcp\": {\n    \"mnema\": {}\n  }\n}\n";
        let concurrent = "{\n  \"mcp\": {\n    \"context7\": {}\n  }\n}\n";
        fs::write(&path, seen).unwrap();
        let without = opencode_without_mnema(seen).unwrap().unwrap();
        // Between the read and the write, as opencode itself can make it move.
        fs::write(&path, concurrent).unwrap();
        let error = write_if_unchanged(&path, seen, &without).unwrap_err();
        assert!(
            error.contains("changed while being updated, try again"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), concurrent);
    }

    /// What Task 4's window reads: four fields, and a link tagged by `kind`.
    #[test]
    fn a_state_reaches_the_window_with_the_fields_it_draws() {
        let state = ClientState {
            id: ClientId::Pi,
            installed: true,
            link: Some(Link::Stale {
                command: "/x/mnema".to_string(),
            }),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            json!({
                "id": "pi",
                "installed": true,
                "link": {"kind": "stale", "command": "/x/mnema"},
                "error": null
            })
        );
        let applied = Applied {
            id: ClientId::Opencode,
            error: Some("boom".to_string()),
        };
        assert_eq!(
            serde_json::to_value(&applied).unwrap(),
            json!({"id": "opencode", "error": "boom"})
        );
    }
}
