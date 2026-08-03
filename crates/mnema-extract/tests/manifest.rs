//! What the worker says its readers are, asked of the built binary rather than
//! of the library it links.
//!
//! The parent (`mnema-ingest`) may not depend on this crate (D40), so it cannot
//! read these constants at compile time — it runs the worker and parses this.
//! Testing the library function instead would prove the map is right and leave
//! the one thing the parent depends on, the `--manifest` branch of `main`,
//! untested.

use std::process::Command;

#[test]
fn the_worker_states_which_reader_takes_each_extension() {
    let out = Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .arg("--manifest")
        .output()
        .expect("the worker binary starts");
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON object");

    // md has a reader of its own since task 11 of G7.1.
    assert_eq!(
        v["by_extension"]["md"]["reader"],
        serde_json::json!("markdown")
    );
    // The default arm is stated, not a gap: identify_plain_text's `_ =>`
    // (crates/mnema-extract/src/typing.rs:342) is a real answer and the
    // manifest carries it as one.
    assert_eq!(v["default"]["reader"], serde_json::json!("text"));
    // Both directions: html is NOT yet its own reader, and the manifest must
    // say so rather than omit it. Task 10 flips this, and this assertion is
    // what makes that flip visible.
    assert!(v["by_extension"].get("html").is_none());
}

#[test]
fn a_header_names_the_reader_that_produced_it() {
    let out = Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            writeln!(
                c.stdin.as_mut().unwrap(),
                "{{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":1048576}}"
            )?;
            c.wait_with_output()
        })
        .expect("the worker runs");
    let first = String::from_utf8(out.stdout).unwrap();
    let first = first.lines().next().expect("a header").to_string();
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["reader"], serde_json::json!("text"));
    assert_eq!(v["reader_version"], serde_json::json!(1));
}

/// The manifest is a *claim about* `typing::identify`, and nothing in the type
/// system holds it to that claim: adding a reader and forgetting its entry
/// leaves a manifest that answers "text" for a file the worker now reads
/// differently, and every such file keeps its old indexing for ever. That is
/// the exact failure this whole mechanism exists to prevent, so it is asserted
/// rather than assumed.
///
/// Both directions, because either alone is satisfied by a mistake. A manifest
/// that listed every extension in `identify` would pass the first loop; one
/// with an empty map would pass the second.
#[test]
fn the_manifest_names_the_reader_that_identify_actually_picks() {
    // Plain text, so `identify` reaches the extension-deciding branch at all
    // rather than being answered by magic bytes.
    let bytes = b"just some prose\n";
    let manifest = mnema_extract::manifest::manifest();

    // Every extension the manifest claims for a named reader is one `identify`
    // really gives to that reader.
    for (ext, id) in &manifest.by_extension {
        let picked = mnema_extract::typing::identify(bytes, Some(ext));
        assert_eq!(
            reader_name(picked.reader),
            id.reader,
            "the manifest gives .{ext} to {}, identify gives it to {:?}",
            id.reader,
            picked.reader
        );
    }

    // And an extension the manifest does *not* claim really does fall to the
    // default reader — including no extension at all, and one that is only a
    // different case of a listed one. `identify_plain_text` matches exactly
    // (`src/typing.rs:336-343`), so `MD` is text; a manifest lookup that
    // lowercased would disagree with the worker about a real file.
    for ext in [
        None,
        Some("txt"),
        Some("rs"),
        Some("csv"),
        Some("html"),
        Some("MD"),
    ] {
        let picked = mnema_extract::typing::identify(bytes, ext);
        assert_eq!(
            reader_name(picked.reader),
            manifest.default.reader,
            "identify sends {ext:?} to {:?}, which the manifest does not list",
            picked.reader
        );
        assert_eq!(manifest.for_extension(ext), &manifest.default);
    }
}

/// The name a reader goes by on the wire. A `match` rather than a `Debug`
/// string: `Frame::Header::reader` is a stored value that outlives the
/// enum's spelling, and renaming a variant must not silently rewrite what
/// every indexed document claims produced it.
fn reader_name(reader: mnema_extract::typing::Reader) -> &'static str {
    use mnema_extract::typing::Reader;
    match reader {
        Reader::PlainText => "text",
        Reader::Markdown => "markdown",
        other => panic!("a reader with no name on the wire yet: {other:?}"),
    }
}
