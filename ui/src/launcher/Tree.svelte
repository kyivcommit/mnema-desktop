<script module lang="ts">
  import type { TreeFile } from '../lib/ipc';

  export type TreeNode =
    | { kind: 'folder'; name: string; path: string; children: TreeNode[] }
    | { kind: 'file'; name: string; path: string; documentId: string };

  // The selection is the (root, path) PAIR, not `documentId`: one document can
  // sit at several paths (`schema.sql`'s `path` table), and a key on the id
  // would mark every row that shows it instead of the one that was opened.
  export type TreeSelection = { rootId: number; relativePath: string };
  // What a click carries: the selection plus the id the pane is opened by.
  export type TreePick = TreeSelection & { documentId: string };

  // Decision 6: the folder tree is a split of `relativePath` on `/`, and
  // nothing else — the backend hands over flat paths per root. Pure and
  // exported so the depth cases (no slash at all, two levels deep, both mixed
  // in one root) can be pinned without a render; a render can only show one
  // depth at a time, and only through the collapse rules.
  export function buildFolderTree(files: TreeFile[]): TreeNode[] {
    const top: TreeNode[] = [];
    const folders = new Map<string, Extract<TreeNode, { kind: 'folder' }>>();

    for (const file of files) {
      const parts = file.relativePath.split('/');
      const name = parts.pop()!; // split never returns an empty array
      let siblings = top;
      let path = '';
      for (const part of parts) {
        path = path ? `${path}/${part}` : part;
        let folder = folders.get(path);
        if (!folder) {
          folder = { kind: 'folder', name: part, path, children: [] };
          folders.set(path, folder);
          siblings.push(folder);
        }
        siblings = folder.children;
      }
      siblings.push({ kind: 'file', name, path: file.relativePath, documentId: file.documentId });
    }
    return top;
  }
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { locale, t } from '../i18n';
  import { listTree } from '../lib/ipc';
  import type { TreeListing } from '../lib/ipc';

  // Ruling O, restated for the pair: the selection is the file's place on
  // disk — `{ rootId, relativePath }` — and the click carries its `documentId`
  // as well, which is what the backend is asked to open.
  let { selected, onPick }: {
    selected: TreeSelection | null;
    onPick: (pick: TreePick) => void;
  } = $props();

  let listing = $state<TreeListing | null>(null);
  let failed = $state(false);
  // Folders the person opened or shut by hand, keyed per root: two roots may
  // hold the same folder path and must not share one open flag. The root row
  // itself lives in the same map under its root's `''` path.
  let toggled = $state<Record<string, boolean>>({});

  // Ruling M holds: this card is not keyed and does not refetch on a state
  // change — `state` changes twice per question and a remount would snap shut
  // every folder the person opened, in the card whose whole purpose is browsing
  // folder neighbours.
  //
  // 🔴 Owner review on PR #24, P3, is the thing that ruling never provided. A
  // mount-time snapshot goes stale: §7.3 keeps the window alive across a hide,
  // so one launcher outlives many index changes and keeps listing rows that are
  // no longer there. The trigger below is the window regaining focus, and it is
  // chosen rather than invented — it is the moment the launcher is in front of
  // the person again, which is when its listing is about to be read. It costs
  // nothing while the window is hidden and it is not tied to the answer state,
  // so the toggles, which are component state, are untouched by it.
  //
  // (The earlier version of this note justified the trigger from a blur handler
  // in `Launcher.svelte` that hides the launcher. There is none, and there was
  // not: a window close hides the launcher in Rust (`CloseRequested` →
  // `hide_launcher`), and the `Focused(false)` arm there only arms the cold
  // timer. Focus is this card's own signal, not half of a pair.)
  //
  // And the shell really does focus this window: every path that raises the
  // launcher goes through `focus_launcher` — the tray item, the second-instance
  // handler and `toggle_launcher` (the global shortcut) — and it runs `show()`,
  // `set_focus()`. No fourth path hides in configuration:
  // `src-tauri/tauri.conf.json` declares the `launcher` window
  // `"visible": false`, so the window manager never raises it on its own and
  // the three above are the whole set. (Review Minor 2: a grep over `*.rs`
  // alone leaves that open.)
  //
  // 🔴 One path needs no Rust at all, and it is the owner's own case (review
  // Minor 3). The launcher is a normal window (§1), so the person can bring it
  // back by clicking it in the Dock or the ⌘-Tab switcher, which regains DOM
  // focus through ordinary OS window activation — exactly the long-lived
  // launcher that outlives an index change. That path does not depend on the
  // link below.
  //
  // ⚠️ One link is still unmeasured, and it is narrower than the whole: whether
  // a webview `set_focus()` produces a DOM `focus` event on `window` in the
  // running application. Nobody has run this application yet, so that step is
  // read from the platform, not from a live launcher. It is the raise paths
  // that need it; the Dock/⌘-Tab path above does not.
  //
  // `loading` is a plain `let`, not `$state`: nothing renders from it. It stops
  // two listings being on the wire at once, where the loser lands last and puts
  // an older index on screen than the one the card already had.
  //
  // ⚠️ The cost of dropping rather than deferring (review Minor 7): a focus that
  // arrives while the mount request is still on the wire is discarded, not
  // queued, so a launcher raised during its own first fetch shows the snapshot
  // taken just before it was raised. The window is milliseconds wide and the
  // next focus refreshes; queueing would trade it for a second request whose
  // answer nobody is waiting for.
  let loading = false;
  function load() {
    if (loading) return;
    loading = true;
    listTree()
      .then((l) => { listing = l; failed = false; })
      .catch((e) => {
        // Non-fatal, but never silent: an unreadable listing must say so
        // rather than look like an empty index (Ruling N).
        console.error('list_tree failed', e);
        // 🔴 What disappears: only a card with NOTHING to show says so. A
        // refresh that fails must not take a listing that works off the screen
        // and leave the person with a message, on an event they did not cause.
        if (listing === null) failed = true;
      })
      .finally(() => { loading = false; });
  }

  onMount(load);

  const emptyLabel = $derived.by(() => { void $locale; return t('tree_empty'); });
  const failedLabel = $derived.by(() => { void $locale; return t('tree_failed'); });

  // Ruling P, reversed on purpose: the key is the (root, path) pair, never
  // `documentId` — two roots can hold the same relative path under different
  // documents, and one document can be named from several paths. Only the row
  // that was opened is marked.
  const selectedKey = $derived(selected === null ? null : key(selected.rootId, selected.relativePath));

  const roots = $derived(
    (listing?.roots ?? []).map((root) => ({ root, nodes: buildFolderTree(root.files) })),
  );
  const isEmpty = $derived(listing !== null && listing.roots.length === 0);

  // NUL as the separator because no path component can contain one, so no two
  // distinct (root, folder) pairs can collide on one key. It must be written as
  // the escape `\0`: a raw NUL byte here makes the whole file binary — grep and
  // rg go silent on it and `git diff` prints "Binary files differ", hiding the
  // component from every review.
  function key(rootId: number, path: string) {
    return `${rootId}\0${path}`;
  }

  // Folders start expanded along the path to the selected file and collapsed
  // everywhere else; the root holding it joins the set, so a root row the
  // person shut is re-opened by a new selection exactly like a folder on the
  // way. Derived from `selected`, so a new selection opens its folder without
  // a refetch; `toggled` still wins, so an opened folder the person is reading
  // does not shut itself on the next answer.
  const openByDefault = $derived.by(() => {
    const open = new Set<string>();
    if (selectedKey === null || listing === null) return open;
    for (const root of listing.roots) {
      for (const file of root.files) {
        if (key(root.rootId, file.relativePath) !== selectedKey) continue;
        open.add(key(root.rootId, ''));
        const parts = file.relativePath.split('/');
        parts.pop();
        let path = '';
        for (const part of parts) {
          path = path ? `${path}/${part}` : part;
          open.add(key(root.rootId, path));
        }
      }
    }
    return open;
  });

  function isOpen(rootId: number, path: string) {
    const k = key(rootId, path);
    // The root row (path '') starts EXPANDED (owner, 2026-10-20): collapsing
    // is the person's option, never the default.
    return k in toggled ? toggled[k] : path === '' || openByDefault.has(k);
  }

  function toggle(rootId: number, path: string) {
    const k = key(rootId, path);
    toggled = { ...toggled, [k]: !isOpen(rootId, path) };
  }

  // Folders on the selected file's path that the person had shut are set open
  // when a NEW selection lands there — and the root row with them, since it is
  // the outermost folder on every path inside it. `toggled` still wins over `openByDefault`
  // everywhere else (Ruling M): a folder the person opened or shut somewhere
  // else is none of this selection's business, and clearing those would snap
  // every folder shut on each click.
  //
  // The stamp is the selection key plus the folders on its way, so the effect
  // fires when the selection changes or when the listing that completes it
  // arrives — and not on every refresh. A refresh that returns the same listing
  // moves nothing. A file that moves to a new path no longer matches the pair,
  // so it is no longer selected and no folder is re-opened: the tree stops
  // claiming the pane's file is there, which is the honest answer.
  //
  // Everything it writes is read inside `untrack`, so this effect depends on
  // the selection and the listing, never on what it sets.
  let lastSelection = '';
  $effect(() => {
    const id = selectedKey;
    const l = listing;
    const onTheWay = openByDefault;
    const stamp = JSON.stringify([id, [...onTheWay].sort()]);
    if (stamp === lastSelection) return;
    lastSelection = stamp;
    if (id === null || l === null) return;

    untrack(() => {
      // Only the folders explicitly SHUT by hand on this selection's path, and
      // they are set open rather than forgotten. Both halves are load-bearing:
      //
      // - a folder the person OPENED by hand is not touched at all. Deleting
      //   its entry was measured and it takes Ruling M's defect back by the
      //   long way round: with the entry gone the folder is open only while
      //   the selection is, so it snapped shut the moment the next answer was
      //   a refusal — a folder the person opened, closing itself on an event
      //   they did not cause.
      // - the shut one is set to `true`, not deleted, for the same reason in
      //   reverse: `true` is what "this folder is open" is written as, and it
      //   survives the selection going away the way a hand-open does.
      //
      // A folder with no entry keeps having none: it is open because the
      // selection is on its path, and it shuts again when that stops being
      // true. That is the default, and it is not a person's decision to keep.
      const next = { ...toggled };
      let changed = false;
      for (const k of onTheWay) {
        if (next[k] === false) { next[k] = true; changed = true; }
      }
      if (changed) toggled = next;
    });
  });
</script>

{#snippet branch(nodes: TreeNode[], rootId: number)}
  <ul>
    {#each nodes as node (node.path)}
      <li>
        {#if node.kind === 'folder'}
          <button
            type="button"
            data-testid={`tree-folder-${node.path}`}
            aria-expanded={isOpen(rootId, node.path)}
            onclick={() => toggle(rootId, node.path)}>{node.name}</button>
          {#if isOpen(rootId, node.path)}
            {@render branch(node.children, rootId)}
          {/if}
        {:else}
          <!-- The row is a real action now: clicking it opens the file in the
               pane at its start (spec §2, §3). The selection — and therefore
               `aria-current` — is the (root, path) pair, so two rows naming the
               same document are not both marked; only the one that was opened
               is. -->
          <button
            type="button"
            class="file-row"
            data-testid={`tree-file-${node.documentId}`}
            aria-current={key(rootId, node.path) === selectedKey ? 'true' : undefined}
            onclick={() => onPick({ rootId, relativePath: node.path, documentId: node.documentId })}
            >{node.name}</button>
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<svelte:window onfocus={load} />

<div class="tree" data-testid="tree-body">
  {#if failed}
    <p data-testid="tree-failed">{failedLabel}</p>
  {:else if isEmpty}
    <p data-testid="tree-empty">{emptyLabel}</p>
  {:else}
    {#each roots as { root, nodes } (root.rootId)}
      <section data-testid={`tree-root-${root.rootId}`}>
        <ul>
          <li>
            <!-- The root is a disclosure row like the folders below it (owner,
                 2026-10-20): a flat list under a bare heading read as a pile
                 with no way to fold it. -->
            <button
              type="button"
              data-testid={`tree-root-toggle-${root.rootId}`}
              aria-expanded={isOpen(root.rootId, '')}
              onclick={() => toggle(root.rootId, '')}>{root.name}</button>
          </li>
        </ul>
        <!-- A sibling of the root's own list, not a child of its <li>: nested
             there, level-1 folders inherit the `ul ul` indent meant for level 2
             and deeper (`launcher.css`). -->
        {#if isOpen(root.rootId, '')}
          {@render branch(nodes, root.rootId)}
        {/if}
      </section>
    {/each}
  {/if}
</div>
