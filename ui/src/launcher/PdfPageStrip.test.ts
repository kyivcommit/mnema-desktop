import { expect, test } from 'vitest';
import { GAP_PX, evict, nextRequests, pageAt, pageTop, wanted } from './PdfPage.svelte';

test('a page starts where the slots above it end, gaps included', () => {
  expect(pageTop(1, 500)).toBe(0);
  expect(pageTop(2, 500)).toBe(500 + GAP_PX);
  expect(pageTop(10, 500, 8)).toBe(9 * 508);
});

test('the page shown is the one with the most of it in the pane', () => {
  // Slots of 100 with gaps of 10: page 2 is 110..210, page 3 220..320.
  expect(pageAt(0, 100, 100, 5, 10)).toBe(1);
  expect(pageAt(150, 100, 100, 5, 10)).toBe(2); // 60 of page 2, 30 of page 3
  expect(pageAt(190, 100, 100, 5, 10)).toBe(3); // 20 of page 2, 70 of page 3
  expect(pageAt(5000, 100, 100, 5, 10)).toBe(5); // past the end: the last page
});

test('a tie goes to the page above', () => {
  expect(pageAt(165, 100, 100, 5, 10)).toBe(2); // 45 of page 2, 45 of page 3
  expect(pageAt(160, 110, 100, 5, 10)).toBe(2); // 50 of page 2, 50 of page 3
  expect(pageAt(166, 100, 100, 5, 10)).toBe(3); // 44 of page 2, 46 of page 3
});

test('one page is page 1 wherever the pane is', () => {
  expect(pageAt(0, 800, 100, 1)).toBe(1);
  expect(pageAt(900, 800, 100, 1)).toBe(1);
});

test('the pages drawn are the one in view first, then the one above, then below', () => {
  expect(wanted(5, 10)).toEqual([5, 4, 6]);
  expect(wanted(1, 10)).toEqual([1, 2]);
  expect(wanted(10, 10)).toEqual([10, 9]);
  expect(wanted(1, 1)).toEqual([1]);
});

test('no more than two are on their way, and nothing drawn or on its way is asked again', () => {
  const none = new Set<number>();
  expect(nextRequests([5, 4, 6], none, none)).toEqual([5, 4]);
  expect(nextRequests([5, 4, 6], new Set([5]), none)).toEqual([4]);
  expect(nextRequests([5, 4, 6], new Set([5, 9]), none)).toEqual([]);
  expect(nextRequests([5, 4, 6], none, new Set([5]))).toEqual([4, 6]);
  expect(nextRequests([5, 4, 6], none, new Set([5, 4, 6]))).toEqual([]);
});

test('past the cap the farthest pictures go, the page above first on a tie', () => {
  expect(evict([1, 2, 3, 4, 5], 3, 5)).toEqual([]);
  expect(evict([1, 2, 3, 4, 5, 9], 3, 5)).toEqual([9]);
  expect(evict([1, 2, 3, 4, 5], 3, 3)).toEqual([1, 5]);
  expect(evict([2, 3, 4], 3, 2)).toEqual([2]);
});
