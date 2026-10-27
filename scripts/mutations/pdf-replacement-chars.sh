# Mutation cases for U+FFFD in PDF text (D186): a page of replacement
# characters is a page without text, and a run of them between words is one
# space. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pdf-replacement-chars.sh
#
# What is silent when it is wrong here is the index, not the reader: a page of
# U+FFFD stored as text is a chunk whose vector sits near every question the
# library cannot answer, and it wins those searches with an excerpt of nothing.
#
# `a_page_drawn_only_in_replacement_characters_is_skipped_like_a_scan` stands on
# two defences on purpose — the reader turns the run into a space before it
# counts, and the count leaves U+FFFD out — so no single mutant here reaches it.
# Each defence is killed through its own test instead: R1 by the probe, which is
# handed the raw page, and R2 by the stored text.

# R1. The count takes U+FFFD as text again. The reader is unaffected (its runs
# are spaces by then), so only the probe, which counts the page as pdfium
# returned it, can see this.
case_ "probe: U+FFFD is not counted as text" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{\.filter\(\|c\| !c\.is_whitespace\(\) && \*c != '"'"'\\u\{FFFD\}'"'"'\)}{.filter(|c| !c.is_whitespace())}' \
  '.filter(|c| !c.is_whitespace())' \
  mnema-extract 'the_probe_does_not_count_replacement_characters_as_text' --test pdf

# R1b. The same mutant against the boundary: 48 letters among a hundred U+FFFD
# are 48, and 47 are below the threshold.
case_ "probe: the threshold is met by letters, not by U+FFFD" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{\.filter\(\|c\| !c\.is_whitespace\(\) && \*c != '"'"'\\u\{FFFD\}'"'"'\)}{.filter(|c| !c.is_whitespace())}' \
  '.filter(|c| !c.is_whitespace())' \
  mnema-extract 'pdfium_probe::tests::replacement_characters_are_not_counted' --lib

# R2. The reader keeps the page as pdfium returned it.
case_ "pdf: a run of U+FFFD is not kept in the stored text" \
  crates/mnema-extract/src/pdf.rs \
  's{let raw = replacement_runs_to_spaces\(&raw\);}{let raw = Cow::<str>::Borrowed(raw.as_str());}' \
  'let raw = Cow::<str>::Borrowed(raw.as_str());' \
  mnema-extract 'a_run_of_replacement_characters_becomes_one_space' --test pdf

# R3. The run is dropped rather than spaced: `About the Authorsxxi`, one word
# the page does not show.
case_ "pdf: a run of U+FFFD leaves a space between the words" \
  crates/mnema-extract/src/pdf.rs \
  "s{out\\.push\\(' '\\);}{out.push_str(\"\");}" \
  'out.push_str("");' \
  mnema-extract 'a_run_of_replacement_characters_becomes_one_space' --test pdf

# R4. Every U+FFFD becomes its own space: twenty spaces where the leader dots
# were, and a run that is no longer one space.
case_ "pdf: a run of U+FFFD is one space, not one per character" \
  crates/mnema-extract/src/pdf.rs \
  's#if !in_run \{#if true {#' \
  'if true {' \
  mnema-extract 'a_run_of_replacement_characters_becomes_one_space' --test pdf
