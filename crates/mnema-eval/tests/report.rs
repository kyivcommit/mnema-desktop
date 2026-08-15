use mnema_eval::{Class, Location, Outcome, Report};

/// Two returned chunks, one the index can place and one it cannot — so every
/// render below walks both arms of the failure list, not only the happy one.
fn outcome(question: &str, class: Class, rank: Option<usize>) -> Outcome {
    Outcome {
        question: question.to_string(),
        class,
        rank,
        returned: vec![7, 8],
        returned_locations: vec![
            Some(Location {
                path: "uk/dohovory/oren-01.md".to_string(),
                first_line: "Договір складено у двох примірниках.".to_string(),
            }),
            None,
        ],
        gold: vec![42],
        text_matched: None,
        content_matched: None,
    }
}

#[test]
fn recall_at_one_counts_only_the_first_position() {
    let outcomes = vec![
        outcome("q-1", Class::Literal, Some(1)),
        outcome("q-2", Class::Literal, Some(3)),
    ];
    let report = Report::of(&outcomes, 70);
    assert_eq!(report.recall_at(Class::Literal, 1), Some(0.5));
    assert_eq!(report.recall_at(Class::Literal, 5), Some(1.0));
}

#[test]
fn a_class_with_no_questions_has_no_recall_rather_than_zero() {
    // Zero would read as "every paraphrase failed"; None reads as "nothing was
    // measured". The distinction is the one the spec spends a section on.
    let report = Report::of(&[outcome("q-1", Class::Literal, Some(1))], 70);
    assert_eq!(report.recall_at(Class::Paraphrase, 1), None);
}

#[test]
fn a_question_with_no_rank_counts_against_every_k() {
    let report = Report::of(&[outcome("q-1", Class::Literal, None)], 70);
    assert_eq!(report.recall_at(Class::Literal, 20), Some(0.0));
}

#[test]
fn every_number_is_printed_beside_its_chance_level() {
    // 20 of 70 chunks is 28.6%: printing recall@20 alone would let 30% read as
    // a result. Both directions — the chance level is there AND it is the
    // right one for this k.
    let report = Report::of(&[outcome("q-1", Class::Literal, Some(2))], 70);
    let text = report.render();
    assert!(
        text.contains("28.6"),
        "chance level for k=20 missing from:\n{text}"
    );
    assert!(
        text.contains("1.4"),
        "chance level for k=1 missing from:\n{text}"
    );
}

#[test]
fn the_failures_are_in_the_report_not_appended_to_it() {
    let outcomes = vec![
        outcome("q-1", Class::Literal, Some(1)),
        outcome("q-2", Class::Literal, None),
    ];
    let text = Report::of(&outcomes, 70).render();
    assert!(
        text.contains("q-2"),
        "the failed question is not named:\n{text}"
    );
    assert!(
        !text.contains("q-1"),
        "a question that succeeded should not be listed:\n{text}"
    );
}

#[test]
fn a_chunk_the_index_cannot_place_says_so_instead_of_a_bare_number() {
    // Chunk 8 has no location. Both directions: it is still listed AND it does
    // not stand there as a number alone, which is the one thing nobody can
    // look up once the run's temporary index is gone.
    let text = Report::of(&[outcome("q-1", Class::Literal, None)], 70).render();
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with('8'))
        .unwrap_or_else(|| panic!("chunk 8 is not listed at all:\n{text}"));
    assert!(
        line.contains("покажчику"),
        "an unplaceable chunk printed as a bare number:\n{line}"
    );
}

#[test]
fn a_long_first_line_is_cut_on_a_character_boundary() {
    // Cutting bytes would split a Cyrillic letter in half and panic. A word
    // whose every letter is two bytes, past the snippet width, is the fixture
    // that would find it.
    let mut failed = outcome("q-1", Class::Literal, None);
    failed.returned_locations[0] = Some(Location {
        path: "uk/dohovory/oren-01.md".to_string(),
        first_line: "Ділянка".repeat(40),
    });
    let text = Report::of(&[failed], 70).render();
    assert!(
        text.contains("Ділянка") && text.contains('…'),
        "the long line was neither shown nor marked as cut:\n{text}"
    );
}

#[test]
fn the_configurations_that_do_not_exist_are_named_not_zeroed() {
    // Both directions in one render: the two unbuilt configurations are named,
    // AND the word that says they are unbuilt is there. Naming them beside a
    // number would be the failure this guards.
    let text = Report::of(&[outcome("q-1", Class::Literal, Some(1))], 70).render();
    for word in ["вмістом", "суміш", "не збудован"] {
        assert!(
            text.contains(word),
            "{word} is not accounted for in:\n{text}"
        );
    }
}

#[test]
fn a_class_that_was_never_asked_says_so_where_its_numbers_would_be() {
    // Only literal questions were put, so two of the three rows have nothing
    // to report. A zero there would read as "every one of them failed" — the
    // substitution the spec spends a section refusing, and the one `recall_at`
    // returning `None` is not enough to prevent, because `render` could print
    // a zero anyway. Both directions: the word is in those rows AND no
    // percentage is.
    let text = Report::of(&[outcome("q-1", Class::Literal, Some(1))], 70).render();
    let unmeasured: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("недоступно"))
        .collect();
    assert_eq!(
        unmeasured.len(),
        2,
        "two classes were never asked, so two rows should say so:\n{text}"
    );
    for row in unmeasured {
        assert!(
            !row.contains('%'),
            "a class with no questions still printed a number:\n{row}"
        );
    }
}
