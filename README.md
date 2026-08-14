# Mnema desktop

A native desktop application that indexes a folder of documents on your own machine and answers
questions over them with cited sources — without the archive ever being stored anywhere but your
own disk.

**The walking skeleton stands; no subsystem is finished.** Eight crates and a Tauri shell compile
and are tested on macOS, Linux and Windows. The skeleton opens its database, applies the schema,
finds a row by lexical search, stores and queries a vector, binds to Pdfium, keeps a key in the OS
credential store, supervises a pool of extraction workers, and talks to a window.

Its purpose was to be falsified, and it was: **every task disproved part of its own plan.** A chunk
and its block could belong to different documents, so a citation named one file and pointed into
another's page. An apostrophe at a word's edge made the word unfindable by its own spelling. A
vector with an undefined distance sorted to rank 1 of every query. A filter naming exactly one
document silently returned nothing. None of that was visible from reading; all of it came from
compiling and running.

## The crates

| | |
| --- | --- |
| `mnema-core` | Types shared by every other crate, with no I/O of its own. |
| `mnema-index` | The only crate that knows SQL. SQLite with FTS5 and `sqlite-vec`. |
| `mnema-extract` | Document extraction, and the Pdfium binding. |
| `mnema-chunk` | Turns a page's blocks into the units that get embedded and searched. |
| `mnema-pool` | The supervised extraction pool: files handed to worker processes. |
| `mnema-ingest` | One file on disk becoming a citation someone can read. |
| `mnema-secrets` | Provider keys in the OS credential store, never in the database. |

## Building

`docs/BUILD.md` is the full account — what a build machine needs, what the package contains, how
signing works and what it does not yet do.

One step comes before `cargo test`:

```sh
scripts/fetch-pdfium.sh   # vendors a pinned, checksum-verified Pdfium into vendor/
cargo test --workspace
```

The library is not committed: it is 7.7 MB and reproducible from a pinned release, with a
per-platform checksum in that script. The build number it pins lives in six places, and
`crates/mnema-extract/tests/pdfium_binding.rs` reads five of them — the crate manifest, the fetch
script, two rows of `crates/mnema-extract/README.md` and one of `docs/BUILD.md` — and fails when
any of them disagrees with the sixth.

## A note on the comments

The comments carry more than usual, on purpose: they record what was measured and what a change
would cost, because most of the defects above were found by running rather than by reading.

Two kinds of reference in them point outside this repository. `§N` names a section of the design
document, and `D<n>` an entry in its decision ledger. That document is kept privately; the code
here is meant to stand without it, and a dangling reference is a citation, not a missing piece.

## Status and licence

This is unreleased software under active development. It extracts, indexes and cites; it does not
yet embed, search or answer, and there is no packaged release to install.

The source is published to be read. It is **not** open source: see [`LICENSE`](LICENSE). All rights
are reserved, and no permission to use, modify or redistribute is granted by publication here.

<!-- test PR: verifying automated PR review integration; not intended to merge -->
<!-- second check-in: confirming the review bot re-triggers on a follow-up commit -->
