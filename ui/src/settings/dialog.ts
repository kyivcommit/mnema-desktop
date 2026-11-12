// The confirmation popups of the folders tree and the mask editor (owner,
// 2026-11-12). A popup takes the keyboard when it opens — on "Cancel", the
// answer that changes nothing — keeps it while open (`aria-modal`: Tab wraps,
// focus sent outside comes back, Esc cancels from anywhere; PR #103 review 2),
// and hands it back to what had it when it closes.
export function dialogFocus(node: HTMLElement, cancel: () => void) {
  const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  // The Cancel button focuses itself (`focusCancel`), also when it appears
  // later — after the "checking" line. Only an empty dialog takes it here.
  if (!node.contains(document.activeElement)) node.focus();

  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
      return;
    }
    if (e.key !== 'Tab') return;
    const stops = [...node.querySelectorAll<HTMLElement>('button:not([disabled])')];
    if (stops.length === 0) {
      e.preventDefault();
      node.focus();
      return;
    }
    const first = stops[0];
    const last = stops[stops.length - 1];
    const at = document.activeElement;
    if (e.shiftKey && (at === first || !node.contains(at) || at === node)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (at === last || !node.contains(at))) {
      e.preventDefault();
      first.focus();
    }
  };
  const onFocusIn = (e: FocusEvent) => {
    if (e.target instanceof Node && !node.contains(e.target)) node.focus();
  };
  document.addEventListener('keydown', onKey, true);
  document.addEventListener('focusin', onFocusIn);
  return {
    update(next: () => void) {
      cancel = next;
    },
    destroy() {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('focusin', onFocusIn);
      before?.focus();
    },
  };
}

export function focusCancel(node: HTMLElement) {
  node.focus();
}
