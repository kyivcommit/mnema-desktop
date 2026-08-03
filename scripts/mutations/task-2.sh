# Mutation cases for Task 2: the reader manifest and the reader named in
# `Frame::Header`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-2.sh
#
# The manifest exists to answer one question — "was this file indexed by the
# reader that would take it today?" — and every way of getting it wrong is
# silent. A manifest that omits an extension, or names the wrong reader for it,
# or a header that reports a reader other than the branch that ran, all produce
# a working build and an index that never re-reads a file whose reader changed
# hands. There is no crash to notice, so each of those wrongs is broken here
# deliberately and required to turn a test red.
#
# The case that motivates the whole design is C1: `.html` is read as text
# today, and the manifest must SAY so by leaving it out rather than by
# accident. Task 10 adds an html reader and flips that assertion; C1 is what
# proves the assertion is load-bearing rather than decorative.

case_ "manifest: the --manifest branch must actually print the manifest" \
  crates/mnema-extract/src/bin/worker.rs \
  's~        println!\(\n            "\{\}",\n            serde_json::to_string\(&manifest::manifest\(\)\)\.expect\("the manifest serialises"\)\n        \);\n        return;~        return;~' \
  '"--manifest" {
        return;' \
  mnema-extract 'the_worker_states_which_reader_takes_each_extension' --test manifest

# Both directions of the html assertion. Listing html — even under the reader
# that really takes it — is the change Task 10 makes, and it must not pass
# unnoticed.
case_ "manifest: html listed (as text) is still a change the test must see" \
  crates/mnema-extract/src/manifest.rs \
  's~    Manifest \{\n        default: ReaderId::new\("text", TEXT_READER_VERSION\),~    by_extension.insert("html".to_string(), ReaderId::new("text", TEXT_READER_VERSION));\n    Manifest {\n        default: ReaderId::new("text", TEXT_READER_VERSION),~' \
  'by_extension.insert("html".to_string(), ReaderId::new("text", TEXT_READER_VERSION));' \
  mnema-extract 'the_worker_states_which_reader_takes_each_extension' --test manifest

case_ "manifest: the default reader is named 'text', not something else" \
  crates/mnema-extract/src/manifest.rs \
  's~default: ReaderId::new\("text", TEXT_READER_VERSION\)~default: ReaderId::new("plain", TEXT_READER_VERSION)~' \
  'default: ReaderId::new("plain", TEXT_READER_VERSION)' \
  mnema-extract 'the_worker_states_which_reader_takes_each_extension' --test manifest

# The manifest is a claim about `typing::identify`, and nothing in the type
# system holds it to that claim. This is the case that would catch a later task
# adding a reader and forgetting its entry — the failure mode that leaves every
# file of that format indexed by the old reader for ever.
case_ "manifest: an extension claimed for a reader identify does not use" \
  crates/mnema-extract/src/manifest.rs \
  's~ReaderId::new\("markdown", MARKDOWN_READER_VERSION\)~ReaderId::new("text", MARKDOWN_READER_VERSION)~' \
  'ReaderId::new("text", MARKDOWN_READER_VERSION)' \
  mnema-extract 'the_manifest_names_the_reader_that_identify_actually_picks' --test manifest

case_ "header: the reader named is the branch that ran" \
  crates/mnema-extract/src/bin/worker.rs \
  's~                reader: "text"\.to_string\(\),~                reader: "markdown".to_string(),~' \
  '                reader: "markdown".to_string(),
                reader_version: manifest::TEXT_READER_VERSION,' \
  mnema-extract 'a_header_names_the_reader_that_produced_it' --test manifest

# A version bumped in the manifest while the header keeps reporting the old
# one — the two are written in different crates and nothing but this test ties
# them together.
case_ "header: the version reported is the constant the manifest publishes" \
  crates/mnema-extract/src/manifest.rs \
  's~pub const TEXT_READER_VERSION: u32 = 1;~pub const TEXT_READER_VERSION: u32 = 2;~' \
  'pub const TEXT_READER_VERSION: u32 = 2;' \
  mnema-extract 'a_header_names_the_reader_that_produced_it' --test manifest

# `for_extension`, all three ways it can be wrong. The first two are the
# one-sided-assertion trap in implementation form: an implementation that always
# answers the default passes every miss case, and one that ignores the argument
# passes every hit case.
case_ "for_extension: always answering the default" \
  crates/mnema-core/src/manifest.rs \
  's~ext\.and_then\(\|ext\| self\.by_extension\.get\(ext\)\)\n            \.unwrap_or\(&self\.default\)~let _ = ext;\n        &self.default~' \
  'let _ = ext;
        &self.default' \
  mnema-core 'manifest::tests::an_extension_in_the_map_wins_and_everything_else_falls_to_the_default' --lib

case_ "for_extension: ignoring the extension and answering the first entry" \
  crates/mnema-core/src/manifest.rs \
  's~ext\.and_then\(\|ext\| self\.by_extension\.get\(ext\)\)\n            \.unwrap_or\(&self\.default\)~let _ = ext;\n        self.by_extension.values().next().unwrap_or(&self.default)~' \
  'self.by_extension.values().next().unwrap_or(&self.default)' \
  mnema-core 'manifest::tests::an_extension_in_the_map_wins_and_everything_else_falls_to_the_default' --lib

# The helpful-looking mistake: `identify_plain_text` matches `Some("md")`
# exactly, so a manifest that lowercased would claim the markdown reader for
# NOTES.MD while the worker reads it as text — and the parent would compare the
# document against a reader that never touched it.
case_ "for_extension: case-folding the extension the worker matches exactly" \
  crates/mnema-core/src/manifest.rs \
  's~ext\.and_then\(\|ext\| self\.by_extension\.get\(ext\)\)~ext.and_then(|ext| self.by_extension.get(&ext.to_lowercase()))~' \
  'self.by_extension.get(&ext.to_lowercase())' \
  mnema-core 'manifest::tests::a_differently_cased_extension_is_not_the_same_extension' --lib

# The shape the parent parses out of stdout is the interface, not a serde
# detail: it reads `by_extension.<ext>.reader`, and a renamed field breaks a
# process boundary rather than a call site the compiler can see.
case_ "manifest: the JSON field names the parent reads are part of the contract" \
  crates/mnema-core/src/manifest.rs \
  's~pub struct ReaderId \{\n    pub reader: String,~pub struct ReaderId {\n    #[serde(rename = "name")]\n    pub reader: String,~' \
  '#[serde(rename = "name")]' \
  mnema-core 'manifest::tests::a_manifest_round_trips_as_the_object_the_parent_reads' --lib

# `reader` is required, NOT `#[serde(default)]` — the opposite of the decision
# taken for `Refused::sha256`, and the reasoning is in the test's own doc
# comment. A default would record every document from a mismatched worker as
# made by the empty reader at version 0, which matches no manifest, so those
# files would be re-read on every run for ever. This case is what keeps a later
# session from "fixing" the strictness.
case_ "wire: a header with no reader must not parse as a default" \
  crates/mnema-core/src/wire.rs \
  's~        reader: String,\n        reader_version: u32,\n        pages: u32,~        #[serde(default)]\n        reader: String,\n        #[serde(default)]\n        reader_version: u32,\n        pages: u32,~' \
  '        #[serde(default)]
        reader: String,' \
  mnema-core 'wire::tests::a_header_from_a_worker_that_predates_the_reader_field_is_a_protocol_error' --lib
