#!/usr/bin/env bash
#
# Fetches the prebuilt Pdfium library that `mnema-extract` binds against.
#
# The version is pinned, and the pin is not a preference. `pdfium-render` compiles
# bindings for one specific Pdfium C API revision, selected by a crate feature.
# `crates/mnema-extract/Cargo.toml` names that feature explicitly, as `pdfium_`
# followed by the build number below. Loading a different Pdfium build under those
# bindings does not fail honestly — struct layouts drift between revisions, so the
# reads come back as plausible garbage. PDFIUM_BUILD below must therefore equal the
# number in that feature and in `mnema_extract::PDFIUM_API_BUILD`, and
# `tests/pdfium_binding.rs` fails if any of the three separate.
#
# Moving the pin means editing every place the build number appears, and the list
# of those places is in `crates/mnema-extract/README.md` — deliberately not
# repeated here. A count kept in two files drifts exactly the way a pin does, and
# it already had: this comment said "four edits" while the README said five and
# the truth was six. What is left here is the rule, which does not go stale: a
# number written anywhere no test reads it is how a pin drifts while everything
# still looks right, so do not leave one in a comment — including in this file.
#
# Only the non-V8 builds are used. The V8 archives carry a full JavaScript engine
# for PDF form scripting, which this product does not execute, and are ~4x larger.
#
# Usage: scripts/fetch-pdfium.sh
# Result: vendor/pdfium/VERSION plus lib/libpdfium.dylib on macOS,
#         lib/libpdfium.so on Linux.

set -euo pipefail

PDFIUM_BUILD=7881
RELEASE_TAG="chromium/${PDFIUM_BUILD}"
BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${RELEASE_TAG}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="${repo_root}/vendor/pdfium"

os="$(uname -s)"
arch="$(uname -m)"

# The checksums are the pin. Fetching by tag alone still trusts whatever the tag
# points at today; comparing content means a substituted archive is a hard failure
# rather than a silent one. They were recorded on 2026-07-25 from the release above.
case "${os}/${arch}" in
  Darwin/arm64)
    asset="pdfium-mac-arm64.tgz"
    sha256="52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40"
    library="lib/libpdfium.dylib"
    ;;
  Darwin/x86_64)
    asset="pdfium-mac-x64.tgz"
    sha256="6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b"
    library="lib/libpdfium.dylib"
    ;;
  # Linux is here because CI builds there, not because the product ships there.
  # `cargo test --workspace` on a Linux runner runs mnema-extract's tests, and
  # those load the library; without a pin here the whole matrix job fails on a
  # missing file rather than on anything it was added to catch. Recorded the same
  # way as the macOS pair and from the same release: both archives carry
  # `lib/libpdfium.so` and the same `BUILD=` manifest as the macOS ones, checked
  # by running this script on both. The number is not repeated here on purpose —
  # see the header.
  Linux/x86_64)
    asset="pdfium-linux-x64.tgz"
    sha256="1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d"
    library="lib/libpdfium.so"
    ;;
  Linux/aarch64)
    asset="pdfium-linux-arm64.tgz"
    sha256="ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674"
    library="lib/libpdfium.so"
    ;;
  # Windows needs bash to reach this script at all, which on that platform means
  # Git Bash, MSYS2 or Cygwin — the three `uname -s` strings below, each of which
  # appends a version to its family name and so is matched by prefix. There is no
  # native branch to add: `cmd` and PowerShell never run this file.
  #
  # Two things differ from every case above, and only one of them is visible here.
  # The archive keeps its library in `bin/`, not `lib/` — hence `archive_dir` — and
  # what lands in `vendor/pdfium/lib/` is that directory under its other name, so
  # that one vendored layout serves all platforms and `library_dir()` on the Rust
  # side stays free of a `#[cfg]`. The second is that Windows is not a delivery
  # target (D3): this pin exists so a Windows machine can run the test suite, the
  # way the Linux pins exist for CI. arm64 is deliberately absent — no arm64
  # Windows machine has run it, and a checksum nobody fetched is a guess wearing
  # the clothes of a measurement.
  MINGW*/x86_64 | MSYS*/x86_64 | CYGWIN*/x86_64)
    asset="pdfium-win-x64.tgz"
    sha256="73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08"
    archive_dir="bin"
    library="lib/pdfium.dll"
    ;;
  *)
    echo "fetch-pdfium: no pinned archive for ${os}/${arch}." >&2
    echo "Pinned here: macOS arm64 and x86-64, Linux x86-64 and arm64, Windows" >&2
    echo "x86-64 under Git Bash, MSYS2 or Cygwin. Adding a platform means adding" >&2
    echo "its asset name and its checksum here, from ${BASE_URL}." >&2
    exit 1
    ;;
esac

# Where the library sits *inside the archive*. Every case above except Windows
# ships it in `lib/`, and that is also where it is installed, so the default
# keeps those four branches from having to say so.
archive_dir="${archive_dir:-lib}"

# The guard names both halves of a usable install. `VERSION` alone is not enough:
# it precedes `lib/` in the archive, so a run interrupted mid-extraction used to
# leave the manifest without the library — after which this script reported
# "already vendored" and installed nothing, while the Rust side failed and told
# the user to run this script. A loop with no exit.
if [ -f "${dest}/VERSION" ] \
  && grep -qx "BUILD=${PDFIUM_BUILD}" "${dest}/VERSION" \
  && [ -f "${dest}/${library}" ]; then
  echo "fetch-pdfium: build ${PDFIUM_BUILD} already vendored at ${dest}"
  exit 0
fi

# The work directory sits inside `vendor/` so that the install below is a rename
# within one filesystem. A `mktemp -d` under /var would be a different volume,
# which turns `mv` back into copy-then-delete — the very thing being avoided.
mkdir -p "${repo_root}/vendor"
work="$(mktemp -d "${repo_root}/vendor/.pdfium-install.XXXXXX")"
trap 'rm -rf "${work}"' EXIT

echo "fetch-pdfium: downloading ${asset} from ${RELEASE_TAG}"
curl --fail --silent --show-error --location \
  --output "${work}/${asset}" "${BASE_URL}/${asset}"

# Neither name is portable, and the split is not the one people remember.
# Measured: macOS 26.5.2 does have /sbin/sha256sum (`sha256sum (Darwin) 1.0`),
# older macOS did not and has only `shasum`; a minimal Linux image has
# `sha256sum` from coreutils and gets `shasum` only when the full perl package
# happens to be installed. Both branches are live, and both print the digest
# first in the same format — checked side by side on this machine.
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${work}/${asset}" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "${work}/${asset}" | cut -d' ' -f1)"
fi
if [ "${actual}" != "${sha256}" ]; then
  echo "fetch-pdfium: checksum mismatch for ${asset}" >&2
  echo "  expected ${sha256}" >&2
  echo "  actual   ${actual}" >&2
  echo "Refusing to install. A mismatch means the release asset changed under the" >&2
  echo "tag, which is exactly the substitution the pin exists to catch." >&2
  exit 1
fi

# Unpack somewhere nobody reads, then move the finished tree into place. Extracting
# straight into `${dest}` means every interruption leaves a half-built vendor tree;
# this way an interrupted run leaves `${dest}` either untouched or absent, and both
# make the next run fetch again.
mkdir -p "${work}/staged"
tar xzf "${work}/${asset}" -C "${work}/staged" "${archive_dir}" VERSION LICENSE

# One vendored layout for every platform: whatever the archive called the
# directory, it is `lib/` once installed. The Rust side hard-codes
# `vendor/pdfium/lib`, so a `bin/` left under its own name would install
# cleanly, satisfy nothing, and report a missing library at load time.
if [ "${archive_dir}" != "lib" ]; then
  mv "${work}/staged/${archive_dir}" "${work}/staged/lib"
fi

if [ ! -f "${work}/staged/${library}" ]; then
  echo "fetch-pdfium: ${asset} unpacked without ${library}. Not installing." >&2
  exit 1
fi

if [ -d "${dest}" ]; then
  mv "${dest}" "${work}/replaced"
fi
mv "${work}/staged" "${dest}"

echo "fetch-pdfium: installed Pdfium build ${PDFIUM_BUILD} into ${dest}"
