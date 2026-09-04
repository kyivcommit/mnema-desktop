import { afterEach, describe, expect, it } from 'vitest';
import { formatIndexedAt, formatIndexedDate } from './recency';
import { setLocale } from './index';

// `locale` is a module-level store shared by every test in this file; restore
// unconditionally, the way the other i18n suites do.
afterEach(() => setLocale('en'));

// A whole number of seconds, so `Math.floor(nowMs / 1000)` has nothing to round
// and every case below is exactly the delta it says it is.
const NOW_MS = 1_700_000_000_000;
const at = (secondsAgo: number) => 1_700_000_000 - secondsAgo;
const ago = (secondsAgo: number) => formatIndexedAt(at(secondsAgo), NOW_MS);

describe('formatIndexedAt', () => {
  // 🔴 The unit trap, and the reason the boundaries are pinned one second either
  // side rather than in the middle of each band. `indexedAt` is SECONDS
  // (`schema.sql:261`, `unixepoch()`) and `Date.now()` is milliseconds: an
  // implementation that subtracted them directly would render `19675 days ago`
  // for a document indexed a minute ago — a plausible sentence, and wrong by a
  // factor of a thousand. Every case here would redden.
  it('crosses each unit boundary where it says it does', () => {
    setLocale('en');
    expect(ago(0)).toBe('just now');
    expect(ago(59)).toBe('just now');
    expect(ago(60)).toBe('1 minute ago');
    expect(ago(119)).toBe('1 minute ago'); // floored, not rounded up
    expect(ago(3599)).toBe('59 minutes ago');
    expect(ago(3600)).toBe('1 hour ago');
    expect(ago(86_399)).toBe('23 hours ago');
    expect(ago(86_400)).toBe('1 day ago');
  });

  // A clock change, or a machine whose zone moved, can hand this card a
  // timestamp in the future. `-30 s` must not read as `-1 minutes ago` or as
  // `0 minutes ago`; under a minute in either direction is "just now".
  it('reads a timestamp from the future as just now, never as a negative count', () => {
    setLocale('en');
    expect(formatIndexedAt(at(-30), NOW_MS)).toBe('just now');
    expect(formatIndexedAt(at(-86_400), NOW_MS)).toBe('just now');
  });

  // Nothing caps the day count, and that is the decision rather than an
  // oversight: the Recents list is what was indexed LAST, so a large number
  // means nothing has been indexed since, and the sentence is still true.
  it('does not cap the day count', () => {
    setLocale('en');
    expect(ago(86_400 * 412)).toBe('412 days ago');
  });

  // The arms, pinned the way `i18n.test.ts` pins the catalogue's other plurals:
  // with counts, which need no fixture. Ukrainian takes the accusative after
  // «тому», and the teen exception is the case a hand-written rule gets wrong.
  it('applies the Ukrainian plural arms, including the teen exception', () => {
    setLocale('uk');
    expect(ago(0)).toBe('щойно');
    expect(ago(60)).toBe('1 хвилину тому');
    expect(ago(120)).toBe('2 хвилини тому');
    expect(ago(300)).toBe('5 хвилин тому');
    expect(ago(3600)).toBe('1 годину тому');
    expect(ago(2 * 3600)).toBe('2 години тому');
    expect(ago(5 * 3600)).toBe('5 годин тому');
    expect(ago(86_400)).toBe('1 день тому');
    expect(ago(2 * 86_400)).toBe('2 дні тому');
    expect(ago(5 * 86_400)).toBe('5 днів тому');
    expect(ago(11 * 86_400)).toBe('11 днів тому'); // teen → many
    expect(ago(21 * 86_400)).toBe('21 день тому');
  });

  it('applies the English arms in both directions', () => {
    setLocale('en');
    expect(ago(2 * 60)).toBe('2 minutes ago');
    expect(ago(2 * 3600)).toBe('2 hours ago');
    expect(ago(2 * 86_400)).toBe('2 days ago');
  });
});

// ---------------------------------------------------------------------------
// `formatIndexedDate` — D-e's other half. The relative phrase above answers
// "how long ago"; §9.3 asks for «останнє оновлення з датою», and a date is what
// a person compares against the file they edited this morning. It takes its
// locale as an ARGUMENT rather than reading the store, so the caller passes
// `$locale` and the value re-derives on a language switch like every other
// string on that screen.
// ---------------------------------------------------------------------------
describe('formatIndexedDate', () => {
  // A fixed instant, and the expectation COMPUTED here rather than written out.
  // The test machine's time zone is not the CI machine's, and a literal would
  // pin whichever zone the author happened to be in — the section deliberately
  // formats in the machine's own zone, because the person reading it is on that
  // machine.
  const AT = 1_700_000_000; // seconds, the unit `lastIndexedAt` arrives in
  const expected = (loc: string) =>
    new Intl.DateTimeFormat(loc, { dateStyle: 'long' }).format(new Date(AT * 1000));

  it('formats the timestamp as a date in the locale it is given', () => {
    // `uk` against `expected('uk')` with its trailing stop stripped: F1's own
    // test below covers the stripping itself in full, so this test's claim is
    // narrower — the rest of the string is ICU's, verbatim.
    expect(formatIndexedDate(AT, 'uk')).toBe(expected('uk').replace(/\.$/, ''));
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
  });

  // Both directions, and this is the assertion that says the argument is READ.
  // A body that ignored its `locale` and formatted in one fixed language would
  // satisfy one of the two cases above and every "is a non-empty string" check
  // anyone would think to add.
  it('gives two different strings for the two locales', () => {
    expect(formatIndexedDate(AT, 'uk')).not.toBe(formatIndexedDate(AT, 'en'));
  });

  // The seconds/milliseconds trap `formatIndexedAt`'s own header names, in the
  // form this function can make it: `Date` takes milliseconds, `lastIndexedAt`
  // is seconds, and a body that passed the number through unmultiplied would
  // render a date in January 1970 for every index ever built.
  it('reads its argument as seconds, not as milliseconds', () => {
    const asMilliseconds = new Intl.DateTimeFormat('en', { dateStyle: 'long' }).format(new Date(AT));
    expect(formatIndexedDate(AT, 'en')).not.toBe(asMilliseconds);
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
  });

  // F1 (measured live, 2026-09-04): `Intl.DateTimeFormat('uk', {dateStyle:
  // 'long'})` ends its own string in «р.» (an abbreviation stop, part of the
  // date itself, not a sentence's end), and `indexing_index_updated`
  // (`catalog.ts`) wraps it in «Останнє оновлення: {date}.» with a full stop
  // of its own — so the rendered sentence read «...1 вересня 2026 р..», two
  // stops where a reader expects one. This function is the one place that can
  // fix it for every caller, uk included: the sentence's own stop is the only
  // one that survives.
  it('carries no trailing stop, so the sentence around it supplies the only one', () => {
    // RED against the un-fixed implementation: ICU's own uk long-date form
    // ends in «р.», and the un-fixed function returned that verbatim.
    expect(formatIndexedDate(AT, 'uk').endsWith('.')).toBe(false);
    expect(formatIndexedDate(AT, 'uk')).toBe(expected('uk').replace(/\.$/, ''));
  });

  // `en`'s ICU form ends in a bare year with no stop at all
  // (`expected('en')` — "September 4, 2026" — is unaffected either way), so
  // this is the direction that catches a body stripping a trailing character
  // unconditionally rather than only a trailing stop.
  it('leaves a locale whose date carries no trailing stop unchanged', () => {
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
    expect(expected('en').endsWith('.')).toBe(false);
  });
});
