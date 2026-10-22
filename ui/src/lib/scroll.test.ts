import { expect, test } from 'vitest';
import { scrollWithin } from './scroll';

// A box 100 tall at the page top, and an element 20 tall placed by `top`.
function place(top: number, left = 0) {
  const box = document.createElement('div');
  const el = document.createElement('div');
  box.append(el);
  document.body.append(box);
  Object.defineProperty(box, 'clientHeight', { value: 100 });
  box.getBoundingClientRect = () => ({ top: 0, bottom: 100, left: 0, right: 100, height: 100 }) as DOMRect;
  el.getBoundingClientRect = () => ({ top, bottom: top + 20, left, right: left + 20, height: 20 }) as DOMRect;
  const moved = { top: 0, left: 0 };
  Object.defineProperty(box, 'scrollTop', { get: () => 0, set: (v: number) => { moved.top = v; } });
  Object.defineProperty(box, 'scrollLeft', { get: () => 0, set: (v: number) => { moved.left = v; } });
  return { box, el, moved };
}

test('center puts the element in the middle of the box', () => {
  const { box, el, moved } = place(300);
  scrollWithin(box, el, 'center');
  expect(moved.top).toBe(300 - 40);
});

test('nearest leaves a visible element and brings a hidden one to the near edge', () => {
  const inside = place(50);
  scrollWithin(inside.box, inside.el, 'nearest');
  expect(inside.moved).toEqual({ top: 0, left: 0 });

  const below = place(150);
  scrollWithin(below.box, below.el, 'nearest');
  expect(below.moved.top).toBe(70); // its bottom edge to the box's bottom

  const above = place(-30, 130);
  scrollWithin(above.box, above.el, 'nearest');
  expect(above.moved).toEqual({ top: -30, left: 50 });
});
