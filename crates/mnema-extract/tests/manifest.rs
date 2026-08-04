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
    // **The entry this whole mechanism was built for.** Until task 10 this
    // asserted the opposite — that `html` was *absent*, because the text reader
    // took it — precisely so that the day a reader arrived would be visible
    // here rather than nowhere. This is that day, and flipping the assertion is
    // the event, not a weakening of it: every `.html` already in an index is
    // recorded `text@1`, and only this entry appearing makes the parent read
    // those files again.
    assert_eq!(
        v["by_extension"]["html"]["reader"],
        serde_json::json!("html")
    );
    assert_eq!(v["by_extension"]["html"]["version"], serde_json::json!(1));
    // Both spellings, and the second is not decoration: `identify_plain_text`
    // matches `Some("html") | Some("htm")`, so a map carrying only one predicts
    // the wrong reader for every `.htm` on disk.
    assert_eq!(
        v["by_extension"]["htm"]["reader"],
        serde_json::json!("html")
    );
    // And the other direction, which is what the old assertion was doing: an
    // extension no reader claims must still be absent, or the map stops being
    // a claim about anything.
    assert!(v["by_extension"].get("txt").is_none());
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

/// The markdown branch names itself too — and it needs its own test, because
/// the two branches in `handle_request` build their headers independently and
/// nothing makes them agree.
///
/// Measured before this existed: writing `reader: "text"` into the markdown
/// branch left `cargo test --workspace` at **478 passed, 0 failed**. Every
/// other place a header is read looks past the field — `worker_cli.rs`'s
/// markdown test destructures `Frame::Header { pages, mime, .. }`, and the
/// rest of the workspace uses synthetic headers. The two costs of that gap are
/// opposite and both silent: with the wrong name every `.md` mismatches the
/// manifest and is re-read on every run for ever, and after a
/// `MARKDOWN_READER_VERSION` bump under the wrong name no `.md` is re-read at
/// all. `.md` is the one extension the whole design is arranged around, so it
/// is the last one that should have been taken on trust.
///
/// This test and the one above are the two directions of the same claim: swap
/// the names between the branches and both go red, not one.
#[test]
fn a_markdown_header_names_the_markdown_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("звіт.md");
    std::fs::write(&path, "вступ\n\n# Розділ перший\n\nтекст\n").unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            writeln!(c.stdin.as_mut().unwrap(), "{request}")?;
            c.wait_with_output()
        })
        .expect("the worker runs");
    let first = String::from_utf8(out.stdout).unwrap();
    let first = first.lines().next().expect("a header").to_string();
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();

    assert_eq!(v["reader"], serde_json::json!("markdown"));
    assert_eq!(v["reader_version"], serde_json::json!(1));
    // Proof that the markdown branch is what ran, independent of the name it
    // reported: the text branch answers 1 page for anything (D37), and this
    // file has prose before its first heading and one heading after it.
    assert_eq!(v["pages"], serde_json::json!(2));
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
        Some("MD"),
        // The case rule again, on the extension that changed hands in task 10:
        // `identify_plain_text` matches `Some("html")` exactly, so `HTML` is
        // read as text and the map must agree rather than be helpful.
        Some("HTML"),
        // The trap on the other side of `pdf` being absent from the map: these
        // bytes are prose, so a file *named* `notes.pdf` is read by the text
        // reader. An entry `pdf → pdf@1` would predict otherwise and re-read
        // this file on every walk for ever.
        Some("pdf"),
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
        // The constant, not `"pdf"`. This is one of the three places the name
        // is written, and the only one on the reading side: `mnema-ingest`
        // matches the same constant to give a PDF chunk a page number, and no
        // compiler joins the two across D40.
        Reader::Pdf => mnema_core::manifest::READER_PDF,
        // The constant again, and this is the third of the three places the
        // html name is written: the header the worker sends, the manifest, and
        // here. A literal `"html"` in this arm would leave the constant with
        // two users out of three and remove the cross-check it exists for —
        // `mnema-ingest` matches the same constant to cite an HTML chunk by its
        // section, and no compiler joins the two across D40.
        Reader::Html => mnema_core::manifest::READER_HTML,
        other => panic!("a reader with no name on the wire yet: {other:?}"),
    }
}

/// **A PDF is decided by content, so it is absent from the manifest — and that
/// absence has a bill.**
///
/// The three facts are asserted together because separately each looks fine.
/// `identify` gives real PDF bytes to the pdf reader whatever the file is
/// called; the manifest predicts by extension and has no `pdf` entry, so it
/// predicts the *text* reader for `report.pdf`; and the parent's cheap arm
/// compares the two (`crates/mnema-ingest/src/lib.rs:274-280`). Every real PDF
/// therefore misses that arm on every walk and is handed to a worker again —
/// which for this format is a full pdfium parse, serialised process-wide,
/// rather than a text read.
///
/// It is not a defect to fix here. An entry `pdf → pdf@1` would be a false
/// claim about `identify` — the loop above proves prose named `notes.pdf` is
/// read as text — and would cost that file the same re-read in the other
/// direction. What closes it is a stored prediction per path or a manifest that
/// can say "chosen by content", both of them decisions of their own. This test
/// exists so the cost is written down where someone changing the map will meet
/// it, rather than measured a third time.
#[test]
fn a_pdf_is_read_by_content_so_the_manifest_predicts_the_wrong_reader_for_it() {
    let pdf = std::fs::read("tests/fixtures/one-page-text.pdf").expect("the fixture is on disk");
    let manifest = mnema_extract::manifest::manifest();

    // Under its own name, and under a name that lies about it: content decides,
    // so both are the pdf reader.
    for ext in [Some("pdf"), Some("md"), None] {
        assert_eq!(
            reader_name(mnema_extract::typing::identify(&pdf, ext).reader),
            mnema_core::manifest::READER_PDF,
            "{ext:?} named a PDF's bytes something other than the pdf reader"
        );
    }

    // And the manifest predicts none of that, because it cannot.
    assert!(!manifest.by_extension.contains_key("pdf"));
    assert_eq!(manifest.for_extension(Some("pdf")), &manifest.default);
    assert_ne!(
        manifest.for_extension(Some("pdf")).reader,
        mnema_core::manifest::READER_PDF,
        "if this ever agrees, the arm above stopped costing a re-read — say so in the ledger"
    );
}
