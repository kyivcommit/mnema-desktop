<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import {
    listTree, addWatchedFolder, removeWatchedFolder,
    listSubfolders, listExclusions, excludeSubfolder, includeSubfolder,
    type StoredExclusion, type Subfolder, type SubfolderListing, type SubfolderState,
    type TreeListing, type TreeRoot,
  } from '../lib/ipc';
  import type { JobController, JobPass, JobPhase } from './jobs';

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
    // PR 8a, Task 8: the path a question was about when a job ending withdrew
    // it, or `null`. The PATH and not the sentence, for the reason `Pending`
    // carries numbers and `alreadyGone` is a boolean: a sentence frozen here
    // would keep its language through a switch.
    withdrawn: string | null;
    // PR 8a, Task 6: the question now in front of the person for this row, or
    // `null`. At most one per row — a second press replaces the first, so no
    // two questions about the same folder can be on screen disagreeing.
    pending: Pending | null;
  };

  // 🔴 Numbers, not a sentence, for the same reason `alreadyGone` is a boolean:
  // a sentence frozen at the moment of the click keeps its language through a
  // switch. And numbers READ ONCE, from one reply — re-deriving them per render
  // would let the question a person is answering renumber itself underneath
  // them.
  type Pending =
    | { kind: 'checking'; path: string }
    | { kind: 'exclude'; path: string; paths: number; documents: number }
    // `existsOnDisk` is carried, not looked up when the question is drawn, for
    // the same reason `paths`/`documents` are: the question states what was
    // true when it was asked, and a re-read landing underneath it must not
    // silently change the sentence a person is in the middle of reading. It is
    // the RULE's own field, never re-derived here (`ipc.ts:118`).
    // `heldBelow` is carried for the same reason, and answers a different
    // question: whether removing THIS rule still leaves rules of the person's
    // own further down the same path. See `heldBelow` for why the sentence
    // needs it.
    | { kind: 'include'; path: string; existsOnDisk: boolean; heldBelow: boolean };

  let panels = $state<Record<number, Panel>>({});

  // Not `$state`: nothing renders from it. One counter per root, bumped by
  // every read AND by every collapse, so a listing still on the wire when the
  // row is shut — or when a newer read has started — is dropped instead of
  // being drawn over whatever the person is looking at now.
  const generations: Record<number, number> = {};

  // Task 6's own counter, and NOT `generations`. The rule, stated once rather
  // than as a list of sites that would drift from the code under it: it is
  // bumped wherever the answer to a question already in flight stops being
  // wanted, so a `list_tree` reply landing after that raises nothing. Held
  // apart from `generations` because that one is ALSO bumped by an ordinary
  // re-read, and a re-read finishing is not a reason to discard a question the
  // person is in the middle of reading.
  const asks: Record<number, number> = {};

  function ask(rootId: number): number {
    const n = (asks[rootId] ?? 0) + 1;
    asks[rootId] = n;
    return n;
  }

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
      ask(rootId); // a question the person shut the row on is not asked again
      const next = { ...panels };
      delete next[rootId];
      panels = next;
      return;
    }
    ask(rootId);
    panels = {
      ...panels,
      [rootId]: {
        tree: null, rules: null, loadError: null, actionError: null,
        alreadyGone: false, withdrawn: null, pending: null,
      },
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

  // ── PR 8a, Task 6: what an exclusion costs, said before it is stored ──────
  //
  // 🔴 Mirrored from what an exclusion rule actually covers: a path lies under
  // a prefix only across a separator. `drop2/y.md` is a SIBLING of `drop`, not
  // a child — `anchored_pattern` produces `!/drop`, which does not match it —
  // so a count written with `startsWith(prefix)` alone would tell a person that
  // `drop2` disappears too. It is the one state in the fixture list that
  // separates the two forms.
  //
  // 🔴 Review round 1, M2. The sentence above used to say "mirrored from
  // `walk.rs`'s own `under`", and that was the wrong Rust function: it is
  // called from two places (`walk.rs:696`, `walk.rs:768`) and both pass a
  // FROZEN prefix; it never sees an exclusion rule. What decides whether a
  // RULE covers a path is
  // `mnema-walk/src/rules.rs:522`'s `anchored_pattern` — `!/{escaped}` — read
  // by `ignore`'s gitignore line parser. This function mirrors THAT, and the
  // two Rust functions agree today because both are right, not because one
  // derives from the other.
  //
  // What makes it drift: a change to `anchored_pattern` or to how that pattern
  // is matched changes what actually disappears, while this count keeps
  // answering by the old rule — so a person is told a number for a deletion
  // that will not happen, or is told nothing about one that will. Under D29 the
  // second direction leaves text in the index and sends it to a third-party
  // provider.
  //
  // What would catch it is a PAIR, neither half sufficient alone:
  // `Folders.test.ts`'s `a sibling whose name merely starts with the prefix is
  // not counted` on this side, and
  // `a_user_prefix_does_not_remove_a_sibling_whose_name_starts_with_it`
  // (`crates/mnema-walk/tests/rules.rs`) on Rust's — which review round 1 had
  // to write, because no Rust fixture paired a prefix with such a sibling. The real fix, still open, is
  // `list_tree` carrying the count so there is one rule and no copy.
  function under(relativePath: string, prefix: string): boolean {
    return relativePath.startsWith(`${prefix}/`);
  }

  // 🔴 Fix round 1, I2. Whether a rule of the person's own remains STRICTLY
  // under `prefix` — the fact two of this screen's sentences were written
  // without and are false in a state this very screen can reach.
  //
  // The state is storable and nothing refuses it: `exclude_subfolder` checks an
  // unknown root, a blank path, the validator and the built-in layers and
  // nothing else (`bridge.rs:450-483` — there is no ancestor guard), and
  // `add_path_exclusion` is `INSERT … ON CONFLICT DO NOTHING`
  // (`write.rs:604-612`), so a rule on `Archive/Held` and a rule on `Archive`
  // are both stored. `subfolder_state` then still reports `Archive` as
  // `Excluded` (`tree.rs:817-848` asks about an ancestor, and `Archive` has
  // none), so this screen offers to remove it — and "anything at this path is
  // indexed again" was contradicted by the rule list two lines below it.
  //
  // Through `under`, so the boundary rule is the one the cost count already
  // uses: a rule on `Archive2` is a sibling of `Archive`, not a rule under it,
  // and `anchored_pattern` agrees. Strict, so the rule being removed is never
  // its own answer.
  //
  // `null` is a panel whose read failed, which draws neither a rule row nor a
  // subfolder row and cannot reach either caller. It answers `false`, and that
  // is the direction to answer it in: `false` selects the UNCONDITIONAL
  // sentence, which claims more comes back than does. Under D29 an
  // over-statement of what reaches the provider is the safe way round.
  function heldBelow(rules: StoredExclusion[] | null, prefix: string): boolean {
    return rules !== null && rules.some((rule) => under(rule.prefix, prefix));
  }

  // The disclosure beside a "remove this rule" control, in its two forms. One
  // function for both places that draw it — the rule list and the subfolder
  // row a rule names — so the two cannot disagree about the same path.
  function ruleCostLabel(rules: StoredExclusion[] | null, prefix: string): string {
    return heldBelow(rules, prefix)
      ? t('settings_folders_rule_cost_held_below')
      : t('settings_folders_rule_cost');
  }

  // The two numbers, from ONE `list_tree` reply, and they are about different
  // things.
  //
  // `paths` is this root's own: a relative path means nothing outside the root
  // it belongs to.
  //
  // `documents` is over EVERY root in the reply, and reading the whole reply is
  // the entire reason it is re-read rather than sampled. A document survives
  // while any path still names it — `forget_if_unnamed` deletes it only when
  // its last path goes, and `deleting_one_copy_keeps_the_document` pins that —
  // so a second copy keeps it findable whether that copy sits in another folder
  // of this root or under a different watched folder altogether. Counting paths
  // and calling them documents overstates the loss; counting within one root
  // overstates it in the same direction.
  function costOf(listing: TreeListing, rootId: number, prefix: string) {
    const doomed = new Set<string>();
    const elsewhere = new Set<string>();
    let paths = 0;
    for (const root of listing.roots) {
      for (const treeFile of root.files) {
        if (root.rootId === rootId && under(treeFile.relativePath, prefix)) {
          paths += 1;
          doomed.add(treeFile.documentId);
        } else {
          elsewhere.add(treeFile.documentId);
        }
      }
    }
    let documents = 0;
    for (const documentId of doomed) if (!elsewhere.has(documentId)) documents += 1;
    return { paths, documents };
  }

  // 🔴 The re-read is not a nicety: a count taken from the listing this window
  // already holds is a number about a moment that has passed — a job may have
  // added or removed paths since the row was drawn.
  async function askExclude(rootId: number, path: string) {
    if (panels[rootId] === undefined) return;
    const generation = ask(rootId);
    // `withdrawn` is cleared HERE and in `askInclude`, and deliberately not
    // also in `exclude`/`include`: every path into those two runs through one
    // of these two functions first (`exclude`'s zero-cost shortcut included),
    // so a third site would be a second guard over one fact.
    patch(rootId, {
      actionError: null, alreadyGone: false, withdrawn: null,
      pending: { kind: 'checking', path },
    });
    let listing: TreeListing;
    try {
      listing = await listTree();
    } catch (e) {
      if (asks[rootId] !== generation) return;
      // The rule is NOT stored, and no loss sentence is shown: this window
      // could not find out what the loss is, and a confirmation over a number
      // it could not read would be worse than none. §10 — what crossed is a
      // sentence, so the sentence is what appears.
      patch(rootId, { pending: null, actionError: message(e) });
      return;
    }
    if (asks[rootId] !== generation) return;
    const cost = costOf(listing, rootId, path);
    // No question over nothing: a confirmation a person can always click
    // through is training for the one that matters.
    if (cost.paths === 0) {
      patch(rootId, { pending: null });
      await exclude(rootId, path);
      return;
    }
    patch(rootId, { pending: { kind: 'exclude', path, ...cost } });
  }

  // No re-read, and deliberately no count. This window does not know what is on
  // disk under a folder the walk has been pruning, so a number here would be
  // invented; what IS known is the consequence, and that is what the sentence
  // states.
  //
  // 🔴 One fact is not invented either, because the window already has it:
  // whether there is a folder at this path at all. The panel prints
  // `settings_folders_rule_gone` from the backend's own `existsOnDisk`
  // (`bridge.rs:117`) in the rule list further down the same panel, so a
  // question promising the provider will get this folder's text contradicted
  // the same panel's own data (review round 1, I1).
  //
  // 🔴 It is an ARGUMENT, not a lookup, and the two callers answer it from
  // different evidence — which is why neither can be written as a search
  // through `panel.rules` with a default. The rules list hands over the rule's
  // own field, never re-derived here (`ipc.ts:118`). A subfolder row hands over
  // `true` because the row IS a directory entry `list_subfolders` read off the
  // disk when the panel was drawn — the same kind of snapshot `existsOnDisk`
  // itself is. A `find` over `panel.rules` would collapse both into one answer
  // plus a fallback for a state that cannot happen — `read` fills `tree` and
  // `rules` from one `Promise.all`, so a rendered row and a null rule list do
  // not coexist, and a default no test can reach is a guard that cannot fail.
  function askInclude(rootId: number, path: string, existsOnDisk: boolean) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    ask(rootId);
    patch(rootId, {
      actionError: null, alreadyGone: false, withdrawn: null,
      // Read from `panel.rules` here and NOT handed over by the caller, which
      // is the opposite of `existsOnDisk` one line up — and the difference is
      // the evidence, not a change of mind. `existsOnDisk` has two answers
      // from two kinds of snapshot; this one is a question about the STORED
      // SET, which both callers would have to answer from that same list.
      pending: { kind: 'include', path, existsOnDisk, heldBelow: heldBelow(panel.rules, path) },
    });
  }

  // What is stored is the path the QUESTION carries, and it is read from the
  // question rather than from anything on screen: the listing under an open
  // question can be redrawn by a re-read, and the sentence the person read
  // named one folder.
  //
  // 🔴 Exhaustive over the three kinds with no default arm, and `checking` is
  // why. This is NOT a second guard over the markup's decision not to draw a
  // control in that state — an `if (kind === 'exclude') … else …` would route
  // `checking` into `include`, which under D29 takes a person's exclusion rule
  // away and sends that folder's text to the provider. The two answers differ,
  // so both are written; `describe`'s default arm explains the case where they
  // do not.
  async function answer(rootId: number) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const pending = panel.pending;
    if (pending === null) return;
    switch (pending.kind) {
      case 'checking':
        return; // no answer has been offered yet; there is nothing to store
      case 'exclude':
        ask(rootId);
        patch(rootId, { pending: null });
        await exclude(rootId, pending.path);
        return;
      case 'include':
        ask(rootId);
        patch(rootId, { pending: null });
        await include(rootId, pending.path);
        return;
    }
  }

  function dismiss(rootId: number) {
    if (panels[rootId] === undefined) return;
    ask(rootId);
    patch(rootId, { pending: null });
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

  // ── PR 8a, Task 8. Live run, finding 1: what a finished scan leaves behind ─
  //
  // `refresh` re-reads the ROW — the roots and their counts — and nothing under
  // it. An expanded panel is read from two other places, the disk
  // (`list_subfolders`) and the stored rules (`list_exclusions`), and a scan
  // ending is exactly the moment both can have moved: measured, `archive` was
  // renamed to `Archive` between two scans, the walk indexed the folder again
  // because the rule still names the old spelling byte for byte, and the row
  // went on reading "archive — excluded by your rule" while that folder's text
  // was on its way to the model provider (D29). Collapsing and re-expanding by
  // hand was the only thing that corrected it.
  //
  // This is PR 7's finding 2 one level deeper, and the asymmetry argument
  // written at the subscription below is the same one: a re-read that finds the
  // listing unchanged rewrites it invisibly, a missed one leaves a falsehood a
  // person can act on.
  //
  // 🔴 EVERY open panel, each with the subfolders that panel currently has
  // open, so an ending that arrives while three roots are expanded does not
  // shut two of them. Started together rather than one after another: the roots
  // are independent — `read` checks THIS root's generation and `patch` writes
  // into THIS root's panel — so the order between them is not observable, and
  // awaiting one would hold the others stale for the length of its call.
  //
  // The limit, stated rather than left silent: `want` is taken from the tree
  // the panel HOLDS, so an expand still on the wire when the ending lands is
  // discarded with every other in-flight read and that subfolder does not open.
  // `exclude` and `include` compute `want` the same way and have the same
  // window; the alternative is a second record of what is open, and
  // `panel.tree` already answers that question.
  //
  // 🔴 PR 8a, Task 8 fix round 2. `reread`'s `pass` is the ending's own, `null`
  // only at `onMount` where there is no ending to have a pass. The RE-READ
  // below stays unconditional — an embedding pass moves `list_exclusions`'
  // `existsOnDisk` and `list_subfolders` exactly as a walk can (D29 sends the
  // same rename window through either). The WITHDRAWAL does not: it is spelled
  // "a scan ended", and only a walk is one. A walk reads ONE folder and moves
  // the two numbers `Pending` freezes (`jobs.ts`'s own words); an embedding
  // pass covers the whole index, takes no root, and changes no rule and no file
  // count for the folder the question is about — so a person's still-open press
  // has nothing invalidated to withdraw it over. Reviewed and reproduced:
  // raising an exclude question while a CHAINED embedding pass runs, then
  // letting that pass end, used to discard the press and print "a scan ended"
  // when none had. That is why the two live in separate functions, and fix
  // round 1 then found the second reason they have to.

  // 🔴 The question goes, and not in silence, ONLY on a walk's own ending.
  // Its two numbers were read from a `list_tree` taken BEFORE this scan,
  // and `Pending` freezes them on purpose: they cannot be corrected in
  // place without renumbering a sentence somebody is part way through
  // reading, and they cannot be left standing, because a walk ending is
  // precisely the event that makes them wrong. The include question is no
  // safer — it carries `existsOnDisk`, the one fact the rename that
  // produced this defect invalidated. So the question is withdrawn and the
  // panel says which folder it was about; pressing again asks it afresh
  // against the state that is now on screen.
  //
  // `ask` and not `generations`: this is that counter's own meaning — the
  // answer to a question already in flight has stopped being wanted — so a
  // `checking` reply still on the wire raises nothing when it lands.
  //
  // 🔴 Fix round 1, I1. Its own function, called from `reread` BEFORE any I/O
  // starts, and that placement is the whole finding. It used to sit at the top
  // of `rereadPanels`, which runs inside `refresh().then(…)` — so a rejected
  // `list_tree` at a walk's ending took the withdrawal down with it, and the
  // ending is consumed exactly once (`seen = phase` advances first), so it
  // never came back. The next successful `refresh` — a chained embedding
  // ending, an add, a remove, none of which withdraw anything — then cleared
  // `loadError` and redrew the panel WITH the question still standing, stating
  // pre-scan numbers as current and carrying no `withdrawnNote` to say a scan
  // had happened underneath it.
  //
  // The question is made wrong by the ENDING, not by a successful re-read, so
  // nothing about withdrawing it may depend on a call that can fail. The
  // direction is chosen and it is the safe one: a question withdrawn once too
  // often costs a second press, a question left standing states frozen numbers
  // as current, and under D29 the include question left standing is a person
  // being asked to unprotect a folder on facts a scan has already moved.
  function withdrawQuestions() {
    for (const [key, panel] of Object.entries(panels)) {
      if (panel.pending === null) continue;
      const rootId = Number(key);
      ask(rootId);
      patch(rootId, { pending: null, withdrawn: panel.pending.path });
    }
  }

  function rereadPanels() {
    for (const [key, panel] of Object.entries(panels)) {
      void read(Number(key), openPathsOf(panel.tree));
    }
  }

  function reread(pass: JobPass | null) {
    // Synchronous, and first: see `withdrawQuestions`. It reads no I/O and
    // cannot fail, so there is no path on which a walk's ending leaves a
    // pending question standing.
    if (pass === 'walk') withdrawQuestions();
    // Panels after the roots, never beside them: `refresh` is what deletes the
    // expansion of a root that has gone, and a `list_subfolders` fired for that
    // root would answer with a rejection drawn into a panel about to vanish.
    refresh().then(rereadPanels).catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
  }

  onMount(() => {
    reread(null); // no ending yet, so no pass — and no panel is open to withdraw
    // Live run, finding 2. Task 7 re-reads after an add and after a remove;
    // the event nobody wired is the one that changes the NUMBER this list shows
    // — a job ending. The row went on stating zero indexed documents while the
    // report under it said four had been added, and the index agreed with the
    // report, not with the row.
    //
    // Task 8 widened what that ending re-reads from the row to the panel under
    // it as well — see `rereadPanels`, which is where the second half of this
    // finding is written up.
    //
    // The RE-READ fires on EVERY ending, not only a walk's, and the reason is
    // asymmetry rather than caution: a re-read that finds the same numbers
    // rewrites them invisibly, while a missed one leaves a falsehood a person
    // can act on. This window does not always know what is running at all
    // (`runningUnobserved` is the state where it has no channel), so keying the
    // RE-READ off the pass would be keying it off something it cannot always
    // see. Endings are rare: at most a handful per run, never one per progress
    // report.
    //
    // The WITHDRAWAL inside `rereadPanels` is narrower — see its own comment.
    // `phase.pass` is passed through here rather than dropped, because that is
    // the one fact `rereadPanels` needs and this subscription is the only place
    // that has it.
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
      if (phase.kind === 'ended') reread(phase.pass);
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
  // **`open` and `excluded` can be expanded; the other four cannot, and
  // `symlink` is the load-bearing case.** `subfolder_state` asks `is_symlink`
  // about the entry itself, so a directory INSIDE a symlinked one comes back
  // `open` — offering "exclude" there would write a rule that excludes
  // nothing, over a subtree the walk never enters. `builtIn` and
  // `unusableName` are shut because there is nothing below them to decide
  // either: both questions are asked about the WHOLE relative path
  // (`layers.prunes` and `check_prefix`, `tree.rs:823` and `:844`), so every
  // child of such a folder comes back in the same state its parent is in.
  //
  // 🔴 `excluded` opens, `excludedByAncestor` stays shut, and the pair is one
  // decision rather than two. The first pass shut both, which cost two things:
  // a person who protected `Work` could never look inside to check what they
  // had protected, and — the reason it was a review finding — every path to an
  // `excludedByAncestor` row runs through an ancestor that is `Excluded`
  // (`subfolder_state` asks about an ancestor before asking about the folder
  // itself, `tree.rs:829-838`), so shutting `excluded` made
  // `excludedByAncestor` a state the running application could not reach at
  // all: tested, and unreachable. Opening `excluded` breaks nothing, because
  // that same precedence is what its children come back as —
  // `ExcludedByAncestor`, which offers no control and does not open in turn.
  // The subtree under a rule is therefore readable exactly one level deep, and
  // no toggle appears over anything the walk has already pruned.
  function describe(state: SubfolderState): {
    sentence: string;
    control: 'exclude' | 'include' | 'none';
    expandable: boolean;
  } {
    switch (state.kind) {
      case 'open':
        return { sentence: t('settings_subfolder_open'), control: 'exclude', expandable: true };
      case 'excluded':
        return { sentence: t('settings_subfolder_excluded'), control: 'include', expandable: true };
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
      // 🔴 A compile-time arm with a run-time consequence, and the two are
      // answered in different places on purpose.
      //
      // Compile time: the `never` binding is what makes a seventh variant
      // mirrored into `ipc.ts` and left undescribed fail `npm run check`.
      //
      // Run time: a variant added to `tree.rs` and NOT mirrored arrives here
      // anyway — Rust and TypeScript share no compiler — and this arm then
      // returns the state object itself, so `control` is `undefined`. The
      // markup used to ask `row.control !== 'none'`, which `undefined`
      // satisfies, and the row grew a button with no text and no `aria-label`
      // whose click routed to `include`: an unlabelled control that removed
      // the person's exclusion rule, and under D29 sent that folder's text to
      // the provider on the next scan. The markup now names the two controls
      // it draws, so an undescribed state offers none.
      //
      // This arm deliberately does NOT also return `control: 'none'`. Two
      // guards for one fact is the shape this branch has paid for repeatedly —
      // each kills the other's mutant, and neither can be shown to work. The
      // run-time answer lives in the markup, alone, where one revert kills it.
      // The pin that stops the state arriving at all is
      // `ipc.test.ts`'s `SubfolderState is exactly what tree.rs defines`.
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

  // `rules` is threaded down the whole recursion rather than read from the
  // panel at each level, because it is one fact about one panel and a second
  // reading of it is a second answer that can disagree.
  function buildLevel(node: SubTree, rules: StoredExclusion[] | null): Level {
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
        costLabel: control === 'include' ? ruleCostLabel(rules, entry.relativePath) : null,
        expandable,
        expandAriaLabel: t('settings_folders_expand_named', { path: entry.relativePath }),
        open: child !== undefined,
        children: child === undefined ? null : buildLevel(child, rules),
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

  // 🔴 The question is built here, inside the `void $locale` rebuild, and not
  // at the moment of the click, for the reason written at the top of this
  // block: a `t()` call frozen into `panels` would keep its language through a
  // switch. The NUMBERS come from `Pending`; only the words come from here.
  type ConfirmView =
    | { kind: 'checking'; checkingLabel: string }
    | {
        kind: 'question';
        heading: string;
        cost: string;
        confirmLabel: string; confirmAriaLabel: string;
        cancelLabel: string; cancelAriaLabel: string;
      };

  function confirmView(pending: Pending): ConfirmView {
    if (pending.kind === 'checking') {
      return { kind: 'checking', checkingLabel: t('settings_folders_exclude_checking') };
    }
    const path = pending.path;
    return {
      kind: 'question',
      heading:
        pending.kind === 'exclude'
          ? t('settings_folders_confirm_exclude_heading', { path })
          : t('settings_folders_confirm_include_heading', { path }),
      cost:
        pending.kind === 'exclude'
          ? t('settings_folders_exclude_cost', { paths: pending.paths, documents: pending.documents })
          : pending.existsOnDisk
            // The `_gone` arm is left unconditioned deliberately: it already
            // says nothing is being indexed at this path today, and its clause
            // about a folder appearing later over-states the exposure rather
            // than under-stating it, which is the D29-safe direction.
            ? pending.heldBelow
              ? t('settings_folders_include_cost_held_below')
              : t('settings_folders_include_cost')
            : t('settings_folders_include_cost_gone'),
      confirmLabel: t('settings_folders_confirm'),
      confirmAriaLabel:
        pending.kind === 'exclude'
          ? t('settings_folders_confirm_exclude_named', { path })
          : t('settings_folders_confirm_include_named', { path }),
      cancelLabel: t('settings_folders_confirm_cancel'),
      cancelAriaLabel: t('settings_folders_confirm_cancel_named', { path }),
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
                level: panel.tree === null ? null : buildLevel(panel.tree, panel.rules),
                loadingLabel:
                  panel.tree === null && panel.loadError === null
                    ? t('settings_subfolders_loading')
                    : null,
                loadError: panel.loadError,
                failedLabel: t('settings_subfolders_failed'),
                actionError: panel.actionError,
                note: panel.alreadyGone ? t('settings_folders_rule_already_gone') : null,
                // Built here, inside the `void $locale` rebuild, for the reason
                // `confirmView` is: the words follow a language switch, the
                // path comes from the panel.
                withdrawnNote:
                  panel.withdrawn === null
                    ? null
                    : t('settings_folders_question_withdrawn', { path: panel.withdrawn }),
                confirm: panel.pending === null ? null : confirmView(panel.pending),
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
                        costLabel: ruleCostLabel(panel.rules, rule.prefix),
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
        <!-- Named, never `!== 'none'` — see `describe`'s default arm. -->
        {#if row.control === 'exclude' || row.control === 'include'}
          <button
            type="button"
            aria-label={row.controlAriaLabel}
            onclick={() => (row.control === 'exclude'
              ? askExclude(rootId, row.entry.relativePath)
              // `true`: this row is a directory entry `list_subfolders` read off
              // the disk, so the folder is there. See `askInclude`.
              : askInclude(rootId, row.entry.relativePath, true))}>{row.controlLabel}</button>
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
              {#if panel.withdrawnNote}
                <p data-testid={`folder-question-withdrawn-${root.rootId}`}>{panel.withdrawnNote}</p>
              {/if}
              <!-- The question, and nothing stored until it is answered. Placed
                   above the listing rather than inside the row that was
                   pressed: the re-read behind it can redraw that row, and a
                   question drawn inside a row that has moved is a question
                   about an unclear subject. The heading names the path. -->
              {#if panel.confirm}
                {@const confirm = panel.confirm}
                <div data-testid={`folder-confirm-${root.rootId}`}>
                  {#if confirm.kind === 'checking'}
                    <p>{confirm.checkingLabel}</p>
                  {:else}
                    <p>{confirm.heading}</p>
                    <p data-testid={`folder-confirm-cost-${root.rootId}`}>{confirm.cost}</p>
                    <button
                      type="button"
                      aria-label={confirm.confirmAriaLabel}
                      onclick={() => answer(root.rootId)}>{confirm.confirmLabel}</button>
                    <button
                      type="button"
                      aria-label={confirm.cancelAriaLabel}
                      onclick={() => dismiss(root.rootId)}>{confirm.cancelLabel}</button>
                  {/if}
                </div>
              {/if}
              {#if panel.loadingLabel}<p>{panel.loadingLabel}</p>{/if}
              {#if panel.level}{@render subfolders(panel.level, root.rootId)}{/if}
              {#if panel.rules}
                <div data-testid={`folder-rules-${root.rootId}`}>
                  {#if panel.rules.length === 0}
                    <p>{panel.rulesEmptyLabel}</p>
                  {:else}
                    <p>{panel.rulesHeading}</p>
                    <ul>
                      <!-- `rule.existsOnDisk` goes to the question the same way
                           `goneLabel` is drawn from it, so the two sentences in
                           this panel cannot disagree about one folder. -->
                      {#each panel.rules as { rule, goneLabel, costLabel, removeAriaLabel: ruleAria } (rule.prefix)}
                        <li data-testid={`folder-rule-${root.rootId}-${rule.prefix}`}>
                          <span>{rule.prefix}</span>
                          {#if goneLabel}<span>{goneLabel}</span>{/if}
                          <span>{costLabel}</span>
                          <button
                            type="button"
                            aria-label={ruleAria}
                            onclick={() => askInclude(root.rootId, rule.prefix, rule.existsOnDisk)}>{removeRuleLabel}</button>
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
