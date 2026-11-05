# The grounding check and the library-language primitives — spec
# 2026-11-05-model-independent-search. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/grounding.sh
#
# What is here, by name:
#
#   the language rule     — a sentence is checked only against a passage in the
#                           answer's own language, never across languages
#   the stem              — an inflected form meets its base by its first five
#                           characters
#   the renumbering       — an opened conversation moves a grounding mark with
#                           the anchor it was on
#   the wiring            — a generated answer is checked against its own hits
#   the mark              — only the unsupported citation carries the mark
#   the library languages — the share floor, and no translation for a
#                           one-language library asked in that language
#
# Not here, and why: the translated eval rows are a measurement instrument whose
# merges `crates/mnema-eval/src/translated.rs` pins with unit tests; the
# product does not translate (spec §8).

case_ "grounding: a passage in another language is not checked" \
  crates/mnema-rag/src/grounding.rs \
  's~            if self::language\(text\) != Some\(language\) \{~            if false { // mutant: checks across languages~' \
  'if false { // mutant: checks across languages' \
  mnema-rag 'grounding::tests::a_sentence_in_another_language_than_its_passage_is_not_checked' --lib

case_ "grounding: words meet by their stem, not whole" \
  crates/mnema-rag/src/grounding.rs \
  's~\.take\(STEM_CHARS\)~.take(100) /* mutant: whole words */~' \
  '.take(100) /* mutant: whole words */' \
  mnema-rag 'grounding::tests::an_inflected_form_meets_its_base' --lib

case_ "grounding: an opened conversation renumbers the mark with its anchor" \
  src-tauri/src/bridge.rs \
  's~grounding: renumber_grounding\(grounding, &order\),~grounding: grounding.clone(), // mutant: mark on the old number~' \
  'grounding: grounding.clone(), // mutant: mark on the old number' \
  mnema-desktop 'bridge::tests::an_opened_conversation_keeps_the_mark_on_the_citation_it_was_on' --lib

case_ "grounding: a generated answer is checked against its own hits" \
  src-tauri/src/bridge.rs \
  's~hits\.get\(n\.wrapping_sub\(1\)\)\.map\(\|h\| h\.text\.as_str\(\)\)~None /* mutant: no passages */~' \
  'None /* mutant: no passages */' \
  mnema-desktop 'bridge::tests::a_generated_answer_carries_the_check_of_its_citations' --lib

case_ "grounding: only the unsupported citation is marked" \
  ui/src/launcher/Answer.svelte \
  "s~answer\.grounding\.kind === 'unsupported' \? answer\.grounding\.anchors : \[\]~answer.citations.map((c) => c.anchor) /* mutant */~" \
  'answer.citations.map((c) => c.anchor) /* mutant */' \
  src/launcher/Answer.test.ts 'only the citation that does not support its sentence is marked' runner=vitest

case_ "translate: a language at the share floor counts" \
  crates/mnema-rag/src/translate.rs \
  's~>= MIN_SHARE~> MIN_SHARE /* mutant */~' \
  '> MIN_SHARE /* mutant */' \
  mnema-rag 'translate::tests::a_rare_language_below_the_share_is_not_the_librarys' --lib

case_ "translate: a one-language library asked in that language is not translated" \
  crates/mnema-rag/src/translate.rs \
  's~\.is_none_or\(\|i\| i\.lang\(\) != \*only\)~.is_some_and(|i| i.lang() != *only) /* mutant */~' \
  '.is_some_and(|i| i.lang() != *only) /* mutant */' \
  mnema-rag 'translate::tests::translation_is_needed_only_where_it_can_find_something' --lib
