//! Guard: no Cyrillic user-facing string literals outside `locale.rs`.
//! Cyrillic-only by design — Latin literals are ubiquitous in Rust (identifiers,
//! `#[error("…")]`), so a Latin check is impractical; EN completeness is covered
//! by the canonical `Key` set + the completeness test, not this guard. (Spec §7
//! said "Cyrillic/Latin"; amend §7 to "Cyrillic-only, with reason" to match.)
use std::fs;
use std::path::{Path, PathBuf};

/// Collects every `.rs` file under `dir` RECURSIVELY, except any named
/// `locale.rs` — the one module translatable strings are allowed to live in.
/// Recursive (not a flat `read_dir`) so a future `src/<subdir>/*.rs` submodule
/// cannot slip a Cyrillic literal past the guard (gap G4). Today `src/` has no
/// subdirectories, so it visits exactly the top-level files it did before —
/// this is defensive, and it is what keeps the now-live gate honest.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && path.file_name().and_then(|n| n.to_str()) != Some("locale.rs")
        {
            out.push(path);
        }
    }
}

/// Every `file:line` in `path` whose code (comments stripped) holds both a `"`
/// and a Cyrillic codepoint. Stops at the first `#[cfg(test)]`: test modules sit
/// at the bottom and their Cyrillic asserts (e.g. the `bridge.rs`
/// `Coordinate::render` fixtures at :807/:822, prompt-only and out of scope) are
/// NOT product strings.
fn cyrillic_offenders(path: &Path) -> Vec<String> {
    let src = fs::read_to_string(path).unwrap();
    let mut offenders = Vec::new();
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or(""); // ignore comments
        if code.contains('"') && code.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
            offenders.push(format!("{}:{}", path.display(), i + 1));
        }
    }
    offenders
}

// Live from Task 6: Tasks 5-6 moved the tray/menu strings into `locale.rs`, so
// this now passes in the normal suite and runs on every `cargo test` (no longer
// `#[ignore]`d). It walks `src/` recursively so a future submodule cannot bypass
// it (gap G4); the recursion itself is proven by the self-check below.
#[test]
fn no_cyrillic_string_literals_outside_locale_module() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&dir, &mut files);
    let mut offenders = Vec::new();
    for path in &files {
        offenders.extend(cyrillic_offenders(path));
    }
    assert!(
        offenders.is_empty(),
        "Cyrillic string literals outside locale.rs: {offenders:#?}"
    );
}

// Proves the recursion the real guard depends on: a Cyrillic literal in a
// NESTED file must be caught, a `locale.rs` at any depth must be exempt, and a
// clean top-level file must not be flagged. Were `rs_files` a flat `read_dir`,
// it would never descend into `sub/` and this would find zero offenders — so
// this test goes red exactly when the G4 fix is lost.
#[test]
fn guard_recursion_catches_a_nested_offender_and_exempts_locale() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("nested.rs"), "let s = \"Привіт\";\n").unwrap();
    std::fs::write(sub.join("locale.rs"), "let s = \"Дозволено\";\n").unwrap();
    std::fs::write(root.path().join("clean.rs"), "let s = \"hello\";\n").unwrap();

    let mut files = Vec::new();
    rs_files(root.path(), &mut files);

    // `locale.rs` is skipped wherever it lives, at any depth.
    assert!(
        !files
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("locale.rs")),
        "locale.rs must be exempt at any depth: {files:#?}"
    );

    let mut offenders = Vec::new();
    for path in &files {
        offenders.extend(cyrillic_offenders(path));
    }
    // Only the nested offender — the clean top-level file has no Cyrillic, and
    // the nested `locale.rs` was never collected.
    assert_eq!(
        offenders.len(),
        1,
        "expected exactly the nested offender: {offenders:#?}"
    );
    assert!(offenders[0].contains("nested.rs"), "{offenders:#?}");
}
