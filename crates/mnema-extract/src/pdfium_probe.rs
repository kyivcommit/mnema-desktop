use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mnema_core::nfc;
use pdfium_render::prelude::*;

/// Minimum characters on a page for its text layer to count as usable.
///
/// NOT a non-zero check: scanned pages in real archives routinely carry a stamp,
/// a Bates number or a scanner footer, and indexing such a page as though it had
/// content is the behaviour requirements §13 names as the worst available.
/// The value is a product decision and lives here so it can be cited. G7.0 §8.1.
pub const TEXT_LAYER_MIN_CHARS: usize = 48;

/// The Pdfium build number these bindings were compiled against.
///
/// `Cargo.toml` selects the `pdfium_<this number>` feature of `pdfium-render`,
/// which fixes the C API the bindings are generated from — spelled out rather than
/// repeated, so that moving the pin does not leave a stale number in prose.
/// Pdfium's own headers expose no runtime
/// version, so nothing can interrogate the loaded binary; what is checked instead
/// is the `VERSION` manifest vendored beside it — see [`verify_build`] for exactly
/// how far that reaches. Loading a build other than this one is not a graceful
/// degradation: struct layouts move between revisions, so the reads succeed and
/// return nonsense.
pub const PDFIUM_API_BUILD: u32 = 7881;

/// Environment override for the directory holding the Pdfium shared library.
/// Intended for packaging and for CI, not for everyday use.
pub const PDFIUM_LIB_DIR_ENV: &str = "MNEMA_PDFIUM_LIB_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageProbe {
    pub page_no: u32,
    pub char_count: usize,
    pub has_text_layer: bool,
}

/// Which step of loading Pdfium an [`Error`] surfaced from, as a value a caller
/// can match on rather than infer from prose.
///
/// It exists because `pdfium()` (below) runs three steps in sequence —
/// `library_dir`, `verify_build`, `bind_to_library` — and, before this type,
/// all three reported failure through the same `Error::Library(String)`
/// variant. `--probe-pdfium` (`src/bin/worker.rs`) answers exactly one
/// question, "can this build load Pdfium at all", and a caller reading only
/// its boolean `loaded` field could not tell "the .dylib is not where
/// expected" apart from "code signing refused to load it" — two failures with
/// completely different fixes. Measured on a real signed bundle: the first
/// failure hit was a missing `VERSION` manifest, not the code-signature
/// question the branch exists to answer; reading only `loaded` would have
/// recorded the wrong one of the two as settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// No shared library file was found at any of the three candidate
    /// locations [`library_dir`] checks.
    LibraryDir,
    /// The `VERSION` manifest beside the library was missing, unreadable, or
    /// named a build these bindings were not compiled for. Two different
    /// causes share this stage — a missing manifest and a build mismatch —
    /// because both are found inside [`verify_build`], before the library
    /// itself is ever asked to load; the full cause is still in `Error`'s own
    /// message.
    VerifyBuild,
    /// The library file was found and its `VERSION` verified, but
    /// `Pdfium::bind_to_library` itself failed. This is the shape a
    /// code-signing refusal takes: the file is present and named the right
    /// build, and the dynamic loader still declines it.
    Bind,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::LibraryDir => "library_dir",
            Stage::VerifyBuild => "verify_build",
            Stage::Bind => "bind",
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("pdfium: {0}")]
    Pdfium(String),

    /// The library, or the version manifest beside it, could not be found or
    /// read, or the library itself failed to bind. `stage` names which of the
    /// three — see [`Stage`] for why the distinction matters and
    /// [`Error::stage`] for how to read it without matching on this variant.
    #[error("pdfium library ({}): {message}", stage.as_str())]
    Library { stage: Stage, message: String },

    /// The shipped binary is not the build the compiled bindings describe.
    #[error(
        "pdfium build mismatch: the bindings are compiled for build {expected}, \
         the library at {path} reports build {found}. Reading structures across \
         that gap returns plausible nonsense rather than an error, so this \
         refuses to load. Run scripts/fetch-pdfium.sh."
    )]
    BuildMismatch {
        expected: u32,
        found: u32,
        path: String,
    },
}

impl Error {
    /// The stage this error surfaced from, as a string a wire consumer can
    /// match on directly rather than parse out of `Display`'s prose.
    /// `"document"` for [`Error::Pdfium`] — a fault in the file itself, after
    /// the library has already loaded — is not one of `Stage`'s three
    /// variants because it is not a step of *loading* Pdfium at all, but a
    /// probe reading this field must still get an answer rather than a gap.
    pub fn stage(&self) -> &'static str {
        match self {
            Error::Pdfium(_) => "document",
            Error::Library { stage, .. } => stage.as_str(),
            Error::BuildMismatch { .. } => Stage::VerifyBuild.as_str(),
        }
    }
}

/// Reports, per page, how much text the layer carries and whether that clears
/// the threshold. Extracting the text itself belongs to the extraction spec;
/// this exists so the skeleton proves the binding links and runs.
pub fn probe_text_layer(path: &Path) -> Result<Vec<PageProbe>, Error> {
    Ok(page_texts(path)?
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let count = text_layer_chars(&text);
            PageProbe {
                page_no: (i + 1) as u32,
                char_count: count,
                has_text_layer: has_text_layer(count),
            }
        })
        .collect())
}

/// How many characters of text layer a page carries, for the purpose of
/// [`TEXT_LAYER_MIN_CHARS`].
///
/// **NFC first, and that is not tidying.** The string a page's text becomes in
/// the index is `nfc::normalise`d (D32, D38), and normalisation changes the
/// character count: a Ukrainian `й` written decomposed is two characters before
/// it and one after. Counting the raw string would let a page of forty
/// decomposed letters count as eighty, clear the threshold, and be stored as
/// forty — below the minimum the threshold exists to enforce, in the alphabet
/// this product is built for.
///
/// Whitespace does not count, because a scanned page's furniture is a handful
/// of glyphs spread over a page and the amount of space between them says
/// nothing about how much text there is.
///
/// U+FFFD does not count either: it is pdfium's answer for a glyph whose font
/// maps to nothing usable, so it marks text that could not be read, not text.
/// A book whose prose font had a wrong `/ToUnicode` gave pages of two thousand
/// of them, every one over the threshold. `pdf::extract_pdf` has already turned
/// them into spaces before it counts; this is for the probe, which counts the
/// page as pdfium returned it.
pub(crate) fn text_layer_chars(text: &str) -> usize {
    nfc::normalise(text)
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{FFFD}')
        .count()
}

/// The threshold decision itself, taken from a count that
/// [`text_layer_chars`] produced.
///
/// One function rather than a comparison at each call site: `probe_text_layer`
/// publishes the answer as a diagnostic and `pdf::extract_pdf` acts on it by
/// dropping a page, and those two must not be able to disagree about which
/// pages of a document have a text layer. Splitting it from the counting is
/// what lets the boundary itself be asserted against the constant rather than
/// against a fixture — see `the_threshold_is_inclusive_at_the_constant`, which
/// is the only thing in this crate that can tell `>=` from `>`.
pub(crate) fn has_text_layer(char_count: usize) -> bool {
    char_count >= TEXT_LAYER_MIN_CHARS
}

/// The text of one page, read inside the box that page shows.
///
/// **`PdfPageText::all()` reads the wrong rectangle, and that is why this
/// function exists rather than a call to it.** `all()` is
/// `inside_rect(self.page.page_size())`
/// (`pdfium-render-0.9.3/src/pdf/document/page/text.rs:246-248`), and
/// `page_size()` is width×height starting at `(0,0)` — the box's own origin is
/// thrown away. A page whose MediaBox or CropBox starts away from the origin
/// therefore loses everything outside that rectangle: measured on a library of
/// books (D187), a document whose MediaBox and CropBox sit at
/// `130.5 279 661.5 945` and whose every line is above the `page_size()` top
/// came back empty page after page, each journalled as a scan, while documents
/// whose CropBox trims the right-hand end of a line lost those ends instead.
///
/// **An empty string is two answers.** It is "no character inside this box", and
/// it is "pdfium could not decode this page": `inside_rect` decodes the whole
/// UTF-16 buffer with `get_string_from_pdfium_utf16le_bytes(...)
/// .unwrap_or_default()` (`text.rs:309-310`), so **one lone surrogate empties
/// the entire page**. A real page of a book whose `/ToUnicode` has one wrong
/// glyph was journalled as a scan that way. The fallback asks the characters
/// directly — the only road left when the string is not there — and it stays
/// inside the same box, so the press marks and back covers that live outside the
/// visible page do not come back with it.
///
/// `pub(crate)` because the probe answers a packaging question about the same
/// pages: `probe_text_layer` and `pdf::extract_pdf` disagreeing about which
/// pages of a file carry text is one question with two answers.
pub(crate) fn page_text(page: &PdfPage) -> Result<String, PdfiumError> {
    let text = page.text()?;
    let bound = page_bound(page);

    let bounded = text.inside_rect(bound);
    if !bounded.is_empty() {
        return Ok(bounded);
    }

    Ok(text_from_code_points(chars_within(&text, bound)))
}

/// The code points `inside_rect` of `bound` would have written, in its order.
///
/// **`inside_rect` is a predicate over characters, and this is that predicate.**
/// `CPDF_TextPage::GetTextByPredicate` — which `FPDFText_GetBoundedText` runs —
/// does three things a plain filter does not, and each of them is a word on a
/// page a person can read:
///
/// - it keeps a character whose **tight** box **intersects** the bound, not one
///   whose loose box is contained by it, so a glyph crossing the edge is in the
///   page (`is_within` carries the measurement);
/// - it writes a space that lies *outside* the box when the character before it
///   was inside, because the space between two visible words is part of the
///   line even when its own box fell over the edge;
/// - it writes the `\r\n` it could not take from the file: the generated break
///   sits at the end of a line, and on a line cut by the box that end is
///   outside, so the break is added before the next character when the y
///   position moved. Without it a rescued page reads `…the Raveand the depot…`.
///
/// Measured, not ported from memory: `broken-utf16.pdf`'s cut line comes back
/// with exactly the string pdfium returns for the same geometry with the broken
/// glyph removed — the `\r\n` included — and `tests/pdf.rs` holds it.
fn chars_within(text: &PdfPageText, bound: PdfRect) -> Vec<u32> {
    let mut values = Vec::new();
    let mut pos_y = 0.0_f32;
    let mut after_text = false;
    let mut line_feed_due = false;
    for character in text.chars().iter() {
        let value = character.unicode_value();
        if is_within(&character, bound) {
            // pdfium keeps the y of the last break it wrote, not of the last
            // character, and sets it before its own emptiness guard — a page
            // whose first line is preceded by nothing but out-of-box characters
            // depends on both.
            let y = character.origin().map(|(_, y)| y.value).unwrap_or(pos_y);
            if y != pos_y && !after_text && line_feed_due {
                pos_y = y;
                if !values.is_empty() {
                    values.extend([u32::from(b'\r'), u32::from(b'\n')]);
                }
            }
            after_text = true;
            line_feed_due = false;
            // pdfium writes no character for a glyph whose code has no Unicode
            // at all (`GetTextByPredicate` appends only `if (charinfo.unicode())`),
            // and `char::from_u32(0)` would otherwise store a NUL in the index.
            if value != 0 {
                values.push(value);
            }
        } else if value == u32::from(b' ') {
            if after_text {
                values.push(u32::from(b' '));
                after_text = false;
                line_feed_due = false;
            }
        } else {
            after_text = false;
            line_feed_due = true;
        }
    }
    values
}

/// Whether this character is one that `inside_rect` of `bound` would have
/// written.
///
/// **Intersection, not containment, and the tight box rather than the loose
/// one** — measured against pdfium rather than reasoned from its name:
/// `CPDF_TextPage::GetTextByRect` is `IsRectIntersect(rect, charinfo.char_box())`,
/// so a glyph crossing the box's edge is part of the page as far as the fast
/// path is concerned. Held by a character drawn across the CropBox's left edge
/// in `broken-utf16.pdf`.
///
/// **A zero-size box is not inside**, and that is not a detail: the characters
/// pdfium generates — the `\r\n` at a line end, the spaces it invents in
/// justified text — have `CFX_FloatRect(origin, origin)` (`GenerateCharInfo`),
/// and an empty rectangle intersects nothing. They are written by
/// [`chars_within`]'s space and line-feed branches instead, which is the only
/// reason a page the fallback rebuilds keeps its spaces and its line breaks.
/// Treating a point inside the box as a character writes the break twice — a
/// blank line on every page whose written break is inside the box.
fn is_within(character: &PdfPageTextChar, bound: PdfRect) -> bool {
    character.tight_bounds().is_ok_and(|b| {
        // A zero-size box is not "overlapping": `PdfRect::does_overlap` compares
        // the four edges, so a point inside the bound passes it, while pdfium's
        // `IsRectIntersect` intersects and finds the result empty. Every
        // generated character is a point, so this is the arm that keeps them
        // out of the boundary test and in the space and line-feed branches.
        b.width().value != 0.0 && b.height().value != 0.0 && b.does_overlap(&bound)
    })
}

/// The box a page is read within: MediaBox ∩ CropBox with `/Pages` inheritance,
/// which is what a viewer clips to and what `pdfinfo` calls the page's boxes.
///
/// **`bounding()` is the only one of the three that is all three of those.**
/// Measured on `page-boxes.pdf`, whose boxes live on the `/Pages` node:
/// `crop()` and `media()` both answer `Err` — they read the page dictionary
/// only — while `bounding()` answers the CropBox. On the same file, a page whose
/// own `/CropBox` is `[0 0 0 0]` answers its MediaBox through `bounding()`, and
/// one whose CropBox is wider than its MediaBox answers the MediaBox — the two
/// clippings a viewer does. The crate's own doc calls `bounding()` "the smallest
/// rectangle that can enclose all the content", which is not what it answers
/// here; do not build on that sentence.
fn page_bound(page: &PdfPage) -> PdfRect {
    page.boundaries()
        .bounding()
        .map(|box_| box_.bounds)
        // Never seen to fail — the page tree is read when the page loads — but a
        // `Result` that is thrown away with `unwrap` would end the whole
        // document, and `page_size()` is the rectangle the reader used before.
        .unwrap_or_else(|_| page.page_size())
}

/// One page's text, built from its characters' code points.
///
/// A code point that is not a `char` is a lone UTF-16 surrogate, and there is no
/// character it could stand for: U+FFFD is what every decoder writes for one,
/// and D186's `replacement_runs_to_spaces` already decides what a run of them
/// means. Split out from [`page_text`] so that the mapping can be held on values
/// — without pdfium, and without a fixture that a machine cannot build.
fn text_from_code_points(values: impl IntoIterator<Item = u32>) -> String {
    values
        .into_iter()
        .map(|value| char::from_u32(value).unwrap_or('\u{FFFD}'))
        .collect()
}

/// The text layer of every page, in document order.
///
/// Kept internal: what a page's text *is* — reading order, hyphenation, column
/// handling — is the extraction spec's subject, and publishing this now would
/// freeze an answer that has not been decided. It exists as its own function so
/// the characters can be asserted directly in the tests below.
pub(crate) fn page_texts(path: &Path) -> Result<Vec<String>, Error> {
    let pdfium = pdfium()?;
    let _serialised = lock_pdfium();

    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| Error::Pdfium(e.to_string()))?;

    // By index over `FPDF_GetPageCount`, for the reason `pdf::extract_pdf`
    // spells out at length: `pages().iter()` swallows the first page
    // `FPDF_LoadPage` declines and ends there, so a short list is returned for
    // a long document with nothing to say it happened. Here that answer is what
    // `--probe-pdfium` reports to a packaging check — a probe that under-counts
    // pages says a bundle is fine when it is not — and the probe and the reader
    // must not disagree about which pages a document has.
    let all_pages = document.pages();
    let mut out = Vec::new();
    for index in 0..all_pages.len() {
        let page = all_pages
            .get(index)
            .map_err(|e| Error::Pdfium(format!("page {} could not be loaded: {e}", index + 1)))?;
        out.push(page_text(&page).map_err(|e| Error::Pdfium(e.to_string()))?);
    }
    Ok(out)
}

/// Serialises every entry into Pdfium, for the whole life of a document.
///
/// Pdfium is not thread-safe and its authors recommend parallel processes rather
/// than threads. `pdfium-render`'s `thread_safe` feature reads as though it covers
/// this — its README says access is "locked behind a mutex" so that calls are
/// "sequenced as if they were single-threaded" — but in 0.9.3 that feature adds
/// `Send`/`Sync` impls and nothing else. The only `Mutex` in the crate guards its
/// own page-index cache, not the FFI. Types that are `Send + Sync` without the
/// serialisation those bounds advertise is the worst of both: the compiler stops
/// objecting and the process segfaults instead. It does — `cargo test` on this
/// crate died with SIGSEGV before this lock existed, and
/// `tests/pdfium_binding.rs::concurrent_probes_do_not_crash_the_process` is what
/// holds the line.
///
/// The guard spans load-to-drop rather than each call, because a document handle
/// is the thing that must not be interleaved, not an individual function. That
/// makes PDF extraction sequential across the process. Recovering the throughput
/// means separate processes, which is the extraction spec's decision, not this
/// probe's.
///
/// `pub(crate)` so that `pdf::extract_pdf` serialises through **this** lock.
/// A second mutex would be the same segfault with two names: what has to be
/// exclusive is the FFI, not any one caller's use of it, and two locks each
/// held correctly protect nothing from each other.
pub(crate) fn lock_pdfium() -> std::sync::MutexGuard<'static, ()> {
    static PDFIUM_IN_USE: Mutex<()> = Mutex::new(());

    // The guarded value is `()`; there is no state for a panicking thread to have
    // corrupted on this side, and refusing every later document because one file
    // panicked would turn a single bad document into a dead indexing run.
    PDFIUM_IN_USE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The process-wide Pdfium handle.
///
/// `pdfium-render` keeps its bindings in a global cell and `Pdfium::new` asserts
/// that cell is empty, so a second construction panics and a second `bind_to_*`
/// returns `PdfiumLibraryBindingsAlreadyInitialized`. Binding once per process and
/// never dropping it is therefore the only correct shape — and it is what keeps a
/// second call from failing with an error about the library instead of about the
/// document. Holding it in a `static` is what the `thread_safe` feature's `Sync`
/// impl permits; what makes it *safe* is [`lock_pdfium`], not that feature.
///
/// **Every error this returns is the reader failing to come up, never a fact
/// about a document**, and `pdf::extract_pdf` relies on exactly that to route
/// it to `Frame::Failed` rather than to a content rule. It holds by
/// construction rather than by care: the closure below runs `library_dir`,
/// `verify_build` and `bind_to_library` and nothing else, and no file has been
/// named at this point, let alone opened.
pub(crate) fn pdfium() -> Result<&'static Pdfium, Error> {
    bound_pdfium().map(|(pdfium, _)| pdfium)
}

/// The directory the loaded library was loaded **from**, recorded by the call
/// that loaded it.
///
/// Not `library_dir()` called a second time. That would re-derive an answer
/// from the same inputs and agree with itself by construction, which is what a
/// packaging check must not accept: `library_dir` has three branches, one of
/// them an absolute path baked into the binary at compile time
/// (`CARGO_MANIFEST_DIR`), and a bundle that reached that branch is loading the
/// developer's checkout while looking identical from the outside. Measured on
/// this repository's own image: the bundled worker did exactly that, and the
/// only reason it was visible at all is that code signing then refused the
/// load. `scripts/verify-bundle.sh` compares this against the mounted image, so
/// it must be the path that was handed to the loader on the run that succeeded.
///
/// The scope is that and no more: this is the path `bind_to_library` was
/// given, not a reading of dyld's loaded-image table. The two can differ only
/// if the same library is already mapped in the process under that install
/// name, and the worker loads nothing else.
pub fn loaded_library_dir() -> Result<&'static Path, Error> {
    bound_pdfium().map(|(_, dir)| dir.as_path())
}

fn bound_pdfium() -> Result<&'static (Pdfium, PathBuf), Error> {
    static PDFIUM: OnceLock<Result<(Pdfium, PathBuf), Error>> = OnceLock::new();

    PDFIUM
        .get_or_init(|| {
            let dir = library_dir()?;
            let library = Pdfium::pdfium_platform_library_name_at_path(&dir);
            verify_build(&dir)?;
            let bindings = Pdfium::bind_to_library(&library).map_err(|e| Error::Library {
                stage: Stage::Bind,
                message: format!("{} could not be loaded: {e}", library.display()),
            })?;
            Ok((Pdfium::new(bindings), dir))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Where to look for the shared library, in order.
///
/// Static linking was the first choice and is not available: the prebuilt Pdfium
/// releases for macOS ship `lib/libpdfium.dylib` and no archive, so
/// `Pdfium::bind_to_statically_linked_library` has nothing to link against.
fn library_dir() -> Result<PathBuf, Error> {
    // 1. An explicit override. Packaging and CI use this.
    if let Some(dir) = std::env::var_os(PDFIUM_LIB_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }

    // 2. Inside a packaged application, which is checked BEFORE the flat layout
    //    below and not after. See [`bundled_library_dir`] for why the vendored
    //    layout is reproduced rather than flattened; the order is its own
    //    decision, and it is a security one.
    //
    //    The bundle grants `com.apple.security.cs.disable-library-validation`,
    //    because an ad-hoc signature has no Team ID and the loader otherwise
    //    refuses the library the bundle itself ships (D54, D65). That
    //    entitlement is not selective: it stops the signature of *any* library
    //    from being checked. With the flat branch first, a `libpdfium.dylib`
    //    dropped into `Contents/MacOS/` beside the worker would win over the
    //    sealed copy under `Resources/`, and be loaded without its signature
    //    being looked at. Checking the sealed location first does not make that
    //    impossible — nothing here can — but it stops the easiest spelling of it
    //    from being the one the search prefers.
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
        && let Some(dir) = bundled_library_dir(exe_dir)
    {
        return Ok(dir);
    }

    // 3. Beside the running executable — a flat install, and the shape a
    //    relocated worker takes. It stays because it is the only branch needing
    //    neither a bundle layout nor an environment, and because a packaged
    //    build no longer reaches it: (2) matches first.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.join(Pdfium::pdfium_platform_library_name()).is_file()
    {
        return Ok(dir.to_path_buf());
    }

    // 4. The vendored copy in a development checkout. Baked in at compile time
    //    because a library cannot ask where the workspace was.
    //
    //    A packaged build must never reach this branch, and "must never" is not
    //    "cannot": before Pdfium was put into the bundle, a signed image on the
    //    machine that built it reached exactly here and loaded the checkout —
    //    which is why `loaded_library_dir` exists and why
    //    `scripts/verify-bundle.sh` asserts on it rather than on this comment.
    let vendored = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/pdfium/lib"
    ));
    if vendored
        .join(Pdfium::pdfium_platform_library_name())
        .is_file()
    {
        return Ok(vendored.to_path_buf());
    }

    Err(Error::Library {
        stage: Stage::LibraryDir,
        message: format!(
            "no {} found. Set {PDFIUM_LIB_DIR_ENV}, or run scripts/fetch-pdfium.sh to \
             vendor Pdfium build {PDFIUM_API_BUILD}.",
            Pdfium::pdfium_platform_library_name().to_string_lossy(),
        ),
    })
}

/// Where a packaged macOS application keeps the library, derived from the
/// directory its executables run in — `Contents/MacOS` becomes
/// `Contents/Resources/pdfium/lib`.
///
/// **The vendored layout is reproduced, not flattened, and that is the whole
/// design.** `verify_build` requires a `VERSION` manifest in the library's
/// directory or its parent, so `pdfium/lib/libpdfium.dylib` puts the manifest
/// at `pdfium/VERSION` — the same two paths `scripts/fetch-pdfium.sh`
/// installs. Flattening the library into `Contents/Resources` would leave the
/// manifest with nowhere to sit that is not shared with every other resource.
/// `Contents/Frameworks` — the conventional home for a `.dylib` — was measured
/// and rejected for the same reason: Tauri's `bundle.macOS.frameworks` accepts
/// only `.dylib` and `.framework` entries, so the manifest cannot go there at
/// all (docs/BUILD.md).
///
/// Returns `None` unless the library is really there, so that a caller cannot
/// mistake "this is where it would be" for "this is where it is".
fn bundled_library_dir(exe_dir: &Path) -> Option<PathBuf> {
    let dir = exe_dir
        .parent()?
        .join("Resources")
        .join("pdfium")
        .join("lib");
    dir.join(Pdfium::pdfium_platform_library_name())
        .is_file()
        .then_some(dir)
}

/// Refuses to go on unless the `VERSION` manifest vendored beside the library
/// announces the build the bindings were compiled for.
///
/// Read the scope narrowly. The manifest is a sibling file, not the library, so
/// this detects a vendored tree **assembled from mismatched parts** — a stale
/// `vendor/` after the pin moved, a manually dropped-in release, two archives
/// unpacked over each other. It does not detect a *substituted* library: replace
/// the `.dylib` by hand and leave `VERSION` untouched and this binds a wrong build
/// in silence.
///
/// That gap is not closable here. Pdfium exposes no runtime version of its own —
/// there is no symbol to ask. What covers substitution is the SHA-256 the fetch
/// script verifies before installing, which is a check on provenance rather than
/// on the bytes at load time.
///
/// The manifest is required, not optional. Treating a missing `VERSION` as "assume
/// it matches" would put the check back where it started: the failure this guards
/// against is silent, so the guard cannot be.
fn verify_build(library_dir: &Path) -> Result<(), Error> {
    // `VERSION` sits at the root of the release archive, one level above `lib/`;
    // a packaged application that ships the library flat keeps it alongside.
    let candidates = [
        library_dir.join("VERSION"),
        library_dir.join("..").join("VERSION"),
    ];
    let manifest = candidates
        .iter()
        .find(|p| p.is_file())
        .ok_or_else(|| Error::Library {
            stage: Stage::VerifyBuild,
            message: format!(
                "no VERSION manifest beside {}. The build of Pdfium cannot be \
             confirmed, and an unconfirmed build is the failure this check exists \
             for. Run scripts/fetch-pdfium.sh.",
                library_dir.display()
            ),
        })?;

    let contents = std::fs::read_to_string(manifest).map_err(|e| Error::Library {
        stage: Stage::VerifyBuild,
        message: format!("{} could not be read: {e}", manifest.display()),
    })?;

    build_from_version_manifest(&contents, &manifest.display().to_string()).map(|_| ())
}

/// Parses `BUILD=` out of a Pdfium release `VERSION` manifest and checks it against
/// [`PDFIUM_API_BUILD`].
fn build_from_version_manifest(contents: &str, path: &str) -> Result<u32, Error> {
    let found = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("BUILD="))
        .ok_or_else(|| Error::Library {
            stage: Stage::VerifyBuild,
            message: format!("{path} declares no BUILD= line"),
        })?;
    let found: u32 = found.trim().parse().map_err(|e| Error::Library {
        stage: Stage::VerifyBuild,
        message: format!("{path} has an unreadable BUILD= line: {e}"),
    })?;

    if found == PDFIUM_API_BUILD {
        Ok(found)
    } else {
        Err(Error::BuildMismatch {
            expected: PDFIUM_API_BUILD,
            found,
            path: path.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn the_characters_of_the_text_layer_survive_the_binding() {
        // The counting tests next door would pass on 119 characters of garbage.
        // This is the assertion that the binding actually reads the page: the
        // exact sentences drawn by tests/fixtures/make_fixtures.py, back again.
        let pages = page_texts(&fixture("text-then-stamp.pdf")).expect("the fixture parses");

        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0],
            "Invented contract 4417 between Northwind Depot and Ravella Freight,\r\n\
             signed 2026-07-25, covering pallet haulage for one calendar quarter."
        );
        assert_eq!(pages[1], "Page 2 of 2");
    }

    /// The one character-level rule the fallback rests on, held without pdfium.
    ///
    /// A lone surrogate is not a `char` and there is none it could stand for;
    /// U+FFFD is what a decoder writes, and D186's `replacement_runs_to_spaces`
    /// already decides what a run of them means. The ordinary letter and the
    /// astral character pass through — a mapping that gave up on everything
    /// above U+FFFF would redden this line.
    #[test]
    fn a_value_that_is_not_a_character_becomes_u_fffd_and_the_others_survive() {
        assert_eq!(
            text_from_code_points([0x41, 0xD800, 0x20, 0x1F600, 0xDC00, 0x5A]),
            "A\u{FFFD} \u{1F600}\u{FFFD}Z"
        );
    }

    /// The fallback and the fast path are one predicate with two spellings, and
    /// this holds them to the same string.
    ///
    /// [`page_text`] runs [`chars_within`] only when the bounded decode came
    /// back empty, and that condition is what makes "run it every time" an
    /// *equivalent* mutant rather than a survivor (task A's report). The
    /// equivalence was measured once, by hand, on these fixtures; a later
    /// simplification of the y rule, the space branch or the line-break rule
    /// could diverge on a good page's geometry and nothing would go red. This is
    /// that measurement as a guard: on every page whose bounded read decodes,
    /// the code points `chars_within` selects are exactly the string
    /// `inside_rect` returns — the breaks it synthesises and the spaces whose own
    /// box fell outside the bound included. It also pins both cut-line
    /// geometries: `offset-cropbox.pdf` page 2's line ends inside the box, and
    /// the fixtures' moved boxes are the shape a containment filter gets wrong.
    ///
    /// **Only pages whose bounded read is non-empty are compared, and that is
    /// the premise rather than a shortcut.** On `broken-utf16.pdf` pages 2 and 3
    /// one lone surrogate empties pdfium's whole UTF-16 buffer, so
    /// `inside_rect` answers `""` about a page that carries text; the fallback is
    /// the only answer there is, and comparing the two would assert the defect
    /// this test exists to prevent.
    #[test]
    fn the_fallback_computes_what_the_bounded_read_computes_on_every_decodable_page() {
        // Every PDF beside this file but the three with no page this test can
        // read: a locked document, one with no pages, and one whose middle page
        // `FPDF_LoadPage` declines (that last one has its own test, `tests/pdf.rs`
        // `a_page_that_will_not_load_refuses_the_document_rather_than_ending_it`).
        const UNREADABLE: [&str; 3] = [
            "password-locked.pdf",
            "no-pages.pdf",
            "unloadable-middle-page.pdf",
        ];

        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("the fixture directory is beside this file")
            .map(|entry| entry.expect("a readable directory entry").file_name())
            .filter_map(|name| name.to_str().map(str::to_string))
            .filter(|name| name.ends_with(".pdf") && !UNREADABLE.contains(&name.as_str()))
            .collect();
        names.sort();

        let pdfium = pdfium().expect("the vendored library loads");
        let _serialised = lock_pdfium();
        let mut compared = 0_usize;
        for name in &names {
            let document = pdfium
                .load_pdf_from_file(&dir.join(name), None)
                .unwrap_or_else(|e| panic!("{name} parses: {e}"));
            let pages = document.pages();
            for index in 0..pages.len() {
                let page = pages
                    .get(index)
                    .unwrap_or_else(|e| panic!("{name} page {} loads: {e}", index + 1));
                let text = page.text().expect("the page's text layer reads");
                let bound = page_bound(&page);
                let bounded = text.inside_rect(bound);
                if bounded.is_empty() {
                    continue;
                }
                compared += 1;
                assert_eq!(
                    text_from_code_points(chars_within(&text, bound)),
                    bounded,
                    "{name} page {}: the fallback must be this predicate, not an \
                     approximation of it",
                    index + 1
                );
            }
        }
        // A literal rather than a count taken from the pages themselves, for the
        // reason `tests/pdf.rs::every_page_of_a_document_is_either_read_or_named`
        // gives for its page counts: a number derived from the reader moves with
        // the reader. 1 + 2 + 3 + 3 + 3 + 1 + 3 + 3 over the eight fixtures that
        // load, with `broken-utf16.pdf` contributing page 1 only — its pages 2 and
        // 3 are the two the doc comment above excludes.
        assert_eq!(
            compared, 19,
            "the comparison must run on the pages it says it ran on: {names:?}"
        );
    }

    #[test]
    fn a_font_mapped_to_u_fffd_reads_back_as_replacement_characters_only() {
        // The premise `tests/pdf.rs` rests on for the skipped page of
        // `replacement-chars.pdf`: pdfium returns that page as U+FFFD, and as
        // more of them than the threshold — so it is the characters, not the
        // length, that make the reader skip it. A page later emptied in the
        // generator would be skipped for having nothing on it, and every test
        // above it would stay green.
        let pages = page_texts(&fixture("replacement-chars.pdf")).expect("the fixture parses");

        let page = &pages[1];
        assert!(
            page.chars().all(|c| c == '\u{FFFD}' || c.is_whitespace()),
            "{page:?}"
        );
        assert!(
            page.chars().filter(|c| *c == '\u{FFFD}').count() >= TEXT_LAYER_MIN_CHARS,
            "{page:?}"
        );
        // And the contents line: a run between two words that stay words.
        assert!(
            pages[0].starts_with("About the Authors\u{FFFD}"),
            "{:?}",
            pages[0]
        );
    }

    // These two build their manifests from `PDFIUM_API_BUILD` rather than from a
    // literal. A literal here would make the tests fight a version bump: the accept
    // case would red on *every* bump, and the refuse case would red on a bump to
    // whichever number it had hard-coded as "some other build". Moving the pin is
    // supposed to be a decision, not a fight with the suite.

    /// The one place `>=` can be told from `>`.
    ///
    /// No fixture can do it. `tests/pdf.rs` runs against pages of 119 and 8
    /// characters — seventy-one clear on one side and forty short on the
    /// other — so every assertion over them survives moving the comparison by
    /// one, and a fixture cut to exactly 48 would pin the constant's value
    /// into a `.pdf` blob that nobody could re-derive after a product decision
    /// moved it. Stated against the constant instead, both directions, so a
    /// bump costs nothing and an off-by-one costs this test.
    #[test]
    fn the_threshold_is_inclusive_at_the_constant() {
        assert!(
            has_text_layer(TEXT_LAYER_MIN_CHARS),
            "a page carrying exactly the minimum has a text layer"
        );
        assert!(
            !has_text_layer(TEXT_LAYER_MIN_CHARS - 1),
            "one character short of the minimum is a stamp"
        );
    }

    /// Counting after NFC, asserted on the alphabet it matters for.
    ///
    /// `й` decomposed is `и` + U+0306: two characters that become one. A
    /// counter that ran before normalisation would answer twice the truth for
    /// a page written that way, and the page stored would be half the size the
    /// threshold admitted. Both spellings, because a counter that normalised
    /// nothing and a counter that normalised everything agree on the composed
    /// input alone.
    ///
    /// The second letter's base is **U+0456, the Cyrillic `і`** — `ї` is
    /// U+0457 and decomposes to U+0456 + U+0308. It was written with a Latin
    /// `i` at first, which composes to `ï` and made this string `йïщ`. The
    /// arithmetic was right and the sentence above it was not: a test that
    /// says "the alphabet this product is built for" while measuring Latin
    /// text is the kind of confidently wrong comment a later task reads and
    /// takes a decision from.
    #[test]
    fn characters_are_counted_after_normalisation_not_before() {
        let composed = "йїщ";
        let decomposed = "и\u{0306}\u{0456}\u{0308}щ";
        // The premise, not assumed: these two really are the same three
        // letters, spelled apart and together. Without it the assertions below
        // would still pass on two unrelated strings that happen to normalise
        // to the same length.
        assert_eq!(nfc::normalise(decomposed), composed);
        assert_eq!(text_layer_chars(composed), 3);
        assert_eq!(
            text_layer_chars(decomposed),
            3,
            "the decomposed spelling of the same three letters must not count as five"
        );
        // Whitespace is not text: a scanner footer spread across a page is
        // still a scanner footer.
        assert_eq!(text_layer_chars(" й \r\n ї\tщ "), 3);
    }

    #[test]
    fn replacement_characters_are_not_counted() {
        let garbage = "\u{FFFD}".repeat(100);
        let at_the_minimum = format!("{garbage}{}{garbage}", "x".repeat(TEXT_LAYER_MIN_CHARS));
        assert_eq!(text_layer_chars(&at_the_minimum), TEXT_LAYER_MIN_CHARS);
        assert!(has_text_layer(text_layer_chars(&at_the_minimum)));
        let one_short = format!("{garbage}{}", "x".repeat(TEXT_LAYER_MIN_CHARS - 1));
        assert_eq!(text_layer_chars(&one_short), TEXT_LAYER_MIN_CHARS - 1);
        assert!(!has_text_layer(text_layer_chars(&one_short)));
    }

    #[test]
    fn a_manifest_for_another_build_is_refused() {
        let another_build = PDFIUM_API_BUILD + 1;
        let err = build_from_version_manifest(
            &format!("MAJOR=151\nMINOR=0\nBUILD={another_build}\nPATCH=0\n"),
            "/vendor/VERSION",
        )
        .expect_err("a manifest naming a different build is not the build we bound to");

        match err {
            Error::BuildMismatch {
                expected, found, ..
            } => {
                assert_eq!(expected, PDFIUM_API_BUILD);
                assert_eq!(found, another_build);
            }
            unexpected => panic!("expected a build mismatch, got {unexpected:?}"),
        }
    }

    #[test]
    fn the_matching_manifest_is_accepted() {
        let build = build_from_version_manifest(
            &format!("MAJOR=151\nMINOR=0\nBUILD={PDFIUM_API_BUILD}\nPATCH=0\n"),
            "/v",
        )
        .expect("the vendored build is the one the bindings describe");
        assert_eq!(build, PDFIUM_API_BUILD);
    }

    #[test]
    fn a_manifest_without_a_build_line_is_refused() {
        // Not "assume it is fine": an unreadable manifest leaves the build
        // unconfirmed, which is the state this check exists to reject.
        let err = build_from_version_manifest("MAJOR=151\nMINOR=0\nPATCH=0\n", "/vendor/VERSION")
            .expect_err("a manifest with no BUILD= line confirms nothing");
        // Both the variant AND the stage it carries: a missing BUILD= line is
        // discovered inside verify_build, not while looking for the library
        // itself or while binding it, and `--probe-pdfium` reports exactly
        // this field to a caller that never sees this match arm.
        assert!(matches!(
            err,
            Error::Library {
                stage: Stage::VerifyBuild,
                ..
            }
        ));
        assert_eq!(err.stage(), "verify_build");
    }

    #[test]
    fn the_vendored_library_passes_the_build_check() {
        // The accept branch against the real file on disk, not a string literal.
        let dir = library_dir().expect("the vendored library is present");
        verify_build(&dir).expect("the vendored build matches the bindings");
    }

    /// The bundle layout, asserted through the check that actually reads it.
    ///
    /// The derived path alone would be worth little: comparing
    /// `bundled_library_dir`'s answer against a second spelling of the same
    /// join proves the two spellings agree. What has to hold is that the
    /// layout satisfies `verify_build`, whose manifest search is `<dir>/VERSION`
    /// and `<dir>/../VERSION` and is written in a different file. So the tree is
    /// built the way the bundler builds it and `verify_build` is run on the
    /// result — which is what fails if either side of that pair moves.
    #[test]
    fn a_bundle_shaped_tree_yields_a_library_dir_whose_manifest_verify_build_finds() {
        let root = tempfile::tempdir().unwrap();
        let macos = root.path().join("Mnema.app/Contents/MacOS");
        let lib = root.path().join("Mnema.app/Contents/Resources/pdfium/lib");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        // Contents, not bytes: nothing here loads it, and a real 7.7 MB library
        // in a unit test would be measuring the fixture.
        std::fs::write(lib.join(Pdfium::pdfium_platform_library_name()), b"x").unwrap();
        std::fs::write(
            lib.parent().unwrap().join("VERSION"),
            format!("MAJOR=151\nMINOR=0\nBUILD={PDFIUM_API_BUILD}\nPATCH=0\n"),
        )
        .unwrap();

        let found = bundled_library_dir(&macos).expect("a bundle-shaped tree has a library");
        assert_eq!(found, lib);
        verify_build(&found).expect("the manifest one directory up is the one verify_build reads");
    }

    /// The other direction, and it is not symmetry for its own sake: the
    /// function is what tells a packaged application apart from a development
    /// one, and an answer of "yes, here" for a tree with no library in it would
    /// send `bind_to_library` at a path that does not exist and report the
    /// failure as a *binding* fault rather than as a missing library.
    #[test]
    fn a_tree_with_no_library_in_it_is_not_a_bundle() {
        let root = tempfile::tempdir().unwrap();
        let macos = root.path().join("Mnema.app/Contents/MacOS");
        // The directory exists and is empty: the near miss, not the easy one.
        std::fs::create_dir_all(root.path().join("Mnema.app/Contents/Resources/pdfium/lib"))
            .unwrap();
        std::fs::create_dir_all(&macos).unwrap();
        assert!(
            bundled_library_dir(&macos).is_none(),
            "an empty pdfium/lib is not a packaged library"
        );
        // And a path with no parent at all, which is the branch `?` covers.
        assert!(bundled_library_dir(Path::new("/")).is_none());
    }

    #[test]
    fn a_directory_with_no_version_manifest_fails_at_the_verify_build_stage() {
        // The real filesystem failure a bundle probe hit first
        // (task-1-report.md): not a signature refusal, a missing VERSION
        // file. This drives verify_build() itself, not the string-based
        // build_from_version_manifest helper above, so it also exercises the
        // "no candidate path is a file" branch that helper never reaches.
        let dir = tempfile::tempdir().unwrap();
        let err = verify_build(dir.path()).expect_err("an empty directory has no VERSION");
        assert_eq!(err.stage(), "verify_build");
        assert!(
            err.to_string().contains("no VERSION manifest"),
            "the message must still say what verify_build() itself found: {err}"
        );
    }
}
