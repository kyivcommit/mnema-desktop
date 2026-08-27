import { t } from './index';
import { formatLocator } from './locator';
import type { AskCitation, Hit } from '../lib/ipc';

// Decision 1, in ONE place (Ruling S). `Answer`'s preview label and `Source`'s
// card header name the same file by the same rule, and two components carrying
// one rule is this project's most expensive defect class: one copy drifts and
// both suites stay green about their own.
//
// Three branches, and the order matters. A path is joined to its locator when
// there is one; a citation with NO path but a real location keeps the location
// — the branch the vanilla `hitLocation` rule got wrong — and only a citation
// with neither says there is no path on disk.
//
// ⚠️ Not reactive on its own: `t()` and `formatLocator()` both read the locale
// store at call time, so every caller must read `$locale` inside the `$derived`
// that calls this (D130).
export function citationLabel(c: AskCitation | Hit): string {
  const parts = [c.relativePath, formatLocator(c.coordinate)].filter(
    (p): p is string => !!p,
  );
  return parts.length > 0 ? parts.join(' · ') : t('no_path_on_disk');
}
