# PR 10d — the launcher stylesheet and its component wiring. Run with:
#
#     scripts/mutation-check.sh scripts/mutations/pr10d-launcher.sh

# Launcher CSS wiring through real components, plus the existing entry guard.
case_ "launcher: current row loses its state selector" \
  ui/src/styles/launcher.css \
  's~\.tree \[aria-current="true"\]~.tree [aria-current="never"]~' \
  '.tree [aria-current="never"]' \
  src/launcher/styles.test.ts 'real tree marks the current file while leaving neighbours plain' runner=vitest

case_ "launcher: primary source highlight loses its outline" \
  ui/src/styles/launcher.css \
  's~mark\[data-primary="true"\]~mark[data-primary="never"]~' \
  'mark[data-primary="never"]' \
  src/launcher/styles.test.ts 'real file pane paints the cited span and keeps its badge on screen' runner=vitest

case_ "launcher: the right area loses its own track" \
  ui/src/launcher/Shell.svelte \
  's~minmax\(0, 1fr\) \$\{trackW\}px~minmax(0, 1fr) 0px~' \
  'minmax(0, 1fr) 0px' \
  src/launcher/Shell.test.ts 'a toggle hands its width to the neighbour' runner=vitest

case_ "launcher: the tree toggle stops hiding the tree" \
  ui/src/launcher/Shell.svelte \
  's~hidden=\{!treeVisible\}~hidden={false}~' \
  'hidden={false}' \
  src/launcher/Shell.test.ts 'the tree toggle does not change the transcript width' runner=vitest

case_ "launcher: the transcript cannot scroll" \
  ui/src/styles/launcher.css \
  's~.centre-body > .transcript { flex: 1 1 auto; min-height: 0; overflow-y: auto; }~.centre-body > .transcript { flex: 1 1 auto; min-height: 0; }~' \
  '.centre-body > .transcript { flex: 1 1 auto; min-height: 0; }' \
  src/launcher/Launcher.test.ts 'the shell is one row of tracks and the transcript is the only scroller' runner=vitest

case_ "launcher: the conversation on screen looks inactive" \
  ui/src/styles/launcher.css \
  's~\.history-row\[aria-current="true"\] \{~.history-row[aria-current="never"] {~' \
  '.history-row[aria-current="never"]' \
  src/launcher/styles.test.ts 'real recents marks the conversation on screen with --select' runner=vitest

case_ "launcher: entry omits its stylesheet" \
  ui/src/launcher/main.ts \
  "s~import '../styles/launcher.css';\n~~" \
  "import '../styles/base.css';
import { mount }" \
  src/styles/tokens.test.ts 'imports the stylesheets each window needs, in order' runner=vitest

case_ "launcher: the window loses its decorations" \
  src-tauri/tauri.conf.json \
  's~"decorations": true~"decorations": false~' \
  '"decorations": false' \
  src/launcher/styles.test.ts 'the launcher is a normal window: decorated, resizable, and not a popup' runner=vitest

case_ "launcher: light citation pair loses AA contrast" \
  ui/src/styles/tokens.css \
  's~--cite: #8F5D05;~--cite: #9A6708;~' \
  '--cite: #9A6708;' \
  src/styles/tokens.test.ts 'uses AA text colours on launcher surfaces' runner=vitest
