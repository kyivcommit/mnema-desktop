# The default model set, and the one rule around it. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr7-default-models.sh
#
# Every case here is against `choose_the_default_models_for_roles_with_none`
# (`src-tauri/src/models.rs`), the whole of what this change added to `set_key`.
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
# has already chosen is replaced by this product's default. `if true`, so the
# write itself is untouched and only the condition in front of it is under test.
case_ "a chat model somebody chose must not be replaced" \
  src-tauri/src/models.rs \
  's{if db\.meta_get\(mnema_index::META_CHAT_MODEL\)\?\.is_none\(\) \{}{if true \{}' \
  'if true {' \
  mnema-desktop 'a_key_entered_over_models_that_are_already_chosen_replaces_neither' --test model_commands

# The other direction of the same line: the guard stands and the write behind it
# is gone, so an index with no chat model is left with none. `DEFAULT_MODELS.chat`
# is still read, so this is the write disappearing and not the constant.
case_ "an index with no chat model must be given one" \
  src-tauri/src/models.rs \
  's{db\.meta_set\(mnema_index::META_CHAT_MODEL, DEFAULT_MODELS\.chat\)\?;}{let _ = DEFAULT_MODELS.chat;}' \
  'let _ = DEFAULT_MODELS.chat;' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The embedding half's guard, removed the same way. An index already pointing at
# a space is repointed at this product's default, and the space it was on is left
# behind holding whatever it held.
case_ "an embedding model already chosen must not be replaced" \
  src-tauri/src/models.rs \
  's{    let unset = matches!\(state\.with_index\(\|db\| db\.active_space\(\)\), Ok\(None\)\);}{    let unset = true;}' \
  '    let unset = true;' \
  mnema-desktop 'a_key_entered_over_models_that_are_already_chosen_replaces_neither' --test model_commands

# And its other direction: the embedding role is never given a model at all, so
# a key entered on a fresh index leaves the product unable to embed anything.
case_ "an index with no embedding model must be given one" \
  src-tauri/src/models.rs \
  's{    let unset = matches!\(state\.with_index\(\|db\| db\.active_space\(\)\), Ok\(None\)\);}{    let unset = false;}' \
  '    let unset = false;' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The width, taken from a constant instead of from the provider's own answer —
# the mistake `set_embedding_model`'s own doc comment exists to prevent, made one
# function over. A literal that happened to match the fixture would survive, so
# the mutant names a width the fixture's provider does not answer with.
case_ "the width must be the one the provider answered with" \
  src-tauri/src/models.rs \
  's{            check\.dim as i64,}{            1536,}' \
  '            1536,' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The two roles crossed. One constant holds both ids precisely so they cannot
# drift apart, and this is the failure that remains possible once they cannot:
# the right constant read for the wrong role.
case_ "the embedding role must get the embedding model, not the chat one" \
  src-tauri/src/models.rs \
  's{        db\.adopt_embedding_model\(\n            DEFAULT_MODELS\.embedding,}{        db.adopt_embedding_model(\n            DEFAULT_MODELS.chat,}' \
  'db.adopt_embedding_model(
            DEFAULT_MODELS.chat,' \
  mnema-desktop 'the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for' --test model_commands

# The job slot, taken and dropped in the same statement — the `let _` that
# `set_embedding_model`'s own comment warns about, one function over. The claim
# still succeeds or fails exactly as before; what changes is that nothing is held
# while the adoption repoints `meta.active_space` under a pass that is reading it.
case_ "the slot must be held while the default is adopted, not merely claimed" \
  src-tauri/src/models.rs \
  's{    let Ok\(_slot\) = state\.claim_job\(\) else \{}{    let Ok(_) = state.claim_job() else \{}' \
  '    let Ok(_) = state.claim_job() else {' \
  mnema-desktop 'a_key_entered_while_a_job_runs_stores_the_key_and_moves_no_pointer' --test model_commands
