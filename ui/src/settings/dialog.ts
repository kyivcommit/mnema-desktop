// The confirmation popups of the folders tree and the mask editor (owner,
// 2026-11-12). A popup takes the keyboard when it opens — on "Cancel", the
// answer that changes nothing — and hands it back to what had it when it closes.
export function dialogFocus(node: HTMLElement) {
  const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  // The Cancel button focuses itself (`focusCancel`), also when it appears
  // later — after the "checking" line. Only an empty dialog takes it here.
  if (!node.contains(document.activeElement)) node.focus();
  return {
    destroy() {
      before?.focus();
    },
  };
}

export function focusCancel(node: HTMLElement) {
  node.focus();
}
