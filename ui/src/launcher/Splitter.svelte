<script lang="ts">
  // §2: a 1px line that widens its HIT TARGET on hover, not its paint — no
  // visible handle, so the flat header of the sheet is not argued with. The
  // drag is reported as a delta; the caller owns the width and its minimums,
  // because every pane's cap depends on the other panes, not on this line.
  //
  // The reset is on the keyboard too (Enter, Home): a double-click is
  // mouse-only, and the affordance exists for everyone.
  let { label, value, min, max, onResize, onReset }: {
    label: string;
    value: number;
    min: number;
    max: number;
    onResize: (delta: number) => void;
    onReset: () => void;
  } = $props();

  let dragging = false;
  let last = 0;

  function down(event: PointerEvent) {
    dragging = true;
    last = event.clientX;
    // jsdom has none; a real webview keeps the drag while the pointer leaves
    // the 1px line.
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  }

  function move(event: PointerEvent) {
    if (!dragging) return;
    const delta = event.clientX - last;
    last = event.clientX;
    if (delta !== 0) onResize(delta);
  }

  function up(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
  }

  function key(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === 'Home') {
      event.preventDefault();
      onReset();
      return;
    }
    const step = event.key === 'ArrowLeft' ? -8 : event.key === 'ArrowRight' ? 8 : 0;
    if (step === 0) return;
    event.preventDefault();
    onResize(step);
  }
</script>

<!-- ARIA 1.2 makes a FOCUSABLE separator a widget: it is a drag target with
     arrow-key resizing, and the ignore is for Svelte's default role table. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={Math.round(value)}
  aria-valuemin={min}
  aria-valuemax={Math.round(max)}
  tabindex="0"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  ondblclick={onReset}
  onkeydown={key}></div>
