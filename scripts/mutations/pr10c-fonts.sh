# PR 10c — the bundled fonts: `ui/src/styles/fonts.css`, the stacks in
# `ui/src/styles/tokens.css`, and `build.assetsInlineLimit` in
# `ui/vite.config.ts`. Run with:
#
#     scripts/mutation-check.sh scripts/mutations/pr10c-fonts.sh
#
# What is here, by name:
#
#   the file          — a src url() that names no file is a face the browser
#                       replaces with the next family, silently; and a file
#                       that no src url() names is dead weight in the bundle
#   the leader        — a stack that does not start with a bundled family
#                       renders in whatever the machine has installed; and a
#                       bundled family that leads no stack ships for nobody
#   the three blocks  — the stacks are the same in light, media-dark and
#                       attribute-dark, or one theme renders in another face
#   the alphabet      — a face without a Ukrainian-only codepoint paints that
#                       character in the fallback family, mid-word
#   the display       — `swap` would flash the fallback family before the
#                       local file, for a file that is already on disk
#   the format        — a file declared as anything but woff2 is not what
#                       the build was told to ship
#   the licence       — a family whose OFL.txt is not in bundle.resources
#                       ships its glyphs and not its licence
#   the inline limit  — without the limit Vite folds every subset under its
#                       default 4096 bytes into a data: URI, which the CSP
#                       refuses; and a value inside a comment is no value,
#                       which the regex this guard first used could not tell
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

case_ "fonts.css: a face pointing at another face's file leaves its own file unnamed" \
  ui/src/styles/fonts.css \
  's~fonts/ibm-plex-mono/ibm-plex-mono-500-normal-latin-ext\.woff2~fonts/ibm-plex-mono/ibm-plex-mono-500-normal-latin.woff2~' \
  'ibm-plex-mono-500-normal-latin.woff2) format' \
  src/styles/tokens.test.ts 'names a file that exists in every src url, and names every font file' runner=vitest

case_ "tokens.css: every stack must lead with a bundled family" \
  ui/src/styles/tokens.css \
  "s~--sans: 'IBM Plex Sans', system-ui~--sans: system-ui~g" \
  '--sans: system-ui' \
  src/styles/tokens.test.ts 'leads every stack in tokens.css with a bundled family, and bundles no family that leads none' runner=vitest

case_ "tokens.css: a bundled family that leads no stack" \
  ui/src/styles/tokens.css \
  "s~--serif: 'Spectral',~--serif: 'IBM Plex Sans',~g" \
  "--serif: 'IBM Plex Sans'," \
  src/styles/tokens.test.ts 'leads every stack in tokens.css with a bundled family, and bundles no family that leads none' runner=vitest

case_ "tokens.css: the stacks must be the same in every theme block" \
  ui/src/styles/tokens.css \
  's~(:root\[data-theme="dark"\] \{[^}]*?)  --serif: '"'"'Spectral'"'"', ~$1  --serif: ~' \
  "--serif: Georgia, 'Times New Roman', serif;" \
  src/styles/tokens.test.ts 'keeps the theme-invariant font stacks identical in every block' runner=vitest

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

case_ "fonts.css: the format must be woff2, not a stray TTF under a woff2 name" \
  ui/src/styles/fonts.css \
  "s~format\('woff2'\)~format('truetype')~g" \
  "format('truetype')" \
  src/styles/tokens.test.ts 'declares font-display: block and format woff2 on every face' runner=vitest

case_ "tauri.conf.json: a family whose licence is not a bundle resource ships unlicensed" \
  src-tauri/tauri.conf.json \
  's~      "\.\./ui/src/styles/fonts/spectral/OFL\.txt": "fonts/spectral/OFL\.txt",\n~~' \
  '      "../vendor/pdfium/LICENSE": "pdfium/LICENSE",
      "../ui/src/styles/fonts/ibm-plex-sans/OFL.txt": "fonts/ibm-plex-sans/OFL.txt",' \
  src/styles/tokens.test.ts 'names every font family licence in tauri.conf.json bundle.resources' runner=vitest

case_ "vite.config.ts: a commented-out inline limit is no limit" \
  ui/vite.config.ts \
  's~    assetsInlineLimit: 0,~    /* assetsInlineLimit: 0, */~' \
  '/* assetsInlineLimit: 0, */' \
  src/styles/tokens.test.ts 'keeps vite from inlining any asset as a data: URI' runner=vitest

case_ "vite.config.ts: the inline limit must be zero, not the default" \
  ui/vite.config.ts \
  's~    assetsInlineLimit: 0,\n~~' \
  '    // family. A font subset can weigh less than the 4096-byte default.
    target: process.env.TAURI_ENV_PLATFORM' \
  src/styles/tokens.test.ts 'keeps vite from inlining any asset as a data: URI' runner=vitest
