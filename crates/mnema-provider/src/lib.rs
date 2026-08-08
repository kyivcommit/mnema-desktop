//! Everything this product knows about talking to a model provider.
//!
//! v1 speaks to OpenRouter alone (spec §2.2). The base URL is a parameter
//! rather than a constant inside the request functions, because the tests point
//! it at a local server — and because v2's second provider arrives as another
//! base URL rather than as another code path.

mod catalogue;
mod http;
mod probe;

pub use catalogue::{
    Catalogue, MIN_CONTEXT_TOKENS, ModelEntry, RecordId, Refusal, Role, UnreadableRecord,
    models_from_json,
};
pub use probe::{
    Balance, EmbeddingCheck, KeyCheck, ProviderMessage, SanitisedText, check_embedding_model,
    check_key,
};

/// Where v1 goes. Not a configuration: v1 has one provider (spec §2.2).
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

/// Asks the provider for one role's models. The key is optional because this
/// endpoint is public — measured 2026-08-08 — which is what lets the choice be
/// shown before an account exists (spec §2.3).
///
/// An empty catalogue (`{"data":[]}`) is a success, not a failure. Whether
/// zero selectable models is actionable is a question for whoever renders the
/// result — the shell sees the full `Catalogue`, including `unreadable`, and
/// can tell "the provider genuinely has none" from "something upstream ate
/// them" far better than this function can guess. Turning an otherwise
/// well-formed, on-topic answer into an error here would hide it instead of
/// reporting it (Task 2 review, item 3).
pub fn list_models(base: &str, key: Option<&str>, role: Role) -> Result<Catalogue, Error> {
    let path = match role.query() {
        Some(filter) => format!("/models?output_modalities={filter}"),
        None => "/models".to_string(),
    };
    let (status, body) = http::get(base, &path, key)?;
    if status == 200 {
        return models_from_json(role, &body);
    }
    let key_sent = if key.is_some() {
        KeySent::Yes
    } else {
        KeySent::No
    };
    Err(error_for_status(status, key_sent))
}

/// Whether a call sent a credential, spelled out as a type rather than a bare
/// `bool` (Task 3 review, item 3). `error_for_status`'s `key_sent` parameter
/// used to be a positional `bool`, with a single existing caller
/// (`list_models`) that always passed the right value in by construction —
/// `key.is_some()`, computed right next to the call — with nothing checking
/// that a *new* caller would too. A stray `false` at a new call site would
/// silently print "this endpoint now requires a key, and none was sent" on a
/// screen where the user has just typed one. `check_key` always sends a key,
/// so it always passes `KeySent::Yes`; naming it makes that fact readable at
/// the call site instead of a bare `true` that means nothing on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeySent {
    Yes,
    No,
}

/// The non-200 status table, pulled out of `list_models` (Task 2 review
/// round 3, H3): Task 3's key check answers the same statuses — always with
/// a key sent — and would otherwise copy this match arm for arm, carrying
/// two branches (`401 if !key_sent`, `403 if !key_sent`) it can never reach.
/// This repository has already paid for exactly that shape of copy: a change
/// in one task silently disarming a test case anchored in another, with
/// every gate staying clean. `list_models` and `check_key` are the only two
/// callers.
///
/// Never called for `200`: a 200 means something different to every caller
/// (a model list here, an account check elsewhere), so only the caller knows
/// what a 200 is worth. Written as "non-200" rather than "non-2xx" on
/// purpose (Task 3 review, item 6) — both callers branch on `status == 200`,
/// not on `is_success()`, so a `204` with no body reaches this function too,
/// and the old wording read as a promise that it would not.
fn error_for_status(status: u16, key_sent: KeySent) -> Error {
    // Four combinations, four true statements (Task 2 review round 2, G1).
    // 401 means the request was not authenticated: with no key sent, this
    // endpoint now requires one; with a key, that key was refused. 403 means
    // the request WAS authenticated (or none was needed) and still refused:
    // with a key, that key is probably not permitted to do this; with none,
    // something between this machine and the provider probably refused an
    // anonymous request — on a public endpoint that is most often a proxy or
    // a gateway, not an account.
    match status {
        401 if key_sent == KeySent::No => Error::KeyRequired,
        401 => Error::Unauthorised { reason: None },
        403 if key_sent == KeySent::Yes => Error::Forbidden { reason: None },
        403 => Error::AnonymousBlocked,
        429 => Error::RateLimited { reason: None },
        other => Error::Provider {
            status: other,
            reason: None,
        },
    }
}

/// Renders `reason` as a `": <text>"` suffix when the provider stated one,
/// or nothing when it did not — shared by every variant below that carries a
/// `reason` (Task 3 review round 1, Minor: `check_key`'s explanation used to
/// reach only `Unauthorised`, so a 403 or a 500 carrying the same
/// `{"error":{"message":…}}` shape dropped the one sentence that would have
/// told the user what to do; `check_key` now attaches it wherever the
/// provider sent one, and Task 4 inherits this helper instead of copying the
/// narrower version).
fn reason_suffix(reason: &Option<ProviderMessage>) -> String {
    reason
        .as_ref()
        .map(|r| format!(": {r}"))
        .unwrap_or_default()
}

/// **Three** facts, three sentences. Round 1 wrote two of them, and two is a
/// definition as surely as a name is (fix round 2, item 3): the pipeline that
/// fills this field has three outcomes, not two, because `status_error` calls
/// `ProviderMessage::from_provider_text` rather than `ProviderMessage::new`,
/// and `from_provider_text` never returns `None` — the least it produces is
/// `Text { text: "" }`.
///
/// - A name this build can show.
/// - A name that survived stripping as nothing at all: an id made only of
///   characters `unsafe_for_display` removes. Round 1's two-arm version read
///   "no model named , or it does not make embeddings" — a name shown, with no
///   name in it.
/// - A name withheld, because the id carried a run of the key's own characters
///   (`probe::contains_key_fragment`). `ProviderMessage`'s own `Display`
///   cannot serve this one: substituted into the first sentence it reads "no
///   model named the provider's explanation could not be shown safely" — a
///   sentence about an explanation, on an error that is about a name.
///
/// Neither of the last two is reachable from a real provider catalogue. Both
/// cost one arm.
fn no_such_model_sentence(model: &ProviderMessage) -> String {
    match model {
        ProviderMessage::Text { text } if text.as_str().is_empty() => {
            "no model by that name, or it does not make embeddings — and nothing was left of the \
             name once what cannot be shown was removed from it"
                .to_string()
        }
        ProviderMessage::Text { text } => {
            format!("no model named {text}, or it does not make embeddings")
        }
        ProviderMessage::Withheld => "no model by that name, or it does not make embeddings — \
                                      and the name itself could not be shown safely"
            .to_string(),
    }
}

/// What a call to the provider can fail with.
///
/// **No variant may carry the key**, and `tests/probe.rs` holds this to it by
/// running every failure path and searching the rendered message for the key
/// it was given. An error message is a log line, and a log line is a place a
/// key leaks from — the same sentence `src-tauri/src/error.rs` already carries.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the provider could not be reached: {0}")]
    Transport(String),
    /// 401 with a key sent: the request was authenticated, and refused.
    /// `reason` is the provider's own explanation, when the failed body said
    /// one and this build could read it (Task 3 review, item 4) — e.g. "the
    /// key was refused: This key was disabled on 2026-08-01" for a revoked
    /// key, instead of the bare sentence, which is true and useless on its
    /// own in that case. `None` when the body carried no such message, or
    /// none this build could parse; `list_models` passes `None`
    /// unconditionally today, since it never reads a failure body for a
    /// message. [`crate::ProviderMessage`] guarantees whatever it holds is
    /// safe to interpolate here — see its own doc comment for why, and see
    /// `reason_suffix` for the format shared with the other three variants
    /// below that carry the same field.
    #[error("the key was refused{}", reason_suffix(reason))]
    Unauthorised { reason: Option<ProviderMessage> },
    /// 401 on a call that sent no key at all (Task 2 review round 1, F2): not
    /// a credential problem, since there was no credential — the provider now
    /// requires one for an endpoint this build calls anonymously.
    #[error("this endpoint now requires a key, and none was sent")]
    KeyRequired,
    /// 403 with a key sent: the request was understood, the key is real, and
    /// it is not permitted to do this (Task 2 review round 2, G1) — a
    /// different fact from `Unauthorised`, which means the key itself was
    /// rejected outright. "Probably" (Task 2 review round 3, Minor): a key
    /// being present is a fact about this build's own request, not about who
    /// answered it — a corporate proxy in front of the provider can return
    /// 403 on its own, regardless of whether the key behind it was ever
    /// checked, and a bare "the key is not permitted" would be false about
    /// the actual cause in that case. `reason` — see `Unauthorised` — the
    /// provider's own explanation, when `check_key` could read one.
    #[error(
        "the key is probably not permitted to do this{}",
        reason_suffix(reason)
    )]
    Forbidden { reason: Option<ProviderMessage> },
    /// 403 on a call that sent no key at all (Task 2 review round 2, G1).
    /// Not naming an account: on a public, key-less endpoint this is most
    /// often something between this machine and the provider — a proxy or a
    /// gateway — refusing an anonymous request, which `Unauthorised` and
    /// `KeyRequired` would both misname as a credential problem when there
    /// was no credential to begin with. "Probably" (Task 2 review round 3,
    /// Minor), the opposite hedge from `Forbidden`: the provider itself can
    /// geo-block or otherwise refuse an anonymous request with no
    /// intermediary involved at all, so naming an intermediary as certain
    /// would be false in that case.
    #[error(
        "something between this machine and the provider probably refused an anonymous request"
    )]
    AnonymousBlocked,
    /// The model the user chose is not there, or does not make embeddings —
    /// a 404 from `/embeddings` (spec §2.6).
    ///
    /// `model` is a [`ProviderMessage`], not a `String` (fix round 1, item 3;
    /// review finding 3). The dispatch order that specified this variant held
    /// it safe "only because it is filled from the user's own model id", and
    /// that premise is false: the user *selects* from the provider's list, and
    /// `ModelEntry::id` (`catalogue.rs:56`) is copied verbatim out of the
    /// provider's own body (`catalogue.rs:412`) with nothing sanitising it on
    /// the way. This was the last variant interpolating an unbounded string
    /// through a plain format, and a newline inside a model id would cut a log
    /// line in half and let provider text pass for a separate entry.
    ///
    /// A `String` field holding a string that happens to have been sanitised
    /// would not close it: that is a store that only *promises* to be safe,
    /// with the promise living in a comment. This field cannot hold anything
    /// else, because `ProviderMessage`'s only constructor is the sanitising
    /// pipeline itself.
    #[error("{}", no_such_model_sentence(model))]
    NoSuchModel { model: ProviderMessage },
    /// Deliberately does not say "this key" (Task 2 review round 1, F2):
    /// `list_models`'s public list is called with no key at all, and
    /// anonymous rate limiting on a public endpoint is real — the sentence
    /// would be false exactly when the user is not signed in. `reason` — see
    /// `Unauthorised` — the provider's own explanation, when `check_key`
    /// could read one (a retry-after note, say).
    #[error("the provider is rate-limiting requests{}", reason_suffix(reason))]
    RateLimited { reason: Option<ProviderMessage> },
    /// `reason` — see `Unauthorised` — the provider's own explanation, when
    /// `check_key` could read one. The catch-all for every status the four
    /// variants above do not name, so this is also where an unanticipated
    /// status (a `5xx` this crate has no specific story for) keeps whatever
    /// the provider said about it.
    #[error("the provider answered {status}{}", reason_suffix(reason))]
    Provider {
        status: u16,
        reason: Option<ProviderMessage>,
    },
    #[error("the provider's answer was not the shape this code expects: {0}")]
    Malformed(&'static str),
    /// The answer's shape was right and its numbers are not coordinates this
    /// product can index (fix round 2, item 4).
    ///
    /// Split off from `Malformed` because that variant's own sentence states a
    /// cause: "the provider's answer was not the shape this code expects".
    /// For two rows of equal width whose components are simply unusable, the
    /// shape is exactly what this code expects, and the limit being hit is
    /// this build's own arithmetic — the provider is told it sent the wrong
    /// shape when it did not. A stated cause nobody asserted, in the crate
    /// that pays for those most.
    ///
    /// Both arms that reach this were written by this task: a non-finite
    /// component, and a squared length that overflows the arithmetic the index
    /// ranks with. Neither is a shape problem, and both used to say they were.
    #[error("the provider's answer is the right shape, but its numbers cannot be indexed: {0}")]
    UnusableVector(&'static str),
    /// A 200 whose body is not an embeddings answer at all, but the provider's
    /// own error envelope (fix round 1, item 2; review finding 2). A gateway —
    /// or the provider itself — answering `200` with
    /// `{"error":{"message":"quota exceeded"}}` used to read as `Malformed`'s
    /// "JSON, but not the shape this code expects": true, and it threw away the
    /// one sentence that says what to do about it. The same defect the status
    /// path had before `attach_reason`, on the 200 path instead.
    ///
    /// Scoped to `check_embedding_model`'s 200 path on purpose. The obvious
    /// alternative — giving `Malformed` a `reason` field — would reach
    /// `balance_from` and `models_from_json`, where a body that does not fit is
    /// deliberately not a problem worth surfacing, and would put an
    /// `Option<ProviderMessage>` that is `None` on almost every construction
    /// into two subsystems that never read a failure body at all.
    ///
    /// `reason` is not an `Option`, unlike the four variants above: this
    /// variant exists *because* a message was found and survived sanitising.
    /// A 200 with no readable message stays `Malformed`, which is the fact
    /// that was true about it all along.
    #[error("the provider answered 200 with an error instead of embeddings: {reason}")]
    ErrorInsteadOfEmbeddings { reason: ProviderMessage },
    /// The status was read successfully, but reading the rest of the
    /// response failed — kept apart from `Transport`, which means the
    /// opposite: no answer was ever read at all. `detail` is `ureq`'s own
    /// protocol-error text, not provider bytes.
    ///
    /// The top-level sentence says nothing about *why* the read failed on
    /// purpose (Task 2 review round 2, G2): a genuine truncation is the
    /// provider's connection stopping, but `ureq` raises the same error for
    /// `BodyExceedsLimit` (`read_to_string` caps at 10 MB) and for a global
    /// timeout during the body read, and in both of those the provider did
    /// nothing wrong — this build did. `detail` carries whichever of the
    /// three it actually was.
    ///
    /// This bypasses the status table in `list_models` entirely (Task 2
    /// review round 2, G3, deliberately not fixed here): a 401 whose body
    /// happens to be truncated arrives as `BodyUnreadable { status: 401, .. }`,
    /// not `Unauthorised`, and will not open a key dialog that keys off that
    /// variant. Reinterpreting the status inside the body-read failure would
    /// rebuild F1 — a status and a body-read outcome flattened back into one
    /// answer — so whoever matches on `Error` for that dialog must check
    /// `BodyUnreadable`'s `status` field too, not only `Unauthorised`.
    #[error("the provider answered {status}, but reading the response body failed: {detail}")]
    BodyUnreadable { status: u16, detail: String },
    /// The trap this whole subsystem exists to catch (spec §2.6): two texts in
    /// one request came back as a single vector, so the model averages a batch
    /// instead of embedding each text. Measured on Google's embedder,
    /// 2026-07-25, skeleton §6.2.
    ///
    /// Exactly one row, and nothing else (fix round 1, item 4): zero rows and
    /// more rows than texts are `Malformed`, and two identical rows are
    /// `IdenticalVectors` below. This sentence names a mechanism, and it is
    /// false about every one of those.
    #[error("this model returns one averaged vector for a batch, so it cannot embed an archive")]
    AveragedBatch,
    /// Two different texts came back as two copies of the same vector (fix
    /// round 1, item 4; review finding 4). Kept apart from `AveragedBatch`,
    /// whose sentence names a mechanism this does not observe: a model
    /// answering with a constant, or ignoring the second input, returned two
    /// vectors and averaged nothing. The consequence for the user is the same
    /// and the stated cause is not — and a support conversation started from
    /// the wrong cause is the class this crate has paid for most.
    ///
    /// Deliberately says what was seen rather than why. "A constant model"
    /// would be a third mechanism this build has not measured either: two
    /// identical answers to two texts do not establish what a third text would
    /// get.
    #[error(
        "this model answered two different texts with the same vector, so it cannot tell \
         documents apart"
    )]
    IdenticalVectors,
    #[error("the provider returned an empty vector")]
    EmptyVector,
}
