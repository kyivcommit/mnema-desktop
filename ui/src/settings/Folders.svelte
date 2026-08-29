<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import {
    listTree, addWatchedFolder, removeWatchedFolder,
    listSubfolders, listExclusions, excludeSubfolder, includeSubfolder,
    type StoredExclusion, type Subfolder, type SubfolderListing, type SubfolderState,
    type TreeRoot,
  } from '../lib/ipc';
  import type { JobController, JobPhase } from './jobs';

  // §9.2, Tasks 7 and 8 — the folder surface: add, list, remove, and scan.
  //
  // The scan starts on a button of its own and NEVER on adding a folder,
  // because excluding subfolders (PR 8) is a configuration move a person still
  // has to make in between — a walk started by `add_watched_folder` would run
  // before that chance exists. This component still never calls the walk
  // itself: it asks the controller `Settings.svelte` owns, so the job outlives
  // a click on another section.
  //
  // Fixture question: `TreeRoot` (`ipc.ts`) carries `files: TreeFile[]` and no
  // flag for whether a walk has ever run. A folder just added and a folder
  // walked to genuine completion with nothing in it are the SAME value on
  // this wire — so this screen never says a folder is "empty"; it states the
  // count `indexed_documents` gives, which is true of both states, and stops
  // there. Telling the two apart belongs to the task that runs the walk.

  // Required, not optional: a folder list that quietly drops its scan control
  // when nobody passes one is the shape a person opens to find nothing happens.
  let { jobs }: { jobs: JobController } = $props();

  let roots = $state<TreeRoot[]>([]);
  // Set on a rejected `list_tree`, at mount or on a later re-read. Held apart
  // from `actionError` (Models.svelte's own precedent) because this one means
  // the list on screen cannot be trusted at all, not that one action failed.
  let loadError = $state<string | null>(null);
  // Set on a rejected add/remove. The backend's own sentence, shown verbatim
  // beside no lead-in of this component's own — §10: a rejection arrives as
  // a sentence, never a kind, and no re-read is implied by it.
  let actionError = $state<string | null>(null);

  // ── PR 8a, Task 5: what an expanded folder holds ──────────────────────────
  //
  // One expansion per watched root, present exactly while that row is open —
  // collapsing DELETES the entry rather than hiding it, because a kept listing
  // is a claim about a moment that has passed and the disk can change between
  // two expands.
  //
  // `tree` mirrors the open state instead of being a flat cache keyed by path:
  // a child is fetched only while it is open, so re-reading after an action is
  // one walk of the same shape and a subfolder that stopped existing — or
  // stopped being expandable, because a rule now names it — simply does not
  // come back.
  type SubTree = { listing: SubfolderListing; children: Record<string, SubTree> };
  type Panel = {
    tree: SubTree | null;
    rules: StoredExclusion[] | null;
    // A rejected `list_subfolders`/`list_exclusions`. Held apart from the two
    // banners above for the reason those two are held apart from each other:
    // this one means the SUBFOLDERS of one row cannot be trusted, while the
    // list of watched folders is fine and no add or remove has failed.
    loadError: string | null;
    // A rejected exclude/include, shown verbatim (§10).
    actionError: string | null;
    // `include_subfolder` answered `false`: there was no rule left to remove.
    // A fact, not an error, and stored as the fact rather than as its sentence
    // so a language switch re-renders it.
    alreadyGone: boolean;
  };
  let panels = $state<Record<number, Panel>>({});

  // Not `$state`: nothing renders from it. One counter per root, bumped by
  // every read AND by every collapse, so a listing still on the wire when the
  // row is shut — or when a newer read has started — is dropped instead of
  // being drawn over whatever the person is looking at now.
  const generations: Record<number, number> = {};

  function message(e: unknown) {
    return e instanceof Error ? e.message : String(e);
  }

  function patch(rootId: number, fields: Partial<Panel>) {
    const panel = panels[rootId];
    if (panel === undefined) return; // the row was shut while this was running
    panels = { ...panels, [rootId]: { ...panel, ...fields } };
  }

  function openPathsOf(node: SubTree | null): Set<string> {
    const paths = new Set<string>();
    const visit = (n: SubTree) => {
      for (const [path, child] of Object.entries(n.children)) {
        paths.add(path);
        visit(child);
      }
    };
    if (node !== null) visit(node);
    return paths;
  }

  async function fetchTree(rootId: number, path: string, want: Set<string>): Promise<SubTree> {
    const listing = await listSubfolders(rootId, path);
    const children: Record<string, SubTree> = {};
    for (const entry of listing.entries) {
      // `describe` decides what may be opened, and it is the same call the row
      // itself renders from — one classifier, so a subtree cannot appear under
      // a row that offers no control to open it.
      if (!want.has(entry.relativePath) || !describe(entry.state).expandable) continue;
      children[entry.relativePath] = await fetchTree(rootId, entry.relativePath, want);
    }
    return { listing, children };
  }

  // The disk and the stored rules, read together: the rule list is not
  // derivable from the listing (a rule may name a folder several levels down,
  // or one that is no longer there at all), and the listing is not derivable
  // from the rules.
  async function read(rootId: number, want: Set<string>) {
    const generation = (generations[rootId] ?? 0) + 1;
    generations[rootId] = generation;
    try {
      const [tree, rules] = await Promise.all([
        fetchTree(rootId, '', want),
        listExclusions(rootId),
      ]);
      if (generations[rootId] !== generation) return;
      patch(rootId, { tree, rules, loadError: null });
    } catch (e) {
      if (generations[rootId] !== generation) return;
      // Both cleared: half a listing beside a sentence saying it could not be
      // read is a screen that contradicts itself.
      patch(rootId, { tree: null, rules: null, loadError: message(e) });
    }
  }

  function toggleRoot(rootId: number) {
    if (panels[rootId] !== undefined) {
      generations[rootId] = (generations[rootId] ?? 0) + 1;
      const next = { ...panels };
      delete next[rootId];
      panels = next;
      return;
    }
    panels = {
      ...panels,
      [rootId]: { tree: null, rules: null, loadError: null, actionError: null, alreadyGone: false },
    };
    void read(rootId, new Set());
  }

  // Shutting a subfolder drops its subtree here and now, without a read: the
  // person asked for less on screen, and nothing about the folders that remain
  // has changed. Opening one re-reads the whole panel — the row that was
  // pressed is not the only one whose state a rule added elsewhere can change.
  function toggleSubfolder(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined || panel.tree === null) return;
    const paths = openPathsOf(panel.tree);
    if (paths.has(path)) {
      patch(rootId, { tree: prune(panel.tree, path) });
      return;
    }
    void read(rootId, new Set([...paths, path]));
  }

  function prune(node: SubTree, path: string): SubTree {
    const children: Record<string, SubTree> = {};
    for (const [key, child] of Object.entries(node.children)) {
      if (key === path) continue;
      children[key] = prune(child, path);
    }
    return { listing: node.listing, children };
  }

  async function exclude(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const want = openPathsOf(panel.tree);
    patch(rootId, { actionError: null, alreadyGone: false });
    try {
      await excludeSubfolder(rootId, path);
    } catch (e) {
      patch(rootId, { actionError: message(e) });
    }
    // Unconditional, and outside the `try`: `exclude_subfolder` can refuse a
    // path this listing showed (`Error::AlreadyPrunedByBuiltIn`), and what the
    // rows must say next is decided by re-reading the state — never by parsing
    // a rejection, which crosses the IPC as a sentence and nothing else.
    await read(rootId, want);
  }

  async function include(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const want = openPathsOf(panel.tree);
    patch(rootId, { actionError: null, alreadyGone: false });
    try {
      const removed = await includeSubfolder(rootId, path);
      patch(rootId, { alreadyGone: !removed });
    } catch (e) {
      patch(rootId, { actionError: message(e) });
    }
    await read(rootId, want);
  }

  async function refresh() {
    const listing = await listTree();
    roots = listing.roots;
    // An expansion belongs to a row, and outlives it for no longer than the
    // row itself: `remove_watched_folder` deletes a database row whose id
    // SQLite may hand out again, and an expansion left behind under that id
    // would draw one folder's subfolders under another folder's path.
    const live = new Set(roots.map((r) => r.rootId));
    const kept = Object.fromEntries(
      Object.entries(panels).filter(([rootId]) => live.has(Number(rootId))),
    );
    if (Object.keys(kept).length !== Object.keys(panels).length) panels = kept;
    loadError = null; // a successful read is proof the earlier one is stale
  }

  function reread() {
    refresh().catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
  }

  onMount(() => {
    reread();
    // Live run, finding 2. Task 7 re-reads after an add and after a remove;
    // the event nobody wired is the one that changes the NUMBER this list shows
    // — a job ending. The row went on stating zero indexed documents while the
    // report under it said four had been added, and the index agreed with the
    // report, not with the row.
    //
    // EVERY ending, not only a walk's, and the reason is asymmetry rather than
    // caution: a re-read that finds the same numbers rewrites them invisibly,
    // while a missed one leaves a falsehood a person can act on. "Which pass
    // writes documents" is also a fact about today's two passes, not about this
    // row — and this window does not always know what is running at all
    // (`runningUnobserved` is the state where it has no channel), so keying off
    // the pass would be keying off something it cannot always see. Endings are
    // rare: at most a handful per run, never one per progress report.
    //
    // Compared by phase IDENTITY, not by kind: the controller writes a fresh
    // phase object per event, so a progress report changes the object without
    // ever being an ending, and an ending is written exactly once. Seeded with
    // what the store already holds so a section switch back to this list does
    // not read it twice on the same mount.
    let seen: JobPhase = get(jobs.state).phase;
    return jobs.state.subscribe(({ phase }) => {
      if (phase === seen) return;
      seen = phase;
      if (phase.kind === 'ended') reread();
    });
  });

  async function addFolder() {
    actionError = null;
    const selected = await open({ directory: true });
    if (selected === null) return; // cancelled dialog — calls nothing further
    try {
      await addWatchedFolder(selected);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      return;
    }
    // Outside the action's own try: a rejection here is list_tree's own
    // sentence, about the list being unreadable, not about the add having
    // failed — the add already succeeded by this point.
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  async function removeFolder(rootId: number) {
    actionError = null;
    try {
      await removeWatchedFolder(rootId);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      return;
    }
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  const emptyLabel = $derived.by(() => { void $locale; return t('settings_folders_empty'); });
  const addLabel = $derived.by(() => { void $locale; return t('settings_folders_add'); });
  const removeLabel = $derived.by(() => { void $locale; return t('settings_folders_remove'); });
  const scanLabel = $derived.by(() => { void $locale; return t('settings_folders_scan'); });
  const loadFailedLabel = $derived.by(() => { void $locale; return t('settings_folders_load_failed'); });

  // Rows carry their own count label, not a bare `t()` call in the markup —
  // a call inside `{#each}` reads `get(locale)` non-reactively (`i18n/index.ts:11`)
  // and would not update on a language switch. The whole array is rebuilt
  // under `void $locale` instead, the shape `launcher/Tree.svelte`'s own
  // `recents` derived already uses for the same reason.
  //
  // `settings_folders_indexed`, not the shared `indexed_documents` (P2-4
  // review): a bare "0 documents" beside a folder path reads as a claim
  // about the FOLDER, and the ruling means every newly added folder shows zero
  // forever, not transiently — this key names the INDEX as the subject
  // ("Indexed: 0 documents") instead. `removeAriaLabel` carries the same
  // row's path so two "Remove" buttons in a two-folder list stay
  // distinguishable to a screen reader (P2-5); the visible button text
  // stays the plain `removeLabel` above.
  // 🔴 ONE classifier over `SubfolderState`, and no default arm. Every fact a
  // row shows about a state — its sentence, which control it offers, whether it
  // can be opened — is decided here and nowhere else, so no two of them can
  // disagree about the same state. The `never` binding is what makes a seventh
  // variant added to `tree.rs` and mirrored in `ipc.ts` fail `npm run check`
  // instead of rendering as an ordinary excludable folder.
  //
  // **Four of the six offer no toggle, and the acceptance criterion this task
  // must not break is why:** no folder the walk will prune may be offered to a
  // person as excludable. `builtIn`, `symlink` and `excludedByAncestor` are all
  // already pruned, and `unusableName` is refused for the opposite reason — its
  // contents ARE walked, and its name is one no rule can carry — which is why
  // its sentence says something different rather than something similar.
  //
  // **Only `open` can be expanded, and `symlink` is the load-bearing case.**
  // `subfolder_state` asks `is_symlink` about the entry itself, so a directory
  // INSIDE a symlinked one comes back `open` — offering "exclude" there would
  // write a rule that excludes nothing, over a subtree the walk never enters.
  // The two rule states are shut for a weaker reason: everything under them is
  // already held by a rule, so there is nothing there to decide.
  function describe(state: SubfolderState): {
    sentence: string;
    control: 'exclude' | 'include' | 'none';
    expandable: boolean;
  } {
    switch (state.kind) {
      case 'open':
        return { sentence: t('settings_subfolder_open'), control: 'exclude', expandable: true };
      case 'excluded':
        return { sentence: t('settings_subfolder_excluded'), control: 'include', expandable: false };
      case 'excludedByAncestor':
        return {
          // The prefix the STATE carries, not this row's own path: they differ,
          // and the one worth showing is the rule that has to go first.
          sentence: t('settings_subfolder_excluded_by_ancestor', { prefix: state.prefix }),
          control: 'none',
          expandable: false,
        };
      case 'builtIn':
        return { sentence: t('settings_subfolder_built_in'), control: 'none', expandable: false };
      case 'symlink':
        return { sentence: t('settings_subfolder_symlink'), control: 'none', expandable: false };
      case 'unusableName':
        return { sentence: t('settings_subfolder_unusable_name'), control: 'none', expandable: false };
      default: {
        const unreachable: never = state;
        return unreachable;
      }
    }
  }

  type SubRow = {
    entry: Subfolder;
    sentence: string;
    control: 'exclude' | 'include' | 'none';
    controlLabel: string | null;
    controlAriaLabel: string | null;
    costLabel: string | null;
    expandable: boolean;
    expandAriaLabel: string;
    open: boolean;
    children: Level | null;
  };
  type Level = { unnameableLabel: string | null; emptyLabel: string | null; rows: SubRow[] };

  function buildLevel(node: SubTree): Level {
    const rows = node.listing.entries.map((entry) => {
      const { sentence, control, expandable } = describe(entry.state);
      const child = node.children[entry.relativePath];
      return {
        entry,
        sentence,
        control,
        controlLabel:
          control === 'exclude' ? t('settings_subfolder_exclude')
          : control === 'include' ? t('settings_subfolder_include')
          : null,
        controlAriaLabel:
          control === 'exclude' ? t('settings_subfolder_exclude_named', { path: entry.relativePath })
          : control === 'include' ? t('settings_subfolder_include_named', { path: entry.relativePath })
          : null,
        // The disclosure sits beside the control BEFORE it is pressed: taking a
        // rule away is not a tidy-up, it puts a subtree back in front of the
        // next scan.
        costLabel: control === 'include' ? t('settings_folders_rule_cost') : null,
        expandable,
        expandAriaLabel: t('settings_folders_expand_named', { path: entry.relativePath }),
        open: child !== undefined,
        children: child === undefined ? null : buildLevel(child),
      };
    });
    return {
      unnameableLabel:
        node.listing.unnameable > 0
          ? t('settings_subfolders_unnameable', { count: node.listing.unnameable })
          : null,
      // Only when there is genuinely nothing: a folder holding entries this
      // listing cannot name says how many instead, and must not also claim to
      // hold no subfolders.
      emptyLabel:
        rows.length === 0 && node.listing.unnameable === 0 ? t('settings_subfolders_none') : null,
      rows,
    };
  }

  const rows = $derived.by(() => {
    void $locale;
    return roots.map((root) => {
      const panel = panels[root.rootId];
      return {
        root,
        countLabel: t('settings_folders_indexed', { count: root.files.length }),
        removeAriaLabel: t('settings_folders_remove_named', { path: root.absolutePath }),
        scanAriaLabel: t('settings_folders_scan_named', { path: root.absolutePath }),
        expandAriaLabel: t('settings_folders_expand_named', { path: root.absolutePath }),
        expanded: panel !== undefined,
        panel:
          panel === undefined
            ? null
            : {
                level: panel.tree === null ? null : buildLevel(panel.tree),
                loadingLabel:
                  panel.tree === null && panel.loadError === null
                    ? t('settings_subfolders_loading')
                    : null,
                loadError: panel.loadError,
                failedLabel: t('settings_subfolders_failed'),
                actionError: panel.actionError,
                note: panel.alreadyGone ? t('settings_folders_rule_already_gone') : null,
                // 🔴 A rule is "the folder is gone" when and only when its own
                // `existsOnDisk` says so. Comparing this list against the
                // one-level listing above would mark every nested rule
                // (`Work/private`) stale and invite a person to delete a rule
                // that is still doing its job.
                rules:
                  panel.rules === null
                    ? null
                    : panel.rules.map((rule) => ({
                        rule,
                        goneLabel: rule.existsOnDisk ? null : t('settings_folders_rule_gone'),
                        costLabel: t('settings_folders_rule_cost'),
                        removeAriaLabel: t('settings_folders_rule_remove_named', { prefix: rule.prefix }),
                      })),
                rulesHeading: t('settings_folders_rules_heading'),
                rulesEmptyLabel: t('settings_folders_rules_none'),
              },
      };
    });
  });

  const expandLabel = $derived.by(() => { void $locale; return t('settings_folders_expand'); });
  const removeRuleLabel = $derived.by(() => { void $locale; return t('settings_folders_rule_remove'); });
</script>

{#snippet subfolders(level: Level, rootId: number)}
  {#if level.unnameableLabel}<p>{level.unnameableLabel}</p>{/if}
  {#if level.emptyLabel}<p>{level.emptyLabel}</p>{/if}
  <ul>
    {#each level.rows as row (row.entry.relativePath)}
      <li data-testid={`subfolder-${rootId}-${row.entry.relativePath}`}>
        <span>{row.entry.name}</span>
        <span>{row.sentence}</span>
        {#if row.costLabel}<span>{row.costLabel}</span>{/if}
        {#if row.control !== 'none'}
          <button
            type="button"
            aria-label={row.controlAriaLabel}
            onclick={() => (row.control === 'exclude'
              ? exclude(rootId, row.entry.relativePath)
              : include(rootId, row.entry.relativePath))}>{row.controlLabel}</button>
        {/if}
        {#if row.expandable}
          <button
            type="button"
            data-testid={`subfolder-expand-${rootId}-${row.entry.relativePath}`}
            aria-expanded={row.open}
            aria-label={row.expandAriaLabel}
            onclick={() => toggleSubfolder(rootId, row.entry.relativePath)}>{expandLabel}</button>
        {/if}
        {#if row.children}{@render subfolders(row.children, rootId)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<div class="folders">
  {#if loadError}
    <p>{loadFailedLabel}</p>
    <p data-testid="folders-load-reason">{loadError}</p>
  {:else if rows.length === 0}
    <p>{emptyLabel}</p>
  {:else}
    <ul>
      {#each rows as { root, countLabel, removeAriaLabel, scanAriaLabel, expandAriaLabel, expanded, panel } (root.rootId)}
        <li data-testid={`folder-row-${root.rootId}`}>
          <span>{root.absolutePath}</span>
          <span>{countLabel}</span>
          <button
            type="button"
            data-testid={`folder-expand-${root.rootId}`}
            aria-expanded={expanded}
            aria-label={expandAriaLabel}
            onclick={() => toggleRoot(root.rootId)}>{expandLabel}</button>
          <button
            type="button"
            data-testid={`folder-scan-${root.rootId}`}
            aria-label={scanAriaLabel}
            onclick={() => jobs.scan(root.rootId)}>{scanLabel}</button>
          <button type="button" aria-label={removeAriaLabel} onclick={() => removeFolder(root.rootId)}>{removeLabel}</button>
          {#if panel}
            <div data-testid={`folder-panel-${root.rootId}`}>
              {#if panel.loadError}
                <p>{panel.failedLabel}</p>
                <p data-testid={`folder-subfolders-reason-${root.rootId}`}>{panel.loadError}</p>
              {/if}
              {#if panel.actionError}
                <p data-testid={`folder-subfolder-error-${root.rootId}`}>{panel.actionError}</p>
              {/if}
              {#if panel.note}<p data-testid={`folder-rule-note-${root.rootId}`}>{panel.note}</p>{/if}
              {#if panel.loadingLabel}<p>{panel.loadingLabel}</p>{/if}
              {#if panel.level}{@render subfolders(panel.level, root.rootId)}{/if}
              {#if panel.rules}
                <div data-testid={`folder-rules-${root.rootId}`}>
                  {#if panel.rules.length === 0}
                    <p>{panel.rulesEmptyLabel}</p>
                  {:else}
                    <p>{panel.rulesHeading}</p>
                    <ul>
                      {#each panel.rules as { rule, goneLabel, costLabel, removeAriaLabel: ruleAria } (rule.prefix)}
                        <li data-testid={`folder-rule-${root.rootId}-${rule.prefix}`}>
                          <span>{rule.prefix}</span>
                          {#if goneLabel}<span>{goneLabel}</span>{/if}
                          <span>{costLabel}</span>
                          <button
                            type="button"
                            aria-label={ruleAria}
                            onclick={() => include(root.rootId, rule.prefix)}>{removeRuleLabel}</button>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <button type="button" onclick={addFolder}>{addLabel}</button>
  {#if actionError}<p data-testid="folders-action-error">{actionError}</p>{/if}
</div>
