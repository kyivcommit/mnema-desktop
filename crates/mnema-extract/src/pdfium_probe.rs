use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
            let count = text.chars().filter(|c| !c.is_whitespace()).count();
            PageProbe {
                page_no: (i + 1) as u32,
                char_count: count,
                has_text_layer: count >= TEXT_LAYER_MIN_CHARS,
            }
        })
        .collect())
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

    let mut out = Vec::new();
    for page in document.pages().iter() {
        let text = page.text().map_err(|e| Error::Pdfium(e.to_string()))?;
        out.push(text.all());
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
fn lock_pdfium() -> std::sync::MutexGuard<'static, ()> {
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
fn pdfium() -> Result<&'static Pdfium, Error> {
    static PDFIUM: OnceLock<Result<Pdfium, Error>> = OnceLock::new();

    PDFIUM
        .get_or_init(|| {
            let dir = library_dir()?;
            let library = Pdfium::pdfium_platform_library_name_at_path(&dir);
            verify_build(&dir)?;
            let bindings = Pdfium::bind_to_library(&library).map_err(|e| Error::Library {
                stage: Stage::Bind,
                message: format!("{} could not be loaded: {e}", library.display()),
            })?;
            Ok(Pdfium::new(bindings))
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

    // 2. Beside the running executable — where a bundled application will ship it.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.join(Pdfium::pdfium_platform_library_name()).is_file()
    {
        return Ok(dir.to_path_buf());
    }

    // 3. The vendored copy in a development checkout. Baked in at compile time
    //    because a library cannot ask where the workspace was; a packaged build
    //    never reaches this branch, having matched (2).
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
    let manifest = candidates.iter().find(|p| p.is_file()).ok_or_else(|| Error::Library {
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

    // These two build their manifests from `PDFIUM_API_BUILD` rather than from a
    // literal. A literal here would make the tests fight a version bump: the accept
    // case would red on *every* bump, and the refuse case would red on a bump to
    // whichever number it had hard-coded as "some other build". Moving the pin is
    // supposed to be a decision, not a fight with the suite.

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
