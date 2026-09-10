# PR 10e — the settings window's stylesheet `ui/src/styles/settings.css`, the
# class an excluded subfolder row carries, the window's stylesheet imports,
# and the guards `tokens.test.ts` and `Folders.test.ts` grew for them. Run:
#
#     scripts/mutation-check.sh scripts/mutations/pr10e-settings.sh
#
# What is here, by name:
#
#   the unused token    — a token tokens.css declares and no stylesheet reads
#                         is a value that can drift in one theme unseen
#   the undeclared token — a stylesheet reading a token tokens.css never
#                         declares leaves the browser to discard the value
#   the second weight   — two font-weights in one rule: the browser takes the
#                         last, a guard reading the first would pass it
#   the shorthand       — `font:` with a weight in it, in base.css where the
#                         shorthand stands alone
#   the family alone    — a rule that sets font-family and nothing else
#                         changes the face under an inherited weight
#   the missing face    — a weight the bundle has no face for is bold the
#                         browser synthesises from 400, without a message
#   the active section  — the pressed navigation item must not look like the
#                         others: the first of the three DOM-only states
#   the excluded folder — the second of them, in the stylesheet
#   the chosen theme    — the third; both rules that mark it go together,
#                         because the general [aria-pressed] rule alone still
#                         tints the background
#   the class           — the component must put `excl` on the row, or the
#                         stylesheet's rule has nothing to match
#   the held-above row  — a row excluded by an ancestor rule is excluded too
#   the import          — the window must import its own sheet; no test
#                         mounts through main.ts, so only this one sees it
#   the layout class    — a row without `sub` keeps `excl` and loses the
#                         rule that dims it
#   the current model   — the fourth DOM-only state; the nav item and the
#                         excluded folder each had a mutant, this one did not
#   the important family — `!important` on font-family would let it win over
#                         a rule that sets all three, silently
#   the open row        — the `open` arm of `describe` marked as excluded
#                         too, which nothing but the negative half catches
#   the at-rule container — a @media block handed to the declaration parser
#                         reads its child rule as a property name and rejects
#                         valid CSS

case_ "tokens.css: a declared token that no stylesheet reads" \
  ui/src/styles/tokens.css \
  's~(--glow: ([^;]+);)~$1\n  --unused-probe: $2;~g' \
  '--unused-probe:' \
  src/styles/tokens.test.ts 'uses every token it declares' runner=vitest

case_ "settings.css: a style reads an undeclared token" \
  ui/src/styles/settings.css \
  's~(border-right: 1px solid var\(--line\);\n  background: )var\(--surface-2\)~${1}var(--missing-probe)~' \
  'var(--missing-probe)' \
  src/styles/tokens.test.ts 'use only tokens that tokens.css declares' runner=vitest

case_ "settings.css: a second font-weight in the same rule" \
  ui/src/styles/settings.css \
  's~(--serif\);\n  font-weight: 600;\n)~${1}  font-weight: 700;\n~' \
  'font-weight: 700;' \
  src/styles/tokens.test.ts 'sets the font family, weight and style together, and only as faces the bundle has' runner=vitest

case_ "base.css: a font shorthand that carries a weight" \
  ui/src/styles/base.css \
  's~font: 14px/1\.55 var\(--sans\);~font: normal 700 14px/1.55 var(--sans);~' \
  'font: normal 700 14px/1.55 var(--sans);' \
  src/styles/tokens.test.ts 'sets the font family, weight and style together, and only as faces the bundle has' runner=vitest

case_ "settings.css: a later rule that sets the family alone" \
  ui/src/styles/settings.css \
  's~\z~\n.spane h2 { font-family: var(--mono); }\n~' \
  '.spane h2 { font-family: var(--mono); }' \
  src/styles/tokens.test.ts 'sets the font family, weight and style together, and only as faces the bundle has' runner=vitest

case_ "settings.css: a weight the bundle has no face for" \
  ui/src/styles/settings.css \
  's~(\.spane h3 \{\n  margin: 0;\n  font-family: var\(--mono\);\n  font-weight: )400~${1}600~' \
  '.spane h3 {
  margin: 0;
  font-family: var(--mono);
  font-weight: 600;' \
  src/styles/tokens.test.ts 'sets the font family, weight and style together, and only as faces the bundle has' runner=vitest

case_ "settings.css: the active navigation item looks like the rest" \
  ui/src/styles/settings.css \
  's~\.snav \.item\[aria-pressed="true"\] \{~.snav .item[aria-pressed="never"] {~' \
  '.snav .item[aria-pressed="never"]' \
  src/styles/tokens.test.ts 'marks the active section in the navigation' runner=vitest

case_ "settings.css: an excluded folder looks like an open one" \
  ui/src/styles/settings.css \
  's~\.sub\.excl \{~.sub.exc {~' \
  '.sub.exc {' \
  src/styles/tokens.test.ts 'dims an excluded folder' runner=vitest

case_ "settings.css: the chosen theme looks like the others" \
  ui/src/styles/settings.css \
  's~button\[aria-pressed="true"\] \{~button[aria-pressed="never"] {~g' \
  'button[aria-pressed="never"]' \
  src/styles/tokens.test.ts 'marks the chosen theme' runner=vitest

case_ "Folders.svelte: the row never carries the class" \
  ui/src/settings/Folders.svelte \
  's~ class:excl=\{row\.excluded\}~~' \
  '<li class="sub" data-testid={`subfolder-' \
  src/settings/Folders.test.ts 'an excluded subfolder and one held from above are marked as excluded, an open one is not' runner=vitest

case_ "Folders.svelte: a row held by an ancestor rule is not excluded" \
  ui/src/settings/Folders.svelte \
  's~(control: '"'"'none'"'"',\n\s*expandable: false,\n\s*excluded: )true~${1}false~' \
  'expandable: false,
          excluded: false' \
  src/settings/Folders.test.ts 'an excluded subfolder and one held from above are marked as excluded, an open one is not' runner=vitest

case_ "settings/main.ts: the window does not import its own sheet" \
  ui/src/settings/main.ts \
  "s~import '../styles/settings.css';\n~~" \
  "import '../styles/base.css';
import { mount }" \
  src/styles/tokens.test.ts 'imports the stylesheets each window needs, in order' runner=vitest

case_ "Folders.svelte: the row loses its layout class" \
  ui/src/settings/Folders.svelte \
  's~<li class="sub" class:excl~<li class:excl~' \
  '<li class:excl={row.excluded}' \
  src/settings/Folders.test.ts 'an excluded subfolder and one held from above are marked as excluded, an open one is not' runner=vitest

# Rebound (PR 10f Task 7 review round 1, task-7-report.md fix report): "the
# current model looks like every other button" named a CSS rule
# (`aria-current`) and a test that are both gone — Task 4 replaced the row
# of buttons this case mutated with a native `<select>` (`Models.svelte`),
# whose selected-option state a browser draws itself, so there is no
# `aria-current` highlight left to break (`grep -n aria-current
# ui/src/styles/settings.css` is empty). But the CASE's own intent — the
# select must actually show the model the app THINKS is current, not some
# other value nobody wrote — has a live carrier one property over:
# `value={selectValue}` (`Models.svelte:942`), asserted at
# `Models.test.ts:1015` and `:1028` among others. Rebound there instead of
# invented back into CSS.
case_ "Models.svelte: the select stops showing which model is actually current" \
  ui/src/settings/Models.svelte \
  "s~        disabled=\{changeBusy\}\n        value=\{selectValue\}~        disabled={changeBusy}\n        value={''}~" \
  "value={''}" \
  src/settings/Models.test.ts 'the shown selection does not change until set_chat_model AND its re-read both resolve — not on the click alone' runner=vitest

case_ "settings.css: !important on a font property that already sets the triple" \
  ui/src/styles/settings.css \
  's~(\.spane h2 \{\n  margin: 0;\n  font-family: var\(--serif\));~$1 !important;~' \
  'var(--serif) !important;' \
  src/styles/tokens.test.ts 'sets the font family, weight and style together, and only as faces the bundle has' runner=vitest

case_ "Folders.svelte: the open arm marks itself excluded" \
  ui/src/settings/Folders.svelte \
  's~(control: '"'"'exclude'"'"', expandable: true, excluded: )false~${1}true~' \
  "control: 'exclude', expandable: true, excluded: true" \
  src/settings/Folders.test.ts 'an excluded subfolder and one held from above are marked as excluded, an open one is not' runner=vitest

case_ "tokens.test.ts: the font guard stops skipping the at-rule container" \
  ui/src/styles/tokens.test.ts \
  "s~    if \(rule\.selector\.startsWith\('\@'\)\) continue;\n~~" \
  'stripComments(css)))) {
    const loc = ' \
  src/styles/tokens.test.ts 'accepts a valid triple inside a media query' runner=vitest
