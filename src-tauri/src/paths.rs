use std::path::{Path, PathBuf};

/// The index lives in the app's LOCAL data directory.
///
/// Not the roaming one — on Windows those are different places and a roaming
/// profile would try to sync a multi-hundred-megabyte index. Not the cache
/// directory either: macOS purges it under disk pressure. G7.0 §4.
pub fn index_path(app_local_data_dir: &Path) -> PathBuf {
    app_local_data_dir.join("index.sqlite")
}

/// The app-preferences file, beside the index. Available at start-up (unlike the
/// index DB), because the tray needs the locale before any folder is opened.
pub fn prefs_path(app_local_data_dir: &Path) -> PathBuf {
    app_local_data_dir.join("prefs.json")
}

/// Where the extraction worker binary is, for a real, running application.
///
/// One path serves both `cargo run` / `cargo tauri dev` and a packaged build,
/// because both join the same thing: the directory next to the running
/// executable. Under `cargo run` that directory is `target/<profile>/`;
/// inside a signed `.dmg` it is `Contents/MacOS/`. What makes the second case
/// true is `bundle.externalBin` in `src-tauri/tauri.conf.json`, which puts
/// `mnema-extract-worker` there at package time — this function did not need
/// to change to become correct for a bundle too. `scripts/stage-sidecar.sh`
/// is what builds and stages the file that declaration names, and
/// `scripts/verify-bundle.sh` is what keeps a stale or missing copy from
/// shipping. What a walk does when the worker still cannot be found at this
/// path is `job.rs`'s concern, not this function's.
pub fn worker_path() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(dir.join(format!(
        "mnema-extract-worker{}",
        std::env::consts::EXE_SUFFIX
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_sits_directly_in_the_local_data_directory() {
        let base = Path::new("/Users/x/Library/Application Support/com.mnema.desktop");
        assert_eq!(
            index_path(base),
            PathBuf::from("/Users/x/Library/Application Support/com.mnema.desktop/index.sqlite")
        );
    }

    #[test]
    fn the_prefs_file_sits_beside_the_index_under_that_exact_name() {
        // From PR 9 two subsystems write this file through `prefs.rs`, and the
        // backup it makes of a malformed one is named from this path. The name
        // is what they agree on, so it is pinned here rather than assumed at
        // each call site.
        let base = Path::new("/Users/x/Library/Application Support/com.mnema.desktop");
        assert_eq!(
            prefs_path(base),
            PathBuf::from("/Users/x/Library/Application Support/com.mnema.desktop/prefs.json")
        );
        assert_eq!(prefs_path(base).parent(), index_path(base).parent());
    }
}
