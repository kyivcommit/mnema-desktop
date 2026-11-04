# macOS-only sibling of `scan-ocr.sh`: the cases that need Apple Vision. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/scan-ocr-macos.sh
# not-in-matrix: its test is #[cfg(target_os = "macos")]; on the ubuntu leg the baseline selects 0 tests and the whole file exits 1 — see pr8-exclusions-macos.sh and ci.yml's mutations comment
#
# **Nothing runs this file automatically.** The `mutations` job has no macOS leg
# (every leg is `ubuntu-24.04`, and the account's 20-job ceiling leaves no room for
# one), so this is local evidence, re-run by hand after touching `ocr.rs`.
# `scripts/mutation-staleness.sh` still proves each case here APPLIES on every
# sweep.
#
# **Cost: ~1 minute per case.** Vision's cold start is paid once per executable
# *path* (`ocr.rs`'s header: 46 s), and the harness builds in a worktree whose path
# changes every run, so the baseline and each mutant pay it again.

# G1. An 8-bit gray bitmap handed to Vision in place of the BGRA one. Measured
# (Task 1): Vision returns ZERO observations for it, with no error, on every page —
# so a scan reads as "no text" and is journalled as content. The test asserts
# words that must come back from `scan-uk.pdf`.
case_ "ocr: Vision is handed the BGRA bitmap pdfium rendered" \
  crates/mnema-extract/src/ocr.rs \
  's#CGColorSpace::new_device_rgb\(\)#CGColorSpace::new_device_gray()#; s#CGImageAlphaInfo::PremultipliedFirst\.0 \| CGImageByteOrderInfo::Order32Little\.0#CGImageAlphaInfo::None.0#; s#                    8,\n                    32,\n#                    8,\n                    8,\n#' \
  'CGColorSpace::new_device_gray()' \
  mnema-extract 'a_scanned_page_is_read_and_the_seam_makes_the_engine_unavailable' --test ocr
