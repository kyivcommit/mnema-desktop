use mnema_core::SourceKind;
use unicode_general_category::{GeneralCategory, get_general_category};

/// The one spelling of an apostrophe that reaches the index.
///
/// U+02BC MODIFIER LETTER APOSTROPHE, not U+0027. Both stay inside a token —
/// U+02BC because it is a letter (Lm), U+0027 because `tokenchars` names it —
/// but only U+02BC also survives FTS5's MATCH *expression* grammar, where every
/// non-ASCII byte belongs to a bareword and U+0027 is a syntax error. Choosing
/// U+0027 put that failure one layer away from every Ukrainian query, and wrote
/// the fragile form into stored rows, which outlive the code that reads them.
pub(crate) const APOSTROPHE: char = '\u{02BC}';

/// Prepares the copy that goes into the lexical index. The original is kept
/// for display, so nothing here has to preserve length — which is what makes
/// NFC normalisation the first step: `remove_diacritics 2` does not fold the
/// Cyrillic breve or diaeresis, so a precomposed `й`/`ї` and its decomposed
/// spelling tokenize as two different words without it, and macOS hands over
/// the decomposed form while a query typed elsewhere is precomposed. D32.
///
/// This is the query side of that obligation; extraction carries the other
/// half, normalising the text that is actually stored — a length change is
/// free here, since nothing downstream takes an offset from this copy, but it
/// is not free there, so it happens once, before any offset or hash is taken,
/// rather than being re-derived on every read. G7.0 §5.4.
pub fn prepare_for_search(text: &str, kind: SourceKind) -> String {
    let normalised = mnema_core::nfc::normalise(text);
    let folded: String = normalised
        .chars()
        .map(|c| match c {
            // Apostrophe variants people actually type, unified so the same
            // Ukrainian word indexes one way regardless of keyboard. A backtick
            // is NOT one of them: it is a quote character, and folding it made
            // `template` index as an apostrophe-wrapped token nobody can ask for
            // by name — a real loss, since code is indexed in v1.
            '\'' | '\u{2019}' | '\u{02B9}' => APOSTROPHE,
            // Genuine orthographic variants.
            'ґ' => 'г',
            'Ґ' => 'Г',
            'ё' => 'е',
            'Ё' => 'Е',
            other => other,
        })
        .collect();
    let folded = separate_edge_apostrophes(&folded);

    match kind {
        SourceKind::Code => {
            let expanded = expand_camel_case(&folded);
            if expanded.is_empty() {
                folded
            } else {
                format!("{folded} {expanded}")
            }
        }
        SourceKind::Document | SourceKind::Data => folded,
    }
}

/// The terms the lexical index will demand for `text`.
///
/// `search_lexical` runs `prepare_for_search(query, SourceKind::Document)`
/// (`search.rs:31`) and then `terms`, one call deeper inside `as_fts5_phrases`
/// (`search.rs:91`); this is those two halves, joined and owned, because
/// `terms` borrows from a string the caller does not hold and is `pub(crate)`
/// besides.
///
/// No `kind` parameter, on purpose: `search_lexical` hardcodes
/// `SourceKind::Document` for every query, not the kind of the thing being
/// searched, because code chunks are indexed with camelCase expanded and
/// preparing the query the same way would turn one identifier into four
/// demanded terms instead of one (`search.rs:14-23`). A caller reasoning
/// about "the words the engine demands" therefore has no legitimate second
/// reading to ask for here — offering one would let it model a query the
/// engine never actually runs. If a caller ever needs the corpus-side reading
/// of a code chunk for something else, that is a different question and
/// belongs behind a new, visible parameter rather than a silently wrong
/// answer from this one.
///
/// Lowercased here on purpose. FTS5's `unicode61` folds case on both sides of a
/// MATCH, so `Договір` and `договір` are one term to the *search* — a caller
/// comparing two texts' terms has to see the same thing. `to_lowercase` is
/// Unicode's default full lowercase *mapping*, a different operation from case
/// *folding* — Unicode's fold maps `ß` to `ss` and `ﬁ` to `fi` while
/// `to_lowercase` leaves both alone — and it is not byte-identical to FTS5's
/// own fold for every script on earth. Where the two disagree, the
/// disagreement belongs in a test rather than in a promise.
///
/// Exactly one is measured, and it is not `ß` or `ﬁ`: `unicode61` leaves those
/// two alone as well, so this function and the index agree on them and there
/// is nothing there to close. The one real divergence is U+0345, which FTS5
/// case folds to ι and `to_lowercase` does not, so this over-reports for a
/// Greek iota subscript. `search_terms_matches_what_fts5_stores_for_every_mark`
/// pins both sides of it by value and proves the affected text is still
/// findable by its own spelling, since a real query folds on both sides.
///
/// Diacritics stripped here too, for the same reason: `schema.sql` configures
/// the tokenizer with `remove_diacritics 2`, which folds a Latin word's
/// accented and unaccented spellings onto one token — `Zürich` and `Zurich`
/// are one term to the index. It does **not** leave Cyrillic alone, which is
/// the correction this line carries: the tokenizer strips by code point
/// without consulting the base, so the Ukrainian stress accent goes and
/// `сло́во` is stored as `слово`. What survives is the *precomposed* letter —
/// `й` and `ї` keep their marks because U+0439 and U+0457 are not in the
/// tokenizer's table, not because their script is spared (D32).
/// `strip_latin_diacritics` mirrors both halves, so this reports the string
/// the index actually stores rather than the merely lowercased one.
pub fn search_terms(text: &str) -> Vec<String> {
    let prepared = prepare_for_search(text, SourceKind::Document);
    let prepared = mnema_core::nfc::strip_latin_diacritics(&prepared);
    terms(&prepared).map(|t| t.to_lowercase()).collect()
}

/// Splits prepared text into the terms the tokenizer will see: letters, digits
/// and word-internal apostrophes, with everything else a separator.
///
/// Shared with the query side, which is the point of it being here. Splitting a
/// query on whitespace alone made `витрати(2024)` a single term, and a single
/// term becomes a single quoted phrase demanding the two words be adjacent — so
/// that query matched nothing while the same words with a space matched.
pub(crate) fn terms(prepared: &str) -> impl Iterator<Item = &str> {
    prepared
        .split(|c: char| !is_term_char(c))
        .map(|term| term.trim_matches(APOSTROPHE))
        .filter(|term| !term.is_empty())
}

/// The category list the tokenizer is configured with — `L*`, `N*`, `Co`, `Mn`,
/// `Mc`, as written in the `CREATE VIRTUAL TABLE` in `schema.sql` — read from
/// Unicode's general category rather than guessed at.
///
/// It replaced `char::is_alphanumeric`, which is Alphabetic plus Numeric and so
/// covers only the marks in Other_Alphabetic: that left out every Indic virama,
/// every NFD combining accent and the whole of `Co`, and a word the tokenizer
/// had kept whole — `हिन्दी`, `শব্দ`, decomposed `König` — became unreachable from
/// its own spelling.
///
/// It is NOT the same list SQLite applies, and the gap is not small. This reads
/// UCD 16.0 through unicode-general-category, while SQLite 3.53.2's unicode61
/// carries a table frozen at **Unicode 6.1** — measured against DerivedAge, not
/// assumed. Take the characters whose category keeps them OUT of the token
/// classes, so that "SQLite classifies it as a separator" means "SQLite's table
/// knows this character": every one assigned up to and including 6.1 is known
/// (54 of 54 at 6.1, 1023 of 1023 at 6.0), and every one from 6.2 onward is not
/// (0 of 1 at 6.2, 0 of 5 at 6.3, 0 of 740 at 7.0). The boundary is that sharp.
///
/// One line reproduces it, on the SQLite this crate bundles. ֏ U+058F ARMENIAN
/// DRAM SIGN arrived in 6.1 and ₺ U+20BA TURKISH LIRA SIGN in 6.2 — both
/// currency symbols, one version apart:
///
/// ```sql
/// CREATE VIRTUAL TABLE t USING fts5(x, tokenize="unicode61 remove_diacritics 2 categories 'L* N* Co Mn Mc'");
/// INSERT INTO t VALUES ('a֏b'), ('a₺b');
/// SELECT rowid FROM t WHERE t MATCH '"a"';   -- returns 1 and not 2
/// ```
///
/// Row 1 comes back because the dram sign separates the word, leaving `a` a
/// token of its own; row 2 does not, because the lira sign is a character the
/// table never heard of and it is kept inside `a₺b`.
///
/// Sweeping the whole range shows the size of it: of the 1,112,032 codepoints
/// from U+0020 to U+10FFFF, surrogates excluded, 822,780 are classified
/// differently, and every one runs the same way — SQLite keeps the character
/// inside the token, this function splits there. Zero run the other way. Of the
/// difference, 3,249 are characters Unicode 16 actually assigns: ₽ U+20BD,
/// ₿ U+20BF, 🙂 U+1F642, the bidi controls U+061C and U+2066. The other 819,531
/// are codepoints Unicode 16 still leaves unassigned and SQLite already treats
/// as letters.
///
/// What it costs: «ціна 100₽ за штуку» indexes as the single token `100₽` and
/// answers neither `100₽` nor `100`.
///
/// Left standing rather than patched. Nothing disagrees in the other direction,
/// so this is strictly better than what it replaced; closing the gap would mean
/// carrying our own copy of SQLite's frozen table, which goes stale in the other
/// direction with every release; and what a query should do with a currency sign
/// glued to a number is the search/RAG spec's decision, not this function's.
///
/// The canonical apostrophe needs no special case: U+02BC is a modifier letter,
/// so `L*` already covers it, and if it ever moved to a character outside these
/// categories the tokenizer would stop binding words with it too — which is a
/// change both sides would make together rather than one silently.
fn is_term_char(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
            | GeneralCategory::PrivateUse
            | GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
    )
}

/// A term character that is not the apostrophe itself. The exclusion is what
/// stops a run of apostrophes propping each other up: U+02BC is a term
/// character, so `hello''world` would otherwise read as apostrophes flanked by
/// term characters on both sides and bind into one word.
fn is_word_char(c: char) -> bool {
    c != APOSTROPHE && is_term_char(c)
}

/// Replaces every apostrophe that is not between two word characters with a
/// separator.
///
/// The apostrophe belongs to its token wherever it sits, which is what makes
/// `п'ять` and `don't` single words — and equally what made `‘hello’` index as
/// `hello'`, `students’` as `students'` and `'quoted'` as `'quoted'`, none of
/// which answer to their own spelling.
///
/// A separator, not a deletion: deleting it closed the gap it left, so
/// `hello''world` became the one token `helloworld` and neither half could find
/// it. Doing this here rather than in the tokenizer keeps the two sides
/// symmetric, because the query passes through this same function.
fn separate_edge_apostrophes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let inside =
            previous.is_some_and(is_word_char) && chars.peek().copied().is_some_and(is_word_char);
        out.push(if c == APOSTROPHE && !inside { ' ' } else { c });
        // Tracked from the input, not from the output, so that a replaced
        // apostrophe cannot make its neighbours look adjacent to each other.
        previous = Some(c);
    }
    out
}

/// Appends split forms of run-together identifiers: getUserName -> "get User Name".
/// Costs about 8% of index size and closes the one gap unicode61 leaves on
/// identifiers. Applied to code only.
fn expand_camel_case(text: &str) -> String {
    let mut out = String::new();
    // The same rule as `terms`, and for the same reason: `is_alphanumeric` here
    // was the last of the approximation, and it disagreed with its neighbour
    // twelve lines up about where a word ends. Harmless where it stood — a wrong
    // boundary only changes which extra terms get appended, never whether the
    // original text is findable — but there is no reason for two answers.
    for word in text.split(|c: char| !is_term_char(c) && c != '_') {
        // Characters, not `len()`. A byte length makes the threshold one or two
        // characters for every script outside ASCII, so `дБ` — two characters,
        // four bytes — cleared it and expanded into two single-letter tokens.
        if word.chars().count() < 3 {
            continue;
        }
        let parts = split_identifier(word);
        if parts.len() < 2 {
            // Nothing split, so appending the word would repeat a term the chunk
            // already has and weight it twice for nothing.
            continue;
        }
        for part in parts {
            out.push_str(part);
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// Splits a run-together identifier: `getUserName` into `get User Name`,
/// `HTTPServer` into `HTTP Server`, `IOSHandler` into `IOS Handler`.
///
/// A capital opens a new part when the character before it is lower case, or
/// when it ends a run of capitals and a lower-case letter follows. That second
/// clause is what keeps acronyms whole: splitting at every capital turned
/// `HTTPServer` into `H T T P Server`, losing `http` as a term altogether and
/// putting single letters into the index, so `h` began matching code chunks.
fn split_identifier(word: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = word.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (offset, c) = chars[i];
        if !c.is_uppercase() {
            continue;
        }
        let previous = chars[i - 1].1;
        let ends_an_acronym = previous.is_uppercase()
            && chars
                .get(i + 1)
                .is_some_and(|&(_, next)| next.is_lowercase());
        if previous.is_lowercase() || ends_an_acronym {
            parts.push(&word[start..offset]);
            start = offset;
        }
    }
    parts.push(&word[start..]);
    parts
}
