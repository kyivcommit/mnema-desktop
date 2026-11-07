//! Recognising one PDF's pages in several `--ocr` helper processes (D196).
//!
//! **Processes, not threads.** Measured 2026-11-06 on an M2 Max: two to six
//! threads calling Vision in one process gave 0.92–1.06× of one; separate
//! processes scaled to 3.3× at eight and no further — the plateau sits at the
//! machine's performance-core count.

use std::collections::VecDeque;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::ocr_wire::{self, Reply};
use crate::{OcrBackend, OwnedPageImage};

/// Set to a count ≥ 1, this replaces the one [`processes`] works out from the
/// machine. For tests and diagnosis; `1` is the in-process path with no helper.
pub const OCR_PROCESSES_ENV: &str = "MNEMA_OCR_PROCESSES";

/// The most helpers one worker starts. Past eight the measured wall time did
/// not move (4.3 s at eight, 4.5 s at twelve) while every process costs
/// ~143 MB.
pub const MAX_PROCESSES: usize = 8;

/// How many processes recognise one PDF's pages: `env` if it names a count,
/// else the performance cores, else half of `parallelism`; never below 1 or
/// above [`MAX_PROCESSES`].
pub fn processes(p_cores: Option<usize>, parallelism: Option<usize>, env: Option<&str>) -> usize {
    let chosen = env
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .or(p_cores.filter(|&n| n >= 1))
        .or(parallelism.map(|n| n / 2))
        .unwrap_or(1);
    chosen.clamp(1, MAX_PROCESSES)
}

/// [`processes`] for this machine and this process's environment.
pub fn configured() -> usize {
    processes(
        p_cores(),
        std::thread::available_parallelism().ok().map(|n| n.get()),
        std::env::var(OCR_PROCESSES_ENV).ok().as_deref(),
    )
}

/// The performance-core count Apple silicon reports, or `None` where there is
/// no such figure (an Intel Mac, another OS, a failed call).
#[cfg(target_os = "macos")]
pub fn p_cores() -> Option<usize> {
    use std::ffi::{c_char, c_int, c_void};

    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }
    let mut value: i32 = 0;
    let mut len = std::mem::size_of::<i32>();
    // SAFETY: `value` is a live i32 and `len` its size; sysctl writes at most
    // `len` bytes into it and reports the length written. Nothing is set.
    let rc = unsafe {
        sysctlbyname(
            c"hw.perflevel0.physicalcpu".as_ptr(),
            (&raw mut value).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == std::mem::size_of::<i32>() && value > 0).then_some(value as usize)
}

#[cfg(not(target_os = "macos"))]
pub fn p_cores() -> Option<usize> {
    None
}

/// How long a helper whose stdin was closed gets to finish its page and exit
/// before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// An [`OcrBackend`] of up to `n` `--ocr` helper processes of `exe` (D196).
///
/// Helpers start lazily, one per page that finds none idle, so a PDF with one
/// scanned page starts one — and **the second only after the first has
/// answered.** Vision's cold start (~46 s) is paid one process after another:
/// eight helpers starting cold together gave the first page after 295 s
/// (2026-11-06, `Tom_05`, M2 Max), against the pool's 300 s per page. One
/// helper pays it alone and warms the cache the rest then find.
///
/// Each helper has a thread that writes its requests and reads its replies;
/// every reply comes back over one channel.
///
/// **A helper that fails is not trusted again for this file.** Its page — the
/// one it held when its pipe broke, it answered garbage, or it answered
/// another page's number — is recognised here, in this process, and the
/// helper is not replaced. With every helper gone, each page is recognised
/// here as before D196: slower, never lost.
pub struct Helpers {
    exe: PathBuf,
    /// Helpers this backend may still start.
    remaining: usize,
    helpers: Vec<Helper>,
    idle: Vec<usize>,
    events: Receiver<Event>,
    events_tx: Sender<Event>,
    /// Pages recognised in this process, waiting to be taken by `next`.
    answered_here: VecDeque<(u32, Reply)>,
    /// A helper has answered a page, so Vision is warm and the rest may start.
    warm: bool,
}

struct Helper {
    jobs: Option<Sender<(u32, OwnedPageImage)>>,
    child: Child,
    thread: Option<JoinHandle<()>>,
    alive: bool,
}

enum Event {
    Answered {
        helper: usize,
        page_no: u32,
        reply: Reply,
    },
    Failed {
        helper: usize,
        page_no: u32,
        image: OwnedPageImage,
    },
}

impl Helpers {
    pub fn new(exe: PathBuf, n: usize) -> Self {
        let (events_tx, events) = mpsc::channel();
        Helpers {
            exe,
            remaining: n.max(1),
            helpers: Vec::new(),
            idle: Vec::new(),
            events,
            events_tx,
            answered_here: VecDeque::new(),
            warm: false,
        }
    }

    /// Starts one more helper, or `None` if the process would not start —
    /// after which no more are tried for this file.
    fn spawn(&mut self) -> Option<usize> {
        self.remaining -= 1;
        // Spawned from this thread only: the helper threads never fork, so no
        // pipe of one helper can leak into another between `pipe` and the
        // `FD_CLOEXEC` std sets on it (the race D160 closed in the pool).
        let spawned = Command::new(&self.exe)
            .arg("--ocr")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The worker's own stderr: the file the pool gave it, never a
            // pipe nobody reads.
            .stderr(Stdio::inherit())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                eprintln!("mnema-extract-worker: an OCR helper would not start: {e}");
                self.remaining = 0;
                return None;
            }
        };
        let index = self.helpers.len();
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let (jobs, inbox) = mpsc::channel::<(u32, OwnedPageImage)>();
        let events = self.events_tx.clone();
        let thread = std::thread::spawn(move || {
            for (page_no, image) in inbox {
                let sent = ocr_wire::write_request(&mut stdin, page_no, &image.as_image())
                    .and_then(|()| stdin.flush());
                let reply = match sent {
                    Ok(()) => ocr_wire::read_reply(&mut stdout),
                    Err(e) => Err(e.into()),
                };
                let event = match reply {
                    Ok(Some((seq, reply))) if seq == page_no => Event::Answered {
                        helper: index,
                        page_no,
                        reply,
                    },
                    other => {
                        eprintln!(
                            "mnema-extract-worker: OCR helper {index} failed on page \
                             {page_no}, reading it in the worker: {}",
                            match other {
                                Ok(Some((seq, _))) => format!("it answered page {seq}"),
                                Ok(None) => "it exited".to_string(),
                                Err(e) => e.to_string(),
                            }
                        );
                        let _ = events.send(Event::Failed {
                            helper: index,
                            page_no,
                            image,
                        });
                        // Dropping stdin here closes the helper's input.
                        return;
                    }
                };
                if events.send(event).is_err() {
                    return;
                }
            }
        });
        self.helpers.push(Helper {
            jobs: Some(jobs),
            child,
            thread: Some(thread),
            alive: true,
        });
        Some(index)
    }

    fn live(&self) -> usize {
        self.helpers.iter().filter(|h| h.alive).count()
    }

    /// An event, turned into the answer it carries.
    fn answer(&mut self, event: Event) -> (u32, Reply) {
        match event {
            Event::Answered {
                helper,
                page_no,
                reply,
            } => {
                self.idle.push(helper);
                self.warm = true;
                (page_no, reply)
            }
            Event::Failed {
                helper,
                page_no,
                image,
            } => {
                self.retire(helper);
                (page_no, crate::recognise(&image.as_image()))
            }
        }
    }

    fn retire(&mut self, index: usize) {
        let helper = &mut self.helpers[index];
        helper.alive = false;
        helper.jobs = None;
        let _ = helper.child.kill();
        let _ = helper.child.wait();
    }
}

impl OcrBackend for Helpers {
    /// The gate on starting helpers is here: the pipeline never has more pages
    /// in flight than this, so before the first answer it holds one.
    fn capacity(&self) -> usize {
        let startable = if self.warm || self.helpers.is_empty() {
            self.remaining
        } else {
            0
        };
        (self.live() + startable).max(1)
    }

    fn submit(&mut self, page_no: u32, image: OwnedPageImage) {
        let helper = match self.idle.pop() {
            Some(index) => Some(index),
            None if self.remaining > 0 => self.spawn(),
            None => None,
        };
        let Some(index) = helper else {
            let reply = crate::recognise(&image.as_image());
            self.answered_here.push_back((page_no, reply));
            return;
        };
        let jobs = self.helpers[index]
            .jobs
            .as_ref()
            .expect("an idle helper is alive");
        if let Err(mpsc::SendError((page_no, image))) = jobs.send((page_no, image)) {
            // Its thread has gone, so its failure event is already queued
            // for a page it held before; this page is read here.
            let reply = crate::recognise(&image.as_image());
            self.answered_here.push_back((page_no, reply));
        }
    }

    fn next(&mut self) -> (u32, Reply) {
        if let Some(answer) = self.answered_here.pop_front() {
            return answer;
        }
        let event = self
            .events
            .recv()
            .expect("this backend holds a sender, so the channel stays open");
        self.answer(event)
    }

    fn try_next(&mut self) -> Option<(u32, Reply)> {
        if let Some(answer) = self.answered_here.pop_front() {
            return Some(answer);
        }
        let event = self.events.try_recv().ok()?;
        Some(self.answer(event))
    }
}

impl Drop for Helpers {
    /// Closes every helper's input, gives it [`EXIT_GRACE`] to finish the page
    /// it holds and exit, and kills what is left: no helper outlives the file.
    fn drop(&mut self) {
        for helper in &mut self.helpers {
            helper.jobs = None;
        }
        let until = Instant::now() + EXIT_GRACE;
        for helper in &mut self.helpers {
            while matches!(helper.child.try_wait(), Ok(None)) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = helper.child.kill();
            let _ = helper.child.wait();
            if let Some(thread) = helper.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
