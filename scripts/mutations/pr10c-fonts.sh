# PR 10c — the bundled fonts: `ui/src/styles/fonts.css`, the stacks in
# `ui/src/styles/tokens.css`, and `build.assetsInlineLimit` in
# `ui/vite.config.ts`. Run with:
#
#     scripts/mutation-check.sh scripts/mutations/pr10c-fonts.sh
#
# What is here, by name:
#
#   the file          — a src url() that names no file is a face the browser
#                       replaces with the next family, silently
#   the leader        — a stack that does not start with a bundled family
#                       renders in whatever the machine has installed
#   the alphabet      — a face without a Ukrainian-only codepoint paints that
#                       character in the fallback family, mid-word
#   the display       — `swap` would flash the fallback family before the
#                       local file, for a file that is already on disk
#   the inline limit  — without the limit Vite folds the three smallest
#                       subsets into data: URIs, which the CSP refuses
#
# Not here: the second direction of "the file" (a font file nobody names) —
# a mutation is an edit to a tracked file, and that state is an ADDED file.
# It was seen red by hand in the task report and is left there.
#
# "the alphabet" targets U+2116 (№), not U+0490-0491 (Ґ/ґ) as a first draft of
# this case did: the cyrillic (non-ext) block lists Ge-with-upturn explicitly,
# but the SAME face's cyrillic-ext block covers U+0460-052F, which already
# contains U+0490-0491 — the guard unions a face's ranges across its subset
# files (by design: decision 5, so "Ukrainian" is a property of the face, not
# of one subset), so removing the redundant listing left the face covered and
# the case dead on arrival. U+2116 is not in any cyrillic-ext range, in any of
# the three families, so removing it is the one visible gap.
#
# "the file"'s test title has no `()`: mutation-check.sh's `-t` treats
# <test-name> as a regex (needs `\(\)` to match a literal paren) but
# mutation-staleness.sh greps the same field as a fixed string against the
# source (needs the unescaped text) — the two disagree on parens, a tension
# already flagged in mutation-staleness.sh's own comments above case_(). The
# title dropped its `()` rather than the harness gaining a second field.

case_ "fonts.css: a src url() must name a file that exists" \
  ui/src/styles/fonts.css \
  's~fonts/spectral/spectral-400-normal-latin\.woff2~fonts/spectral/spectral-400-normal-latin-missing.woff2~' \
  'spectral-400-normal-latin-missing.woff2' \
  src/styles/tokens.test.ts 'names a file that exists in every src url, and names every font file' runner=vitest

case_ "tokens.css: every stack must lead with a bundled family, in all three theme blocks" \
  ui/src/styles/tokens.css \
  "s~--sans: 'IBM Plex Sans', system-ui~--sans: system-ui~g" \
  '--sans: system-ui' \
  src/styles/tokens.test.ts 'leads every stack in tokens.css with a bundled family, and bundles no family that leads none' runner=vitest

case_ "fonts.css: every face must cover the Ukrainian alphabet" \
  ui/src/styles/fonts.css \
  's~U\+04B0-04B1, U\+2116;~U+04B0-04B1;~g' \
  'U+04B0-04B1;' \
  src/styles/tokens.test.ts 'covers the Ukrainian alphabet, the hryvnia sign and the typographic punctuation in every face' runner=vitest

case_ "fonts.css: font-display must be block, not swap" \
  ui/src/styles/fonts.css \
  's~font-display: block;~font-display: swap;~g' \
  'font-display: swap;' \
  src/styles/tokens.test.ts 'declares font-display: block and format woff2 on every face' runner=vitest

case_ "vite.config.ts: the inline limit must be zero, not the default" \
  ui/vite.config.ts \
  's~    assetsInlineLimit: 0,\n~~' \
  '    // under the 4096-byte default and would vanish into the fallback family.
    target: process.env.TAURI_ENV_PLATFORM' \
  src/styles/tokens.test.ts 'keeps vite from inlining any asset as a data: URI' runner=vitest
