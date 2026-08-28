//! The acceptance checks that only a real provider can answer.
//!
//! Every other test in this workspace talks to `mnema_mock_provider`, which
//! answers whatever its fixture says — so a mock can confirm that this build
//! reads a shape correctly, and can never confirm that the shape is the one the
//! provider sends. Three facts in this cycle were therefore unmeasurable until
//! a real key existed: whether the balance fields parse, whether a model this
//! product recommends answers a two-text request with two *different* vectors,
//! and what width it actually returns.
//!
//! `#[ignore]`d, and that is a decision rather than a convenience — the same one
//! `crates/mnema-secrets/tests/roundtrip.rs` records. These tests spend the
//! owner's money (a few thousand tokens, a fraction of a cent) and depend on a
//! third party being reachable, so a gate that ran them would fail for reasons
//! that are not defects. Run them deliberately:
//!
//! ```text
//! cargo test -p mnema-desktop --test live_provider -- --ignored --nocapture
//! ```
//!
//! **The key comes from `MNEMA_LIVE_KEY` and never from the credential store**,
//! and the reason is measured rather than stylistic: this crate's
//! `[dev-dependencies]` enable `mnema-secrets/test-store`, so every test target
//! here is served by an in-memory store and **cannot reach the real one**. That
//! isolation is deliberate — `.github/workflows/ci.yml` requires it — and it
//! means a live check simply cannot live behind `mnema_secrets::load`. The first
//! version of this file did, ran green, and measured nothing.
//!
//! **The key is never printed.** It is a local binding passed to one function;
//! every `Error` variant in `mnema_provider` is held by `tests/probe.rs` to
//! carry no part of it, which is what makes `expect` safe on these calls. Pass
//! it without leaving it in shell history — a leading space where the shell is
//! configured to drop such lines, or `read -s`.

use mnema_desktop::models::DEFAULT_MODELS;
use mnema_provider::{OPENROUTER_BASE, Role, check_embedding_model, check_key, list_models};

/// Skips rather than fails when no key was supplied: not having run the
/// acceptance check is not a defect in the product. It says so on the way out,
/// because a green run that measured nothing is the shape this cycle spent
/// eleven rounds removing — and the first version of this file was one.
fn key() -> Option<String> {
    match std::env::var("MNEMA_LIVE_KEY") {
        Ok(k) if !k.is_empty() => Some(k),
        _ => None,
    }
}

#[test]
#[ignore = "spends the owner's money and needs the network"]
fn the_balance_arm_the_plan_guessed_is_the_arm_the_provider_reaches() {
    let Some(key) = key() else {
        eprintln!("MNEMA_LIVE_KEY is not set; this run measured NOTHING");
        return;
    };
    let check = check_key(OPENROUTER_BASE, &key).expect("the provider answers");
    // Printed as the variant's own name and its magnitude, never the account's
    // figure: this file's output is quoted into a versioned ledger.
    println!(
        "balance arm reached: {:?}",
        std::mem::discriminant(&check.balance)
    );
    assert!(
        matches!(check.balance, mnema_provider::Balance::Known { .. }),
        "the plan's field names were a guess; this is the run that settles them: {:?}",
        check.balance
    );
}

/// The trap the whole embedding probe exists for, run against a real model for
/// the first time. Measured 2026-07-25 on Google's embedder: several texts in
/// one request come back as **one averaged vector**, which fills an index with
/// numbers that look right and retrieve at random.
#[test]
#[ignore = "spends the owner's money and needs the network"]
fn a_recommended_model_answers_two_texts_with_two_different_vectors() {
    let Some(key) = key() else {
        eprintln!("MNEMA_LIVE_KEY is not set; this run measured NOTHING");
        return;
    };
    let check = check_embedding_model(OPENROUTER_BASE, &key, "baai/bge-m3")
        .expect("baai/bge-m3 is usable for this product");
    println!("baai/bge-m3: dim={} norm={:.4}", check.dim, check.norm);
    assert_eq!(
        check.dim, 1024,
        "the width is measured from the answer, never read from the list — if this \
         moved, the list and the index would disagree about the same model"
    );
}

/// The other side of the same question: a model this product refuses must be
/// refused for the reason stated, not merely absent. `google/gemini-embedding-2`
/// is the family the averaging measurement came from.
#[test]
#[ignore = "spends the owner's money and needs the network"]
fn the_averaging_family_is_still_what_the_measurement_said_it_was() {
    let Some(key) = key() else {
        eprintln!("MNEMA_LIVE_KEY is not set; this run measured NOTHING");
        return;
    };
    match check_embedding_model(OPENROUTER_BASE, &key, "google/gemini-embedding-2") {
        Ok(check) => println!(
            "ACCEPTED: dim={} norm={:.4} — the 2026-07-25 measurement no longer \
             describes this model, and the ledger entry that cites it is stale",
            check.dim, check.norm
        ),
        Err(e) => println!("refused, as measured: {e}"),
    }
}

/// **A stated model id is a claim about somebody else's catalogue**, and this is
/// the only run that can settle it. Every other test in this workspace answers
/// from `mnema_mock_provider`, so an id the provider retired last week leaves
/// both tabs green while nothing works — the failure no unit test in this
/// repository can see, because none of them asks the provider anything.
///
/// Marked "needs the network" and **not** "spends the owner's money", unlike its
/// neighbours: `/models` is public and takes no key, measured 2026-08-08 and
/// written down on `models::provider_models`. So this one runs without
/// `MNEMA_LIVE_KEY` and there is nothing to skip for.
///
/// It calls `mnema_provider::list_models` with the arguments
/// `models::provider_models` passes it — the same base, no key, the role — which
/// is that command with its one `State` parameter removed. Building an
/// `AppState` pointed at the real provider would measure Tauri's managed state,
/// not the catalogue.
///
/// **Both directions, per role.** Present is the claim this build’s owner ruling makes; selectable is
/// the claim the window makes when it offers the id as a default — a model this
/// build's own rules refuse is listed, greyed, and cannot be chosen, which is a
/// default nobody can use. Asserted separately so a failure says which of the
/// two it was.
#[test]
#[ignore = "needs the network"]
fn both_default_models_are_still_in_the_providers_catalogue() {
    for (role, id) in [
        (Role::Embedding, DEFAULT_MODELS.embedding),
        (Role::Chat, DEFAULT_MODELS.chat),
    ] {
        let catalogue = list_models(OPENROUTER_BASE, None, role).expect("the catalogue answers");
        println!(
            "{role:?}: {} entries, {} records unreadable",
            catalogue.entries.len(),
            catalogue.unreadable
        );
        let entry = catalogue.entries.iter().find(|e| e.id == id);
        let Some(entry) = entry else {
            panic!(
                "`{id}` is not in the provider's {role:?} list at all — this product ships it as \
                 its default for that role, so an installation that enters a key gets a model \
                 the provider does not have"
            );
        };
        assert!(
            entry.refusal.is_none(),
            "`{id}` is listed for {role:?} and this build's own rules refuse it, so it is \
             offered greyed and cannot be chosen: {:?}",
            entry.refusal
        );
    }
}
