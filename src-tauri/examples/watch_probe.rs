//! Prints every event `notify` delivers for one directory, for the spec's
//! platform probes: does reading emit `Access`, does a deleted-and-recreated
//! root keep its subscription, what a detached mount point looks like, and
//! in what form Windows paths arrive.
//!
//!   cargo run -p mnema-desktop --example watch_probe -- /path/to/dir 120
use notify::{RecursiveMode, Watcher};
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: watch_probe <dir> [seconds]");
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(120);
    let mut watcher =
        notify::recommended_watcher(|res: Result<notify::Event, notify::Error>| match res {
            Ok(e) => println!("{:?} flag={:?} paths={:?}", e.kind, e.flag(), e.paths),
            Err(e) => println!("ERR {e}"),
        })
        .expect("watcher");
    watcher
        .watch(std::path::Path::new(&dir), RecursiveMode::Recursive)
        .expect("watch");
    println!("watching {dir} for {secs}s");
    std::thread::sleep(Duration::from_secs(secs));
}
