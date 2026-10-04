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
const CHUNK: usize = 16 * 1024;

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
        if free < needed {
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
            let mut body = resp.into_body().into_reader();
            let mut buf = vec![0u8; CHUNK];
            let mut written = offset;
            // Opened per chunk, not held: a failing disk or a changed permission
            // surfaces as an error on the next write, with the bytes so far kept.
            let mut first = offset == 0;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let n = body.read(&mut buf).map_err(io)?;
                if n == 0 {
                    break;
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
                    .and_then(|mut file| file.write_all(&buf[..n]))
                    .map_err(io)?;
                written += n as u64;
                report(base + written.min(f.size));
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if written != f.size {
                // Short or long: keep a short one to resume; a long one can only be wrong.
                if written > f.size {
                    let _ = fs::remove_file(&part);
                    return Err(Error::Checksum {
                        file: f.name.clone(),
                    });
                }
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
    fs::rename(part, final_path).map_err(io)
}

fn part_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.part"))
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

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
        return 0;
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    (st.f_bavail as u64).saturating_mul(st.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space(_: &Path) -> u64 {
    u64::MAX
}
