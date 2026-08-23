//! Guard: no Cyrillic user-facing string literals outside `locale.rs`.
//! Cyrillic-only by design — Latin literals are ubiquitous in Rust (identifiers,
//! `#[error("…")]`), so a Latin check is impractical; EN completeness is covered
//! by the canonical `Key` set + the completeness test, not this guard. (Spec §7
//! said "Cyrillic/Latin"; amend §7 to "Cyrillic-only, with reason" to match.)
use std::fs;

#[test]
// Green once Tasks 5-6 move the tray/menu strings into locale.rs; Task 6 lifts
// this attribute. Left in the normal run as ignored (not deleted) so the RED
// state stays visible in `cargo test` output rather than silently missing.
#[ignore = "green after Tasks 5-6 move tray/menu strings into locale.rs"]
fn no_cyrillic_string_literals_outside_locale_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().unwrap() == "locale.rs" {
            continue; // the one place translatable strings are allowed to live
        }
        let src = fs::read_to_string(&path).unwrap();
        for (i, line) in src.lines().enumerate() {
            // Test modules sit at the bottom; their Cyrillic asserts (e.g. the
            // bridge.rs `Coordinate::render` fixtures at :807/:822, which are
            // prompt-only and out of scope) are NOT product strings. Stop at the
            // first `#[cfg(test)]`.
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let code = line.split("//").next().unwrap_or(""); // ignore comments
            if code.contains('"') && code.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
                offenders.push(format!("{}:{}", path.display(), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Cyrillic string literals outside locale.rs: {offenders:#?}"
    );
}
