# PR 10d — the launcher stylesheet and its component wiring. Run with:
#
#     scripts/mutation-check.sh scripts/mutations/pr10d-launcher.sh

# Launcher CSS wiring through real components, plus the existing entry guard.
case_ "launcher: current row loses its state selector" \
  ui/src/styles/launcher.css \
  's~\.tree \[aria-current="true"\]~.tree [aria-current="never"]~' \
  '.tree [aria-current="never"]' \
  src/launcher/styles.test.ts 'real tree marks current file and recent while leaving neighbours plain' runner=vitest

case_ "launcher: primary source highlight loses its outline" \
  ui/src/styles/launcher.css \
  's~mark\[data-primary="true"\]~mark[data-primary="never"]~' \
  'mark[data-primary="never"]' \
  src/launcher/styles.test.ts 'real source distinguishes primary highlight from its sibling' runner=vitest

case_ "launcher: source is placed into the tree column" \
  ui/src/styles/launcher.css \
  's~(main > \.doc \{ grid-column: )3~${1}1~' \
  'main > .doc { grid-column: 1;' \
  src/launcher/styles.test.ts 'real launcher places all cards and keeps the document transparent' runner=vitest

case_ "launcher: source loses its grid hook" \
  ui/src/launcher/Selection.svelte \
  's~class="float doc"~class="float"~' \
  'class="float"' \
  src/launcher/styles.test.ts 'real launcher places all cards and keeps the document transparent' runner=vitest

case_ "launcher: cards cannot scroll" \
  ui/src/styles/launcher.css \
  's~padding: 14px; overflow: auto;~padding: 14px; overflow: hidden;~' \
  'padding: 14px; overflow: hidden;' \
  src/launcher/styles.test.ts 'real launcher places all cards and keeps the document transparent' runner=vitest

case_ "launcher: the selected tab looks inactive" \
  ui/src/styles/launcher.css \
  's~\.tree \.tabs button\[aria-pressed="true"\]~.tree .tabs button[aria-pressed="never"]~' \
  '.tree .tabs button[aria-pressed="never"]' \
  src/launcher/styles.test.ts 'real tree marks current file and recent while leaving neighbours plain' runner=vitest

case_ "launcher: pinned state loses its style" \
  ui/src/styles/launcher.css \
  's~\.pin\[aria-pressed="true"\]~.pin[aria-pressed="never"]~' \
  '.pin[aria-pressed="never"]' \
  src/launcher/styles.test.ts 'real pin exposes pressed state through its style' runner=vitest

case_ "launcher: entry omits its stylesheet" \
  ui/src/launcher/main.ts \
  "s~import '../styles/launcher.css';\n~~" \
  "import '../styles/base.css';
import { mount }" \
  src/styles/tokens.test.ts 'imports the stylesheets each window needs, in order' runner=vitest
