//! Model files on disk: pinned download, resume, sha256, removal, free space.
//!
//! A file is `Ready` only under its final name with the pinned size, and it
//! gets that name only after its sha256 matched (`<file>.part` until then).

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::manifest::{FileSpec, Manifest};
use crate::{Error, ModelId};

const RESERVE: u64 = 512 * 1024 * 1024;
const CHUNK: usize = 1 << 20;
/// No byte for this long: the connection is dead, give up (the part is kept).
const IDLE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    Absent,
    Downloading { done: u64, total: u64 },
    Ready,
    Failed(String),
}

pub struct Store {
    root: PathBuf,
    hub: String,
    manifest: Manifest,
    probe: fn(&Path) -> u64,
    chunk: usize,
    idle: Duration,
    /// Downloading/Failed live here only; the disk is the rest of the truth.
    live: Mutex<HashMap<u8, ModelState>>,
}

fn key(id: ModelId) -> u8 {
    id as u8
}

impl Store {
    pub fn new(root: PathBuf, hub: String) -> Self {
        Self {
            root,
            hub,
            manifest: Manifest::pinned(),
            probe: free_space,
            chunk: CHUNK,
            idle: IDLE,
            live: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_manifest(mut self, m: Manifest) -> Self {
        self.manifest = m;
        self
    }

    pub fn with_free_space_probe(mut self, f: fn(&Path) -> u64) -> Self {
        self.probe = f;
        self
    }

    /// Test knobs: read/write granularity and how long a silent body is waited on.
    pub fn with_transfer(mut self, chunk: usize, idle: Duration) -> Self {
        self.chunk = chunk;
        self.idle = idle;
        self
    }

    pub fn dir(&self, id: ModelId) -> PathBuf {
        self.root.join(self.manifest.spec(id).dir_name())
    }

    pub fn state(&self, id: ModelId) -> ModelState {
        if let Some(s) = self.live.lock().unwrap().get(&key(id)) {
            return s.clone();
        }
        let dir = self.dir(id);
        let ready = self.manifest.spec(id).files.iter().all(|f| {
            fs::metadata(dir.join(&f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
        });
        if ready {
            ModelState::Ready
        } else {
            ModelState::Absent
        }
    }

    pub fn remove(&self, id: ModelId) -> Result<(), Error> {
        self.live.lock().unwrap().remove(&key(id));
        match fs::remove_dir_all(self.dir(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Io(e.to_string())),
        }
    }

    pub fn download(
        &self,
        id: ModelId,
        progress: &dyn Fn(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        let result = self.run(id, progress, cancel);
        let mut live = self.live.lock().unwrap();
        match &result {
            Ok(()) | Err(Error::Cancelled) | Err(Error::NoSpace { .. }) => {
                live.remove(&key(id));
            }
            Err(e) => {
                live.insert(key(id), ModelState::Failed(e.to_string()));
            }
        }
        result
    }

    fn run(
        &self,
        id: ModelId,
        progress: &dyn Fn(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        let spec = self.manifest.spec(id);
        let dir = self.dir(id);
        let total: u64 = spec.files.iter().map(|f| f.size).sum();

        // What is already in place (a final file, or a partial one) is not owed.
        let have = |f: &FileSpec| -> u64 {
            let len = |p: PathBuf| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            if len(dir.join(&f.name)) == f.size {
                f.size
            } else {
                len(part_path(&dir, &f.name)).min(f.size)
            }
        };
        let owed: u64 = spec.files.iter().map(|f| f.size - have(f)).sum();
        let needed = owed + RESERVE;
        let free = (self.probe)(&self.root);
        // Nothing owed (a finished model) needs no room.
        if owed > 0 && free < needed {
            return Err(Error::NoSpace { needed, free });
        }

        fs::create_dir_all(&dir).map_err(io)?;
        let mut done: u64 = 0;
        for f in &spec.files {
            let report = |d: u64| {
                self.live
                    .lock()
                    .unwrap()
                    .insert(key(id), ModelState::Downloading { done: d, total });
                progress(d, total);
            };
            report(done);
            self.fetch_file(spec, &dir, f, done, &report, cancel)?;
            done += f.size;
            report(done);
        }
        Ok(())
    }

    fn fetch_file(
        &self,
        spec: &crate::manifest::ModelSpec,
        dir: &Path,
        f: &FileSpec,
        base: u64,
        report: &dyn Fn(u64),
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        let final_path = dir.join(&f.name);
        if fs::metadata(&final_path).is_ok_and(|m| m.len() == f.size) {
            return Ok(());
        }
        let part = part_path(dir, &f.name);
        if let Some(bytes) = f.bundled {
            fs::write(&part, bytes).map_err(io)?;
            return finish(&part, &final_path, f);
        }

        let mut offset = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if offset > f.size {
            fs::remove_file(&part).map_err(io)?;
            offset = 0;
        }
        if offset < f.size {
            let url = format!(
                "{}/{}/resolve/{}/{}",
                self.hub, spec.repo, spec.commit, f.name
            );
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(30)))
                .build()
                .into();
            let mut req = agent.get(&url);
            if offset > 0 {
                req = req.header("range", &format!("bytes={offset}-"));
            }
            let resp = req.call().map_err(|e| Error::Http(e.to_string()))?;
            match resp.status().as_u16() {
                206 => {}
                // The server ignored the range (or there was none): the body is
                // the whole file, so the partial copy starts over.
                200 => offset = 0,
                s => return Err(Error::Http(format!("{url}: status {s}"))),
            }
            // The body is read on its own thread so that a silent connection can
            // be cancelled and timed out here; ureq has no idle-read timeout. An
            // abandoned reader ends when its socket errors or the server closes.
            let mut body = resp.into_body().into_reader();
            let chunk = self.chunk;
            let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, String>>(2);
            std::thread::spawn(move || {
                loop {
                    let mut buf = vec![0u8; chunk];
                    let mut n = 0;
                    let mut failed = None;
                    while n < chunk {
                        match body.read(&mut buf[n..]) {
                            Ok(0) => break,
                            Ok(k) => n += k,
                            Err(e) => {
                                failed = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    buf.truncate(n);
                    // Bytes that arrived before a failure are still worth keeping.
                    if n > 0 && tx.send(Ok(buf)).is_err() {
                        return;
                    }
                    if let Some(e) = failed {
                        let _ = tx.send(Err(e));
                        return;
                    }
                    if n < chunk {
                        return;
                    }
                }
            });
            let mut written = offset;
            // Opened per chunk, not held: a changed permission surfaces as an
            // error on the next write, with the bytes so far kept.
            let mut first = offset == 0;
            let mut last_byte = std::time::Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let data = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(d)) => d,
                    Ok(Err(e)) => return Err(Error::Http(format!("{}: {e}", f.name))),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if last_byte.elapsed() >= self.idle {
                            return Err(Error::Http(format!(
                                "{}: no data for {:?}",
                                f.name, self.idle
                            )));
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                last_byte = std::time::Instant::now();
                // Longer than pinned: wrong whatever it is. Stop before filling the disk.
                if written + data.len() as u64 > f.size {
                    let _ = fs::remove_file(&part);
                    return Err(Error::Checksum {
                        file: f.name.clone(),
                    });
                }
                let mut opts = fs::OpenOptions::new();
                opts.create(true);
                if first {
                    opts.write(true).truncate(true);
                    first = false;
                } else {
                    opts.append(true);
                }
                opts.open(&part)
                    .and_then(|mut file| file.write_all(&data))
                    .map_err(io)?;
                written += data.len() as u64;
                report(base + written);
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if written != f.size {
                // Short: keep it to resume.
                return Err(Error::Http(format!(
                    "{}: body ended at {written} of {}",
                    f.name, f.size
                )));
            }
        }
        finish(&part, &final_path, f)
    }
}

/// Renames `part` to `final_path` only if its sha256 is the pinned one.
fn finish(part: &Path, final_path: &Path, f: &FileSpec) -> Result<(), Error> {
    let mut file = fs::File::open(part).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // Data on disk before the name: a crash must not leave a final-named file
    // of the pinned size whose bytes never reached the platter.
    file.sync_all().map_err(io)?;
    drop(file);
    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != f.sha256 {
        let _ = fs::remove_file(part);
        return Err(Error::Checksum {
            file: f.name.clone(),
        });
    }
    fs::rename(part, final_path).map_err(io)?;
    sync_dir(final_path.parent())
}

/// Makes the rename itself durable. Unix only (a directory cannot be opened on Windows).
fn sync_dir(dir: Option<&Path>) -> Result<(), Error> {
    #[cfg(unix)]
    if let Some(d) = dir {
        fs::File::open(d).and_then(|f| f.sync_all()).map_err(io)?;
    }
    let _ = dir;
    Ok(())
}

fn part_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.part"))
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// Unknown (statvfs failing) is reported as unlimited, so the writes themselves
/// report a real disk error instead of a misleading NoSpace.
/// Free bytes where `path` will live: asked of its nearest existing ancestor.
#[cfg(unix)]
fn free_space(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let mut p = path;
    while !p.exists() {
        match p.parent() {
            Some(parent) => p = parent,
            None => break,
        }
    }
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return u64::MAX;
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return u64::MAX;
    }
    (st.f_bavail as u64).saturating_mul(st.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space(_: &Path) -> u64 {
    u64::MAX
}
