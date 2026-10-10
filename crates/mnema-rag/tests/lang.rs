use mnema_rag::{EN, Lang, UK, detect};

#[test]
fn cyrillic_is_ukrainian() {
    let qs = [
        "Як налаштувати резервне копіювання бази даних?",
        "Де зберігається файл конфігурації?",
        "Що означає ця помилка при запуску?",
        "Скільки коштує річна підписка?",
        "Коли закінчується строк дії договору?",
        "Хто відповідає за випуск нової версії?",
        "Як вивести ls -la у файл?",
        "Чому не працює синхронізація між пристроями?",
        "Які кроки потрібні для встановлення?",
        "Покажи всі згадки про бюджет на наступний рік",
        "Опиши головні відмінності між двома планами",
        "Де знайти інструкцію з підключення принтера?",
        "Скільки сторінок у звіті за квартал?",
        "Чи можна змінити пароль без пошти?",
        "Яка адреса офісу в Києві?",
        "Як часто оновлюється індекс?",
        "Назви трьох авторів цього документа",
        "Про що йдеться в розділі про безпеку?",
        "Який розмір дозволеного вкладення?",
        "Чому звіт містить порожні рядки?",
        "Як запустити команду git status у теці проєкту?",
        "Де описано порядок погодження витрат?",
        "Яка версія програми потрібна для імпорту?",
        "Що таке робочий простір і навіщо він потрібен?",
        "Знайди таблицю з підсумками за липень",
        "Хто підписав додаток до угоди?",
        "Як експортувати нотатки у форматі PDF?",
        "Які обмеження має безкоштовний тариф?",
        "Коли востаннє змінювали політику конфіденційності?",
        "Поясни, як працює пошук за змістом",
    ];
    for q in qs {
        assert_eq!(detect(q, None, None), UK, "{q}");
    }
}

#[test]
fn latin_is_english_when_whatlang_is_unsure_or_english() {
    let qs = [
        "How do I configure the database backup?",
        "Where is the configuration file stored?",
        "What does this error mean at startup?",
        "How much does the annual subscription cost?",
        "When does the contract expire?",
        "Who is responsible for the new release?",
        "How to print ls -la into a file?",
        "Why does synchronization between devices fail?",
        "Which steps are required for installation?",
        "Show every mention of the budget for next year",
        "Describe the main differences between the two plans",
        "Where can I find the printer setup guide?",
        "How many pages does the quarterly report have?",
        "Can the password be changed without email?",
        "What is the address of the office?",
        "How often is the index updated?",
        "Name three authors of this document",
        "What does the security section say?",
        "What is the maximum attachment size?",
        "Why does the report contain empty rows?",
        "How do I run git status in the project folder?",
        "Where is the expense approval process described?",
        "Which version of the app is needed for import?",
        "What is a workspace and why do I need one?",
        "Find the table with the July totals",
        "Who signed the addendum to the agreement?",
        "How can I export notes as a PDF file?",
        "What limits does the free plan have?",
        "When was the privacy policy last changed?",
        "Explain how semantic search works",
    ];
    for q in qs {
        assert_eq!(detect(q, None, None), EN, "{q}");
    }
}

#[test]
fn russian_only_when_whatlang_is_sure_and_no_ukrainian_letter() {
    let ru = Lang {
        code: "ru",
        name: "Russian",
    };
    let q = "Как настроить резервное копирование базы данных?";
    assert_eq!(detect(q, None, None), ru);
    // The same sentence with one Ukrainian-only letter is Ukrainian.
    assert_eq!(detect(&q.replace('и', "і"), None, None), UK);
}

#[test]
fn a_short_latin_question_keeps_the_previous_language() {
    assert_eq!(detect("ls -la?", Some(UK), None), UK);
}

#[test]
fn a_letterless_first_question_takes_the_system_language() {
    assert_eq!(
        detect("1.2.3?", None, Some("de-DE")),
        Lang {
            code: "de",
            name: "German"
        }
    );
}

#[test]
fn an_unknown_system_locale_is_english() {
    assert_eq!(detect("?", None, Some("ja-JP")), EN);
    assert_eq!(detect("?", None, None), EN);
}

#[test]
fn a_sure_latin_question_in_another_allowed_language_keeps_it() {
    let cases = [
        (
            "Wie richte ich die Sicherung der Datenbank ein?",
            "de",
            "German",
        ),
        (
            "Quelle est la différence entre ces deux formules d'abonnement proposées ?",
            "fr",
            "French",
        ),
        (
            "¿Cómo configuro la copia de seguridad de la base de datos?",
            "es",
            "Spanish",
        ),
        (
            "Qual è la differenza tra questi due piani di abbonamento che avete proposto?",
            "it",
            "Italian",
        ),
        (
            "Jak skonfigurować kopię zapasową bazy danych?",
            "pl",
            "Polish",
        ),
    ];
    for (q, code, name) in cases {
        assert_eq!(detect(q, Some(UK), None), Lang { code, name }, "{q}");
    }
}

#[test]
fn an_unreliable_latin_question_is_english() {
    // whatlang labels this German but reports it as unreliable.
    assert_eq!(detect("xqzv wprt kjhg bnmz", Some(UK), Some("de-DE")), EN);
}

#[test]
fn the_previous_language_wins_over_the_system_one() {
    assert_eq!(detect("ls -la?", Some(UK), Some("de-DE")), UK);
}

#[test]
fn an_unreliable_russian_looking_phrase_is_ukrainian() {
    // whatlang says Russian here but not reliably, and there is no Ukrainian-only letter.
    assert_eq!(detect("Как настроить", None, None), UK);
}
