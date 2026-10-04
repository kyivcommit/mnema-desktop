//! Supervision of the MLX sidecar (`sidecar/mnema-mlx`): start it, read its
//! port, hold its token, restart it once, leave no orphan.

mod process;

pub use process::{Endpoint, Sidecar};

/// Which of the sidecar's two models a call is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    Embed,
    Chat,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the local model process was not started")]
    NotStarted,
    /// The process died (twice, or at start); `stderr_tail` is why.
    #[error("the local model process died: {stderr_tail}")]
    Crashed { stderr_tail: String },
    #[error("local model process: {0}")]
    Io(String),
    #[error("local model process request failed: {0}")]
    Http(String),
}

/// Whether this machine can run the sidecar: Apple Silicon, macOS 14 or newer.
pub fn available() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && macos_major().is_some_and(|v| v >= 14)
}

#[cfg(target_os = "macos")]
fn macos_major() -> Option<u32> {
    let out = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[cfg(not(target_os = "macos"))]
fn macos_major() -> Option<u32> {
    None
}
