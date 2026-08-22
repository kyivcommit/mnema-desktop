//! Interface localization (§D129). English is the fallback for any locale the
//! app does not support (§D80, amended). This module owns the effective locale;
//! `mnema-core::Coordinate::render` is prompt-only and deliberately untouched.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Uk,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleChoice {
    Auto,
    Uk,
    En,
}

/// The OS may report `uk-UA`, `uk_UA.UTF-8` (POSIX), `UK-ua`, `C`, `POSIX`, or
/// nothing. Lowercase, cut at the first `-`/`_`/`.`, and reject the non-language
/// locales `c`/`posix`/empty.
pub fn primary_subtag(os: Option<&str>) -> Option<String> {
    let raw = os?.trim().to_lowercase();
    let head = raw.split(['-', '_', '.']).next().unwrap_or("");
    match head {
        "" | "c" | "posix" => None,
        tag => Some(tag.to_string()),
    }
}

pub fn resolve(choice: LocaleChoice, os: Option<&str>) -> Lang {
    match choice {
        LocaleChoice::Uk => Lang::Uk,
        LocaleChoice::En => Lang::En,
        LocaleChoice::Auto => match primary_subtag(os).as_deref() {
            Some("uk") => Lang::Uk, // the only subtag that does not fall to EN
            _ => Lang::En,          // en, de, zh, None → En (D80: English is the fallback)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_subtag_handles_real_os_grammar() {
        assert_eq!(primary_subtag(Some("uk-UA")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("uk_UA.UTF-8")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("UK-ua")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("en")).as_deref(), Some("en"));
        assert_eq!(primary_subtag(Some("C")), None);
        assert_eq!(primary_subtag(Some("POSIX")), None);
        assert_eq!(primary_subtag(Some("")), None);
        assert_eq!(primary_subtag(None), None);
    }

    #[test]
    fn resolve_auto_picks_supported_else_english() {
        assert_eq!(resolve(LocaleChoice::Auto, Some("uk-UA")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::Auto, Some("uk")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::Auto, Some("en-US")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, Some("de-DE")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, Some("zh-CN")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, None), Lang::En);
    }

    #[test]
    fn resolve_explicit_choice_ignores_os() {
        assert_eq!(resolve(LocaleChoice::Uk, Some("en-US")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::En, Some("uk-UA")), Lang::En);
    }
}
