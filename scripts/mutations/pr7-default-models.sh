# The default model set, and the one rule around it. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr7-default-models.sh
#
# Every case here is against `provider::fill_roles` and its helpers
# (`src-tauri/src/provider.rs`), the one substitution path `set_key` (through
# `on_configured`) and the boot share since provider-per-role Task 4.
# Seven cases and two named victims, because the behaviour has exactly two
# directions and each case belongs to one of them: the roles the index has no
# answer for are given this product's models, and the roles it does have an
# answer for are left alone.
#
# ⚠️ **The live check is deliberately NOT a case here, and that is a gap rather
# than an oversight.** `both_default_models_are_still_in_the_providers_catalogue`
# is `#[ignore]`d — it needs the network — and `mutation-check.sh` builds
# `cargo test -p <pkg> <args> -- --exact <test>` and owns the `--` itself, so no
# case can pass `--include-ignored`. An ignored test selected by `--exact`
# reports `0 passed`, which this harness prints as `BASELINE FAILURE: no test
# named …`: the case would fail as broken rather than as a killed mutant. What
# that check protects is a fact about the provider's catalogue, which no mutation
# of this repository can change in any case.

# The guard removed in the direction that costs something: a chat model somebody
# has already chosen is replaced by this product's default. Only the "model
# missing" half of the guard goes, so the role-is-on-this-provider half stands.
case_ "a chat model somebody chose must not be replaced" \
  src-tauri/src/provider.rs \
  's{            Some\(q\) => q == p && model_missing\(state, role\),}{            Some(q) => q == p,}' \
  '            Some(q) => q == p,' \
  mnema-desktop 'a_key_entered_over_models_that_are_already_chosen_replaces_neither' --test model_commands

# The other direction: the write is gone, so an index with no chat model is
# left with none. `model` is still read, so this is the write disappearing.
case_ "an index with no chat model must be given one" \
  src-tauri/src/provider.rs \
  's{    let _ = state\.with_index\(\|db\| db\.meta_set\(mnema_index::META_CHAT_MODEL, model\)\);}{    let _ = model;}' \
  '    let _ = model;' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The default adopted over a space that already has a model — the mutation the
# provider-per-role Task 4 brief names. The space here holds no vectors, so the
# adoption succeeds and repoints the index.
case_ "an embedding model already chosen must not be replaced" \
  src-tauri/src/provider.rs \
  's{        Ok\(Some\(model\)\) => p == ProviderChoice::OpenRouter \|\| model == LOCAL_EMBED_MODEL,}{        Ok(Some(_)) => adopt_default_embedding(state, p).is_ok(),}' \
  '        Ok(Some(_)) => adopt_default_embedding(state, p).is_ok(),' \
  mnema-desktop 'a_key_entered_over_models_that_are_already_chosen_replaces_neither' --test model_commands

# And its other direction: with no space the embedding role is never given a
# model, so a key entered on a fresh index leaves nothing able to embed.
case_ "an index with no embedding model must be given one" \
  src-tauri/src/provider.rs \
  's{        Ok\(None\) => adopt_default_embedding\(state, p\)\.is_ok\(\),}{        Ok(None) => false,}' \
  '        Ok(None) => false,' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The width, taken from a constant instead of from the provider's own answer.
# The mutant names a width the fixture's provider does not answer with.
case_ "the width must be the one the provider answered with" \
  src-tauri/src/provider.rs \
  's{            \(model, check\.dim as i64\)}{            (model, 1536)}' \
  '            (model, 1536)' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The two roles crossed: the right constant read for the wrong role.
case_ "the embedding role must get the embedding model, not the chat one" \
  src-tauri/src/provider.rs \
  's{            let model = crate::models::DEFAULT_MODELS\.embedding;}{            let model = crate::models::DEFAULT_MODELS.chat;}' \
  '            let model = crate::models::DEFAULT_MODELS.chat;' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The job slot, not taken at all: the default is adopted while a pass is
# reading `meta.active_space`, and the index is repointed under it.
#
# ⚠️ The narrower `let _ = …` mutant (claim and drop at once) survives, as it
# did before: no test here can start a job inside one call.
case_ "the slot must be taken before the default is adopted" \
  src-tauri/src/provider.rs \
  's{    let Ok\(_slot\) = adoption_slot\(state\) else \{\n        return;\n    \};}{    let _slot = ();}' \
  '    let _slot = ();' \
  mnema-desktop 'a_key_entered_while_a_job_runs_stores_the_key_and_moves_no_pointer' --test model_commands
