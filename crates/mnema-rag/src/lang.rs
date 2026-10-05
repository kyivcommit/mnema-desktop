//! Language of a question: script first, then whatlang, then context.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lang {
    pub code: &'static str,
    pub name: &'static str,
}

pub const UK: Lang = Lang {
    code: "uk",
    name: "Ukrainian",
};
const RU: Lang = Lang {
    code: "ru",
    name: "Russian",
};
pub const EN: Lang = Lang {
    code: "en",
    name: "English",
};

pub fn detect(question: &str, previous: Option<Lang>, system: Option<&str>) -> Lang {
    if has_cyrillic_word(question) {
        return if is_russian(question) { RU } else { UK };
    }
    if ascii_letters(question) >= 8 {
        return latin(question);
    }
    if let Some(p) = previous {
        return p;
    }
    system.and_then(from_locale).unwrap_or(EN)
}

/// Russian only when whatlang is sure and no Ukrainian-only letter is there.
fn is_russian(q: &str) -> bool {
    !q.chars().any(|c| "іїєґІЇЄҐ".contains(c))
        && whatlang::detect(q).is_some_and(|i| i.lang() == whatlang::Lang::Rus && i.is_reliable())
}

const LATIN: [(whatlang::Lang, Lang); 6] = [
    (whatlang::Lang::Eng, EN),
    (
        whatlang::Lang::Deu,
        Lang {
            code: "de",
            name: "German",
        },
    ),
    (
        whatlang::Lang::Fra,
        Lang {
            code: "fr",
            name: "French",
        },
    ),
    (
        whatlang::Lang::Spa,
        Lang {
            code: "es",
            name: "Spanish",
        },
    ),
    (
        whatlang::Lang::Ita,
        Lang {
            code: "it",
            name: "Italian",
        },
    ),
    (
        whatlang::Lang::Pol,
        Lang {
            code: "pl",
            name: "Polish",
        },
    ),
];

/// whatlang inside the Latin allowlist; unsure means English.
fn latin(q: &str) -> Lang {
    let detector = whatlang::Detector::with_allowlist(LATIN.iter().map(|(w, _)| *w).collect());
    detector
        .detect(q)
        .filter(|i| i.is_reliable())
        .and_then(|i| LATIN.iter().find(|(w, _)| *w == i.lang()))
        .map_or(EN, |(_, l)| *l)
}

/// Letters in ASCII-only words of 2+ letters.
fn ascii_letters(q: &str) -> usize {
    words(q).filter(|w| w.is_ascii()).map(str::len).sum()
}

/// A word of 2+ letters, all of them Cyrillic.
fn has_cyrillic_word(q: &str) -> bool {
    words(q).any(|w| w.chars().all(|c| ('\u{400}'..='\u{4FF}').contains(&c)))
}

fn words(q: &str) -> impl Iterator<Item = &str> {
    q.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 2)
}

/// `de-DE` / `de_DE.UTF-8` -> German, if it is one of the languages we know.
fn from_locale(locale: &str) -> Option<Lang> {
    let tag = locale.split(['-', '_', '.']).next()?.to_ascii_lowercase();
    [UK, RU]
        .into_iter()
        .chain(LATIN.map(|(_, l)| l))
        .find(|l| l.code == tag)
}
