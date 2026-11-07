# Mutation cases for the PDF page view's window half. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pdf-page-view-ui.sh
#
# What is silent when it is wrong here is a page that is not the page cited, a
# picture of another page painted late over the right one, or a strip whose
# pages sit where its arithmetic does not put them: the window shows a PDF page
# with a box on it and nothing in it says which request it answered.
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
ST=src/launcher/PdfPageStrip.test.ts

# ── Which pages, and which answers (the strip, D198) ─────────────────────────

# P1. The strip does not wait for the cited page: a neighbour goes out with it,
# before the page count is known.
case_ "strip: the cited page is not asked for alone" \
  $PP \
  's~const want = pages === 0 \? \[focusPage\]~const want = pages === 0 ? [focusPage, focusPage - 1]~' \
  'pages === 0 ? [focusPage, focusPage - 1]' \
  $PT 'the cited page is asked for first and alone; then the pages either side' runner=vitest

# P2. Every page is sent the cited page's spans: boxes on the wrong page.
case_ "strip: a neighbour is sent the cited page's spans" \
  $PP \
  's~renderPdfPage\(location, n, width, rangesFor\(n\)\)~renderPdfPage(location, n, width, rangesFor(focusPage))~' \
  'rangesFor(focusPage))' \
  $PT 'each page is sent its own spans as ranges' runner=vitest

# P3. Every span of the document is sent, whichever page it is on.
case_ "strip: the spans of other pages are sent as ranges" \
  $PP \
  's~return view\.spans\.filter\(\(s\) => onPage\.has\(s\.blockId\)\);~return view.spans;~' \
  'return view.spans;' \
  $PT 'each page is sent its own spans as ranges' runner=vitest

# P4. An answer asked in an older generation is painted over a newer one.
case_ "strip: a late answer of an older generation is painted" \
  $PP \
  's~    if \(g !== gen\) return;\n    inFlight\.delete\(n\);~    inFlight.delete(n);~' \
  '  function settle(n: number, g: number, got: PdfPageView) {
    inFlight.delete(n);' \
  $PT 'a late answer asked before the window came back is not painted' runner=vitest

# P5. An old generation's picture counts as drawn: the window coming back
# draws nothing again, and the boxes stay those of the old read.
case_ "strip: an old generation's picture counts as drawn" \
  $PP \
  's~if \(s\.gen === gen\) drawn\.add\(n\);~drawn.add(n);~' \
  'for (const [n, s] of slots) drawn.add(n);' \
  $PT 'the window coming back draws the pages in view again, the citation boxed again' runner=vitest

# P6. Pictures are never let go.
case_ "strip: no picture is ever let go" \
  $PP \
  's~for \(const k of evict\(\[\.\.\.slots\.keys\(\)\], here, CAP\)\) slots\.delete\(k\);~~' \
  'for (const [n, s] of slots) if (s.gen === gen) drawn.add(n);' \
  $PT 'past twelve pictures the farthest from the page in view are let go' runner=vitest

# P7. The cap is not twelve.
case_ "strip: the cap is not twelve" \
  $PP \
  's~const CAP = 12;~const CAP = 13;~' \
  'const CAP = 13;' \
  $PT 'past twelve pictures the farthest from the page in view are let go' runner=vitest

# P8. Scrolling asks for nothing: the strip below the cited page stays blank.
case_ "strip: a scroll asks for no page" \
  $PP \
  's~(at = pageAt\(scrollTop\(\), viewport\(\) - \(above \?\? ABOVE_GUESS_PX\), tall, pages\);\n)    pump\(\);\n~$1~' \
  'tall, pages);
  }' \
  $PT 'scrolling draws the page in view and its neighbours, and the page line follows' runner=vitest

# P9. The page line says the cited page wherever the strip is.
case_ "strip: the page line does not follow the scroll" \
  $PP \
  "s~t\\('page_of', \\{ n: here, m: pages \\}\\)~t('page_of', { n: focusPage, m: pages })~" \
  "t('page_of', { n: focusPage, m: pages })" \
  $PT 'scrolling draws the page in view and its neighbours, and the page line follows' runner=vitest

# P10. A neighbour's shape changes the width asked: a mixed book redraws the
# whole strip on every landscape page, for ever.
case_ "strip: any page's shape sets the width" \
  $PP \
  's~if \(n === focusPage && got\.heightPx > 0\) aspect~if (got.heightPx > 0) aspect~' \
  'if (got.heightPx > 0) aspect' \
  $PT "the cited page's shape sets the width asked; a neighbour of another shape does not" runner=vitest

# P11. A new width is asked for twice: by the answer that changed it and by
# the generation it starts.
case_ "strip: a new shape asks for the pages twice" \
  $PP \
  's~if \(width === asked\) pump\(\);~pump();~' \
  '    pump();
  }

  // The strip opens' \
  $PT "the cited page's shape sets the width asked; a neighbour of another shape does not" runner=vitest

# P12. The cited page's shape never corrects the portrait guess.
case_ "strip: the cited page's shape never corrects the width asked" \
  $PP \
  's~      if \(n === focusPage && got\.heightPx > 0\) aspect = got\.widthPx / got\.heightPx;\n~~' \
  'pages = got.pages;
      if (n === focusPage && !placed)' \
  $PT "the cited page's shape sets the width asked; a neighbour of another shape does not" runner=vitest

# ── Opening on the citation ──────────────────────────────────────────────────

# O1. The strip opens at its top, the cited page possibly hundreds of pages down.
case_ "open: the strip does not scroll to the cited page" \
  $PP \
  's~      setScrollTop\(pageTop\(focusPage, tall\)\);\n~~' \
  'void tick().then(() => {
      const r' \
  $PT 'every slot is as tall as the page, and the strip opens with the cited page at the top' runner=vitest

# O2. The slots have no gap: every page after the first is placed short of
# where the scroll arithmetic puts it.
case_ "open: the slots have no gap" \
  $PP \
  's~ style:margin-bottom="\{GAP_PX\}px"~~' \
  'style:height="{tall}px">' \
  $PT 'every slot is as tall as the page, and the strip opens with the cited page at the top' runner=vitest

# O3. A slot is not as tall as the page: the arithmetic places pages where
# they are not.
case_ "open: a slot is not as tall as the page" \
  $PP \
  's~data-page=\{n\}\n          style:height="\{tall\}px"~data-page={n}\n         ~' \
  'data-page={n}
          style:margin-bottom' \
  $PT 'every slot is as tall as the page, and the strip opens with the cited page at the top' runner=vitest

# O4. Every redraw scrolls back to the citation, over the person's own scroll.
case_ "open: a redraw scrolls back to the citation" \
  $PP \
  's~if \(n === focusPage && !placed\) place\(got\);~if (n === focusPage) place(got);~' \
  'if (n === focusPage) place(got);' \
  $PT "the strip opens scrolled sideways to the citation, and a redraw keeps the person's scroll" runner=vitest

# O5. The strip opens at its left edge, the citation possibly out of sight.
case_ "open: the strip does not scroll sideways to the citation" \
  $PP \
  's~strip\.scrollLeft = Math\.max~void Math.max~' \
  'void Math.max' \
  $PT "the strip opens scrolled sideways to the citation, and a redraw keeps the person's scroll" runner=vitest

# O6. The strip's real top is never measured: a page fitted to the guess runs
# below the pane's bottom.
case_ "open: where the strip starts is not measured" \
  $PP \
  's~      if \(top > 0\) above = top;~      void top;~' \
  '      void top;' \
  $PT 'the page takes its height from where the strip starts, under the page line' runner=vitest

# O7. A page not drawn yet is not marked busy for a screen reader.
case_ "open: a waiting page is not busy" \
  $PP \
  's~ aria-busy="true"~~' \
  'data-testid="page-wait"
' \
  $PT 'the first load says it is loading; a page not drawn yet holds its place' runner=vitest

# O8. Every picture is told as the same page.
case_ "open: the picture's text has no page number" \
  $PP \
  "s~t\\('page_image_alt', \\{ n \\}\\)~t('page_image_alt', { n: 1 })~" \
  "t('page_image_alt', { n: 1 })" \
  $PT "each page has its number in the picture's text" runner=vitest

# ── The notes over a cited page ──────────────────────────────────────────────

# T1. The two notes swapped.
case_ "note: changed and scan say each other's line" \
  $PP \
  "s~case 'changed': return t\\('page_note_changed'\\);\\s*case 'scan': return t\\('page_note_scan'\\);~case 'changed': return t('page_note_scan'); case 'scan': return t('page_note_changed');~" \
  "case 'changed': return t('page_note_scan');" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T2. A changed file says nothing.
case_ "note: a changed file has no note" \
  $PP \
  "s~case 'changed': return t\\('page_note_changed'\\);~case 'changed': return '';~" \
  "case 'changed': return '';" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T3. A scan says nothing.
case_ "note: a scan has no note" \
  $PP \
  "s~case 'scan': return t\\('page_note_scan'\\);~case 'scan': return '';~" \
  "case 'scan': return '';" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T4. A shown highlight is annotated as changed.
case_ "note: a shown highlight carries a note" \
  $PP \
  "s~case 'shown': return '';~case 'shown': return t('page_note_changed');~" \
  "case 'shown': return t('page_note_changed');" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T5. A page the index holds no text for is annotated as changed.
case_ "note: a page with no indexed text carries a note" \
  $PP \
  "s~case 'noText': return '';~case 'noText': return t('page_note_changed');~" \
  "case 'noText': return t('page_note_changed');" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T6. The unplaced note says nothing: a cited page with no box and no word.
case_ "note: an unplaced passage has no note" \
  $PP \
  "s~case 'unplaced': return t\\('page_note_unplaced'\\);~case 'unplaced': return '';~" \
  "case 'unplaced': return '';" \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# T7. Every page carries its note: a scanned book is a column of "scan" lines.
case_ "note: a page with nothing cited carries a note" \
  $PP \
  "s~    if \\(rangesFor\\(n\\)\\.length === 0\\) return '';\\n~~" \
  'void $locale;
    switch' \
  $PT 'each note says its line over the cited page, and a page with nothing cited has none' runner=vitest

# ── A page or a file that cannot be drawn ────────────────────────────────────

# V1. No way out to the text.
case_ "unavailable: the page offers no way to the text" \
  $PP \
  's~\s*<button type="button" class="page-show-text" data-testid="page-show-text" onclick=\{onShowText\}>\s*\{L\.showText\}\s*</button>~~' \
  '<p role="status" data-testid="page-unavailable">{unavailable}</p>
  {:else if pages === 0}' \
  $PT 'a file that cannot be drawn says so for the whole document and offers the text' runner=vitest

# V2. The way out is not connected.
case_ "unavailable: the show-text button does nothing" \
  $PP \
  's~data-testid="page-show-text" onclick=\{onShowText\}~data-testid="page-show-text" onclick={() => {}}~' \
  'data-testid="page-show-text" onclick={() => {}}' \
  $PT 'a file that cannot be drawn says so for the whole document and offers the text' runner=vitest

# V3. A file that is gone is told as a page that failed.
case_ "unavailable: a missing file has no line of its own" \
  $PP \
  "s~return gone\\.reason === 'fileMissing' \\? t\\('page_unavailable_missing'\\) : t\\('page_unavailable'\\);~return t('page_unavailable');~" \
  "return t('page_unavailable');" \
  $PT 'a file that cannot be drawn says so for the whole document and offers the text' runner=vitest

# V4. One page that fails takes the whole strip down.
case_ "unavailable: a page that fails takes the strip down" \
  $PP \
  "s~got\\.reason !== 'failed' \\|\\| pages === 0~got.reason !== 'failed' || true~" \
  "got.reason !== 'failed' || true" \
  $PT 'a page that could not be drawn says so in its own place; the rest of the strip is drawn' runner=vitest

# V5. A neighbour that finds the file gone is a page that failed: the strip
# goes on drawing pages of a file that is not there.
case_ "unavailable: a gone file is one page's failure" \
  $PP \
  "s~got\\.reason !== 'failed' \\|\\| pages === 0~pages === 0~" \
  "'unavailable' && (pages === 0))" \
  $PT 'a file that cannot be drawn says so for the whole document and offers the text' runner=vitest

# V6. A rejected command is no answer at all: the page waits for ever.
case_ "unavailable: a rejected command leaves the page waiting" \
  $PP \
  "s~        settle\\(n, g, \\{ kind: 'unavailable', reason: 'failed' \\}\\);~        void g;~" \
  '        void g;' \
  $PT 'a page that could not be drawn says so in its own place; the rest of the strip is drawn' runner=vitest

# ── The strip's arithmetic ───────────────────────────────────────────────────

# A1. A slot's top without the gaps above it.
case_ "math: pageTop leaves out the gaps" \
  $PP \
  's~return \(n - 1\) \* \(tall \+ gap\);~return (n - 1) * tall;~' \
  'return (n - 1) * tall;' \
  $ST 'a page starts where the slots above it end, gaps included' runner=vitest

# A2. A tie goes to the page below.
case_ "math: pageAt breaks a tie downward" \
  $PP \
  's~if \(seen > most\)~if (seen >= most)~' \
  'if (seen >= most)' \
  $ST 'a tie goes to the page above' runner=vitest

# A3. The page below is drawn before the page above.
case_ "math: wanted asks below before above" \
  $PP \
  's~\[at, at - 1, at \+ 1\]~[at, at + 1, at - 1]~' \
  '[at, at + 1, at - 1]' \
  $ST 'the pages drawn are the one in view first, then the one above, then below' runner=vitest

# A4. Three on their way.
case_ "math: three renders on their way" \
  $PP \
  's~inFlight\.size \+ start\.length >= limit~inFlight.size + start.length > limit~' \
  'inFlight.size + start.length > limit' \
  $ST 'no more than two are on their way, and nothing drawn or on its way is asked again' runner=vitest

# A5. A drawn page is asked for again.
case_ "math: a drawn page is asked again" \
  $PP \
  's~!inFlight\.has\(n\) && !cached\.has\(n\)~!inFlight.has(n)~' \
  'if (!inFlight.has(n)) start.push' \
  $ST 'no more than two are on their way, and nothing drawn or on its way is asked again' runner=vitest

# A6. The nearest pictures go first.
case_ "math: evict lets the nearest go" \
  $PP \
  's~Math\.abs\(b - at\) - Math\.abs\(a - at\)~Math.abs(a - at) - Math.abs(b - at)~' \
  'Math.abs(a - at) - Math.abs(b - at)' \
  $ST 'past the cap the farthest pictures go, the page above first on a tie' runner=vitest

# A7. On a tie the page below goes first.
case_ "math: evict breaks a tie downward" \
  $PP \
  's~\|\| a - b\)~|| b - a)~' \
  '|| b - a)' \
  $ST 'past the cap the farthest pictures go, the page above first on a tie' runner=vitest

# ── The width asked ──────────────────────────────────────────────────────────

# W1. Rounded to the nearest step, not up: the picture is blurrier than the pane.
case_ "width: rounded to the nearest 200, not up" \
  $PP \
  's~Math\.ceil\(\(clientWidth \* ratio\) / 200\) \* 200~Math.round((clientWidth * ratio) / 200) * 200~' \
  'Math.round((clientWidth * ratio) / 200) * 200' \
  $PT "the page is as tall as the pane: the width asked follows, rounded up to 200 and capped at 3000" runner=vitest

# W2. The device pixel ratio is not applied: a retina screen gets half the pixels.
case_ "width: the device pixel ratio is not applied" \
  $PP \
  's~Math\.ceil\(\(clientWidth \* ratio\) / 200\)~Math.ceil(clientWidth / 200)~' \
  'Math.ceil(clientWidth / 200)' \
  $PT "the page is as tall as the pane: the width asked follows, rounded up to 200 and capped at 3000" runner=vitest

# W3. The cap is above the backend's.
case_ "width: the cap is not 3000" \
  $PP \
  's~Math\.min\(3000, Math\.max\(200,~Math.min(30000, Math.max(200,~' \
  'Math.min(30000, Math.max(200,' \
  $PT "the page is as tall as the pane: the width asked follows, rounded up to 200 and capped at 3000" runner=vitest

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
