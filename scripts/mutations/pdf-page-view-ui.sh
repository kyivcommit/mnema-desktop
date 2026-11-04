# Mutation cases for the PDF page view's window half. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pdf-page-view-ui.sh
#
# What is silent when it is wrong here is a page that is not the page cited, or a
# picture of another page painted late over the right one: the window shows a
# PDF page with a box on it and nothing in it says which request it answered.
# The sibling `pdf-page-view.sh` holds the Rust half.
#
# Every case has one mutation and one test, and all of them run in jsdom. Expressions
# use `~` as the delimiter; a quote in a test name is written with the other kind
# of shell quote around the name.

PP=ui/src/launcher/PdfPage.svelte
FP=ui/src/launcher/FilePane.svelte
LA=ui/src/launcher/Launcher.svelte
PT=src/launcher/PdfPage.test.ts
FT=src/launcher/FilePane.test.ts
LT=src/launcher/LauncherPdf.test.ts

# ── Which page, and which answer ─────────────────────────────────────────────

# P1. A late answer for an older request is painted over a newer one.
case_ "page: a late reply is painted over a newer one" \
  $PP \
  's~\(got\) => \{ if \(id === request\) shown = got; \},~(got) => { shown = got; },~' \
  '(got) => { shown = got; },' \
  $PT 'a late answer for an older request is not painted' runner=vitest

# P2. The page opens on the first, not on the cited one.
case_ "page: the cited block's page is not the page shown" \
  $PP \
  's~const page = \$derived\(chosen \?\? focusPage\);~const page = \$derived(chosen ?? 1);~' \
  'chosen ?? 1);' \
  $PT "the page shown is the cited block's page, else the first" runner=vitest

# P3. Every span of the document is sent, whichever page it is on.
case_ "page: the spans of other pages are sent as ranges" \
  $PP \
  's~return view\.spans\.filter\(\(s\) => onPage\.has\(s\.blockId\)\);~return view.spans;~' \
  'return view.spans;' \
  $PT "only the spans of the shown page's block are sent as ranges" runner=vitest

# P4. The blocks of every page count as being on the shown one.
case_ "page: the blocks of other pages count as the shown page's" \
  $PP \
  's~view\.blocks\.filter\(\(b\) => b\.pageNo === page\)~view.blocks.filter((b) => b.pageNo > 0)~' \
  'b.pageNo > 0' \
  $PT "only the spans of the shown page's block are sent as ranges" runner=vitest

# P5. The old picture is dropped while the next page loads: the loading line
# flashes on every turn.
case_ "page: the old picture is dropped while the next page loads" \
  $PP \
  's~const id = \+\+request;~const id = ++request; shown = null;~' \
  'const id = ++request; shown = null;' \
  $PT 'the first load says it is loading; a later page keeps the old picture meanwhile' runner=vitest

# P6. A rejected command is still "loading", not "unavailable".
case_ "page: a rejected command leaves the page loading" \
  $PP \
  's~shown = \{ kind: .unavailable., reason: .failed. \};~void e;~' \
  'void e;' \
  $PT 'a missing file has its own line, and so does a rejected command' runner=vitest

# P7. A late rejection is shown over the newer page.
case_ "page: a late rejection replaces a newer page" \
  $PP \
  's~if \(id !== request\) return;~void id;~' \
  'void id;' \
  $PT 'a late answer for an older request is not painted' runner=vitest

# ── Turning the page ─────────────────────────────────────────────────────────

# N1. Before the first page.
case_ "nav: the keys turn before the first page" \
  $PP \
  's~if \(to >= 1 && \(pages === 0 \|\| to <= pages\)\) chosen = to;~if (pages === 0 \|\| to <= pages) chosen = to;~' \
  'if (pages === 0 || to <= pages) chosen = to;' \
  $PT 'the arrow keys turn the page inside the pane, and stop at both ends' runner=vitest

# N2. Past the last page.
case_ "nav: the keys turn past the last page" \
  $PP \
  's~if \(to >= 1 && \(pages === 0 \|\| to <= pages\)\) chosen = to;~if (to >= 1) chosen = to;~' \
  'if (to >= 1) chosen = to;' \
  $PT 'the arrow keys turn the page inside the pane, and stop at both ends' runner=vitest

# N3. The previous button is never disabled.
case_ "nav: the previous button is enabled on the first page" \
  $PP \
  's~data-testid="page-prev" disabled=\{page <= 1\}~data-testid="page-prev" disabled={false}~' \
  'data-testid="page-prev" disabled={false}' \
  $PT 'prev and next stop at the first and the last page' runner=vitest

# N4. The next button is never disabled.
case_ "nav: the next button is enabled on the last page" \
  $PP \
  's~disabled=\{page >= shown\.pages\}~disabled={false}~' \
  'disabled={false}' \
  $PT 'prev and next stop at the first and the last page' runner=vitest

# ── The notes under the page ─────────────────────────────────────────────────

# T1. The two notes swapped.
case_ "note: changed and scan say each other's line" \
  $PP \
  "s~case 'changed': return t\\('page_note_changed'\\);\\s*case 'scan': return t\\('page_note_scan'\\);~case 'changed': return t('page_note_scan'); case 'scan': return t('page_note_changed');~" \
  "case 'changed': return t('page_note_scan');" \
  $PT 'each note state says its line, and shown and noText say nothing' runner=vitest

# T2. A changed file says nothing.
case_ "note: a changed file has no note" \
  $PP \
  "s~case 'changed': return t\\('page_note_changed'\\);~case 'changed': return '';~" \
  "case 'changed': return '';" \
  $PT 'each note state says its line, and shown and noText say nothing' runner=vitest

# T3. A scan says nothing.
case_ "note: a scan has no note" \
  $PP \
  "s~case 'scan': return t\\('page_note_scan'\\);~case 'scan': return '';~" \
  "case 'scan': return '';" \
  $PT 'each note state says its line, and shown and noText say nothing' runner=vitest

# T4. A shown highlight is annotated as changed.
case_ "note: a shown highlight carries a note" \
  $PP \
  "s~case 'shown': return '';~case 'shown': return t('page_note_changed');~" \
  "case 'shown': return t('page_note_changed');" \
  $PT 'each note state says its line, and shown and noText say nothing' runner=vitest

# T5. A page the index holds no text for is annotated as changed.
case_ "note: a page with no indexed text carries a note" \
  $PP \
  "s~case 'noText': return '';~case 'noText': return t('page_note_changed');~" \
  "case 'noText': return t('page_note_changed');" \
  $PT 'each note state says its line, and shown and noText say nothing' runner=vitest

# ── An unavailable page ──────────────────────────────────────────────────────

# V1. No way out to the text.
case_ "unavailable: the page offers no way to the text" \
  $PP \
  's~\s*<button type="button" class="page-show-text" data-testid="page-show-text" onclick=\{onShowText\}>\s*\{L\.showText\}\s*</button>~~' \
  '<p role="status" data-testid="page-unavailable">{unavailable}</p>
  {:else}' \
  $PT 'an unavailable page says so and offers the text, which switches the mode' runner=vitest

# V2. The way out is not connected.
case_ "unavailable: the show-text button does nothing" \
  $PP \
  's~data-testid="page-show-text" onclick=\{onShowText\}~data-testid="page-show-text" onclick={() => {}}~' \
  'data-testid="page-show-text" onclick={() => {}}' \
  $PT 'an unavailable page says so and offers the text, which switches the mode' runner=vitest

# V3. A file that is gone is told as a page that failed.
case_ "unavailable: a missing file has no line of its own" \
  $PP \
  "s~return shown\\.reason === 'fileMissing' \\? t\\('page_unavailable_missing'\\) : t\\('page_unavailable'\\);~return t('page_unavailable');~" \
  "return t('page_unavailable');" \
  $PT 'a missing file has its own line, and so does a rejected command' runner=vitest

# ── The width asked ──────────────────────────────────────────────────────────

# W1. Rounded to the nearest step, not up: the picture is blurrier than the pane.
case_ "width: rounded to the nearest 200, not up" \
  $PP \
  's~Math\.ceil\(\(clientWidth \* ratio\) / 200\) \* 200~Math.round((clientWidth * ratio) / 200) * 200~' \
  'Math.round((clientWidth * ratio) / 200) * 200' \
  $PT "the width asked is the pane's device pixels, rounded up to 200 and capped at 3000" runner=vitest

# W2. The device pixel ratio is not applied: a retina screen gets half the pixels.
case_ "width: the device pixel ratio is not applied" \
  $PP \
  's~Math\.ceil\(\(clientWidth \* ratio\) / 200\)~Math.ceil(clientWidth / 200)~' \
  'Math.ceil(clientWidth / 200)' \
  $PT "the width asked is the pane's device pixels, rounded up to 200 and capped at 3000" runner=vitest

# W3. The cap is above the backend's.
case_ "width: the cap is not 3000" \
  $PP \
  's~Math\.min\(3000, Math\.max\(200,~Math.min(30000, Math.max(200,~' \
  'Math.min(30000, Math.max(200,' \
  $PT "the width asked is the pane's device pixels, rounded up to 200 and capped at 3000" runner=vitest

# W4. Not rounded at all: every pixel of a drag asks for a new picture.
case_ "width: not rounded, every pixel of a resize asks again" \
  $PP \
  's~Math\.ceil\(\(clientWidth \* ratio\) / 200\) \* 200~Math.ceil(clientWidth * ratio)~' \
  'Math.ceil(clientWidth * ratio)' \
  $PT 'a resize asks again only when the rounded width changes' runner=vitest

# ── The pane that holds it ───────────────────────────────────────────────────

# F1. The page mode does not say the cited passage is gone: page 1 is taken for
# the citation.
case_ "pane: the page mode does not say the passage is gone" \
  $FP \
  's~\{#if focusGone\}\s*<p role="status" data-testid="file-focus-gone">\{focusGoneLabel\}</p>\s*\{/if\}\s*<PdfPage~<PdfPage~' \
  '<PdfPage {location} {view}' \
  $FT 'a focus the backend rejected says the passage is gone in the page mode too' runner=vitest

# F2. Anything is drawn as a page in the page mode.
case_ "pane: a document that is not a PDF is drawn as a page" \
  $FP \
  "s~\\{:else if mode === 'page' && view\\.mime === 'application/pdf'\\}~{:else if mode === 'page'}~" \
  "{:else if mode === 'page'}" \
  $PT 'the text mode draws no page, and neither does a document that is not a PDF' runner=vitest

# F3. The page mode is drawn whatever the mode says.
case_ "pane: the text mode draws the page" \
  $FP \
  "s~\\{:else if mode === 'page' && view\\.mime === 'application/pdf'\\}~{:else if view.mime === 'application/pdf'}~" \
  "{:else if view.mime === 'application/pdf'}" \
  $PT 'the text mode draws no page, and neither does a document that is not a PDF' runner=vitest

# F4. The type of the document is never reported upward.
case_ "pane: the document's type is not reported" \
  $FP \
  's~\$effect\(\(\) => onMime\?\.\(view\?\.kind === .file. \? view\.mime : null\)\);~\$effect(() => onMime?.(null));~' \
  '$effect(() => onMime?.(null));' \
  $PT "the pane reports the document's type upward" runner=vitest

# ── The window: the choice of page or text ───────────────────────────────────

# L1. The choice is offered for any file that has a type.
case_ "launcher: the choice is offered for a file that is not a PDF" \
  $LA \
  "s~\\{#if fileMime === 'application/pdf'\\}~{#if fileMime !== null}~" \
  '{#if fileMime !== null}' \
  $LT 'the choice is not offered for anything but a PDF' runner=vitest

# L2. The choice is not written down.
case_ "launcher: the choice is not remembered" \
  $LA \
  's~try \{ localStorage\.setItem\(PDF_VIEW_KEY, v\); \} catch~try { void v; } catch~' \
  'try { void v; } catch' \
  $LT 'choosing the text shows the text, and the choice survives a remount' runner=vitest

# L3. The choice is written and never read back.
case_ "launcher: the remembered choice is not read" \
  $LA \
  "s~try \\{ return localStorage\\.getItem\\(PDF_VIEW_KEY\\) === 'text' \\? 'text' : 'page'; \\}~try { return 'page'; }~" \
  "try { return 'page'; }" \
  $LT 'choosing the text shows the text, and the choice survives a remount' runner=vitest

# L4. Nothing stored means the text.
case_ "launcher: the default is the text, not the page" \
  $LA \
  "s~=== 'text' \\? 'text' : 'page'; \\}~!== 'page' ? 'text' : 'page'; }~" \
  "!== 'page' ? 'text' : 'page'; }" \
  $LT 'a PDF is drawn as its page by default, with the choice in the right head' runner=vitest

# L5. A storage that refuses means the text.
case_ "launcher: a refusing storage gives the text, not the page" \
  $LA \
  "s~catch \\{ return 'page'; \\}~catch { return 'text'; }~" \
  "catch { return 'text'; }" \
  $LT 'a stored value that is neither is the default, and a refusing storage does not break it' runner=vitest

# L6. Whatever is stored is taken as the choice.
case_ "launcher: a stored value that is neither is taken as it is" \
  $LA \
  "s~return localStorage\\.getItem\\(PDF_VIEW_KEY\\) === 'text' \\? 'text' : 'page';~return (localStorage.getItem(PDF_VIEW_KEY) ?? 'page') as 'page' \\| 'text';~" \
  "?? 'page') as 'page' | 'text';" \
  $LT 'a stored value that is neither is the default, and a refusing storage does not break it' runner=vitest

# L7. The pane is told the page whatever was chosen.
case_ "launcher: the pane is not given the choice" \
  $LA \
  's~mode=\{pdfView\}~mode="page"~' \
  'mode="page"' \
  $LT 'choosing the text shows the text, and the choice survives a remount' runner=vitest

# L8. "Show text" on an unavailable page does not switch the pane.
case_ "launcher: show-text on an unavailable page does nothing" \
  $LA \
  "s~onShowText=\\{\\(\\) => setPdfView\\('text'\\)\\}~onShowText={() => {}}~" \
  'onShowText={() => {}}' \
  $LT '"Show text" on an unavailable page switches the whole pane to the text' runner=vitest
