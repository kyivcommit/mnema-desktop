// `scrollIntoView` scrolls every ancestor that has anything to scroll, the
// window's own `overflow: hidden` containers included, and a citation at the
// end of a file shifted the whole launcher up under its title bar (owner,
// 2026-10-22). This moves `box` alone.
export function scrollWithin(box: Element, el: Element, block: 'center' | 'nearest') {
  const b = box.getBoundingClientRect();
  const r = el.getBoundingClientRect();
  if (block === 'center') {
    box.scrollTop += r.top - b.top - (box.clientHeight - r.height) / 2;
    return;
  }
  box.scrollTop += nearest(r.top - b.top, r.bottom - b.bottom);
  box.scrollLeft += nearest(r.left - b.left, r.right - b.right);
}

// How far to move so the element's edges are inside the box's: nothing when
// they already are, its start edge first when it is the larger of the two.
function nearest(start: number, end: number): number {
  if (start < 0) return start;
  if (end > 0) return Math.min(start, end);
  return 0;
}
