import { writable } from 'svelte/store';

// Owner, 2026-11-12: a stored tick or mask does nothing until a scan, and the
// window said so nowhere. Set by every change the folders tree or the mask
// editor stores; `Apply.svelte` shows "Apply changes" while it is set and
// clears it when a scan runs, from anywhere.
// shortcut: lives in this window only; closing it forgets the reminder, not the change.
export const unapplied = writable(false);
