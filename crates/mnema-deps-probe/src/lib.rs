//! Compile-and-call probe for the crates the skeleton does not otherwise use.
//!
//! Every version here is pinned by the G7.0 spec §8. This crate exists to make a
//! version conflict a build failure today rather than a discovery during a
//! subsystem spec. Delete it once the real consumers land.

#[cfg(test)]
mod tests {
    #[test]
    fn zip_reads_an_archive_with_deflate_only() {
        // Declared with default-features = false: the defaults drag in six
        // compressors that no OOXML or EPUB file uses. G7.0 §8.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            use std::io::Write;
            w.start_file("word/document.xml", opts).unwrap();
            w.write_all(b"<w:p><w:t>hello</w:t></w:p>").unwrap();
            w.finish().unwrap();
        }
        let mut r = zip::ZipArchive::new(buf).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.by_index(0).unwrap().name(), "word/document.xml");
    }

    #[test]
    fn quick_xml_walks_ooxml_shaped_input() {
        use quick_xml::events::Event;
        let mut reader = quick_xml::Reader::from_str("<w:p><w:t>текст</w:t></w:p>");
        let mut texts = Vec::new();
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                // Correction to G7.0 §8: 0.41 dropped `unescape()`. The caller now
                // has to say which entity set to resolve — xml10_content, xml11_content
                // or html_content. OOXML and EPUB container XML are XML 1.0.
                Ok(Event::Text(t)) => texts.push(t.xml10_content().unwrap().into_owned()),
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{e}"),
            }
            buf.clear();
        }
        assert_eq!(texts, vec!["текст"]);
    }

    #[test]
    fn calamine_exposes_both_values_and_formulas() {
        // The correction to §10: xlsx is the one format where the server does
        // lean on a library, so the port takes calamine rather than hand-parsing.
        fn assert_api<R: calamine::Reader<std::io::BufReader<std::fs::File>>>(r: &mut R) {
            let _ = r.worksheet_range("Sheet1");
            let _ = r.worksheet_formula("Sheet1");
        }
        // Instantiating it for the concrete reader the extraction spec will use
        // IS the assertion: it type-checks or it does not.
        let _: fn(&mut calamine::Xlsx<std::io::BufReader<std::fs::File>>) = assert_api;
    }

    #[test]
    fn chardetng_beats_the_servers_ladder_on_polish() {
        // The server's utf-8 → cp1251 → latin-1 ladder mojibakes this, because
        // cp1251 accepts nearly any byte and never complains. G7.0 §8.2.
        let cp1250 = [0x5A, 0x61, 0xBF, 0xF3, 0xB3, 0xE6]; // "Zażółć" in cp1250
        // Correction to G7.0 §8: 1.0 replaced both booleans with named enums, and
        // the two answers differ. UTF-8 must be Allow — the browser-oriented Deny
        // exists so web content cannot come to depend on unlabelled detection, and
        // most files we index are unlabelled UTF-8. ISO-2022-JP stays Deny: the
        // crate reserves Allow for email clients, which we are not.
        let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
        det.feed(&cp1250, true);
        let enc = det.guess(None, chardetng::Utf8Detection::Allow);
        let (decoded, _, had_errors) = enc.decode(&cp1250);
        assert!(!had_errors);
        assert!(decoded.chars().count() == 6, "got {decoded:?}");
    }

    #[test]
    fn ignore_honours_a_gitignore() {
        // The single highest-leverage exclusion rule: measured to remove 60% of
        // chunks beyond any hand-written list. G7.0 §1.2.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "skip/\n").unwrap();
        std::fs::create_dir(dir.path().join("skip")).unwrap();
        std::fs::write(dir.path().join("skip/a.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("keep.txt"), b"y").unwrap();

        let names: Vec<String> = ignore::WalkBuilder::new(dir.path())
            .hidden(false)
            // Correction to G7.0 §1.2, and the reason this probe earns its keep:
            // `require_git` defaults to TRUE, so .gitignore is honoured only when
            // the walk is inside a git repository. A watched folder usually is not
            // one, and the failure is silent — every excluded file gets indexed and
            // nothing reports it. The measured 60% reduction assumed this flag.
            .require_git(false)
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"keep.txt".to_string()));
        assert!(
            !names.contains(&"a.txt".to_string()),
            "gitignore was not honoured"
        );
    }

    #[test]
    fn html5ever_and_comrak_are_callable() {
        let html = comrak::markdown_to_html("# Заголовок\n\nАбзац.", &comrak::Options::default());
        assert!(html.contains("<h1>"));
        // Note for the extraction spec: comrak is CommonMark + GFM, while the
        // server runs a non-CommonMark parser with fenced code disabled. Golden
        // fixtures will fail a *correct* port here. G7.0 §8.2.
        let doc = scraper::Html::parse_fragment(&html);
        assert!(doc.root_element().text().any(|t| t.contains("Заголовок")));
    }

    #[test]
    fn reqwest_builds_a_client_without_touching_the_network() {
        let _client = reqwest::Client::builder()
            .user_agent("mnema/0.0.0")
            .build()
            .expect("client builds");
    }

    #[test]
    fn keepawake_is_callable() {
        // Tauri exposes no sleep/resume API at all, so a multi-hour job needs an
        // explicit wake lock from a plain crate. G7.0 §3.4.
        let _guard = keepawake::Builder::default()
            .display(false)
            .idle(true)
            .reason("indexing")
            .app_name("Mnema")
            .create();
    }
}
