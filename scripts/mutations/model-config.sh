# Mutation cases for the model configuration cycle. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/model-config.sh
#
# Every rule here is silent when broken. A model that truncates at 512 tokens
# still returns a vector; a model that answers two texts with two copies of one
# vector still returns two vectors; a space swapped out from under a filled
# index still searches — it just answers from an empty table. None of them
# crashes, so each is broken deliberately and required to turn a named test red.
#
# The cases are grouped by the rule they break, and several rules need more than
# one case because one mutation cannot show both directions of an assertion. A
# rule with only the refusing direction covered is satisfied by refusing
# everything; a rule with only the permitting direction is satisfied by
# permitting everything. Where both appear they are adjacent and say so.
#
# ── Rules of this cycle that are NOT below, and what holds each instead ──────
#
# They are here in comments rather than left out silently, because a case file
# read as a list of everything that is held would be read wrongly.
#
# - **The two probe texts differ, and one is ASCII while the other is not**
#   (`crates/mnema-provider/src/probe.rs`, the `const _: () = assert!` pins).
#   Breaking a `const` assertion is a compile error, and `mutation-check.sh`
#   classifies a mutation that does not compile as a BROKEN CASE — correctly,
#   since the named test never ran. A two-part mutation that disabled the pins
#   AND equalised the texts would compile; it would report STILL GREEN, which is
#   the accurate way to say this rather than "no case is possible". Task 6
#   measured exactly that: with the pins disabled and the texts made equal the
#   whole suite stayed green, because no test built on `mnema_mock_provider` can
#   see it — the mock answers with a canned body whatever it is sent. The pins
#   are stronger than a case would be.
#
# - **`model_settings` cannot lose half its answer.** The structural half is held
#   by a type and not by a test: `model_settings`, `index_settings` and
#   `key_state` return no `Result` at all, so there is no `?` for a caller to
#   write and no `Err` for one half to take the other out through. A mutation
#   that reintroduces the `Result` does not compile at the call site. What IS
#   testable is the other half — that each half still carries its own answer
#   rather than a summary or an empty state — and that is what the
#   `model_settings` section below breaks, one way per case.
#
# - **The window's own lists of these discriminants** — `REFUSALS` /
#   `BALANCES` / `RECORD_IDS` — were a hand-made copy of the Rust ones.
#   Nothing tied the two languages together, and tying them would need the
#   cross-language artefact D39 withdrew. The copy does not exist today: PR 1
#   deleted the shell that held it, and PR 7 owns the settings surfaces that
#   need it again. `mutation-check.sh` runs `cargo test` only, so a case could
#   not reach the window's suite either way. The Rust pin below is what stops
#   the build when a variant is added; a person still carries it across. The
#   same sentence disposes of Task 9's fifteenth — "the window does not claim
#   things about the provider it cannot know" — whose witness went with the
#   same deletion and is therefore out of this harness's reach too.
#
# - **`Error::Transport` must not carry the request.** Measured rather than
#   asserted (review round 1, F2): the neighbouring clause, "`to_string()`, never
#   `Debug`", cannot be broken into a leak, because no reachable `ureq::Error`
#   carries the key in either rendering — a refused connection gives
#   `Io(Custom { kind: ConnectionRefused, .. })` and a header value the `http`
#   crate rejects gives `Http(http::Error(InvalidHeaderValue))`, that crate
#   keeping the offending value out of its own `Debug` on purpose. Both were run
#   with a key in the `authorization` header. A case aimed at `{e:?}` would go
#   green for a reason that says nothing about this code. What holds the clause
#   that CAN leak — never the request — is the key-in-a-header case below, and
#   what holds the payload reaching the window at all is the transport case
#   beside it. The same section says which half of that rule could not be given
#   a case, and why.

# ─────────────────────────────────────────────────────────────────────────────
# The input floor.

case_ "the input floor stops refusing anything" \
  crates/mnema-provider/src/catalogue.rs \
  's~pub const MIN_CONTEXT_TOKENS: i64 = 2048;~pub const MIN_CONTEXT_TOKENS: i64 = 512;~' \
  'pub const MIN_CONTEXT_TOKENS: i64 = 512;' \
  mnema-provider 'a_model_that_takes_512_tokens_is_refused_and_says_both_numbers' --test catalogue

# The floor removed outright rather than lowered. Both, because a constant that
# is merely wrong and a comparison that is gone are different edits and only the
# second survives a later "the floor is not doing anything, delete it".
case_ "the input floor is not consulted at all" \
  crates/mnema-provider/src/catalogue.rs \
  's~                InputLimit::Known \{ tokens \} if \*tokens < MIN_CONTEXT_TOKENS => \{\n                    Some\(Refusal::InputTooSmall \{\n                        limit: \*tokens,\n                        floor: MIN_CONTEXT_TOKENS,\n                    \}\)\n                \}\n                InputLimit::Known \{ \.\. \} => None,~                InputLimit::Known { .. } => None,~' \
  '            Role::Embedding => match &input_limit {
                InputLimit::Known { .. } => None,' \
  mnema-provider 'a_model_that_takes_512_tokens_is_refused_and_says_both_numbers' --test catalogue

# The other direction of the same rule, and the expensive one: the floor is an
# EMBEDDING rule. A rerank model is sent a query and a document, not an archive,
# and applying the embedder's floor to it greys out models that work — a refusal
# nobody can act on, over a limit that does not apply.
case_ "the input floor is applied to rerank as well" \
  crates/mnema-provider/src/catalogue.rs \
  's~            Role::Embedding => match &input_limit \{~            Role::Embedding | Role::Rerank => match &input_limit {~; s~            Role::Chat \| Role::Rerank => None,~            Role::Chat => None,~' \
  '            Role::Embedding | Role::Rerank => match &input_limit {' \
  mnema-provider 'the_rerank_list_parses_and_the_input_floor_does_not_apply_to_it' --test catalogue

# ─────────────────────────────────────────────────────────────────────────────
# What a chat model must be able to do, and what may be claimed about it.

case_ "the chat rule ignores output_modalities" \
  crates/mnema-provider/src/catalogue.rs \
  's~            Role::Chat if !output_modalities_stated => Some\(Refusal::NoStatedOutputModalities\),\n            Role::Chat if !writes_text => Some\(Refusal::NoTextOutput\),\n            Role::Chat \| Role::Rerank => None,~            Role::Chat | Role::Rerank => None,~' \
  '            },
            Role::Chat | Role::Rerank => None,' \
  mnema-provider 'a_chat_model_that_does_not_write_text_is_refused' --test catalogue

# The cycle's own defect class, in the place it was first found here: "the
# provider did not say whether this model writes text" and "the provider said,
# and text was not among it" are opposite statements ABOUT THE PROVIDER. Folded
# into one, this build states as a fact about a model something nobody said.
case_ "not saying and saying no are the same refusal" \
  crates/mnema-provider/src/catalogue.rs \
  's~            Role::Chat if !output_modalities_stated => Some\(Refusal::NoStatedOutputModalities\),~            Role::Chat if !output_modalities_stated => Some(Refusal::NoTextOutput),~' \
  'Role::Chat if !output_modalities_stated => Some(Refusal::NoTextOutput),' \
  mnema-provider 'a_chat_model_with_no_stated_architecture_is_refused_for_not_saying_not_for_not_writing_text' --test catalogue

# Where the line "did the provider say?" is drawn. At `architecture` instead of
# at `output_modalities`, a provider who keeps the outer object and renames the
# inner field reads as "said, and text was not among it".
case_ "the line is drawn at architecture rather than at output_modalities" \
  crates/mnema-provider/src/catalogue.rs \
  's~        let output_modalities_stated = output_modalities\.is_some\(\);~        let output_modalities_stated = raw.architecture.is_some();~' \
  'let output_modalities_stated = raw.architecture.is_some();' \
  mnema-provider 'an_architecture_present_with_no_output_modalities_field_is_not_stated_either' --test catalogue

# ─────────────────────────────────────────────────────────────────────────────
# How many vectors came back, and what that is allowed to mean.
#
# The row count used to be `if parsed.data.len() != PROBE_TEXTS.len()`, one lump
# meaning "not two". It is a `match` with named arms now, because `AveragedBatch`
# is a claim about the provider — "this model returns one averaged vector for a
# batch" — true of exactly one row and false of every other count. One case per
# arm below, each breaking the NEW distinction; restoring the old lump is what
# each of them does, in its own direction.

case_ "one vector for two texts is named as something other than averaging" \
  crates/mnema-provider/src/probe.rs \
  's~        1 => return Err\(Error::AveragedBatch\),~        1 => {\n            return Err(Error::Malformed(\n                "the provider answered with a number of vectors this build does not understand",\n            ));\n        }~' \
  '        1 => {
            return Err(Error::Malformed(' \
  mnema-provider 'a_model_that_averages_a_batch_is_refused_by_name' --test probe

case_ "no vectors at all is reported as averaging a batch" \
  crates/mnema-provider/src/probe.rs \
  's~        0 => \{\n            return Err\(Error::Malformed\(\n                "two texts were sent and the provider answered with no vectors at all",\n            \)\);\n        \}~        0 => return Err(Error::AveragedBatch),~' \
  '        0 => return Err(Error::AveragedBatch),' \
  mnema-provider 'no_vectors_at_all_is_not_a_claim_that_the_model_averages_a_batch' --test probe

case_ "more vectors than texts is reported as averaging a batch" \
  crates/mnema-provider/src/probe.rs \
  's~        _ => \{\n            return Err\(Error::Malformed\(\n                "the provider answered with more vectors than the two texts this check sent",\n            \)\);\n        \}~        _ => return Err(Error::AveragedBatch),~' \
  '        _ => return Err(Error::AveragedBatch),' \
  mnema-provider 'more_vectors_than_texts_is_not_a_claim_that_the_model_averages_a_batch' --test probe

# ─────────────────────────────────────────────────────────────────────────────
# Two identical answers to two different texts.
#
# Counting the rows does not catch this: a model can return two copies of one
# answer. Every document in the archive would land on the same point and
# retrieval would be random, with no message anywhere.
#
# A pair, and the pairing is the point. The first says the answer is refused at
# all; the second says it is refused under its own name. `IdenticalVectors`
# exists because `AveragedBatch`'s sentence names a mechanism this build did not
# observe — a model answering with a constant returned two vectors and averaged
# nothing — so a case that only removes the guard would leave the naming free to
# collapse back.
case_ "two identical vectors are not refused at all" \
  crates/mnema-provider/src/probe.rs \
  's~    if first == second \{~    if false \&\& first == second {~' \
  '    if false && first == second {' \
  mnema-provider 'two_identical_vectors_for_two_different_texts_are_refused_too' --test probe

case_ "two identical vectors are refused as a batch this model averaged" \
  crates/mnema-provider/src/probe.rs \
  's~        return Err\(Error::IdenticalVectors\);~        return Err(Error::AveragedBatch);~' \
  '        return Err(Error::AveragedBatch);' \
  mnema-provider 'two_identical_vectors_for_two_different_texts_are_refused_too' --test probe

# ─────────────────────────────────────────────────────────────────────────────
# What "this space holds embeddings" counts.
#
# A vec0 table cannot be the target of a foreign key, so a vector outlives the
# chunk it embeds and the bookkeeping row that cascaded away with it: the two
# sources are allowed to disagree, and a check reading one of them is an
# assertion satisfied by zero from the wrong side. Both directions, adjacent.

case_ "emptiness is decided by the bookkeeping table alone" \
  crates/mnema-index/src/space.rs \
  's~                     SELECT chunk_id FROM \{table\}\n                 \)~                     SELECT chunk_id FROM {table} WHERE 0\n                 )~' \
  'SELECT chunk_id FROM {table} WHERE 0' \
  mnema-index 'a_space_is_empty_only_when_both_sources_say_so' --test adopt

case_ "emptiness is decided by the vector table alone" \
  crates/mnema-index/src/space.rs \
  's~                      WHERE space_id = \?1 AND state = 1~                      WHERE space_id = ?1 AND state = 1 AND 0~' \
  'WHERE space_id = ?1 AND state = 1 AND 0' \
  mnema-index 'bookkeeping_without_a_vector_also_makes_a_space_not_empty' --test adopt

# The arithmetic, which emptiness cannot see: zero and zero is zero however the
# two are combined, so `UNION` versus `UNION ALL` is invisible to both cases
# above and visible only in the number a refusal puts in front of a person
# deciding whether to pay to rebuild.
case_ "one chunk recorded in both places is counted twice" \
  crates/mnema-index/src/space.rs \
  's~                     UNION\n                     SELECT chunk_id FROM \{table\}~                     UNION ALL\n                     SELECT chunk_id FROM {table}~' \
  '                     UNION ALL' \
  mnema-index 'one_chunk_recorded_in_both_places_is_counted_once' --test adopt

# ─────────────────────────────────────────────────────────────────────────────
# The rule itself: every space except the requested one must be empty.
#
# Both directions again. Without the refusal the archive is left behind in a
# space search never reads; without the skip, adopting the model an index is
# already full of — the ordinary case, not a switch — is refused, and since
# adoption is the only path that writes `credential_ref`, the API key becomes
# unchangeable.

case_ "a space holding embeddings does not block the move" \
  crates/mnema-index/src/space.rs \
  's~            if embedded_chunks > 0 \{\n                return Err\(Error::SpaceNotEmpty \{\n                    space_id,\n                    embedded_chunks,\n                \}\);\n            \}\n~~' \
  '            };
        }
        Ok(())' \
  mnema-index 'a_different_model_is_refused_once_a_vector_exists' --test adopt

# ⚠️ The test named here was written for this case, because the obvious one was
# green. `the_same_model_is_still_accepted_once_vectors_exist` — the brief's
# candidate — never reaches this `continue` at all: its pointer is on the space
# it re-adopts, so `refuse_if_the_move_would_orphan_anything` exempts the call
# one level up and the rule is never asked. Measured, not reasoned: the mutation
# left the whole crate green until the witness below existed.
case_ "the space being adopted is counted against itself" \
  crates/mnema-index/src/space.rs \
  's~            if Some\(space_id\) == requested \{\n                continue;\n            \}\n~~' \
  '        for space_id in ids {
            let embedded_chunks = match self.embedded_chunk_count(space_id) {' \
  mnema-index 'the_model_a_space_is_full_of_can_be_adopted_even_with_nothing_pointing_at_it' --test adopt

# ─────────────────────────────────────────────────────────────────────────────
# Who the rule is asked of: the exemption, at each of its two call sites.
#
# `refuse_if_the_move_would_orphan_anything` is one function called twice — once
# before anything is written, once under the write lock — so a mutation of the
# function itself cannot tell the two sites apart. Each case below therefore
# inlines a weakened form of the exemption at ONE site and leaves the other
# whole. That is the only way to ask which site a given test is actually
# standing on, and Task 6 measured that it matters: weakening the exemption at
# the pre-flight alone left the entire crate green, because in all nine refusal
# tests of the day `requested` was `None` and no test had the shape "requested
# is `Some` and the call must still be refused".
#
# `requested.is_some()` is load-bearing and not defensive: without it two `None`s
# compare equal — a fresh index with no pointer, asking for a space that does not
# exist yet — and the check is skipped on precisely the call that mints the first
# space beside an already-full one.

case_ "pre-flight: the exemption no longer requires a space to have been named" \
  crates/mnema-index/src/space.rs \
  's~        self\.refuse_if_the_move_would_orphan_anything\(requested\)\?;~        if requested != self.active_space()? {\n            self.refuse_unless_every_other_space_is_empty(requested)?;\n        }~' \
  '        if requested != self.active_space()? {
            self.refuse_unless_every_other_space_is_empty(requested)?;
        }' \
  mnema-index 'a_full_space_blocks_a_switch_even_though_nothing_points_at_it' --test adopt

case_ "pre-flight: the exemption fires for any space that already exists" \
  crates/mnema-index/src/space.rs \
  's~        self\.refuse_if_the_move_would_orphan_anything\(requested\)\?;~        if requested.is_none() {\n            self.refuse_unless_every_other_space_is_empty(requested)?;\n        }~' \
  '        if requested.is_none() {
            self.refuse_unless_every_other_space_is_empty(requested)?;
        }' \
  mnema-index 'a_refusal_over_a_space_that_already_exists_still_writes_nothing' --test adopt

# The other direction at the same site: no exemption at all. This is the
# sanctioned migration's own middle — the new space built and filled while the
# old one is still there — refused.
case_ "pre-flight: no call is ever exempt" \
  crates/mnema-index/src/space.rs \
  's~        self\.refuse_if_the_move_would_orphan_anything\(requested\)\?;~        self.refuse_unless_every_other_space_is_empty(requested)?;~' \
  '        self.refuse_unless_every_other_space_is_empty(requested)?;

        let model_config_id = match existing_config {' \
  mnema-index 're_adopting_the_model_the_index_is_already_on_moves_nothing_and_is_allowed' --test adopt

# The decisive site, under the write lock. Its `requested` is `Some(space_id)`
# by construction, so dropping `requested.is_some()` HERE changes nothing at all
# — measured, see the report — and that half of the condition is held at this
# site by the shape of the argument rather than by any test. What is testable
# here is the equality half, whose loss disarms the decisive check completely.
case_ "decisive: the exemption fires for every call, so the check never runs" \
  crates/mnema-index/src/space.rs \
  's~            self\.refuse_if_the_move_would_orphan_anything\(Some\(space_id\)\)\?;~            if Some(space_id).is_none() {\n                self.refuse_unless_every_other_space_is_empty(Some(space_id))?;\n            }~' \
  '            if Some(space_id).is_none() {
                self.refuse_unless_every_other_space_is_empty(Some(space_id))?;
            }' \
  mnema-index 'a_vector_written_while_a_switch_is_deciding_still_refuses_it' --test adopt

case_ "decisive: no call is ever exempt" \
  crates/mnema-index/src/space.rs \
  's~            self\.refuse_if_the_move_would_orphan_anything\(Some\(space_id\)\)\?;~            self.refuse_unless_every_other_space_is_empty(Some(space_id))?;~' \
  '            self.refuse_unless_every_other_space_is_empty(Some(space_id))?;' \
  mnema-index 're_adopting_the_model_the_index_is_already_on_moves_nothing_and_is_allowed' --test adopt

# ─────────────────────────────────────────────────────────────────────────────
# The key: checked before it is stored, and never written down anywhere else.

case_ "the key is stored before it is checked" \
  src-tauri/src/models.rs \
  's~    let check = mnema_provider::check_key\(state\.provider_base\(\), &key\)\?;\n    mnema_secrets::store\(state\.credential_ref\(\), &key\)\?;~    mnema_secrets::store(state.credential_ref(), \&key)?;\n    let check = mnema_provider::check_key(state.provider_base(), \&key)?;~' \
  '    mnema_secrets::store(state.credential_ref(), &key)?;
    let check = mnema_provider::check_key(state.provider_base(), &key)?;' \
  mnema-desktop 'a_key_is_checked_before_it_is_stored' --test model_commands

# The property "check, then store" has that the case above cannot see: it starts
# from an empty store, so it is satisfied both by a refusal that stored nothing
# and by a refusal that first deleted what was there. Task 7 measured this
# reordering leaving the then-existing test green while destroying a working key
# on every mistyped attempt at a new one.
case_ "a refused key first forgets the one that was working" \
  src-tauri/src/models.rs \
  's~    let check = mnema_provider::check_key\(state\.provider_base\(\), &key\)\?;~    mnema_secrets::forget(state.credential_ref())?;\n    let check = mnema_provider::check_key(state.provider_base(), \&key)?;~' \
  '    mnema_secrets::forget(state.credential_ref())?;
    let check = mnema_provider::check_key(state.provider_base(), &key)?;' \
  mnema-desktop 'a_refusal_leaves_the_key_that_was_already_working' --test model_commands

# The key into the database, by the shortest realistic road: one argument over.
# The database travels to colleagues (D33) and the key must not travel with it.
# The mutation deliberately leaves `credential_ref` alone, so the scan's own
# positive control still passes and the red is the leak rather than the control.
case_ "the key is written into the index beside the reference" \
  src-tauri/src/models.rs \
  's~            &model,\n            dim,\n            state\.credential_ref\(\),~            \&key,\n            dim,\n            state.credential_ref(),~' \
  '            &key,
            dim,
            state.credential_ref(),' \
  mnema-desktop 'the_key_never_reaches_the_database_file' --test model_commands

# ─────────────────────────────────────────────────────────────────────────────
# The key in a message.
#
# A provider that rejects a malformed credential commonly echoes it back inside
# its own error text, and an error message is a log line. The scan and its
# positive half hold different things: an absence assertion on its own is
# satisfied by a build that shows nothing at all.
#
# The pipeline is strip → redact → fragment check, and **no case here breaks one
# stage into a leak**, deliberately. With the key at 23 characters and
# `FRAGMENT_LEN` at 12, the fragment net catches a whole key redaction missed and
# redaction catches what the net would have: two defences over one path. So the
# leak case bypasses the pipeline as a whole, and each stage's own contribution
# is broken separately below against the unit tests written for it — which is a
# different question from "does it leak", and the one a case can actually ask
# (review round 1, F3).
case_ "provider text reaches a message without going through the pipeline" \
  crates/mnema-provider/src/probe.rs \
  's~    fn from_provider_text\(raw: &str, key: &str\) -> Self \{~    fn from_provider_text(raw: \&str, key: \&str) -> Self {\n        return ProviderMessage::Text {\n            text: SanitisedText(raw.to_string()),\n        };~' \
  '        return ProviderMessage::Text {
            text: SanitisedText(raw.to_string()),
        };' \
  mnema-provider 'no_failure_path_puts_the_key_into_the_message' --test probe

# The positive half of the same property, and the reason it needs its own case:
# withholding the provider's whole sentence satisfies the leak scan perfectly.
# A key echoed back must be redacted, not swallowed — a support conversation
# still needs the rest of what the provider said.
case_ "redaction is a no-op, so the whole explanation is withheld instead" \
  crates/mnema-provider/src/probe.rs \
  's~fn redact_key\(text: &str, key: &str\) -> String \{\n    if key\.is_empty\(\) \{~fn redact_key(text: \&str, key: \&str) -> String {\n    if true {~' \
  'fn redact_key(text: &str, key: &str) -> String {
    if true {' \
  mnema-provider 'a_key_echoed_back_by_the_provider_is_redacted_not_dropped' --test probe

case_ "the key's place in the sentence is closed up rather than marked" \
  crates/mnema-provider/src/probe.rs \
  's~        result\.push_str\(REDACTED_PLACEHOLDER\);~        result.push_str("");~' \
  '        result.push_str("");' \
  mnema-provider 'a_key_echoed_back_by_the_provider_is_redacted_not_dropped' --test probe

# The fragment net, both directions, against the unit tests written for each.
# It is the stage `redact_key` cannot stand in for: whole-key redaction cannot
# catch a *run* of the key's own characters, and a net that fired on any short
# shared prefix would withhold every message this provider ever sends.
case_ "a surviving run of the key's characters no longer withholds the message" \
  crates/mnema-provider/src/probe.rs \
  's~fn contains_key_fragment\(text: &str, key: &str\) -> bool \{~fn contains_key_fragment(text: \&str, key: \&str) -> bool {\n    return false;~' \
  'fn contains_key_fragment(text: &str, key: &str) -> bool {
    return false;' \
  mnema-provider 'probe::tests::a_surviving_key_fragment_withholds_the_message_entirely' --lib

case_ "the fragment net withholds everything, including a short shared prefix" \
  crates/mnema-provider/src/probe.rs \
  's~fn contains_key_fragment\(text: &str, key: &str\) -> bool \{~fn contains_key_fragment(text: \&str, key: \&str) -> bool {\n    return true;~' \
  'fn contains_key_fragment(text: &str, key: &str) -> bool {
    return true;' \
  mnema-provider 'probe::tests::a_fragment_shorter_than_the_window_does_not_withhold' --lib

# The order of the first two stages, which is a Task 3 fix with a witness written
# precisely because the fragment net masks a plain revert of it. That is why the
# witness uses a key SHORTER than `FRAGMENT_LEN` — the net has nothing to catch —
# and why this case reddens it and nothing else.
case_ "redaction runs before stripping, so the key reassembles afterwards" \
  crates/mnema-provider/src/probe.rs \
  's~        let stripped: String = raw\.chars\(\)\.filter\(\|c\| !unsafe_for_display\(\*c\)\)\.collect\(\);\n        // 2\. Redact whole-key occurrences\.\n        let redacted = redact_key\(&stripped, key\);~        let redacted_first = redact_key(raw, key);\n        let redacted: String = redacted_first\n            .chars()\n            .filter(|c| !unsafe_for_display(*c))\n            .collect();~' \
  '        let redacted_first = redact_key(raw, key);' \
  mnema-provider 'probe::tests::strip_then_redact_matters_even_when_the_fragment_net_cannot_help' --lib

# ─────────────────────────────────────────────────────────────────────────────
# The one provider failure that is not about the provider's answer.
#
# `http.rs:104` builds `Error::Transport` from ureq's own text, `error.rs:144`
# carries that payload verbatim into `ProviderUnreachable`, and `Serialize` for
# that type is `serialize_str(&self.to_string())` — so it crosses to the window.
# Until review round 1 there was no test anywhere on that path: `http.rs`'s own
# five are about timeouts, trust roots and non-2xx bodies, and `error.rs` has no
# test module.

case_ "the transport error's own text is replaced by a summary" \
  crates/mnema-provider/src/http.rs \
  's~    let mut response = result\.map_err\(\|e\| Error::Transport\(e\.to_string\(\)\)\)\?;~    let mut response = result.map_err(|_| Error::Transport("the provider could not be reached".to_string()))?;~' \
  'Error::Transport("the provider could not be reached".to_string())' \
  mnema-desktop 'a_provider_that_never_answered_reaches_the_window_with_why_and_without_the_key' --test model_commands

# "Never the request" is the clause of that rule which can actually leak, and
# what holds it is that the key travels in a header and nowhere else.
case_ "the key travels in the query string of the list request" \
  crates/mnema-provider/src/http.rs \
  's~        \.get\(format!\("\{base\}\{path\}"\)\)~        .get(format!("{base}{path}\&key={}", key.unwrap_or_default()))~' \
  '.get(format!("{base}{path}&key={}", key.unwrap_or_default()))' \
  mnema-provider 'the_role_decides_the_query_and_the_key_travels_in_a_header' --test probe

# ⚠️ The same mutation on the POST side is deliberately NOT here, and the reason
# is a fact about that test rather than about the code. Its first assertion pins
# the whole request line — `starts_with("POST /embeddings ")`, with the trailing
# space — so any way the key can reach a request line also changes the path and
# trips that assertion first. Measured: the mutation reddens
# `the_model_check_posts_to_the_embeddings_endpoint_with_the_key_only_in_a_header`
# on the endpoint, not on the key, which is the false attribution this file is
# supposed to avoid rather than produce. The key-in-the-request-line half of that
# test cannot be witnessed on its own; the GET case above witnesses the class,
# because its first assertion pins one query parameter rather than the path.
#
# What IS witnessable there is the endpoint itself, and it is worth a case: the
# key is scoped to a header of a request to a URL, and a check posted somewhere
# else is a different request with the same credential on it.
case_ "the embedding check posts to an endpoint other than the one it names" \
  crates/mnema-provider/src/probe.rs \
  's~(pub fn check_embedding_model\(.*?)    let \(status, answer\) = match http::post_json\(base, "/embeddings", key, &request\) \{~$1    let (status, answer) = match http::post_json(base, "/embed", key, \&request) {~s' \
  'http::post_json(base, "/embed", key, &request)' \
  mnema-provider 'the_model_check_posts_to_the_embeddings_endpoint_with_the_key_only_in_a_header' --test probe

# ─────────────────────────────────────────────────────────────────────────────
# The settings screen: two facts, two answers.
#
# `model_settings` answers two questions — is there a key, and what does the
# index hold — and it has twice been the command where one half ate the other.
# The structural half of that fix is held by a type (see the header). What a type
# cannot hold is broken below: that each half still carries the answer it was
# given rather than a summary, that neither failure is drawn as an ordinary empty
# state, and that an index nobody opened is not filed as a read that failed.

case_ "a store that will not answer is reported as nobody having entered a key" \
  src-tauri/src/models.rs \
  's~        Err\(e\) => KeyState::Unreadable \{\n            cause: KeyStoreFailure::of\(&e\),\n            reason: e\.to_string\(\),\n        \},~        Err(_) => KeyState::Absent,~' \
  '        Err(_) => KeyState::Absent,' \
  mnema-desktop 'a_store_that_will_not_answer_does_not_take_the_index_with_it' --test model_commands

# The sentence, not only the discriminant. `!reason.is_empty()` stood here once
# and is satisfied by any string at all — the mutation that proved the index half
# carries its sentence verbatim stayed green on this side of the same struct.
case_ "the credential store's own sentence is replaced by a summary" \
  src-tauri/src/models.rs \
  's~            cause: KeyStoreFailure::of\(&e\),\n            reason: e\.to_string\(\),~            cause: KeyStoreFailure::of(\&e),\n            reason: "the credential store could not be read".to_string(),~' \
  '            reason: "the credential store could not be read".to_string(),' \
  mnema-desktop 'a_store_that_will_not_answer_does_not_take_the_index_with_it' --test model_commands

case_ "the index's own sentence is replaced by a summary" \
  src-tauri/src/models.rs \
  's~            cause: UnreadableCause::of\(&e\),\n            reason: e\.to_string\(\),~            cause: UnreadableCause::of(\&e),\n            reason: "the index could not be read".to_string(),~' \
  '            reason: "the index could not be read".to_string(),' \
  mnema-desktop 'a_key_that_is_there_survives_an_index_that_is_not' --test model_commands

# The harm Task 8 named when it made this command unable to reject anything: if
# an index that could not be read is drawn as an empty one, the defect goes
# invisible and a permanent wall — an index written by a newer Mnema — looks
# like an ordinary cold start.
case_ "an index that could not be read is answered as an empty one" \
  src-tauri/src/models.rs \
  's~        Err\(e\) => IndexSettings::Unreadable \{\n            cause: UnreadableCause::of\(&e\),\n            reason: e\.to_string\(\),\n        \},~        Err(_) => IndexSettings::Read(IndexRead {\n            embedding_model: None,\n            embedding_dim: None,\n            active_space: None,\n            embedded_chunks: 0,\n            total_chunks: 0,\n            rerank_model: None,\n            chat_model: None,\n        }),~' \
  '        Err(_) => IndexSettings::Read(IndexRead {' \
  mnema-desktop 'a_key_that_is_there_survives_an_index_that_is_not' --test model_commands

# An index nobody opened and an index that failed to open are two facts. The
# classifier is where they are told apart, and a classifier that answered
# `ReadFailed` to everything would satisfy two of that test's three assertions.
case_ "an index nobody opened is classified as a read that failed" \
  src-tauri/src/models.rs \
  's~            Error::IndexNotOpen => Self::NotOpen,~            Error::IndexNotOpen => Self::ReadFailed,~' \
  '            Error::IndexNotOpen => Self::ReadFailed,' \
  mnema-desktop 'models::tests::a_read_that_failed_is_told_apart_from_an_index_that_is_not_open' --lib

# ─────────────────────────────────────────────────────────────────────────────
# The spellings the window reads.
#
# `Refusal`, `Balance` and `RecordId` cross the IPC and a window looks each one
# up by its `kind` in a table with a fallback sentence, so a spelling that
# drifts renders as "this build did not recognise the reason" and reddens
# nothing. That window interpolates the payload fields as well — `inputTooSmall`
# reads `limit` and `floor`, `known` reads the record's `id` — which is why the
# pin compares the whole serialised value and why the last case below is a field
# rather than a variant. The table itself is PR 7's; the pin holds the list it
# will be written from.
#
# The pin's other guarantee — that a variant added one crate over stops the build
# — is not a case here, for the reason the header gives: a mutation that does not
# compile is a broken case, not a red one. It is carried by the `match` arms
# having no wildcard and no `..`, and precisely it stops the **test** target:
# those arms are inside `#[cfg(test)] mod tests`, so `cargo build` still
# succeeds and `cargo test` does not. That is the gate, so the guarantee holds
# where it is needed — but it is not `cargo build`.

case_ "Refusal's variants stop being spelled the way the window reads them" \
  crates/mnema-provider/src/catalogue.rs \
  's~    rename_all = "camelCase",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n\)\]\npub enum Refusal \{~    rename_all = "snake_case",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n)]\npub enum Refusal {~' \
  '    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Refusal {' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

case_ "RecordId's variants stop being spelled the way the window reads them" \
  crates/mnema-provider/src/catalogue.rs \
  's~    rename_all = "camelCase",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n\)\]\npub enum RecordId \{~    rename_all = "snake_case",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n)]\npub enum RecordId {~' \
  '    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RecordId {' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

case_ "Balance's variants stop being spelled the way the window reads them" \
  crates/mnema-provider/src/probe.rs \
  's~    rename_all = "camelCase",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n\)\]\npub enum Balance \{~    rename_all = "snake_case",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n)]\npub enum Balance {~' \
  '    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Balance {' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

# Not the spelling but the shape. Without `tag = "kind"` the discriminant stops
# being a field the window can read at all and becomes an outer object key, and
# every `?.kind` lookup on the far side quietly answers `undefined`.
case_ "Refusal stops carrying its discriminant in a kind field" \
  crates/mnema-provider/src/catalogue.rs \
  's~#\[serde\(\n    rename_all = "camelCase",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n\)\]\npub enum Refusal \{~#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]\npub enum Refusal {~' \
  '#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Refusal {' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

# And a payload field rather than a variant, which is the half a pin that read
# only `kind` left open (review round 1, F6): the discriminant stays correct and
# the window draws "input limit undefined tokens, at least undefined needed".
case_ "a Refusal payload field is renamed under a correct discriminant" \
  crates/mnema-provider/src/catalogue.rs \
  's~pub enum Refusal \{\n    InputTooSmall \{\n        limit: i64,~pub enum Refusal {\n    InputTooSmall {\n        #[serde(rename = "inputLimit")]\n        limit: i64,~' \
  '        #[serde(rename = "inputLimit")]
        limit: i64,' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

# ─────────────────────────────────────────────────────────────────────────────
# What the shipped binary links.
#
# ⚠️ Both cases below edit a manifest, and Task 7 measured twice that a manifest
# probe rewrites `Cargo.lock` while restoring the manifest does not restore the
# lock. That is safe here and only here: `mutation-check.sh` works in a
# throwaway worktree and restores it with `git checkout`. Do not run either
# mutation by hand in a working tree without checking `git status` afterwards.

# Deleting one word from a manifest leaves the whole gate green while every
# installation panics at the first key check: `RootCerts::PlatformVerifier`
# compiles to a `panic!` under `#[cfg(not(feature = "platform-verifier"))]`, and
# nothing in this workspace performs a TLS handshake. Loudness that first sounds
# in front of a user is silence to a gate.
case_ "trust roots stop coming from the machine" \
  Cargo.toml \
  's~ureq = \{ version = "3\.3", features = \["json", "platform-verifier"\] \}~ureq = { version = "3.3", features = ["json"] }~' \
  'ureq = { version = "3.3", features = ["json"] }' \
  mnema-desktop 'the_shipped_graph_takes_tls_roots_from_the_machine_and_carries_no_test_store' --test dependency_boundary

# The other half of the same test, and the first feature in this workspace whose
# wrong enabling loses keys silently: `test-store` compiles a credential store
# that reports durability and keeps nothing. It belongs to `[dev-dependencies]`;
# this is the assertion that says a normal build did not get it.
case_ "the shipped binary gets the credential store that keeps nothing" \
  src-tauri/Cargo.toml \
  's~mnema-secrets = \{ path = "\.\./crates/mnema-secrets" \}~mnema-secrets = { path = "../crates/mnema-secrets", features = ["test-store"] }~' \
  'mnema-secrets = { path = "../crates/mnema-secrets", features = ["test-store"] }' \
  mnema-desktop 'the_shipped_graph_takes_tls_roots_from_the_machine_and_carries_no_test_store' --test dependency_boundary

# ─────────────────────────────────────────────────────────────────────────────
# What the first live run found, and one finding folded in with it.
#
# None of the three below was caught by 840 tests, 45 mutation cases or eleven
# review rounds; a person pressing one button found all of them. They are the
# same defect class as everything above — two facts arrive and one message
# leaves — and the cases are here so the next edit that folds them back has
# something to go red.
#
# ⚠️ The window's half of these rules is out of this harness's reach, for the
# reason the header already gives about `REFUSALS` / `BALANCES` / `RECORD_IDS`:
# `mutation-check.sh` runs `cargo test` and nothing else. And they are unheld as
# well as unreachable: "a stated zero is never a promise that the model is
# free", "a number that cannot be a price is never rendered as one" and
# "nothing stated about the input limit reads differently from something
# unreadable" were assertions of the shell PR 1 deleted, and PR 7 owes them
# again. What the cases below hold is the Rust side: the facts reaching the
# window at all, and their spellings.

# The key, and the request that used to leave the machine for it. Pressing
# "Check and save" with the box empty handed the empty string to `check_key`,
# and the provider's "Missing Authentication header" came back as "the key was
# not saved: provider: the key was refused: …". Nobody had typed a key.
case_ "an empty key goes to the provider and its answer becomes a verdict on a key" \
  src-tauri/src/models.rs \
  's~    if key\.is_empty\(\) \{\n        return Err\(Error::EmptyKey\);\n    \}\n    let check = mnema_provider::check_key~    let check = mnema_provider::check_key~' \
  '-> Result<KeyStatus, Error> {
    let check = mnema_provider::check_key' \
  mnema-desktop 'an_empty_key_is_refused_here_rather_than_being_sent_and_reported_as_a_verdict_on_a_key' --test model_commands

# The half a fixed message does not fix, and the finding names them as two
# things: the sentence can be right while a pointless request still leaves the
# machine with an empty bearer token in it. This is the case that fails if the
# guard is ever moved below the call it exists to prevent.
case_ "the message is right and the request still leaves the machine" \
  src-tauri/src/models.rs \
  's~    if key\.is_empty\(\) \{\n        return Err\(Error::EmptyKey\);~    if key.is_empty() {\n        let _ = mnema_provider::check_key(state.provider_base(), \&key);\n        return Err(Error::EmptyKey);~' \
  '        let _ = mnema_provider::check_key(state.provider_base(), &key);' \
  mnema-desktop 'an_empty_key_is_refused_here_rather_than_being_sent_and_reported_as_a_verdict_on_a_key' --test model_commands

# And the fold this cycle is about, in its cheapest form: "you submitted
# nothing" and "the store holds nothing" are two facts, and the second is false
# about somebody who has a working key and an empty box.
case_ "an empty submission is reported as an empty credential store" \
  src-tauri/src/models.rs \
  's~        return Err\(Error::EmptyKey\);~        return Err(Error::NoKey);~' \
  '        return Err(Error::NoKey);' \
  mnema-desktop 'an_empty_key_is_refused_here_rather_than_being_sent_and_reported_as_a_verdict_on_a_key' --test model_commands

# The price. `-1` is what the provider sends for a model it prices at routing
# time; nothing rejected it, and the window multiplied it by a million and drew
# `$-1000000.000 per million tokens`.
case_ "a negative number is accepted as the price of a token" \
  crates/mnema-provider/src/catalogue.rs \
  's~        if amount\.is_finite\(\) && amount >= 0\.0 \{~        if amount.is_finite() {~' \
  '        if amount.is_finite() {' \
  mnema-provider 'the_states_a_price_arrives_in_are_not_folded_into_one_another' --test catalogue

# The other half of the same guard, and it needs its own case because the
# condition has two clauses and either one alone lets something through: a
# price stated as the string "inf" parses as an f64 and is not an amount of
# money. Both clauses, both directions.
case_ "a price that is not a finite number is accepted as one" \
  crates/mnema-provider/src/catalogue.rs \
  's~        if amount\.is_finite\(\) && amount >= 0\.0 \{~        if amount >= 0.0 {~' \
  '        if amount >= 0.0 {' \
  mnema-provider 'a_price_that_is_not_a_finite_number_is_not_a_price' --test catalogue

# The fold `Price` exists to undo, in the arm where it used to live: `"free"`
# and a record with no `pricing` block at all both reaching the window as "the
# provider did not state a price". This is N1 one field over, and the honesty
# question `flexible_f64`'s own doc comment recorded rather than answered.
case_ "a price this build cannot read is reported as one the provider never stated" \
  crates/mnema-provider/src/catalogue.rs \
  's~                Err\(_\) => Price::Unreadable \{\n                    raw: cap_raw\(s\.clone\(\)\),\n                \},~                Err(_) => Price::NotStated,~' \
  '                Err(_) => Price::NotStated,' \
  mnema-provider 'the_states_a_price_arrives_in_are_not_folded_into_one_another' --test catalogue

# I4, the finding Task 10 routed to the ledger. The mutation is the state this
# code was in: the input limit survives for the one role whose refusals already
# carry it, and for the other two "nothing was stated" and "stated in a shape
# this build cannot read" arrive as the same silence.
case_ "the input limit reaches the window only for the role that refuses over it" \
  crates/mnema-provider/src/catalogue.rs \
  's~        let input_limit = combined_limit\(&raw\.context_length, &top_provider_limit\);~        let input_limit = match role {\n            Role::Embedding => combined_limit(\&raw.context_length, \&top_provider_limit),\n            Role::Rerank | Role::Chat => InputLimit::NotStated,\n        };~' \
  '        let input_limit = match role {
            Role::Embedding => combined_limit(&raw.context_length, &top_provider_limit),' \
  mnema-provider 'a_limit_stated_unreadably_is_told_apart_from_no_limit_for_every_role' --test catalogue

# The spelling half, for the two unions the acceptance run added — the same case
# `Refusal`, `RecordId` and `Balance` already have above, and needed for the
# same reason: a window looks the `kind` up in a table and falls back to a
# sentence about not knowing, so a renamed variant reaches a person as that
# sentence and reddens nothing on its own.
case_ "Price's variants stop being spelled the way the window reads them" \
  crates/mnema-provider/src/catalogue.rs \
  's~    rename_all = "camelCase",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n\)\]\npub enum Price \{~    rename_all = "snake_case",\n    rename_all_fields = "camelCase",\n    tag = "kind"\n)]\npub enum Price {~' \
  '    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Price {' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

# And a payload field under a correct discriminant, which is the half F6 found
# open on `Refusal`: `InputLimit::Known`'s number is the one the picker prints,
# and renaming it draws "input undefined" with every `kind` still right.
case_ "InputLimit's token count is renamed under a correct discriminant" \
  crates/mnema-provider/src/catalogue.rs \
  's~    Known \{ tokens: i64 \},~    Known {\n        #[serde(rename = "contextLength")]\n        tokens: i64,\n    },~' \
  '        #[serde(rename = "contextLength")]
        tokens: i64,' \
  mnema-desktop 'models::tests::every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned' --lib

# Two rules the first review round found had no case, both about a decision that
# was written down and held by nothing.

# "Empty is refused here, blank is decided by the provider" is a deliberate line
# in `set_key`'s doc, and `trim()` passes every other test in the workspace. It
# states about a person who typed spaces that they typed nothing, which is the
# harm the doc names — and the same edit made before the check would store a
# credential other than the one entered.
case_ "a key of spaces is called nothing rather than sent" \
  src-tauri/src/models.rs \
  's~    if key\.is_empty\(\) \{~    if key.trim().is_empty() {~' \
  '    if key.trim().is_empty() {' \
  mnema-desktop 'a_key_of_spaces_is_decided_by_the_provider_rather_than_called_nothing' --test model_commands

# The direction the I4 case above cannot reach: the embedding role's own
# unknown states were held by its refusal alone for a round, so dropping the
# field for exactly that role — "the refusal says it anyway" — stayed green and
# drew a label whose two halves contradict each other.
case_ "the input limit is dropped for the one role whose refusal repeats it" \
  crates/mnema-provider/src/catalogue.rs \
  's~        let input_limit = combined_limit\(&raw\.context_length, &top_provider_limit\);~        let input_limit = match role {\n            Role::Embedding => InputLimit::NotStated,\n            Role::Rerank | Role::Chat => combined_limit(\&raw.context_length, \&top_provider_limit),\n        };~' \
  '        let input_limit = match role {
            Role::Embedding => InputLimit::NotStated,' \
  mnema-provider 'a_limit_stated_unreadably_is_told_apart_from_no_limit_for_every_role' --test catalogue

# ─────────────────────────────────────────────────────────────────────────────
# The whole-branch review's own findings.
#
# ⚠️ Only one half of I2 can have a case at all. Its window half — that every
# sentence in the model configuration block comes from one place, checked by a
# test that read the window's own source as text — went with the shell PR 1
# deleted, and `mutation-check.sh` runs `cargo test` and nothing else anyway,
# the same reach this file's header already names for `REFUSALS` / `BALANCES` /
# `RECORD_IDS`. PR 7 owes that half. What is below is I1, which is Rust on both
# of its halves.

# The button that reported an event it had not caused. `mnema_secrets::forget`
# is idempotent by design, so the deletion's own answer is the only place the
# two events are still distinguishable — and this is the arm that used to throw
# it away.
case_ "a deletion that found nothing reports it as a removal" \
  crates/mnema-secrets/src/lib.rs \
  's~        Err\(keyring_core::Error::NoEntry\) => Ok\(Forgotten::NothingToRemove\),~        Err(keyring_core::Error::NoEntry) => Ok(Forgotten::Removed),~' \
  '        Err(keyring_core::Error::NoEntry) => Ok(Forgotten::Removed),' \
  mnema-secrets 'tests::forgetting_says_whether_there_was_anything_to_forget' --lib

# And the same fold one layer out, where the fact reaches the window: an
# exhaustive `match` is perfectly happy to map both values to one.
case_ "the two answers to a deletion are folded on the way to the window" \
  src-tauri/src/models.rs \
  's~            mnema_secrets::Forgotten::NothingToRemove => Self::NothingToRemove,~            mnema_secrets::Forgotten::NothingToRemove => Self::Removed,~' \
  '            mnema_secrets::Forgotten::NothingToRemove => Self::Removed,' \
  mnema-desktop 'removing_a_key_that_is_not_there_says_so_rather_than_reporting_a_removal' --test model_commands

# The fix for a count trap wrote two more counts, in prose, held by nothing —
# `_mnema_note` inside the excerpt and the header of `tests/catalogue.rs`. The
# case breaks the file rather than the code, which is the only way this rule can
# be broken: nothing in `src/` reads `total_count`, and that is exactly why it
# went unchecked.
case_ "an excerpt of the provider's list claims to be the whole of it" \
  crates/mnema-provider/tests/fixtures/embeddings-2026-08-08.json \
  's~"total_count": 33~"total_count": 6~' \
  '"total_count": 6' \
  mnema-provider 'each_fixture_says_what_it_is_and_its_own_numbers_agree' --test catalogue
