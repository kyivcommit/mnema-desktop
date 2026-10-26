//! Each agent CLI's own answer about the `mnema` MCP server, read back as one
//! of three states. Four CLIs, four shapes — Claude prints labelled lines, Codex
//! and pi JSON, opencode keeps it in `opencode.json` — so the shapes are pinned
//! here against output recorded 2026-10-26 rather than guessed at.
//!
//! The parsers are pure functions over [`Output`] and text, which is what lets a
//! test reproduce any shape without an agent installed. Everything that touches
//! the machine sits below them: [`Env`] is what the two commands take from it,
//! [`run`] is where a process starts, and [`write_atomically`] the one place
//! this module writes — opencode has no command that removes a server, so its
//! config file is the one file the application edits, and the surface for that
//! is kept as small as it can be.

use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    // Exit 1 is pi saying some server failed to start — still a full list.
    // Any broken server of the person's, or our own stale path, does it
    // (measured 2026-10-26 on the stand; the sandbox run hid it behind a pipe).
    if out.code != 0 && out.code != 1 {
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

/// How long the login shell may take. It reads the person's own start-up files
/// before it answers, and it is asked once per command.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// How often [`run`] asks a child whether it has finished.
const POLL: Duration = Duration::from_millis(50);

/// How long a finished child's readers are given to see the end of their pipes.
/// Not the child's own timeout: a command that exits at the last second still
/// owes this application what it printed, and an answer cut off by the clock is
/// not an answer.
const READER_GRACE: Duration = Duration::from_secs(2);

/// The marker the login shell is asked to print its `PATH` after.
///
/// Marked rather than read as the whole output, because a login shell prints its
/// greeting and its rc's chatter around whatever it was asked.
const PATH_MARK: &str = "__MNEMA_PATH__";

/// What the login shell is asked to run. `printf` exists in sh, bash, zsh and
/// fish; `"$PATH"` is one colon-joined string in all four, fish included, which
/// joins a path variable with colons when it is quoted.
const PATH_QUESTION: &str = "printf \"\\n__MNEMA_PATH__%s\\n\" \"$PATH\"";

/// The shell tried when the person's own will not answer at all: `tcsh` reads
/// `-l -c` as an option it does not know, and a start-up file that exits leaves
/// the command unrun. A `PATH` from another shell beats four checkboxes saying
/// "not installed".
const FALLBACK_SHELL: &str = "/bin/zsh";

/// What the login shell said when it was asked for its `PATH`.
#[derive(Clone, Debug)]
pub enum ShellPath {
    /// Not asked yet: [`with_login_shell`] fills this in once per command.
    Unread,
    /// The directories a CLI is looked for in.
    Path(String),
    /// Why there is none, in the shells' own words where they had any.
    Unreadable(String),
}

/// Everything the two commands take from the machine, so a test can point them
/// at a temporary directory instead.
#[derive(Clone, Debug)]
pub struct Env {
    /// The person's login shell: `$SHELL`, or [`FALLBACK_SHELL`] when it is
    /// unset.
    pub shell: PathBuf,
    /// The shell tried when `shell` will not answer — [`FALLBACK_SHELL`] in the
    /// build, a fake in a test.
    pub fallback: PathBuf,
    /// `$HOME`; `None` when it is unset or empty, which is an error for the one
    /// file this module reads rather than an empty directory to join onto.
    pub home: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
    /// The `mnema` binary an agent is pointed at.
    pub ours: String,
    /// [`ShellPath::Unread`] until [`with_login_shell`] has asked.
    pub path: ShellPath,
}

impl Env {
    /// The machine this process is running on.
    pub fn from_process() -> Env {
        Env {
            shell: non_empty("SHELL").unwrap_or_else(|| PathBuf::from(FALLBACK_SHELL)),
            fallback: PathBuf::from(FALLBACK_SHELL),
            home: non_empty("HOME"),
            xdg_config: non_empty("XDG_CONFIG_HOME"),
            ours: crate::agent::agent_command_path(),
            path: ShellPath::Unread,
        }
    }

    /// The `PATH` a CLI must run with, or `None` when the shell would not say.
    fn cli_path(&self) -> Option<&str> {
        match &self.path {
            ShellPath::Path(path) => Some(path),
            ShellPath::Unread | ShellPath::Unreadable(_) => None,
        }
    }

    /// The file opencode keeps its servers in — see [`opencode_config`].
    ///
    /// An empty `HOME` is an error and not a path to join: joined, it would make
    /// this a relative path, read and written against whatever directory the
    /// application was started from.
    fn opencode_file(&self) -> Result<PathBuf, String> {
        let xdg = self
            .xdg_config
            .as_deref()
            .filter(|dir| !dir.as_os_str().is_empty());
        match (xdg, self.home.as_deref()) {
            (Some(xdg), _) => Ok(opencode_config(Some(xdg), Path::new(""))),
            (None, Some(home)) => Ok(opencode_config(None, home)),
            (None, None) => Err("could not find your home directory".to_string()),
        }
    }
}

/// A `$NAME` whose empty value means "unset", as it does for `SHELL` and `HOME`.
fn non_empty(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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

/// One box the person moved: which client, and which way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: ClientId,
    pub connect: bool,
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
    let env = with_login_shell(env, shell_timeout);
    let machine = &env;
    std::thread::scope(|scope| {
        let mut threads = Vec::with_capacity(ALL.len());
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
            .collect()
    })
}

/// Where this client's CLI is on the login `PATH`: `Ok(None)` is a CLI that is
/// not installed, and `Err` is a `PATH` that could not be read at all — a
/// different sentence for the window than "not installed".
fn locate(env: &Env, id: ClientId) -> Result<Option<PathBuf>, String> {
    match &env.path {
        ShellPath::Path(path) => Ok(resolve(path, id.name())),
        ShellPath::Unreadable(error) => Err(error.clone()),
        ShellPath::Unread => Err("the login shell was not asked for its PATH".to_string()),
    }
}

fn state_of(env: &Env, id: ClientId, run_timeout: Duration) -> ClientState {
    let cli = match locate(env, id) {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            return ClientState {
                id,
                installed: false,
                link: None,
                error: None,
            };
        }
        Err(error) => {
            return ClientState {
                id,
                installed: false,
                link: None,
                error: Some(error),
            };
        }
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

/// Applies exactly the listed changes, one client at a time, with one answer
/// per client it touched.
///
/// 🔴 **Only the clients in `changes` are read, run or answered.** A client that
/// is not listed is not this call's business, and the difference is not
/// cosmetic: the window sends a change only for a box the person moved, and a
/// box whose state could not be read — a timed-out `mcp get`, an `opencode.json`
/// it refused — is disabled and therefore never listed. An `apply` that read
/// "not listed" as "not wanted" would disconnect the working agent whose state
/// that one failed read hid, on a press that was about another client.
///
/// Sequential, and without an early return: the listed CLIs write the person's
/// configuration files, and a client that fails is a sentence under its own
/// checkbox rather than a reason to leave the rest as they were.
pub fn apply(
    env: &Env,
    changes: &[Change],
    run_timeout: Duration,
    shell_timeout: Duration,
) -> Vec<Applied> {
    let env = with_login_shell(env, shell_timeout);
    let mut applied = Vec::new();
    for change in changes {
        match locate(&env, change.id) {
            // The shell would not say what is installed, so neither a connect
            // nor a disconnect happened. Every listed client answers with that
            // reason; `states` is what marks all four — which is also what
            // disables Apply.
            Err(error) => applied.push(Applied {
                id: change.id,
                error: Some(error),
            }),
            // Not installed: nothing to add and nothing to remove.
            Ok(None) => {}
            Ok(Some(cli)) => applied.push(apply_one(
                &env,
                change.id,
                &cli,
                change.connect,
                run_timeout,
            )),
        }
    }
    applied
}

fn apply_one(env: &Env, id: ClientId, cli: &Path, connect: bool, timeout: Duration) -> Applied {
    let outcome = read_link(env, id, cli, timeout).and_then(|link| match (connect, link) {
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
    let read = |args: &[&str]| run(cli, args, timeout, READER_GRACE, env.cli_path());
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
    let out = run(program, args, timeout, READER_GRACE, env.cli_path())?;
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
    let path = env.opencode_file()?;
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
    let path = env.opencode_file()?;
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
    #[cfg(test)]
    second_writer_window();
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

// The moment between reading `opencode.json` and re-reading it before the
// write, which is where a test puts a second writer. Thread-local: a command
// does its work on the thread that called it, and no other test's `apply` may
// see this one's writer.
#[cfg(test)]
thread_local! {
    static SECOND_WRITER: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn second_writer_window() {
    SECOND_WRITER.with(|writer| {
        if let Some(writer) = writer.borrow().as_ref() {
            writer();
        }
    });
}

/// Runs a program with no shell between us and it, and answers what it printed.
///
/// `path` is the login shell's `PATH`, put into the child's environment: the
/// npm-installed CLIs are scripts whose `#!/usr/bin/env node` needs a directory
/// an application started from Finder does not have.
///
/// A child that outlives `timeout` is killed **and waited for** — an unreaped
/// child is a zombie holding its pid for as long as this application runs.
///
/// A child that exits is given `grace` for its pipes to end. A reader still open
/// after it holds a pipe that something the child started has not let go of:
/// that is an error and never an empty answer, because an empty `claude mcp get`
/// reads as "no server connected" and the person's record would then never be
/// removed.
fn run(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    grace: Duration,
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
    // Its own process group, so that everything this child starts can be ended
    // with it. A CLI — or the login shell's start-up file — may leave a
    // background job behind, and a job that inherited these pipes keeps them
    // open long after the command itself has finished.
    #[cfg(unix)]
    command.process_group(0);
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
                // Their own grace, not what is left of the child's clock: a
                // command that exits at the last second still owes its output.
                let end = Instant::now() + grace;
                let (stdout, stderr) = (joined(stdout, end), joined(stderr, end));
                let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
                    // Something the child started still holds one of the pipes,
                    // so the answer is cut short. It is a member of the child's
                    // group — which is the one case where signalling that group
                    // is both safe (a member keeps the id alive) and right.
                    kill_group(&mut child);
                    return Err(format!("{} kept its output open", program.display()));
                };
                return Ok(Output {
                    // A signal has no exit code, and -1 is not a code any CLI
                    // answers with.
                    code: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_group(&mut child);
                let _ = child.wait();
                return Err(format!("timed out after {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => {
                // The system will not say whether this child is alive, so there
                // is nothing left to wait for: end it and reap it rather than
                // leave a zombie behind.
                kill_group(&mut child);
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

/// Ends the child's process group, and the child with it.
///
/// Every child leads its own group (`process_group(0)`), so a background job it
/// left behind — a login shell's `(update-check &)`, an agent's helper — goes
/// with it and the pipes it holds close.
#[cfg(unix)]
fn kill_group(child: &mut Child) {
    // `killpg`, not the `kill` utility: Linux procps took `kill -KILL -<pgid>`'s
    // pid for an option and the group lived on (CI on PR #79). A group with no
    // members left answers `ESRCH`, which is nothing to report.
    if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: plain syscall on a pid we spawned as a group leader.
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_group(child: &mut Child) {
    let _ = child.kill();
}

fn drain(mut stream: impl Read + Send + 'static) -> std::sync::mpsc::Receiver<String> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stream.read_to_string(&mut text);
        let _ = send.send(text);
    });
    receive
}

/// A reader's text, or `None` when `deadline` came first: a pipe reaches EOF
/// only when every process holding it closes it, and that is not always the
/// child. `None` is not an empty answer — the caller must tell them apart.
fn joined(reader: Option<std::sync::mpsc::Receiver<String>>, deadline: Instant) -> Option<String> {
    match reader {
        None => Some(String::new()),
        Some(receive) => receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok(),
    }
}

/// The directories of a `PATH` this application will run a CLI from: absolute
/// ones only.
///
/// A shell reads `.`, `bin` and `node_modules/.bin` against its own working
/// directory; this application's working directory is wherever it happened to be
/// started from, which is not a place to find an agent.
fn search_dirs(path: &str) -> impl Iterator<Item = PathBuf> + '_ {
    std::env::split_paths(path).filter(|dir| dir.is_absolute())
}

/// The first directory of `path` that holds an executable file called `name`.
///
/// Resolved here and not by the shell's `command -v`: the name is a file name
/// and nothing else, and a shell is asked one question — its `PATH` — instead of
/// a command per client. `tcsh` has no `-lc` and `fish` has no `$1`, so a shell
/// asked to run `command -v` is a shell that answers nothing at all.
fn resolve(path: &str, name: &str) -> Option<PathBuf> {
    search_dirs(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// One start of a login shell, asked for its `PATH`.
fn ask(shell: &Path, timeout: Duration) -> Result<String, String> {
    let out = run(
        shell,
        &["-l", "-c", PATH_QUESTION],
        timeout,
        READER_GRACE,
        None,
    )?;
    after_last_mark(&out.stdout).ok_or_else(|| {
        let said = said(&out);
        if said.is_empty() {
            format!("{} answered nothing", shell.display())
        } else {
            said
        }
    })
}

/// The text after the last line that starts with [`PATH_MARK`]: exactly one
/// line is the answer, and a greeting before it or a word after it is neither.
fn after_last_mark(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix(PATH_MARK))
        .rfind(|path| !path.is_empty())
        .map(str::to_string)
}

/// The login shell's `PATH`, from the person's shell or, failing that, from
/// [`FALLBACK_SHELL`].
///
/// Two shells rather than one because the absence of an answer is silent: `tcsh`
/// takes `-l -c` for an option it does not know, and a start-up file that exits
/// leaves the command unrun. Either way the person's own directories are the
/// ones this application has to look in, and a `PATH` from another shell is much
/// better than four checkboxes claiming nothing is installed.
fn login_path(shell: &Path, fallback: &Path, timeout: Duration) -> Result<String, String> {
    let first = match ask(shell, timeout) {
        Ok(path) => return Ok(path),
        Err(error) => error,
    };
    match ask(fallback, timeout) {
        Ok(path) => Ok(path),
        Err(second) => Err(format!(
            "could not read your shell's PATH ({}: {first}; {}: {second})",
            shell.display(),
            fallback.display()
        )),
    }
}

/// [`Env`] with the login `PATH`, asked for once so that every CLI run of one
/// command shares it.
fn with_login_shell(env: &Env, timeout: Duration) -> Env {
    if !matches!(env.path, ShellPath::Unread) {
        return env.clone();
    }
    Env {
        path: match login_path(&env.shell, &env.fallback, timeout) {
            Ok(path) => ShellPath::Path(path),
            Err(error) => ShellPath::Unreadable(error),
        },
        ..env.clone()
    }
}

/// What the four checkboxes show.
#[tauri::command(async)]
pub fn agent_clients() -> Vec<ClientState> {
    states(&Env::from_process(), CLI_TIMEOUT, SHELL_TIMEOUT)
}

/// Applies exactly the listed changes: the boxes the person moved, and no others.
#[tauri::command(async)]
pub fn apply_agent_clients(changes: Vec<Change>) -> Vec<Applied> {
    apply(&Env::from_process(), &changes, CLI_TIMEOUT, SHELL_TIMEOUT)
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

    /// A box the person moved: `on(ClientId::Codex)` is "connect Codex".
    fn on(id: ClientId) -> Change {
        Change { id, connect: true }
    }

    fn off(id: ClientId) -> Change {
        Change { id, connect: false }
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

    /// `pi mcp list --json` exits 1 when a server fails to start, and still
    /// prints the whole list — recorded 2026-10-26 on the stand.
    #[test]
    fn pi_exit_one_with_a_failed_server_is_still_read() {
        let text = PI_LIST.replace(
            "/tmp/My App's/mnema mcp",
            "/Applications/Mnema.app/Contents/MacOS/mnema mcp",
        );
        assert_eq!(
            parse_pi(&out(1, &text, ""), OURS),
            Ok(Link::Stale {
                command: "/Applications/Mnema.app/Contents/MacOS/mnema".to_string()
            })
        );
        assert_eq!(
            parse_pi(&out(1, "Error: no config", ""), OURS),
            Err("Error: no config".to_string())
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

    /// The person's login shell: it answers the one question this application
    /// asks it — its `PATH` — on a marked line. `noisy` makes it print a
    /// greeting before the answer and a word after it, as a real login shell
    /// with a chatty start-up file does.
    #[cfg(unix)]
    fn shell_body(login: &str, noisy: bool) -> String {
        let mut body = String::from("#!/bin/sh\n");
        if noisy {
            body.push_str("printf '%s\\n' 'Last login: Mon Oct 26 09:00:00 on ttys001'\n");
        }
        body.push_str(&format!("printf '\\n__MNEMA_PATH__%s\\n' '{login}'\n"));
        if noisy {
            body.push_str("printf '%s\\n' 'bye'\n");
        }
        body
    }

    /// A temporary machine: a fake login shell, a fake second shell, four fake
    /// CLIs and the fixtures they answer from, all in one `TempDir`. Nothing
    /// here reads the real `~`, the real `$SHELL` or a real `claude`.
    #[cfg(unix)]
    struct Stand {
        dir: TempDir,
        bin: PathBuf,
        /// The `PATH` the fake shell answers with: the fakes, the interpreter
        /// only they use, and the system's own tools for the scripts to run
        /// (`cat`, `sleep`). No `~/.local/bin`, no Homebrew.
        login: String,
        fallback: PathBuf,
    }

    #[cfg(unix)]
    impl Stand {
        fn new() -> Stand {
            let dir = TempDir::new().unwrap();
            let bin = dir.path().join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(dir.path().join("pathbin")).unwrap();
            let login = format!(
                "{}:{}:/usr/bin:/bin",
                bin.display(),
                dir.path().join("pathbin").display()
            );
            let stand = Stand {
                fallback: dir.path().join("fallback-shell"),
                dir,
                bin,
                login,
            };
            stand.shell(false);
            stand.fallback_shell(None);
            stand
        }

        fn shell(&self, noisy: bool) {
            executable(&self.bin.join("shell"), &shell_body(&self.login, noisy));
        }

        /// The second shell; `None` restores one that answers, `Some` breaks it.
        fn fallback_shell(&self, body: Option<&str>) {
            let body = body
                .map(str::to_string)
                .unwrap_or_else(|| shell_body(&self.login, false));
            executable(&self.fallback, &body);
        }

        fn env(&self) -> Env {
            Env {
                shell: self.bin.join("shell"),
                fallback: self.fallback.clone(),
                home: Some(self.dir.path().join("home")),
                xdg_config: Some(self.dir.path().join("xdg")),
                ours: OURS.to_string(),
                path: ShellPath::Unread,
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

    /// Waits for a file a child writes, and answers its first line — a bounded
    /// wait, because the file is written by the child rather than before it.
    #[cfg(unix)]
    fn wait_for_text(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && !text.trim().is_empty()
            {
                return text.trim().to_string();
            }
            assert!(
                Instant::now() < deadline,
                "{} was never written",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Whether a pid is still there. `kill -0` answers yes for a zombie too,
    /// which is the point: a child nobody reaped is still there.
    #[cfg(unix)]
    fn alive(pid: &str) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid)
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// Waits — bounded — for a pid to be gone: a killed process is reaped by
    /// whoever inherits it, which is not this test.
    #[cfg(unix)]
    fn wait_gone(pid: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
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
    /// a listed client that is already current is written nothing for.
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
            &[on(ClientId::Claude), on(ClientId::Codex)],
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

    /// A client that is not listed is not read, not run and not answered — even
    /// when it is `Current`. That is the difference between "the person moved
    /// this box" and "the person did not": a box whose state could not be read
    /// is disabled and unticked, and treating it as "not wanted" would remove a
    /// working connection on a press that was about another agent.
    #[cfg(unix)]
    #[test]
    fn a_client_the_person_did_not_touch_is_never_run() {
        let stand = Stand::new();
        stand.install("claude");
        stand.install("codex");
        // Claude is connected with this build's own path; Codex is not connected.
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        stand.fixture("codex.mcp-get.code", "1");
        stand.fixture(
            "codex.mcp-get.err",
            "Error: No MCP server named 'mnema' found.",
        );
        let applied = apply(&stand.env(), &[on(ClientId::Codex)], TIMEOUT, TIMEOUT);
        assert_eq!(
            applied.iter().map(|applied| applied.id).collect::<Vec<_>>(),
            [ClientId::Codex]
        );
        assert_eq!(applied[0].error, None);
        assert_eq!(
            stand.calls(),
            [
                "codex [mcp] [get] [mnema] [--json]",
                "codex [mcp] [add] [mnema] [--] [/tmp/My App's/mnema] [mcp]",
            ]
        );
    }

    /// Unchecked and current is a `remove`, with the arguments measured on the
    /// same day.
    #[cfg(unix)]
    #[test]
    fn apply_removes_a_current_client_the_person_unticked() {
        let stand = Stand::new();
        stand.install("claude");
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        let applied = apply(&stand.env(), &[off(ClientId::Claude)], TIMEOUT, TIMEOUT);
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
        let applied = apply(&stand.env(), &[on(ClientId::Claude)], TIMEOUT, TIMEOUT);
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
        let applied = apply(&stand.env(), &[on(ClientId::Codex)], TIMEOUT, TIMEOUT);
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
        let applied = apply(&stand.env(), &[off(ClientId::Opencode)], TIMEOUT, TIMEOUT);
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
            &[on(ClientId::Claude), on(ClientId::Codex)],
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
        let applied = apply(&stand.env(), &[on(ClientId::Claude)], TIMEOUT, TIMEOUT);
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
        let applied = apply(&stand.env(), &[on(ClientId::Opencode)], TIMEOUT, TIMEOUT);
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

    /// The shell prints a greeting before the answer and a word after it; the
    /// CLI still runs with exactly the `PATH` between the markers.
    #[cfg(unix)]
    #[test]
    fn a_chatty_shell_does_not_corrupt_the_path() {
        let stand = Stand::new();
        stand.shell(true);
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
            "the greeting reached the child's PATH"
        );
    }

    /// A name is a file name and nothing else: no shell ever sees it, so a name
    /// whose meaning in a shell is "run this too" resolves to nothing and starts
    /// nothing.
    #[cfg(unix)]
    #[test]
    fn a_name_with_shell_metacharacters_resolves_to_nothing() {
        let stand = Stand::new();
        assert_eq!(resolve(&stand.login, "a;touch PWNED"), None);
        assert!(
            !stand.bin.join("PWNED").exists(),
            "the name was run as a command"
        );
    }

    /// `command -v`'s own rule, done here: the first directory whose entry is an
    /// executable regular file.
    #[cfg(unix)]
    #[test]
    fn a_name_resolves_to_the_first_executable_file_on_the_path() {
        let dir = TempDir::new().unwrap();
        let one = dir.path().join("one");
        let two = dir.path().join("two");
        fs::create_dir_all(one.join("claude")).unwrap();
        fs::create_dir_all(&two).unwrap();
        let path = format!("{}:{}", one.display(), two.display());
        // A directory called `claude`, and a file nobody may run.
        let plain = two.join("claude");
        fs::write(&plain, "#!/bin/sh\n").unwrap();
        assert_eq!(resolve(&path, "claude"), None);
        // The execute bit is the whole difference.
        executable(&plain, "#!/bin/sh\n");
        assert_eq!(resolve(&path, "claude"), Some(plain.clone()));
        // And the first directory that holds one wins.
        fs::remove_dir(one.join("claude")).unwrap();
        let first = executable(&one.join("claude"), "#!/bin/sh\n");
        assert_eq!(resolve(&path, "claude"), Some(first));
        // An empty entry means "the current directory" to a shell; this
        // application does not run a CLI out of the directory it was started in.
        assert_eq!(
            resolve(&format!(":{}", two.display()), "claude"),
            Some(plain)
        );
    }

    /// Only absolute entries are searched: `.`, `bin` and `node_modules/.bin`
    /// mean a shell's working directory, and this application's is wherever it
    /// happened to be started from.
    #[cfg(unix)]
    #[test]
    fn a_path_entry_that_is_relative_is_not_searched() {
        assert_eq!(
            search_dirs(".:bin:/usr/local/bin:node_modules/.bin::/bin").collect::<Vec<_>>(),
            [PathBuf::from("/usr/local/bin"), PathBuf::from("/bin")]
        );
    }

    /// A child that outstays its timeout is killed with everything it started,
    /// and waited for: a kill without the wait leaves a zombie whose pid answers
    /// `kill -0` until this process exits.
    #[cfg(unix)]
    #[test]
    fn a_child_that_outlives_the_timeout_is_killed_with_its_group() {
        let dir = TempDir::new().unwrap();
        let pid_file = dir.path().join("pid");
        let job_file = dir.path().join("job");
        let script = executable(
            &dir.path().join("slow"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\n\
                 ( sleep 30 & printf '%s\\n' \"$!\" > '{}' )\nsleep 30\n",
                pid_file.display(),
                job_file.display()
            ),
        );
        let started = Instant::now();
        let error = run(&script, &[], Duration::from_secs(2), READER_GRACE, None).unwrap_err();
        assert!(error.contains("timed out after 2 s"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the timeout did not cut the child short"
        );
        for file in [pid_file, job_file] {
            let pid = wait_for_text(&file);
            assert!(wait_gone(&pid), "still there after the timeout: {pid}");
        }
    }

    /// A command whose output is still held after the grace is an error and
    /// never an empty answer: an empty `claude mcp get` reads as "no server
    /// connected", and the person's record would then never be removed.
    #[cfg(unix)]
    #[test]
    fn a_command_whose_output_stays_open_is_an_error_not_an_empty_answer() {
        let dir = TempDir::new().unwrap();
        let job_file = dir.path().join("job");
        let script = executable(
            &dir.path().join("holder"),
            &format!(
                "#!/bin/sh\n( sleep 30 & printf '%s\\n' \"$!\" > '{}' )\nprintf 'done\\n'\n",
                job_file.display()
            ),
        );
        let started = Instant::now();
        // A timeout far longer than the grace, so that the wait below can only
        // have ended because of the grace.
        let error = run(
            &script,
            &[],
            Duration::from_secs(30),
            Duration::from_millis(100),
            None,
        )
        .expect_err("a cut-off answer was handed back as a success");
        assert!(error.contains("kept its output open"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the grace did not end the wait"
        );
        // The group is ended when the pipe is held: a member of it holds it.
        let job = wait_for_text(&job_file);
        assert!(wait_gone(&job), "the holder outlived its command: {job}");
    }

    /// A background job that kept no pipe is none of this application's
    /// business: a login shell's `(brew update &)` may not be killed by opening
    /// a settings section.
    #[cfg(unix)]
    #[test]
    fn a_finished_command_leaves_a_background_job_that_holds_no_pipe_alone() {
        let dir = TempDir::new().unwrap();
        let job_file = dir.path().join("job");
        let script = executable(
            &dir.path().join("quiet"),
            &format!(
                "#!/bin/sh\n( sleep 30 >/dev/null 2>&1 & printf '%s\\n' \"$!\" > '{}' )\nprintf 'done\\n'\n",
                job_file.display()
            ),
        );
        let output = run(
            &script,
            &[],
            Duration::from_secs(5),
            Duration::from_millis(100),
            None,
        )
        .expect("a command with nothing holding its pipes is a success");
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "done\n");
        let job = wait_for_text(&job_file);
        assert!(alive(&job), "the command killed a job it did not start");
        // Cleanup: nobody else is going to end this one.
        let _ = Command::new("kill").arg("-KILL").arg(&job).status();
        assert!(wait_gone(&job), "the job would not die: {job}");
    }

    /// A reader that answers late is not waited for past the deadline: a pipe
    /// held by a process that left the child's group — a daemonizing CLI — never
    /// reaches EOF.
    #[cfg(unix)]
    #[test]
    fn a_reader_that_answers_late_does_not_hold_the_command_past_its_deadline() {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = send.send("late".to_string());
        });
        let started = Instant::now();
        let text = joined(Some(receive), Instant::now() + Duration::from_millis(50));
        assert_eq!(text, None, "a deadline is not an empty answer");
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "the answer waited past its deadline"
        );
    }

    /// A shell that will not answer is replaced by the second one: `tcsh` reads
    /// `-l -c` as an option it does not know, and then every client would be
    /// "not installed" with nothing said.
    #[cfg(unix)]
    #[test]
    fn a_shell_that_will_not_answer_falls_back_to_the_second_one() {
        let stand = Stand::new();
        stand.script(
            "shell",
            "#!/bin/sh\nprintf '%s\\n' \"Unknown option: '-lc'\" >&2\nexit 1\n",
        );
        stand.install("claude");
        stand.fixture("claude.mcp-get.out", CLAUDE_GET);
        let state = &states(&stand.env(), TIMEOUT, TIMEOUT)[0];
        assert_eq!(state.error, None);
        assert_eq!(state.link, Some(Link::Current));
        assert!(state.installed);
    }

    /// Neither shell answers: every client says so, and `apply` answers for
    /// every client rather than for none.
    #[cfg(unix)]
    #[test]
    fn a_shell_that_will_not_say_its_path_is_an_error_for_every_client() {
        let stand = Stand::new();
        stand.script(
            "shell",
            "#!/bin/sh\nprintf '%s\\n' \"Unknown option: '-lc'\" >&2\nexit 1\n",
        );
        stand.fallback_shell(Some("#!/bin/sh\nexit 1\n"));
        let states = states(&stand.env(), TIMEOUT, TIMEOUT);
        assert_eq!(states.len(), 4);
        for state in &states {
            assert!(!state.installed, "{:?}", state.id);
            assert_eq!(state.link, None);
            let error = state.error.as_deref().unwrap_or_default();
            assert!(
                error.contains("could not read your shell's PATH"),
                "{error}"
            );
        }
        assert!(
            states[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Unknown option: '-lc'"),
            "the shell's own words are not in the details: {:?}",
            states[0].error
        );
        // Only the listed clients were asked for, so only those answer;
        // `states` is what marks all four and disables Apply.
        let applied = apply(
            &stand.env(),
            &[on(ClientId::Claude), on(ClientId::Pi)],
            TIMEOUT,
            TIMEOUT,
        );
        assert_eq!(
            applied.iter().map(|applied| applied.id).collect::<Vec<_>>(),
            [ClientId::Claude, ClientId::Pi]
        );
        assert!(applied.iter().all(|applied| {
            applied
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("could not read your shell's PATH")
        }));
    }

    /// `$HOME` unset and no `$XDG_CONFIG_HOME` is an error, not a relative path
    /// read and written against whatever directory the application was started
    /// from.
    #[cfg(unix)]
    #[test]
    fn an_unset_home_is_an_error_and_not_a_relative_path() {
        let stand = Stand::new();
        stand.install("opencode");
        let env = Env {
            home: None,
            xdg_config: None,
            ..stand.env()
        };
        let state = &states(&env, TIMEOUT, TIMEOUT)[3];
        assert!(state.installed);
        assert_eq!(state.link, None);
        assert!(
            state
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("home directory"),
            "{:?}",
            state.error
        );
    }

    /// The window between our read and our rename belongs to whichever writer
    /// got there first — opencode itself can be running. Driven through `apply`,
    /// because that is the only caller of the disconnect.
    #[cfg(unix)]
    #[test]
    fn a_file_another_writer_changes_mid_disconnect_is_left_to_that_writer() {
        let stand = Stand::new();
        stand.install("opencode");
        let file = stand.opencode_file();
        fs::write(&file, OPENCODE_JSON).unwrap();
        let concurrent = "{\n  \"mcp\": {\n    \"context7\": {}\n  }\n}\n";
        let _writer = SecondWriter::arm(&file, concurrent);
        let applied = apply(&stand.env(), &[off(ClientId::Opencode)], TIMEOUT, TIMEOUT);
        assert_eq!(applied.len(), 1);
        let error = applied[0].error.as_deref().unwrap_or_default();
        assert!(
            error.contains("changed while being updated, try again"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), concurrent);
    }

    /// A second writer for the window between [`remove_opencode`]'s read and its
    /// re-read, alive for as long as this guard is.
    #[cfg(unix)]
    struct SecondWriter;

    #[cfg(unix)]
    impl SecondWriter {
        fn arm(path: &Path, text: &str) -> SecondWriter {
            let path = path.to_path_buf();
            let text = text.to_string();
            SECOND_WRITER.with(|writer| {
                *writer.borrow_mut() = Some(Box::new(move || fs::write(&path, &text).unwrap()));
            });
            SecondWriter
        }
    }

    #[cfg(unix)]
    impl Drop for SecondWriter {
        fn drop(&mut self) {
            SECOND_WRITER.with(|writer| *writer.borrow_mut() = None);
        }
    }

    /// What Task 4's window sends: the client and the direction, deserialized
    /// from the JSON the command receives — the direction that actually runs.
    /// `connect: true` on purpose: a dropped or misnamed field would default to
    /// `false` and pass a `false` fixture.
    #[test]
    fn a_change_reaches_the_backend_with_the_fields_it_names() {
        let change: Change = serde_json::from_value(json!({"id": "codex", "connect": true}))
            .expect("a change deserializes");
        assert_eq!(
            change,
            Change {
                id: ClientId::Codex,
                connect: true,
            }
        );
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
