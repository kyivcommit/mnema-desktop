# Mutation cases for the per-platform Pdfium resource declaration. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/linux-resource.sh
#
# The subject is src-tauri/tests/vendored_library_resource.rs, and none of the
# four files it reads is Rust: two platform configs, the base config, and the
# shell script that installs the library. That is the whole reason the test
# exists — a mismatch between a JSON key and a `case` arm stopped `src-tauri`
# from compiling on Linux and on Windows, and nothing in the workspace could
# see it from a macOS machine.
#
# Every case below is the defect that was actually shipped, or one step away
# from it. The first is the shipped one exactly.

# ---------------------------------------------------------------- the defect

# What the branch shipped: the platform file names macOS's library, so
# `tauri-build` validates a path that `fetch-pdfium.sh` never creates on Linux
# and the crate does not compile there at all. Written as an edit to the
# platform file rather than as its deletion, because the harness mutates files
# in place — the effect on the merged configuration is the same one, and it is
# the one the first completed Linux job reported:
#
#   resource path `../vendor/pdfium/lib/libpdfium.dylib` doesn't exist
case_ "linux: the platform file names the macOS library" \
  src-tauri/tauri.linux.conf.json \
  's{"\.\./vendor/pdfium/lib/libpdfium\.so"}{"../vendor/pdfium/lib/libpdfium.dylib.disarmed"}' \
  '../vendor/pdfium/lib/libpdfium.dylib.disarmed' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# The same defect on the platform no workflow builds. It is the reason this file
# checks three platforms rather than the one that broke: a Windows machine can
# run the suite — `fetch-pdfium.sh` pins an archive so it can — and nothing but
# a person would ever have found this.
case_ "windows: the platform file names the macOS library" \
  src-tauri/tauri.windows.conf.json \
  's{"\.\./vendor/pdfium/lib/pdfium\.dll"}{"../vendor/pdfium/lib/pdfium.dll.disarmed"}' \
  '../vendor/pdfium/lib/pdfium.dll.disarmed' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# ------------------------------------------------- the merge, misunderstood

# The mistake anyone reaches for first. A platform config is not an override:
# `read_from` merges it as an RFC 7396 merge patch, so listing the `.so` ADDS a
# key and leaves the `.dylib` in place. The build then fails on the `.dylib`
# exactly as before, having looked fixed. Only `null` removes a key, and this is
# the case that says so.
case_ "linux: the .dylib entry is left in place beside the .so" \
  src-tauri/tauri.linux.conf.json \
  's{"\.\./vendor/pdfium/lib/libpdfium\.dylib": null}{"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/lib/libpdfium.dylib"}' \
  '"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/lib/libpdfium.dylib"' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# --------------------------------------------------- what the bundle needs

# The library declared, and filed where `pdfium_probe.rs` does not look. This is
# the quiet one: the build succeeds, the bundle carries a libpdfium, and the
# worker walks to `Contents/Resources/pdfium/lib`, finds nothing, and falls
# through to the branch that ignores the bundle entirely — which is the state a
# signed image was measured in before task 16.
case_ "linux: the library is filed outside pdfium/lib" \
  src-tauri/tauri.linux.conf.json \
  's{"pdfium/lib/libpdfium\.so"}{"libpdfium.so"}' \
  '"libpdfium.so"' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# The manifest dropped while the library stays. `verify_build` reads `BUILD=`
# out of `VERSION` to tell a matching library from one whose struct layouts have
# drifted, and a drifted Pdfium does not fail honestly — it returns plausible
# garbage. Deleting the key is spelled the way the merge spells deletion.
case_ "linux: the VERSION manifest is dropped from the bundle" \
  src-tauri/tauri.linux.conf.json \
  's{"\.\./vendor/pdfium/lib/libpdfium\.dylib": null,}{"../vendor/pdfium/lib/libpdfium.dylib": null, "../vendor/pdfium/VERSION": null,}' \
  '"../vendor/pdfium/VERSION": null' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# ------------------------------------------------- the file nobody compiles

# A platform file is never deserialised on any other platform, so a typo in one
# is a Linux-only build failure — the same shape of hole as the defect itself.
# It is caught here only because the test deserialises into `Config`, which is
# `deny_unknown_fields`. Delete that step and this case goes green.
case_ "linux: a mistyped key in the platform file" \
  src-tauri/tauri.linux.conf.json \
  's{"bundle"}{"bundel"}' \
  '"bundel"' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource

# ---------------------------------------------------- the other half of it

# The check compares two files. Blind it on the script side and every assertion
# above is satisfied by an empty list — the vacuous-pass family this repository
# has now closed six times. This is the case that fails if the guard test is
# ever weakened into a one-directional one.
case_ "script: no case arm sets a library any more" \
  scripts/fetch-pdfium.sh \
  's{^    library=}{    lib_file=}mg' \
  '    lib_file=' \
  mnema-desktop 'the_script_pins_a_library_for_every_platform_this_file_checks' \
  --test vendored_library_resource

# And the reverse blinding: the script keeps pinning, but for a platform the
# test's own list does not name. Skipping it silently is how a fourth platform
# would arrive unchecked, so the parser refuses rather than filters.
case_ "script: a platform the test does not know about" \
  scripts/fetch-pdfium.sh \
  's{^  Linux/x86_64\)}{  Haiku/x86_64)}m' \
  '  Haiku/x86_64)' \
  mnema-desktop 'the_script_pins_a_library_for_every_platform_this_file_checks' \
  --test vendored_library_resource

# --------------------------------------------------- the macOS guarantee

# macOS reads `tauri.conf.json` and nothing else, and that is load-bearing
# rather than incidental: it is the configuration the signed image was verified
# against. This mutates the base file into the shape a careless split would
# leave — the library moved out of the file that was measured — and the test
# that pins macOS to one config file is what notices.
case_ "macos: the base file stops naming the library" \
  src-tauri/tauri.conf.json \
  's{"\.\./vendor/pdfium/lib/libpdfium\.dylib": "pdfium/lib/libpdfium\.dylib",\n}{}' \
  '"../vendor/pdfium/VERSION"' \
  mnema-desktop 'every_platform_declares_the_library_its_installer_installs' \
  --test vendored_library_resource
