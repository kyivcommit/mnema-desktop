use mnema_search::Provider;

use crate::{EvalError, IndexedCorpus};

/// The model this harness measures with by default: the product's own,
/// recorded as D30 and verified live at this width (2026-07-25).
/// `MNEMA_EVAL_MODEL` overrides it, because B8 is still open.
pub const EVAL_MODEL: &str = "baai/bge-m3";

/// Not a secret — the name of the variable the key came from, so a lookup
/// against a real credential store finds nothing and fails loudly rather
/// than picking up somebody else's key by accident. `embed_corpus` takes no
/// parameter a caller could pass a real key through in its place.
const CREDENTIAL_REF: &str = "env:MNEMA_EVAL_KEY";

/// The product's own batch size (`src-tauri/src/embed_job.rs:55`) — this
/// harness measures the product, not a number of its own choosing.
const BATCH: usize = 32;

/// What the run that just completed did to the index.
/// Pinned by `the_space_is_built_at_the_width_the_provider_measured`.
#[derive(Debug, PartialEq, Eq)]
pub struct Embedded {
    pub model: String,
    pub width: i64,
    pub embedded: i64,
    pub total: i64,
}

/// The step `IndexedCorpus::build` deliberately does not take, so that every
/// fixture in this crate can stay silent about a provider: measures the
/// model's width, adopts it as the active space, and embeds every chunk.
/// Pinned by `a_corpus_that_was_only_walked_can_be_asked_after_it_is_embedded`.
pub fn embed_corpus(
    indexed: &IndexedCorpus,
    provider: &Provider,
    model: &str,
) -> Result<Embedded, EvalError> {
    let db = indexed.db();
    let check = mnema_provider::check_embedding_model(&provider.base, &provider.key, model)?;
    let dim = check.dim as i64;
    let space = db
        .adopt_embedding_model(model, dim, CREDENTIAL_REF, &mnema_chunk::chunker_hash())?
        .space_id;
    mnema_embed::run(
        db,
        &provider.base,
        &provider.key,
        BATCH,
        &|| false,
        &mut |_| {},
    )?;

    let embedded = db.embedded_chunk_count(space)?;
    let total = db.chunk_count()?;
    // Not `tally.failed != 0`: a corroborated refusal is one way a space
    // stays incomplete, not the only one — a chunk behind a document
    // `chunks_needing_embedding` skips because it is not yet `'indexed'`
    // never reaches the provider at all, so `failed` stays zero over a
    // space that is still not complete (`space_is_complete`'s own doc
    // comment, `mnema-index/src/space.rs:963-972`, argues this exact gap).
    // `a_partly_embedded_corpus_is_refused_rather_than_measured` cannot
    // tell the two predicates apart — both are true in the one scenario it
    // builds — so this comment carries the argument the test cannot.
    if !db.space_is_complete(space)? {
        return Err(EvalError::CorpusNotEmbedded { embedded, total });
    }

    Ok(Embedded {
        model: model.to_string(),
        width: dim,
        embedded,
        total,
    })
}
