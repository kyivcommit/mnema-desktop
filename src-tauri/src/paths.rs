use std::path::{Path, PathBuf};

/// The index lives in the app's LOCAL data directory.
///
/// Not the roaming one — on Windows those are different places and a roaming
/// profile would try to sync a multi-hundred-megabyte index. Not the cache
/// directory either: macOS purges it under disk pressure. G7.0 §4.
pub fn index_path(app_local_data_dir: &Path) -> PathBuf {
    app_local_data_dir.join("index.sqlite")
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
