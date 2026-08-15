use mnema_search::{ContentArm, Missing};

/// "Off", "cannot be asked" and "asked and failed" are three facts, and a
/// person shown one list for all three learns something false about their
/// documents. The type is what keeps them apart; this pins that it can hold
/// all three.
#[test]
fn the_content_arms_silences_are_told_apart_by_type() {
    let states = [
        ContentArm::Off,
        ContentArm::NotConfigured(Missing::NoKey),
        ContentArm::NotConfigured(Missing::NoModel),
        ContentArm::Failed {
            reason: "no route to host".to_string(),
        },
        ContentArm::Answered {
            chunks: vec![],
            embedded: 0,
            total: 9,
        },
    ];
    // Each is distinct from every other, including the two `NotConfigured`s and
    // including an `Answered` that answered with nothing.
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

/// An arm that answered nothing is not an arm that could not be asked, and the
/// coverage numbers ride on the answer rather than beside it.
#[test]
fn an_empty_answer_still_carries_its_coverage() {
    let arm = ContentArm::Answered {
        chunks: vec![],
        embedded: 3,
        total: 9,
    };
    match arm {
        ContentArm::Answered {
            chunks,
            embedded,
            total,
        } => {
            assert!(chunks.is_empty());
            assert_eq!((embedded, total), (3, 9));
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}
