use std::path::{Path, PathBuf};

/// The index lives in the app's LOCAL data directory.
///
/// Not the roaming one — on Windows those are different places and a roaming
/// profile would try to sync a multi-hundred-megabyte index. Not the cache
/// directory either: macOS purges it under disk pressure. G7.0 §4.
pub fn index_path(app_local_data_dir: &Path) -> PathBuf {
    app_local_data_dir.join("index.sqlite")
}

/// Where the extraction worker binary is, for a real, running application.
///
/// **Provisional**, and known to be incomplete: nothing in this repository
/// packages a worker binary next to the application yet.
/// `src-tauri/tauri.conf.json` has no `bundle.externalBin` entry, and
/// `scripts/verify-bundle.sh` — which is otherwise careful to check every
/// dependency `mnema-desktop` links, including refusing a missing
/// `libpdfium.dylib` — says nothing about this binary at all, checked
/// directly rather than assumed. So this resolves to a path that exists in
/// one specific case, `cargo tauri dev` / `cargo run`, where the sibling
/// directory this joins happens to be `target/<profile>/`, the same place
/// `cargo build -p mnema-extract --bin mnema-extract-worker` puts it — and
/// resolves to a path that does not exist inside a signed `.dmg`, where the
/// executable sits at `Contents/MacOS/mnema-desktop` and nothing copies the
/// worker beside it. A folder added through a packaged build would start a
/// walk job whose `Pool` fails on its first `extract()` call — reported as
/// `EndReason::Failed`, not silently — rather than fail to start at all. This
/// is the smallest thing that lets the shell's own tests exercise a real
/// walk; sidecar packaging is an open question for whoever ships the first
/// build a user runs outside a development checkout.
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
}
