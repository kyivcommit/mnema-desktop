use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, prepare_for_search, register_vector_extension};

/// A finished document holding one chunk per text.
///
/// **Finished**, and the last line is what makes it so. `insert_document`
/// leaves the row at `pending` — a document mid-write — and under D61 a search
/// does not answer with one of those. Every test in this file is about the
/// tokenizer rather than about visibility, so the fixture says what `ingest_
/// file`'s step 5 says and gets out of the way; `tests/visibility.rs` is where
/// the status itself is the subject.
fn db_with(texts: &[(&str, SourceKind)]) -> (tempfile::TempDir, Db, Vec<i64>) {
    register_vector_extension().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("index.sqlite")).unwrap();
    let doc = db
        .insert_document(&"d".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let mut ids = Vec::new();
    for (i, (t, kind)) in texts.iter().enumerate() {
        let block = db
            .insert_block(
                page,
                &Block {
                    block_type: BlockType::Paragraph,
                    reading_order: i as i64,
                    language: None,
                    text: (*t).to_string(),
                    line_start: None,
                    line_end: None,
                },
            )
            .unwrap();
        let id = db
            .insert_chunk(
                &doc,
                i as i64,
                t,
                &Locator {
                    spans: vec![Segment {
                        block_id: block,
                        start: 0,
                        end: t.chars().count() as u32,
                        block_start: 0,
                    }],
                    coordinate: Coordinate::None,
                },
                *kind,
            )
            .unwrap();
        ids.push(id);
    }
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .unwrap();
    (dir, db, ids)
}

/// D32's correction, and the test that would have caught the defect it names:
/// a precomposed `й` (U+0439) and its decomposed spelling `и` (U+0438) +
/// combining breve (U+0306) are two different tokens without NFC. macOS hands
/// over the decomposed form; a query typed on another machine is precomposed —
/// so without normalisation the document is unfindable by its own spelling, in
/// either direction.
///
/// Not because `remove_diacritics 2` spares the breve. Measured in
/// `search_terms_matches_what_fts5_stores_for_every_mark`, a standalone U+0306
/// is deleted whatever precedes it; what the tokenizer's table does not carry
/// is the *precomposed* U+0439. So the decomposed spelling tokenizes as plain
/// `и` and the precomposed one as `й` — still two tokens, by the opposite
/// mechanism to the one this comment used to give.
#[test]
fn both_forms_of_the_same_ukrainian_word_produce_one_token() {
    let precomposed = "йод"; // й = U+0439
    let decomposed = "\u{0438}\u{0306}од"; // и + combining breve
    assert_ne!(
        precomposed, decomposed,
        "the fixture itself must differ, or the test proves nothing"
    );

    let (_d, db, ids) = db_with(&[(precomposed, SourceKind::Document)]);
    assert_eq!(
        db.search_lexical(decomposed, 10).unwrap(),
        vec![ids[0]],
        "macOS delivers the decomposed form; a query typed elsewhere is precomposed"
    );
    assert_eq!(db.search_lexical(precomposed, 10).unwrap(), vec![ids[0]]);
}

#[test]
fn ukrainian_apostrophe_variants_all_match_one_query() {
    // U+0027, U+2019 and U+02BC are three different characters people actually
    // type. Without normalisation the same word indexes three ways. G7.0 §5.4.
    let (_d, db, ids) = db_with(&[
        ("п'ять договорів", SourceKind::Document),
        ("п’ять актів", SourceKind::Document),
        ("пʼять рахунків", SourceKind::Document),
    ]);
    let hits = db.search_lexical("п'ять", 10).unwrap();
    assert_eq!(hits.len(), 3, "all three apostrophe forms must match");
    for id in ids {
        assert!(hits.contains(&id));
    }
}

/// Companion to the test above, and the one that carries the canonical form.
///
/// The test above cannot see a tokenizer that shatters the word, because the
/// query shatters by the same rule: `п'ять` would index as `п` + `ять`, the
/// query would become the adjacent phrase `п ять`, and all three rows would come
/// back exactly as they do now. Any weakening symmetric across document and
/// query is invisible to a lookup of a word by itself.
///
/// A fragment breaks the symmetry, because a fragment is a token only in the
/// shattered index. What it defends is that the canonical apostrophe is a
/// character which stays inside a token — U+02BC is a letter (Lm) and does, and
/// the fixture holds all three spellings people type.
#[test]
fn an_apostrophe_does_not_split_the_word_into_separately_findable_parts() {
    let (_d, db, _) = db_with(&[
        ("п'ять договорів", SourceKind::Document),
        ("п’ять актів", SourceKind::Document),
        ("пʼять рахунків", SourceKind::Document),
    ]);
    // The positive control. Without it every assertion here is satisfied by a
    // search that has stopped returning anything at all.
    assert_eq!(db.search_lexical("п'ять", 10).unwrap().len(), 3);
    assert_eq!(db.search_lexical("ять", 10).unwrap().len(), 0);
    assert_eq!(db.search_lexical("п", 10).unwrap().len(), 0);
}

/// The apostrophe stays in the token wherever it sits, so at a word's edge it
/// sticks there and the word stops answering to its own spelling: `‘hello’`
/// indexed as `hello'`, and the query `hello` found nothing. Quoted speech,
/// possessives and a backtick-quoted identifier are not exotic input, and every
/// apostrophe in the tests above happens to be word-internal and Ukrainian.
///
/// So an apostrophe survives preparation only between two word characters, and
/// anywhere else becomes a separator — see the test below for why a separator
/// and not a deletion.
#[test]
fn an_apostrophe_at_a_word_edge_does_not_hide_the_word() {
    let (_d, db, _) = db_with(&[
        ("He said ‘hello’ today", SourceKind::Document),
        ("the 'quoted' word", SourceKind::Document),
        ("students’ books", SourceKind::Document),
        ("js: `template` here", SourceKind::Code),
        ("don't stop", SourceKind::Document),
    ]);
    assert_eq!(db.search_lexical("hello", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("quoted", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("students", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("template", 10).unwrap().len(), 1);
    // And the case the apostrophe is a token character FOR still works.
    assert_eq!(db.search_lexical("don't", 10).unwrap().len(), 1);
}

/// An apostrophe binds two letters into one word; a run of them binds nothing.
///
/// Deleting a non-word-internal apostrophe closed the gap it left, so
/// `hello''world` became the single token `helloworld`, findable by neither
/// half. Replacing it with a separator instead leaves the two words two words,
/// and leaves the word-internal case exactly as it was.
#[test]
fn a_run_of_apostrophes_separates_words_rather_than_merging_them() {
    let (_d, db, ids) = db_with(&[
        ("hello''world", SourceKind::Document),
        ("'don''t'", SourceKind::Document),
        ("п'ять договорів", SourceKind::Document),
        ("don't stop", SourceKind::Document),
    ]);
    assert_eq!(db.search_lexical("hello", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("world", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("helloworld", 10).unwrap().len(), 0);
    assert_eq!(db.search_lexical("don", 10).unwrap(), vec![ids[1]]);
    assert_eq!(db.search_lexical("dont", 10).unwrap().len(), 0);
    // Untouched: an apostrophe between two letters still makes one word.
    assert_eq!(db.search_lexical("п'ять", 10).unwrap(), vec![ids[2]]);
    assert_eq!(db.search_lexical("don't", 10).unwrap(), vec![ids[3]]);
}

/// U+02B9 MODIFIER LETTER PRIME is an apostrophe substitute in transliteration,
/// and the fold that unifies it had no fixture — removing it reddened nothing.
#[test]
fn modifier_letter_prime_folds_to_the_canonical_apostrophe() {
    let (_d, db, _) = db_with(&[("пʹять договорів", SourceKind::Document)]);
    assert_eq!(db.search_lexical("пʼять", 10).unwrap().len(), 1);
}

/// A backtick is a quote character, not an apostrophe variant, and folding it
/// bound `` `template` `` into a token nobody could ask for by name.
///
/// The test above cannot see this one: dropping the fold and trimming word-edge
/// apostrophes each rescue `` `template` `` on their own. A backtick BETWEEN two
/// word characters is what tells them apart — folded, it binds the two into one
/// token and neither half answers to its own name.
#[test]
fn a_backtick_is_not_an_apostrophe_variant() {
    assert_eq!(prepare_for_search("a`b", SourceKind::Code), "a`b");
}

#[test]
fn ghe_with_upturn_folds_to_ghe() {
    let (_d, db, _) = db_with(&[("ґрунтовий аналіз", SourceKind::Document)]);
    assert_eq!(db.search_lexical("грунтовий", 10).unwrap().len(), 1);
}

/// The other fold in `prepare_for_search`, which had no test at all. Nothing in
/// the tokenizer does it: `remove_diacritics 2` leaves Cyrillic alone entirely,
/// so without this fold `ёлка` and `елка` are two different words.
#[test]
fn yo_folds_to_ye() {
    let (_d, db, _) = db_with(&[("ёлка зелена", SourceKind::Document)]);
    assert_eq!(db.search_lexical("елка", 10).unwrap().len(), 1);
}

#[test]
fn yi_and_i_stay_distinct() {
    // The opposite of the fold above, and deliberate: ї and і are different
    // letters. Folding them buys typo tolerance at the cost of precision. D32.
    let (_d, db, _) = db_with(&[("їхній дім", SourceKind::Document)]);
    assert_eq!(db.search_lexical("іхній", 10).unwrap().len(), 0);
    // The positive control: zero hits must mean the fold is absent, not that
    // the search returned nothing to anybody.
    assert_eq!(db.search_lexical("їхній", 10).unwrap().len(), 1);
}

/// A positive control, and only that. It is worth being exact about what it does
/// NOT do, because its first comment claimed the opposite: this assertion stays
/// green under `categories 'L* N* Co'`, under no `categories` clause at all,
/// under plain `unicode61` and under `ascii`. A pointed word can always be
/// looked up by its own spelling, because whatever shatters the document
/// shatters the query into the same adjacent phrase. What it does prove is that
/// pointed text reaches the index and comes back — which the two tests below
/// build on.
#[test]
fn pointed_yiddish_survives_tokenisation() {
    let (_d, db, _) = db_with(&[("ייִדיש איז אַ שפּראַך", SourceKind::Document)]);
    assert_eq!(db.search_lexical("ייִדיש", 10).unwrap().len(), 1);
}

/// What defends `Mn`. Dropping it really does shatter the word — the four tokens
/// become seven, `ייִדיש` splitting into `יי` and `דיש` — and a fragment is the
/// assertion that can see it, because the fragment is a token only in the
/// shattered index.
#[test]
fn niqqud_do_not_split_the_word_into_separately_findable_parts() {
    let (_d, db, _) = db_with(&[("ייִדיש איז אַ שפּראַך", SourceKind::Document)]);
    assert_eq!(db.search_lexical("דיש", 10).unwrap().len(), 0);
    assert_eq!(db.search_lexical("ייִדיש", 10).unwrap().len(), 1);
}

/// What defends `Mc`, the other half of the category list and the one no test
/// touched. Devanagari carries its vowels as spacing marks, so dropping `Mc`
/// reduces `हिन्दी भाषा` to the bare consonants `ह`, `न्द`, `भ`, `ष` — a far worse
/// shattering than the niqqud one, and equally invisible to a lookup of the word
/// by itself.
#[test]
fn spacing_marks_do_not_split_the_word_into_separately_findable_parts() {
    let (_d, db, _) = db_with(&[("हिन्दी भाषा", SourceKind::Document)]);
    assert_eq!(db.search_lexical("भा", 10).unwrap().len(), 0);
    assert_eq!(db.search_lexical("भाषा", 10).unwrap().len(), 1);
    // The word this test was written about. It was never asserted, and it is the
    // one that carries a virama — so the query side broke on it while `भाषा`,
    // which has none, went on passing.
    assert_eq!(db.search_lexical("हिन्दी", 10).unwrap().len(), 1);
}

/// What defends `N*`, and with it every numeric query the product exists to
/// answer. Without it the digits are separators: «витрати за 2024 рік» indexes
/// as `витрати`, `за`, `рік` and the year is not in the database at all.
#[test]
fn numbers_are_indexed_and_searchable() {
    let (_d, db, ids) = db_with(&[
        (
            "витрати за 2024 рік: квартал 4, склали 15 млн",
            SourceKind::Document,
        ),
        ("витрати за 2023 рік", SourceKind::Document),
    ]);
    assert_eq!(db.search_lexical("2024", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("15", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("2023", 10).unwrap(), vec![ids[1]]);
}

/// Renamed from `snake_case_is_findable_whole_and_in_part`, which asserted the
/// "whole" half without ever checking it: with a single document in the fixture,
/// one hit is satisfied by anything holding `get`, `user` and `name` in any
/// arrangement at all. The second document here has no `get_user_name` in it and
/// comes back for that query, which is the honest behaviour — quotes are not
/// phrase syntax on this route, they are separators, and there is no way to ask
/// for adjacency through this function. Whether the product offers one is the
/// search/RAG spec's decision, still open, and not to be invented here.
#[test]
fn an_identifier_is_findable_by_its_parts() {
    let (_d, db, ids) = db_with(&[
        ("fn get_user_name(id) -> Option<String>", SourceKind::Code),
        ("fn name(user) { get(); }", SourceKind::Code),
        ("fn unrelated(x) -> bool", SourceKind::Code),
    ]);
    for part in ["get", "user", "name"] {
        assert!(
            db.search_lexical(part, 10).unwrap().contains(&ids[0]),
            "part {part:?} must reach the identifier that contains it"
        );
    }
    // Two, not one: the third document has none of the three parts, so this is
    // not "everything matches" — it is adjacency going unasked for.
    assert_eq!(db.search_lexical("\"get_user_name\"", 10).unwrap().len(), 2);
    assert!(!db.search_lexical("get", 10).unwrap().contains(&ids[2]));
}

#[test]
fn camel_case_is_findable_by_its_parts() {
    // The one genuine identifier gap: unicode61 keeps getUserName as a single
    // token. Closed by expanding code text at index time, not by the tokenizer.
    let (_d, db, _) = db_with(&[("const parseHttpResponse = () => {}", SourceKind::Code)]);
    assert_eq!(db.search_lexical("http", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("parseHttpResponse", 10).unwrap().len(), 1);
}

/// Splitting at every capital destroyed the acronyms that identifiers are full
/// of: `HTTPServer` became `H T T P Server`, which loses `http` as a term
/// entirely and puts single letters into the index, so `h` started matching code
/// chunks. A capital opens a new part when a lower-case letter precedes it, or
/// when it ends a run of capitals and a lower-case letter follows.
#[test]
fn acronyms_survive_camel_case_expansion() {
    let (_d, db, _) = db_with(&[("class HTTPServer implements IOSHandler", SourceKind::Code)]);
    assert_eq!(db.search_lexical("http", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("ios", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("server", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("handler", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("h", 10).unwrap().len(), 0);
}

/// `expand_camel_case` split identifiers on `char::is_alphanumeric` — the last
/// of the approximation `is_term_char` replaced, surviving in its neighbour and
/// disagreeing with it about where a word ends.
///
/// A decomposed accent inside an identifier is where the two answers differ. As
/// a separator it cuts `getKönigName` into `getKo` and `nigName`, and the
/// expansion then offers `ko` and `nig` as searchable parts instead of `könig`.
#[test]
fn an_identifier_splits_on_the_same_rule_as_the_index() {
    let (_d, db, _) = db_with(&[("fn getKo\u{308}nigName()", SourceKind::Code)]);
    assert_eq!(db.search_lexical("konig", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("name", 10).unwrap().len(), 1);
}

/// `дБ` is two characters and four bytes. The guard that keeps short words out
/// of expansion measured `word.len()`, which is the byte length, so for every
/// script outside ASCII the threshold was really one or two characters: `дБ`
/// cleared it and expanded into `д Б`, putting single letters into the index.
#[test]
fn a_short_word_is_short_in_characters_not_in_bytes() {
    let (_d, db, _) = db_with(&[("const дБ = 10.0", SourceKind::Code)]);
    assert_eq!(db.search_lexical("д", 10).unwrap().len(), 0);
    assert_eq!(db.search_lexical("дБ", 10).unwrap().len(), 1);
}

#[test]
fn camel_case_is_not_expanded_in_prose() {
    // Expansion is for code only: doing it to prose would pollute the vocabulary.
    let out = prepare_for_search("Компанія OpenRouter підписала", SourceKind::Document);
    assert!(!out.contains("Open Router"));
}

#[test]
fn latin_diacritics_still_fold() {
    let (_d, db, _) = db_with(&[("Zażółć café König", SourceKind::Document)]);
    assert_eq!(db.search_lexical("cafe", 10).unwrap().len(), 1);
}

/// What defends the digit in `remove_diacritics 2`. The fixture above folds
/// identically under `1`, so it cannot tell the two apart; Vietnamese can,
/// because `1` leaves the marks that `2` removes and `tieng` then finds nothing.
#[test]
fn vietnamese_diacritics_fold_where_remove_diacritics_1_would_not() {
    let (_d, db, _) = db_with(&[("Tiếng Việt", SourceKind::Document)]);
    assert_eq!(db.search_lexical("tieng", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("viet", 10).unwrap().len(), 1);
}

/// What defends the query side splitting exactly where the tokenizer splits.
///
/// `terms` decides where a query word ends; `categories 'L* N* Co Mn Mc'` decides
/// where an indexed word ends. Approximating the second with `char::is_alphanumeric`
/// looked close enough and is not: `is_alphanumeric` is Alphabetic plus Numeric,
/// which leaves out every mark outside Other_Alphabetic — every Indic virama, every
/// NFD combining accent — and the whole of `Co`. Wherever the two disagree the query
/// splits where the document did not, the phrase it builds is not a token anybody
/// indexed, and the word becomes unreachable from its own spelling while sitting
/// whole in the table.
///
/// Decomposed input is not hypothetical: macOS hands back NFD text. This test
/// passes on category splitting alone — the combining marks it fixtures are
/// `Mn`, so the rule keeps them attached without composing anything — and is
/// not the test for NFC itself; see
/// `both_forms_of_the_same_ukrainian_word_produce_one_token` above for that,
/// and D32 for why NFC is owed on both sides of the pipeline rather than at
/// extraction alone.
#[test]
fn a_query_splits_where_the_tokenizer_splits_and_nowhere_else() {
    let (_d, db, ids) = db_with(&[
        ("हिन्दी भाषा", SourceKind::Document),   // Devanagari virama, Mn
        ("শব্দ", SourceKind::Document),          // Bengali virama
        ("ಕನ್ನಡ", SourceKind::Document),         // Kannada virama
        ("Ko\u{308}nig", SourceKind::Document), // NFD: o + combining diaeresis
        ("nai\u{308}ve", SourceKind::Document), // NFD
        ("logo \u{F8FF} mark", SourceKind::Document), // private use, Co
    ]);
    assert_eq!(db.search_lexical("हिन्दी", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("শব্দ", 10).unwrap(), vec![ids[1]]);
    assert_eq!(db.search_lexical("ಕನ್ನಡ", 10).unwrap(), vec![ids[2]]);
    assert_eq!(db.search_lexical("Ko\u{308}nig", 10).unwrap(), vec![ids[3]]);
    assert_eq!(db.search_lexical("nai\u{308}ve", 10).unwrap(), vec![ids[4]]);
    // `Co` is reachable from the query only once the rule is the real category
    // list, which is what makes this one line rather than a fixture of its own.
    assert_eq!(db.search_lexical("\u{F8FF}", 10).unwrap(), vec![ids[5]]);
    assert_eq!(db.search_lexical("logo", 10).unwrap(), vec![ids[5]]);
}

/// Terms are split the way the indexed text is, so punctuation between two words
/// separates them instead of binding them. Splitting the query on whitespace
/// alone made `витрати(2024)` a single term, quoted into a phrase that demanded
/// the two be adjacent, and it matched nothing — while the same query with a
/// space matched. Punctuation between words is how people type.
#[test]
fn punctuation_between_query_words_leaves_a_conjunction_not_a_phrase() {
    let (_d, db, ids) = db_with(&[
        (
            "витрати за 2024 рік: квартал 4, склали 15 млн",
            SourceKind::Document,
        ),
        ("витрати за 2023 рік", SourceKind::Document),
    ]);
    for query in [
        "витрати (2024)",
        "витрати(2024)",
        "витрати-2024",
        "квартал/2024",
        "витрати,2024",
    ] {
        assert_eq!(
            db.search_lexical(query, 10).unwrap(),
            vec![ids[0]],
            "query {query:?}"
        );
    }
    // And the conjunction is a real conjunction: the second document has
    // `витрати` but not the year, and must not come back for a query naming both.
    assert_eq!(db.search_lexical("витрати 2023", 10).unwrap(), vec![ids[1]]);
}

/// `index_chunk_text` is public and indexes whatever it is handed, prepared or
/// not. That is what removing `tokenchars` from the tokenizer string buys: the
/// clause was inert for text that went through `prepare_for_search`, since every
/// apostrophe variant it named had already been folded away — and a trap for
/// text that did not, because raw `students’ books` indexed as `students’` and
/// the plain word `students` returned nothing.
#[test]
fn raw_text_that_skipped_preparation_is_still_findable() {
    let (_d, db, ids) = db_with(&[("students’ books", SourceKind::Document)]);
    // Overwrite the prepared copy with the raw text, as a caller reaching past
    // `prepare_for_search` would. This is also the only test of the UPDATE
    // trigger, which has to replace the FTS row rather than add to it.
    db.index_chunk_text(ids[0], "students’ books").unwrap();
    assert_eq!(db.search_lexical("students", 10).unwrap(), vec![ids[0]]);
    assert_eq!(db.search_lexical("books", 10).unwrap(), vec![ids[0]]);
}

/// The guard in `search_lexical_with` is load-bearing: `MATCH ''` is a syntax
/// error, so without it a user typing punctuation into a search box gets an
/// engine error handed back — `fts5: syntax error near ""` — rather than no
/// results. Run under every rule, not only `AllTerms`: `TermsInIndexOrAnyTerm`
/// carries a second guard of its own, for the widened attempt's own
/// `MATCH ''`, and a term-free query reaches both.
#[test]
fn a_query_with_no_terms_returns_no_rows_rather_than_an_error() {
    let (_d, db, _) = db_with(&[("витрати за 2024 рік", SourceKind::Document)]);
    for query in ["", "   ", "!!!", "…—", "'", "()"] {
        for rule in mnema_index::QueryRule::ALL {
            assert_eq!(
                db.search_lexical_with(query, rule, 10).unwrap(),
                Vec::<i64>::new(),
                "query {query:?} rule {rule:?}"
            );
        }
    }
    // The fixture is reachable, so the emptiness above is the query's and not
    // the database's.
    assert_eq!(db.search_lexical("витрати", 10).unwrap().len(), 1);
}

/// Pinned, not endorsed. Every term is quoted, so `OR` reaches FTS5 as a third
/// required word rather than as an operator — which makes the query strictly
/// worse than the same words with no operator at all. Whether the product offers
/// a query language is the search/RAG spec's decision and is open; this exists so
/// the next person meets the boundary as a documented one.
#[test]
fn fts5_operators_are_not_a_query_language_here() {
    let (_d, db, _) = db_with(&[("витрати і бюджет на 2024", SourceKind::Document)]);
    assert_eq!(db.search_lexical("витрати бюджет", 10).unwrap().len(), 1);
    assert_eq!(db.search_lexical("витрати OR бюджет", 10).unwrap().len(), 0);
}

/// The harness reasons about "the words the engine will demand". That is
/// `prepare_for_search` plus the index's own splitting, and the second half is
/// `pub(crate)` — so a caller outside this crate that split the prepared string
/// itself would be inventing a second definition of "term".
#[test]
fn search_terms_are_the_words_the_index_demands() {
    let t = mnema_index::search_terms("hello world");
    assert_eq!(t, vec!["hello".to_string(), "world".to_string()]);

    // Case is folded here rather than left to FTS5. The tokenizer lowercases at
    // index time and at query time, so a caller comparing a question's terms
    // with an answer's would otherwise miss `Договір` against `договір` — a
    // difference the search itself does not have.
    assert_eq!(
        mnema_index::search_terms("Договір Оренди"),
        vec!["договір".to_string(), "оренди".to_string()]
    );

    // The other direction, which a length assertion alone would not catch:
    // separators alone are no terms at all. This is the same emptiness
    // `search_lexical` turns into "no rows" instead of a syntax error
    // (`search.rs:34-37`).
    assert!(mnema_index::search_terms("(((").is_empty());

    // The discriminating case for the hardcoded `SourceKind::Document`: every
    // assertion above holds identically whether `search_terms` prepares as
    // Document or as Code, because `expand_camel_case` leaves a word that
    // does not split untouched. An identifier is what tells the two apart —
    // as Code this prepares to `"getUserName get User Name"` and yields four
    // terms, so only the Document reading collapses it to one.
    assert_eq!(
        mnema_index::search_terms("getUserName"),
        vec!["getusername".to_string()]
    );
}

/// The proof for `search_terms`'s diacritics fold. Not a check that
/// `search_lexical` finds the chunk when queried with the reported term —
/// that does not discriminate, because `search_lexical` re-runs
/// `remove_diacritics 2`'s fold on whatever query string it is handed, so an
/// *unstripped* term would be found too, by a second, independent pass
/// through the same fold, and the check would pass whether or not
/// `search_terms` stripped anything at all. Measured: it still passed with
/// the stripping step removed.
///
/// This reads FTS5's own vocabulary instead. `fts5vocab('chunk_fts',
/// 'instance')` names, per occurrence, the term actually stored and the
/// rowid (chunk id) it occurs in — so this compares `search_terms`'s output
/// against the term FTS5 truly recorded for the chunk, not against a second
/// pass through the same tokenizer.
///
/// Covers a Latin word with a diaeresis, one with an acute accent, plain
/// ASCII, two Ukrainian words — one carrying `й`, one carrying `ї` — a
/// Ukrainian word carrying the stress accent, and a set of Latin letters
/// straddling the boundary between those with a canonical decomposition and
/// those without.
///
/// `сло́во` is the case this fixture was extended for. U+043E has no
/// precomposed acute form, so the mark survives NFC and stands alone in the
/// text the tokenizer receives — the tokenizer deletes it and stores `слово`,
/// while a `search_terms` that required an ASCII base before stripping
/// reported `сло́во`. Quoted from the run that found it: `search_terms
/// reported {"сло\u{301}во"}, FTS5 actually stored {"слово"}`.
///
/// What is claimed here is agreement, and only agreement. Both sides fold
/// `Zürich`, `łódź`, `ō`, `ď`, `ĥ` and `ĺ` the same way and leave `й`, `ї`,
/// `ł`, `ø`, `æ`, `đ`, `ħ`, `ŋ`, `ð`, `þ`, `œ` and `ı` alone; the mechanisms
/// behind that are **not** the same and this test does not say they are.
/// `strip_latin_diacritics` asks NFD for a base and takes it when it is
/// ASCII; SQLite's `fts5_remove_diacritic` binary-searches a hardcoded code
/// point table and never decomposes anything. Two mechanisms landing on one
/// result is exactly what a test can establish and a comment cannot, which is
/// why the agreement is measured here per input rather than argued anywhere.
#[test]
fn search_terms_reports_the_terms_fts5_actually_stored() {
    let texts = [
        "Zürich",
        "café",
        "hello",
        "йод",
        "їжак",
        "łódź",
        "Ærø",
        "сло\u{0301}во",
        // The decomposition boundary, measured rather than reasoned about:
        // each atomic letter is paired with a look-alike that decomposes, so
        // this fixture is what backs `mnema_core::nfc`'s claim that the two
        // columns fold differently.
        "đď",
        "ħĥ",
        "łĺ",
        "øō",
        "ŋðþœı",
        // Three stroke letters no other test names, so that `mnema_core::nfc`'s
        // assertion about them stands on a measurement rather than on the same
        // reasoning it is trying to check.
        "ƀɏƶ",
    ];
    let (_d, db, ids) = db_with(&texts.map(|t| (t, SourceKind::Document)));

    db.conn()
        .execute_batch("CREATE VIRTUAL TABLE chunk_vocab USING fts5vocab('chunk_fts', 'instance')")
        .unwrap();

    for (i, text) in texts.iter().enumerate() {
        let mut stmt = db
            .conn()
            .prepare("SELECT DISTINCT term FROM chunk_vocab WHERE doc = ?1")
            .unwrap();
        let stored: std::collections::BTreeSet<String> = stmt
            .query_map([ids[i]], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        let reported: std::collections::BTreeSet<String> =
            mnema_index::search_terms(text).into_iter().collect();

        assert_eq!(
            reported, stored,
            "{text:?}: search_terms reported {reported:?}, FTS5 actually stored {stored:?}"
        );
    }
}

/// Where `mnema_core::nfc::is_stripped_mark`'s set comes from, and what stops
/// it drifting from the tokenizer it claims to mirror.
///
/// The set is hand-written, so on its own it is an assertion about SQLite's
/// internals — the kind this project has already paid for believing. This
/// sweeps the whole combining-marks block through the real index and compares
/// what `search_terms` reports against what FTS5 stored, mark by mark, so the
/// set is measured on every run rather than decoded once from `sqlite3.c`.
/// It goes red in both directions: a mark added to the set that the tokenizer
/// keeps, and a mark dropped from it that the tokenizer deletes.
///
/// Two bases, because the fold has two independent halves. `ф` composes with
/// none of these marks, so each one reaches the tokenizer standing alone —
/// the case a base-conditional predicate got wrong, and the case Ukrainian
/// stress accents fall into. `a` composes with many of them, so the same
/// sweep also exercises the precomposed-letter half without a second test.
///
/// Hebrew, Arabic and Devanagari are swept too, and not for symmetry.
/// `is_stripped_mark`'s doc comment claims their marks are outside the set
/// and that this is the whole of their protection — a claim that named this
/// sweep as its evidence while the sweep ran only U+0300–U+036F, so the
/// evidence did not exist. U+0591–U+05C7, U+064B–U+0652 and U+0900–U+0954
/// are now in the loop, and for them the check is stronger than agreement:
/// every code point Unicode calls a combining mark must still be *present* in
/// what the tokenizer stored. Agreement alone would be satisfied by both
/// sides deleting a mark, which is precisely the outcome those ranges are
/// claimed to be safe from.
///
/// The counts at the end are floors, no more: they catch a sweep that stored
/// nothing, one in which every mark went the same way, or one that quietly
/// stopped covering a script. The per-input equality is what does the real
/// work, and a floor of one-and-one cannot stand in for it.
///
/// **One floor per script, and that is the point.** A single floor summed over
/// the three non-Latin ranges was measured to let two of them vanish in
/// silence: dropping Devanagari left 59 marks and dropping Arabic left 80,
/// both clear of a combined floor of 50, so only Hebrew was ever really
/// guarded. A floor an entire script can walk under is not a floor for that
/// script — and Devanagari, the one the doc comment names most concretely, was
/// the least protected of the three.
///
/// **What this instrument cannot see.** Both sides of the comparison run
/// through `prepare_for_search`. If diacritic stripping ever moved *into*
/// that function, the query side and the indexed side would fold identically,
/// every input here would agree, and this test would stay green — while
/// `chunk.text` and the index diverged, which is the exact failure this cycle
/// exists to prevent. The harness matches answer sentences against
/// `chunk.text`, so that divergence would be silent and this is the wrong
/// instrument to notice it. Anything that moves folding earlier in the
/// pipeline needs its own check against the unprepared column.
#[test]
fn search_terms_matches_what_fts5_stores_for_every_mark() {
    use unicode_general_category::{GeneralCategory, get_general_category};

    // The Latin marks behind both bases; the non-Latin ranges behind `ф`
    // alone, since none of them composes with either base and a second base
    // would only repeat the same measurement.
    // Each non-Latin range carries its own name and its own floor, because one
    // floor summed over all three is a guard a whole script can walk under.
    // Measured: Hebrew contributes 51 combining marks, Devanagari 29, Arabic 8
    // — so a combined floor of 50 fired only when Hebrew vanished, and dropping
    // Devanagari (59 left) or Arabic (80 left) passed in silence. Devanagari is
    // the script `is_stripped_mark`'s doc comment names most concretely, "the
    // Devanagari virama and matra", so it was the one least protected.
    //
    // Each floor is about half its measured count: enough that a range going to
    // zero or being truncated fails, loose enough that a Unicode update moving
    // a few marks does not fail a test which is not about the count. The
    // measured numbers are here rather than in the assertion for the reason
    // `is_stripped_mark`'s own list carries no total — a number beside a
    // definition is a second definition, and it drifts.
    const NON_LATIN: &[(&str, u32, u32, usize)] = &[
        ("Hebrew", 0x0591, 0x05C7, 25),
        ("Arabic", 0x064B, 0x0652, 4),
        ("Devanagari", 0x0900, 0x0954, 14),
    ];

    // `None` marks the Latin block, `Some(i)` indexes `NON_LATIN`.
    let mut marks: Vec<(char, u32, Option<usize>)> = ['ф', 'a']
        .into_iter()
        .flat_map(|base| (0x0300u32..=0x036F).map(move |m| (base, m, None)))
        .collect();
    for (i, (_, first, last, _)) in NON_LATIN.iter().enumerate() {
        marks.extend((*first..=*last).map(|m| ('ф', m, Some(i))));
    }
    let texts: Vec<String> = marks
        .iter()
        .map(|(base, m, _)| format!("{base}{}", char::from_u32(*m).unwrap()))
        .collect();
    let pairs: Vec<(&str, SourceKind)> = texts
        .iter()
        .map(|t| (t.as_str(), SourceKind::Document))
        .collect();
    let (_d, db, ids) = db_with(&pairs);

    db.conn()
        .execute_batch("CREATE VIRTUAL TABLE chunk_vocab USING fts5vocab('chunk_fts', 'instance')")
        .unwrap();

    // U+0345 is not a diacritics case and is excluded on purpose, with the
    // measurement recorded rather than the input quietly dropped. FTS5 *case
    // folds* it to ι (U+03B9) — `"ф\u{0345}"` stores as `"фι"` — while
    // `to_lowercase` leaves it alone, which is the `to_lowercase`-is-not-
    // case-folding gap `search_terms`'s own doc comment names. Closing it
    // needs a case-folding dependency and a decision about which fold this
    // product owes; both are outside a fix round about diacritics.
    //
    // Asserted as a *live* disagreement, not skipped: if a later change makes
    // it agree, this goes red and says so, instead of leaving a stale
    // exception nobody revisits. The measured values are pinned rather than
    // merely required to differ — `assert_ne!` alone is satisfied by a
    // `search_terms` broken on this input for any reason at all, including the
    // one thing this branch is meant to rule out, a predicate that started
    // stripping U+0345 as if it were a diacritic.
    const KNOWN_FOLD_GAP: char = '\u{0345}';

    let mut disagreements = Vec::new();
    let (mut mark_deleted, mut mark_survived, mut gaps_seen) = (0, 0, 0);
    let mut non_latin_marks_checked = vec![0usize; NON_LATIN.len()];
    for (i, text) in texts.iter().enumerate() {
        let mut stmt = db
            .conn()
            .prepare("SELECT DISTINCT term FROM chunk_vocab WHERE doc = ?1")
            .unwrap();
        let stored: std::collections::BTreeSet<String> = stmt
            .query_map([ids[i]], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let reported: std::collections::BTreeSet<String> =
            mnema_index::search_terms(text).into_iter().collect();

        assert!(!stored.is_empty(), "{text:?} stored no term at all");
        if stored.iter().any(|t| t.chars().count() > 1) {
            mark_survived += 1;
        } else {
            mark_deleted += 1;
        }

        // For the non-Latin ranges the claim is survival, not just agreement:
        // both sides deleting the mark would agree perfectly and would be
        // exactly the outcome `is_stripped_mark`'s doc comment says cannot
        // happen. Only real combining marks are held to it — these ranges also
        // contain Hebrew punctuation and Devanagari consonants, which are not
        // marks and have no business surviving as one.
        let (base, mark, range) = marks[i];
        let mark = char::from_u32(mark).unwrap();
        if let Some(range) = range
            && matches!(
                get_general_category(mark),
                GeneralCategory::NonspacingMark | GeneralCategory::SpacingMark
            )
        {
            non_latin_marks_checked[range] += 1;
            assert!(
                stored.iter().all(|t| t.contains(mark)),
                "U+{:04X} is a {} combining mark and the tokenizer dropped it: stored {stored:?}",
                mark as u32,
                NON_LATIN[range].0
            );
        }

        if text.contains(KNOWN_FOLD_GAP) {
            gaps_seen += 1;
            // The measured values, not merely "these differ". FTS5 case folds
            // U+0345 to ι; `to_lowercase` leaves it standing.
            assert_eq!(
                stored,
                std::collections::BTreeSet::from([format!("{base}\u{03B9}")]),
                "the stored side of the known gap changed"
            );
            assert_eq!(
                reported,
                std::collections::BTreeSet::from([format!("{base}{KNOWN_FOLD_GAP}")]),
                "the reported side of the known gap changed"
            );
            // And the gap is the *report's*, not the search's: both sides of a
            // real query fold identically, so the chunk is still findable by
            // its own spelling. Without this the exception would be hiding an
            // actual search defect behind a note about reporting.
            assert_eq!(
                db.search_lexical(text, 10).unwrap(),
                vec![ids[i]],
                "{text:?} must still be findable by its own spelling"
            );
            continue;
        }

        if reported != stored {
            disagreements.push(format!(
                "{text:?}: reported {reported:?}, stored {stored:?}"
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "{} of {} inputs disagreed:\n{}",
        disagreements.len(),
        texts.len(),
        disagreements.join("\n")
    );
    assert_eq!(gaps_seen, 2, "both bases must have exercised the known gap");
    // Floors, not coverage. The per-input equality above is what proves the
    // predicate; these only rule out a run that stored nothing, swept an empty
    // range, or saw every mark go the same way — states in which the equality
    // assertions would pass without having been asked anything.
    assert!(
        mark_deleted > 0 && mark_survived > 0,
        "the sweep saw only one outcome ({mark_deleted} deleted, {mark_survived} survived), so \
         agreement here says nothing about telling them apart"
    );
    // Collected rather than asserted in place, so one run names every script
    // that fell under its floor. Asserting inside the loop would stop at the
    // first, and "the other two were fine" would be an inference rather than
    // something the output says.
    let starved: Vec<String> = NON_LATIN
        .iter()
        .enumerate()
        .filter(|(range, (_, _, _, floor))| non_latin_marks_checked[*range] <= *floor)
        .map(|(range, &(script, _, _, floor))| {
            format!(
                "{script}: {} marks reached, floor {floor}",
                non_latin_marks_checked[range]
            )
        })
        .collect();
    assert!(
        starved.is_empty(),
        "a script stopped being covered and the sweep still agreed on everything it did reach:\n{}",
        starved.join("\n")
    );
}

/// `search_lexical` is `AllTerms` and nothing else. The equality is the point:
/// the product's entry point moves to `search_lexical_with` in this cycle, and
/// a divergence between the two would make every existing assertion in this
/// file measure a path the product no longer takes.
#[test]
fn the_unparameterised_search_is_the_all_terms_rule() {
    let (_d, db, _) = db_with(&[
        ("витрати і бюджет на 2024", SourceKind::Document),
        ("бюджет затверджено", SourceKind::Document),
    ]);
    for query in ["витрати бюджет", "бюджет", "витрати немає", ""] {
        let lexical = db.search_lexical(query, 10).unwrap();
        if query == "бюджет" {
            // Otherwise two empty vectors would satisfy the equality below
            // just as well as two agreeing, non-trivial ones.
            assert!(!lexical.is_empty(), "{query:?} must match indexed rows");
        }
        assert_eq!(
            lexical,
            db.search_lexical_with(query, mnema_index::QueryRule::AllTerms, 10)
                .unwrap(),
            "diverged on {query:?}"
        );
    }
}

/// The measured cause of `recall = 0.0%`: a question carries a word no document
/// contains, and under `AllTerms` that one word empties the result. `AnyTerm`
/// is the crudest answer to it — and the test asserts both directions, because
/// a rule that returned everything would satisfy the first half alone.
#[test]
fn any_term_survives_a_word_no_document_has() {
    let (_d, db, ids) = db_with(&[
        ("бюджет затверджено на ремонт даху", SourceKind::Document),
        ("протокол засідання комісії", SourceKind::Document),
    ]);
    let all = db
        .search_lexical_with("де бюджет", mnema_index::QueryRule::AllTerms, 10)
        .unwrap();
    assert!(all.is_empty(), "AllTerms must still be empty here: {all:?}");

    let any = db
        .search_lexical_with("де бюджет", mnema_index::QueryRule::AnyTerm, 10)
        .unwrap();
    assert_eq!(
        any,
        vec![ids[0]],
        "only the budget chunk holds any of these words, and only it should come back"
    );

    // The other direction: a word no chunk holds still returns nothing, so
    // `AnyTerm` is not "return everything".
    assert!(
        db.search_lexical_with("деінде", mnema_index::QueryRule::AnyTerm, 10)
            .unwrap()
            .is_empty()
    );

    // A rule that kept only the LAST term (a broken separator loop) would
    // pass the assertion above too, because "бюджет" happens to be that
    // last term. Terms living in different chunks close that gap: `OR`
    // must reach both halves of this query, not just the one that is last.
    let cross = db
        .search_lexical_with("протокол даху", mnema_index::QueryRule::AnyTerm, 10)
        .unwrap();
    assert_eq!(
        cross.len(),
        2,
        "AnyTerm must reach every chunk holding a term: {cross:?}"
    );
    assert!(cross.contains(&ids[0]));
    assert!(cross.contains(&ids[1]));

    // A literal `OR` is a third term to search for here, not FTS5's own
    // operator — mirrors `fts5_operators_are_not_a_query_language_here`,
    // but under `AnyTerm`, which already emits real `OR` as its separator.
    assert_eq!(
        db.search_lexical_with("бюджет OR немає", mnema_index::QueryRule::AnyTerm, 10)
            .unwrap(),
        vec![ids[0]],
        "OR is data even under AnyTerm"
    );
}

/// The predicate that Task 4 will build `TermsInIndex` on, exercised alone
/// here so a later failure can be told apart from a failure of the rule
/// that uses it.
#[test]
fn a_terms_presence_is_asked_of_the_whole_index() {
    let (_d, db, _) = db_with(&[("бюджет затверджено", SourceKind::Document)]);
    let prepared = mnema_index::prepare_for_search("де бюджет", mnema_core::SourceKind::Document);

    assert_eq!(
        db.terms_present(&prepared).unwrap(),
        vec!["бюджет".to_string()]
    );

    // Both directions: a query of only-absent words yields nothing, and a query
    // of only-present words yields all of them.
    let absent = mnema_index::prepare_for_search("деінде колись", mnema_core::SourceKind::Document);
    assert!(db.terms_present(&absent).unwrap().is_empty());

    let present =
        mnema_index::prepare_for_search("бюджет затверджено", mnema_core::SourceKind::Document);
    assert_eq!(
        db.terms_present(&present).unwrap(),
        vec!["бюджет".to_string(), "затверджено".to_string()]
    );
}

/// Drops the words the index has never seen and demands the rest — all of them.
/// The second half is what separates this from `AnyTerm`, and a test that only
/// showed the question word being dropped would pass for either rule.
#[test]
fn terms_in_index_drops_the_unseen_word_and_still_demands_the_rest() {
    let (_d, db, _) = db_with(&[
        ("бюджет затверджено на ремонт даху", SourceKind::Document),
        ("бюджет комісії", SourceKind::Document),
    ]);
    let rule = mnema_index::QueryRule::TermsInIndex;

    // The question word is gone, so the remaining two are satisfiable.
    assert_eq!(
        db.search_lexical_with("де бюджет ремонт", rule, 10)
            .unwrap()
            .len(),
        1
    );

    // Still a conjunction: both surviving words exist in the index, but not in
    // one chunk. `AnyTerm` would return two here.
    assert!(
        db.search_lexical_with("комісії ремонт", rule, 10)
            .unwrap()
            .is_empty(),
        "surviving terms are still all required"
    );

    // Every word unseen leaves nothing to demand, and that is no rows rather
    // than every row.
    assert!(
        db.search_lexical_with("деінде колись", rule, 10)
            .unwrap()
            .is_empty()
    );
}

/// The case `TermsInIndex` cannot reach: every surviving word exists somewhere,
/// but no chunk holds them together. The fallback must fire there and must NOT
/// fire where the stricter rule already answered — otherwise it is `AnyTerm`
/// wearing another name.
#[test]
fn the_fallback_fires_only_where_the_stricter_rule_came_back_empty() {
    let (_d, db, _) = db_with(&[
        ("бюджет затверджено на ремонт даху", SourceKind::Document),
        ("бюджет комісії", SourceKind::Document),
    ]);
    let strict = mnema_index::QueryRule::TermsInIndex;
    let with_fallback = mnema_index::QueryRule::TermsInIndexOrAnyTerm;

    // Where the strict rule answers, the fallback changes nothing.
    assert_eq!(
        db.search_lexical_with("де бюджет ремонт", with_fallback, 10)
            .unwrap(),
        db.search_lexical_with("де бюджет ремонт", strict, 10)
            .unwrap()
    );

    // Where it does not, the fallback answers.
    assert!(
        db.search_lexical_with("комісії ремонт", strict, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.search_lexical_with("комісії ремонт", with_fallback, 10)
            .unwrap()
            .len(),
        2
    );

    // Nothing to demand and nothing to widen to: the fallback's own second
    // `MATCH ''` guard, exercised where the first guard alone would not
    // reach it.
    assert!(
        db.search_lexical_with("()", with_fallback, 10)
            .unwrap()
            .is_empty()
    );

    // Both words absent from the index, so the widened attempt still runs
    // a real (non-empty) `MATCH`, and still finds nothing to return.
    assert!(
        db.search_lexical_with("деінде колись", with_fallback, 10)
            .unwrap()
            .is_empty()
    );
}
