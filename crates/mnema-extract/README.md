# mnema-extract

Document extraction. At present it holds one thing: a probe that proves the Pdfium
binding works. Extraction itself — reading order, hyphenation, tables, OCR fallback
— is the extraction subsystem's spec and is not decided here.

## The version pair

| | |
|---|---|
| `pdfium-render` | `=0.9.3` |
| crate feature selecting the C API | `pdfium_7881` |
| Pdfium binary, non-V8 | `chromium/7881` |
| the same number in code | `mnema_extract::PDFIUM_API_BUILD` |
| the same number in the fetcher | `scripts/fetch-pdfium.sh` |

The three rows above that carry a value are checked against the files they describe
— `tests/pdfium_binding.rs` fails if this table drifts from the pin. It is the first
thing anyone reads to learn which release to fetch, so it is the last place a stale
number should be able to survive.

The full four-part Pdfium version is not repeated here; it is in
`vendor/pdfium/VERSION` after a fetch, and only its `BUILD=` field means anything to
the bindings.

### Every place the build number lives

This list is the only count in the repository, and other files point here rather than
carrying a number of their own. A count kept in two places drifts exactly the way a
pin does, and it had: `scripts/fetch-pdfium.sh` said four, this file said five, and
the truth was six.

1. `src/pdfium_probe.rs`, `PDFIUM_API_BUILD` — the number itself. Nothing checks it,
   because everything else is derived from it.
2. `Cargo.toml`, the `pdfium_<n>` feature.
3. `scripts/fetch-pdfium.sh`, the `PDFIUM_BUILD=` assignment.
4. this README, the crate-feature row.
5. this README, the binary row.
6. `docs/BUILD.md`, the binary row — a packager reads that page and not this one.

Every one of 2–6 is read by a test in `tests/pdfium_binding.rs`, whole line, against
an expectation derived from 1. So a bump is six edits, plus the five checksums in the
fetch script, and it fights no test.

One further mention exists and is deliberately off this list: entry D35 of the design
document, which is kept outside this repository. It is a dated ledger entry, and it is
supposed to keep the number that was true when it was written. Anything else — a
comment, a paragraph, a mutation pattern — is a place the number can go stale
unnoticed, and belongs on this list or nowhere.

These six have to move together. `pdfium-render` generates bindings for one
specific Pdfium C API revision; loading a different build under them does not fail,
it reads structures whose layout has moved and returns plausible nonsense.

Each pair is held together by something that fails when they separate:

- **constant ↔ binary**, at load time. Pdfium exposes no runtime version of its
  own, so `verify_build` reads the `VERSION` manifest that ships in the release
  archive and refuses to bind unless `BUILD=` matches. A missing manifest is
  refused too — an unconfirmed build is the state the check exists to reject.
  Note the scope: `VERSION` is a sibling file, so this catches a vendored tree
  assembled from mismatched parts, not a library someone swapped by hand while
  leaving the manifest alone. Substitution is covered by the checksum the fetch
  script verifies before installing, which is provenance rather than a check on
  the bytes at load time.
- **script ↔ binary**, by the pinned SHA-256.
- **constant ↔ script**, **constant ↔ crate feature** and **constant ↔ this table**,
  by tests that read those files and match whole lines. The feature link needs one:
  a feature is a compile-time choice inside a dependency, so no `cfg!` here can
  observe it and nothing at run time can either. Before those tests existed, editing
  the feature alone to another revision left all thirteen tests green — bindings for
  one revision, binary from another, exactly the failure this crate is about.
  They match whole lines rather than searching for the number, because a comment or
  a table cell still naming the old build is what a person leaves behind while
  moving a pin — which would satisfy a substring search in precisely the situation
  the check exists for.

Nothing asserts what the number *is*. Moving all of them together is a version
bump, and a bump should not have to defeat a test; `PDFIUM_API_BUILD` is the single
place it is written down, and every other check derives from there.

Note the crate feature is named outright rather than left to the default
`pdfium_latest`. `pdfium_latest` is an alias that moves with each `pdfium-render`
release, so under it a crate bump would silently start demanding a different binary.
Named explicitly, a bump that drops the revision fails to compile instead.

## Getting the binary

The library is 7.7 MB and is not committed. Fetch it:

```
scripts/fetch-pdfium.sh
```

It downloads the pinned release into `vendor/pdfium/` (gitignored) and verifies the
archive against a recorded SHA-256 before installing. These are the archives it will
install, and no others:

| platform | archive | why it is pinned |
|---|---|---|
| macOS arm64 | `pdfium-mac-arm64.tgz` | the delivery target |
| macOS x86-64 | `pdfium-mac-x64.tgz` | the delivery target |
| Linux x86-64 | `pdfium-linux-x64.tgz` | CI builds there — see `docs/BUILD.md` |
| Linux arm64 | `pdfium-linux-arm64.tgz` | CI builds there — see `docs/BUILD.md` |
| Windows x86-64 | `pdfium-win-x64.tgz` | so a Windows machine can run the suite |

`tests/pdfium_binding.rs` fails if this table and the script disagree, in either
direction. That guard exists because the sentence it replaced had already gone
stale: it said only macOS was pinned, and went on saying it for as long as the two
Linux pins sat in the script beside `docs/BUILD.md` describing them.

Windows is not a delivery target — D3 puts it on the horizon — and arm64 Windows is
deliberately absent, because no arm64 Windows machine has run this script. Two
things differ on Windows and both are handled in the script rather than here: the
archive keeps its library in `bin/`, which is renamed to `lib/` on install so that
one vendored layout serves every platform, and the script needs Git Bash, MSYS2 or
Cygwin, since `cmd` and PowerShell cannot run it.

Adding a platform means adding its asset name and hash to the script, and its row
above.

The install is a rename of a finished tree, not an extraction into the live
directory, and the "already vendored" guard requires the library and not only the
manifest. Both are the same bug: `VERSION` precedes `lib/` in the archive, so an
interrupted extraction used to leave a manifest with no library — after which the
script reported "already vendored" and did nothing, while the Rust side failed and
told the user to run the script. A loop with no exit.

At run time the library is looked for in this order:

1. `$MNEMA_PDFIUM_LIB_DIR`
2. beside the running executable — where a bundled application will ship it
3. `vendor/pdfium/lib` in a development checkout, baked in at compile time

Static linking was the first choice and is not available: the prebuilt macOS
releases ship `lib/libpdfium.dylib` and no archive, so
`Pdfium::bind_to_statically_linked_library` has nothing to link against.

## Pdfium is not thread-safe, and the crate feature does not make it so

`pdfium-render`'s README says its `thread_safe` feature locks access behind a mutex
so that calls are "sequenced as if they were single-threaded". In 0.9.3 that feature
adds `Send` and `Sync` impls and nothing else; the only `Mutex` in the crate guards
its own page-index cache, not the FFI. The result is types the compiler will happily
share across threads and a library that then segfaults — which this crate's test
binary did, before `mnema-extract` added a lock of its own.

So `page_texts` serialises every entry into Pdfium, holding the guard for the whole
life of a document rather than per call, because the document handle is the thing
that must not be interleaved. PDF extraction is therefore sequential across the
process. Recovering that throughput means separate processes, which is Pdfium's own
recommendation and the extraction spec's decision to make.

`tests/pdfium_binding.rs::concurrent_probes_do_not_crash_the_process` is what keeps
the lock in place. It fails by killing the process, not by failing an assertion.

## Fixtures

`tests/fixtures/*.pdf` are built by `tests/fixtures/make_fixtures.py`, which writes
the PDF structure by hand so the files stay small and their content stays plainly
invented. Regenerate with:

```
python3 crates/mnema-extract/tests/fixtures/make_fixtures.py
```

It prints the non-whitespace character count per page; the tests assert those counts
exactly, so a change to the script means a change to the constants at the top of
`tests/pdfium_binding.rs`.
