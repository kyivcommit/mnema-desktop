//! A coupling the compiler cannot see.
//!
//! `bridge::start_probe_job` uses `catch_unwind` to guarantee that a job which
//! panics still sends its terminal message. `catch_unwind` catches nothing when
//! the profile aborts on panic, and no test can observe that directly: the test
//! profile always unwinds, so every test in this workspace stays green while the
//! release binary loses the guarantee.
//!
//! Reading the manifest is the only place the two can be compared.

use std::path::Path;

/// Whether a section body declares exactly this directive.
///
/// Whole lines with any trailing comment removed — NOT a substring search over
/// the raw text. A substring search is satisfied by prose, and this particular
/// section is prose: it quotes `panic = "abort"` twice while declaring the
/// opposite. So "the text appears somewhere in the section" and "the directive
/// is set" are different questions there, and an edit reading
/// `# this used to be panic = "unwind"` above an `abort` would answer the first
/// one yes. The same defect was closed in a README check in task 6, the same
/// way.
fn declares(section: &str, directive: &str) -> bool {
    section
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .any(|line| line == directive)
}

/// The body of a `[section]` in a TOML file, as text: everything up to the next
/// line that starts a new section.
fn section<'a>(manifest: &'a str, header: &str) -> Option<&'a str> {
    let start = manifest.find(header)? + header.len();
    let rest = &manifest[start..];
    let end = rest
        .match_indices('[')
        .find(|(i, _)| rest[..*i].ends_with('\n'))
        .map_or(rest.len(), |(i, _)| i);
    Some(&rest[..end])
}

#[test]
fn the_release_profile_unwinds_because_the_shell_catches_unwinds() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has no parent")
        .join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", manifest_path.display()));

    let profile = section(&manifest, "[profile.release]").expect(
        "the workspace manifest has no [profile.release] section. It needs one \
         declaring `panic = \"unwind\"`: bridge.rs relies on catch_unwind, and \
         with no section here nothing stops a later edit from adding \
         `panic = \"abort\"` under it",
    );

    assert!(
        declares(profile, "panic = \"unwind\""),
        "[profile.release] does not declare `panic = \"unwind\"`. If this was \
         changed to \"abort\" to shrink the bundle, note what it costs: \
         bridge.rs catches the unwind of a panicking job in order to send its \
         terminal message, and aborting skips that — the window keeps a disabled \
         Start button for the rest of its life, with no way to reach the state \
         it needs. The section reads:\n{profile}"
    );
}
