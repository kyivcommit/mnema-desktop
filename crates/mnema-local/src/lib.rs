//! Supervision of the MLX sidecar (`sidecar/mnema-mlx`): start it, read its
//! port, hold its token, restart it once, leave no orphan.

mod manifest;
mod models;
mod process;

pub use manifest::{FileSpec, Manifest, ModelSpec};
pub use models::{ModelState, Store};

pub use process::{Endpoint, Sidecar};

/// Which of the sidecar's two models a call is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    Embed,
    Chat,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("the local model process was not started")]
    NotStarted,
    /// The process died (twice, or at start); `stderr_tail` is why. The tail
    /// (up to 8 KB) goes to the app's log where it is raised, not into this
    /// message: the message can reach a window, the tail is a diagnostic.
    #[error("the local model process stopped")]
    Crashed { stderr_tail: String },
    #[error("local model process: {0}")]
    Io(String),
    #[error("local model process request failed: {0}")]
    Http(String),
    /// Not enough room for what is still to download (plus a reserve).
    #[error("not enough free space: {needed} bytes needed, {free} free")]
    NoSpace { needed: u64, free: u64 },
    /// The bytes of `file` did not match the pinned sha256; the partial copy is gone.
    #[error("checksum mismatch for {file}")]
    Checksum { file: String },
    #[error("download cancelled")]
    Cancelled,
}

/// Whether this machine can run the sidecar: Apple Silicon, macOS 14 or newer.
pub fn available() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && macos_major().is_some_and(|v| v >= 14)
}

/// `kern.osproductversion` ("14.5"), read in-process: spawning `sw_vers` would be
/// one more spawn outside the shared guard.
#[cfg(target_os = "macos")]
fn macos_major() -> Option<u32> {
    let name = c"kern.osproductversion";
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    // SAFETY: `buf`/`len` describe a writable buffer; `name` is NUL-terminated.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    String::from_utf8_lossy(&buf[..len])
        .trim_end_matches('\0')
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[cfg(not(target_os = "macos"))]
fn macos_major() -> Option<u32> {
    None
}
