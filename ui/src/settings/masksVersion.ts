import { writable } from 'svelte/store';

// Folders tree PR 2. A mask is global and the folders tree marks the files it
// removes (`mask *.log`), so the tree's open levels are stale the moment a mask
// is stored or removed. `Masks.svelte` bumps this after every answer; the
// folders tree re-reads what it has open. A counter, not an event: a
// subscriber that mounts later is not owed a re-read.
export const masksVersion = writable(0);
