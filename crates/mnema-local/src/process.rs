use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{Error, ModelId};

/// Where to send requests, and the secret that goes with them.
#[derive(Clone)]
pub struct Endpoint {
    /// `http://127.0.0.1:<n>/v1`
    pub base: String,
    /// `http://127.0.0.1:<n>/interactive/v1`
    pub query_base: String,
    pub token: String,
}

impl std::fmt::Debug for Endpoint {
    /// The token is a bearer secret: never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("base", &self.base)
            .field("query_base", &self.query_base)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// How long the process may take to print its `PORT` line.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How much of the end of stderr a crash report keeps.
const TAIL_BYTES: usize = 8 * 1024;

/// One live (or just dead) process and everything read from it.
struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    port: u16,
    token: String,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    stderr_reader: Option<JoinHandle<()>>,
}

struct State {
    running: Option<Running>,
    /// The one restart has been spent.
    restarted: bool,
    /// Set once the process died with no restart left; nothing spawns after.
    failed: Option<String>,
    /// What stderr said when the process died the first time, kept for the
    /// report if it dies again.
    first_tail: String,
}

pub struct Sidecar {
    binary: PathBuf,
    embed_dir: PathBuf,
    chat_dir: PathBuf,
    env: Vec<(String, String)>,
    state: Mutex<State>,
}

impl Sidecar {
    pub fn start(binary: &Path, embed_dir: &Path, chat_dir: &Path) -> Result<Sidecar, Error> {
        Self::start_with_env(binary, embed_dir, chat_dir, &[])
    }

    /// `start` with extra environment for the child, kept for the restart too.
    /// Exists so tests can steer the fake without touching the process-wide env.
    pub fn start_with_env(
        binary: &Path,
        embed_dir: &Path,
        chat_dir: &Path,
        env: &[(&str, &str)],
    ) -> Result<Sidecar, Error> {
        let sidecar = Sidecar {
            binary: binary.to_owned(),
            embed_dir: embed_dir.to_owned(),
            chat_dir: chat_dir.to_owned(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            state: Mutex::new(State {
                running: None,
                restarted: false,
                failed: None,
                first_tail: String::new(),
            }),
        };
        let running = sidecar.spawn()?;
        sidecar.state.lock().unwrap().running = Some(running);
        Ok(sidecar)
    }

    fn spawn(&self) -> Result<Running, Error> {
        let token = new_token()?;
        let mut child = {
            // The guard shared with `mnema-pool` (D160): this child lives as long as
            // the application, so a pool worker's half-made pipe must not leak into it.
            let _lock = mnema_pool::spawn_guard();
            Command::new(&self.binary)
                .arg("--embed")
                .arg(&self.embed_dir)
                .arg("--chat")
                .arg(&self.chat_dir)
                .envs(self.env.iter().map(|(k, v)| (k, v)))
                .env("MNEMA_MLX_TOKEN", &token)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| Error::Io(format!("cannot start {}: {e}", self.binary.display())))?
        };
        let stdin = child.stdin.take();
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        let stderr_reader = child.stderr.take().map(|mut err| {
            let tail = Arc::clone(&stderr_tail);
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = tail.lock().unwrap();
                    t.extend_from_slice(&buf[..n]);
                    let excess = t.len().saturating_sub(TAIL_BYTES);
                    t.drain(..excess);
                }
            })
        });
        let mut running = Running {
            child,
            stdin,
            port: 0,
            token,
            stderr_tail,
            stderr_reader,
        };

        // First line of stdout is `PORT <n>`; the rest is drained so a chatty
        // process never blocks on a full pipe.
        let stdout = running.child.stdout.take().expect("stdout is piped");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            let _ = tx.send(lines.next().and_then(|l| l.ok()));
            for _ in lines {}
        });
        let port = rx
            .recv_timeout(START_TIMEOUT)
            .ok()
            .flatten()
            .and_then(|l| l.strip_prefix("PORT ")?.trim().parse::<u16>().ok())
            .filter(|p| *p != 0);
        match port {
            Some(p) => {
                running.port = p;
                Ok(running)
            }
            None => Err(Error::Crashed {
                stderr_tail: running.finish(),
            }),
        }
    }

    pub fn endpoint(&self) -> Result<Endpoint, Error> {
        let mut st = self.state.lock().unwrap();
        if let Some(tail) = &st.failed {
            return Err(Error::Crashed {
                stderr_tail: tail.clone(),
            });
        }
        let alive = match st.running.as_mut() {
            Some(r) => r.child.try_wait().ok().flatten().is_none(),
            None => return Err(Error::NotStarted),
        };
        if !alive {
            let tail = st.running.as_mut().unwrap().finish();
            if st.restarted {
                return Err(self.give_up(&mut st, &tail));
            }
            st.restarted = true;
            st.first_tail = tail;
            match self.spawn() {
                Ok(r) => st.running = Some(r),
                Err(e) => {
                    let why = match e {
                        Error::Crashed { stderr_tail } => stderr_tail,
                        other => other.to_string(),
                    };
                    return Err(self.give_up(&mut st, &why));
                }
            }
        }
        let r = st.running.as_ref().unwrap();
        Ok(Endpoint {
            base: format!("http://127.0.0.1:{}/v1", r.port),
            query_base: format!("http://127.0.0.1:{}/interactive/v1", r.port),
            token: r.token.clone(),
        })
    }

    /// Records the terminal failure: both deaths' reasons, the first one first.
    fn give_up(&self, st: &mut State, second: &str) -> Error {
        let tail = format!(
            "{}\n--- restarted once, died again ---\n{second}",
            st.first_tail
        );
        st.failed = Some(tail.clone());
        Error::Crashed { stderr_tail: tail }
    }

    pub fn load(&self) -> Result<(), Error> {
        self.post("/mnema/load", "", Duration::from_secs(600))
    }

    /// `None` unloads both models (empty body).
    pub fn unload(&self, which: Option<ModelId>) -> Result<(), Error> {
        let body = match which {
            None => "",
            Some(ModelId::Chat) => r#"{"models":["chat"]}"#,
            Some(ModelId::Embed) => r#"{"models":["embed"]}"#,
        };
        self.post("/mnema/unload", body, Duration::from_secs(30))
    }

    /// The `/mnema/…` routes live at the root, outside `base`.
    fn post(&self, path: &str, body: &str, timeout: Duration) -> Result<(), Error> {
        let e = self.endpoint()?;
        let root = e.base.trim_end_matches("/v1");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut resp = agent
            .post(format!("{root}{path}"))
            .header("authorization", &format!("Bearer {}", e.token))
            .header("content-type", "application/json")
            .send(body)
            .map_err(|err| Error::Http(err.to_string()))?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        Err(Error::Http(format!("{path}: {status} {text}")))
    }

    pub fn pid(&self) -> u32 {
        self.state
            .lock()
            .unwrap()
            .running
            .as_ref()
            .map_or(0, |r| r.child.id())
    }
}

impl Drop for Sidecar {
    /// Close stdin (the process's cue to leave), wait up to 2 s, then kill.
    fn drop(&mut self) {
        let st = self.state.get_mut().unwrap_or_else(|p| p.into_inner());
        if let Some(r) = st.running.as_mut() {
            drop(r.stdin.take());
            if !exited_within(&mut r.child, Duration::from_secs(2)) {
                let _ = r.child.kill();
                exited_within(&mut r.child, Duration::from_secs(1));
            }
            // Only a process that is gone closes stderr; joining a survivor's
            // reader would hang.
            if let Some(h) = r.stderr_reader.take()
                && r.child.try_wait().ok().flatten().is_some()
            {
                let _ = h.join();
            }
        }
    }
}

/// Polls rather than `wait()`s, so a process that will not die costs a bounded
/// time and not a hang.
fn exited_within(child: &mut Child, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Running {
    /// Kill if still alive, reap, and return what stderr said.
    fn finish(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Bounded: if the pipe leaked into another process the reader never sees
        // EOF, and this runs under the state mutex.
        if let Some(h) = self.stderr_reader.take() {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = h.join();
                let _ = tx.send(());
            });
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
        String::from_utf8_lossy(&self.stderr_tail.lock().unwrap()).into_owned()
    }
}

/// 32 random bytes as hex, from the OS. No dependency for it: `/dev/urandom` is
/// there on every platform the sidecar runs on; elsewhere `start` fails, which
/// is fine because `available()` is false there.
fn new_token() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| Error::Io(format!("no random bytes for the token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
