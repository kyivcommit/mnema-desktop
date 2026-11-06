//! Recognising one PDF's pages in several `--ocr` helper processes (D196).
//!
//! **Processes, not threads.** Measured 2026-11-06 on an M2 Max: two to six
//! threads calling Vision in one process gave 0.92–1.06× of one; separate
//! processes scaled to 3.3× at eight and no further — the plateau sits at the
//! machine's performance-core count.

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
