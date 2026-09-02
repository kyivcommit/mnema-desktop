<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { listMasks, maskPreview, addMask, removeMask } from '../lib/ipc';

  // 🔴 The editor computes no counts of its own. `mask_preview` is the only
  // thing that may answer "how much goes", because it asks the walk's own
  // matcher (`tree.rs`); a number derived here would be a second
  // implementation of the rule and would disagree with the walk at exactly the
  // edges the mask layer was built to pin — the `.gitignore` parser edges, the
  // case folding, the normalisation form. There is no glob library in
  // `ui/package.json` and none is wanted.
  //
  // 🔴 And the preview is called on an explicit press, never per keystroke.
  // `mask_preview` holds the index mutex across a READ of every indexed path of
  // every root, of every root's stored exclusions and of the mask list
  // (`tree.rs`), so a call per character would contend with a running walk. A
  // debounce would only make the contention later; an explicit press makes it
  // once, and the person is already pressing something.
  type Pending =
    // Numbers, not a sentence, for `Folders.svelte`'s reason: a sentence frozen
    // at the press keeps its language through a switch. And read ONCE, from one
    // reply — re-deriving them per render would let the question a person is
    // answering renumber itself underneath them.
    | { kind: 'checking'; mask: string }
    | { kind: 'add'; mask: string; paths: number; documents: number }
    | { kind: 'remove'; mask: string };

  // `null` until the first read answers, and NOT `[]`: "no mask is stored" and
  // "nobody has said yet" are different claims, and the first one printed on a
  // screen that does not know it yet is a claim about the person's protection.
  let masks = $state<string[] | null>(null);
  let loadError = $state<string | null>(null);
  let draft = $state('');
  let pending = $state<Pending | null>(null);
  // 🔴 The mask AS TYPED, held beside the sentence rather than taken from the
  // sentence. `RulesError::InvalidMask`'s `reason` is `globset`'s own text and
  // it quotes the FOLDED pattern — someone who typed `[A-_]x.txt` reads about
  // `[a-_]x.txt`, which they never wrote. Nothing here may branch on an error
  // kind, so the fix is presentation: the frame names what they typed, the
  // sentence is shown verbatim under it, and a third line says why the two can
  // differ in case.
  //
  // `of` is carried because the same two lines serve all three failures, and
  // each is a different sentence: "was not added" is false of a removal that
  // failed, and a failure from `add_mask` itself is not what "the check
  // answered" — the check passed, or there would be no question to confirm.
  // `add-check`: `mask_preview` (inside `askAdd`) rejected — nothing was ever
  //   asked to store the mask.
  // `add-store`: the check passed, the question was answered, and `add_mask`
  //   (inside `answer`) rejected.
  // `remove`: `remove_mask` (inside `answer`) rejected.
  let refused = $state<{ mask: string; of: 'add-check' | 'add-store' | 'remove' } | null>(null);
  let actionError = $state<string | null>(null);
  let alreadyGone = $state(false);
  // 🔴 The stored SPELLING, not a flag, and that is the difference between this
  // and `alreadyGone` above. The store keys masks on their bytes while the walk
  // compares them caselessly, so `*.PDF` and `*.pdf` are one rule and two
  // possible rows; the sentence has to name the row that is already there, or a
  // person told "you already have this" looks for what they typed and does not
  // find it in the list above.
  let alreadyStored = $state<string | null>(null);

  // Not `$state`: nothing renders from it. The list read is reachable from
  // three places — mount, an add and a remove — so two `list_masks` calls can
  // be on the wire at once, and the one asked for first is not the one that has
  // to answer first. The older answer would otherwise land behind the newer one
  // and draw a list that has been superseded.
  let reads = 0;

  // Not `$state`, same reason as `reads`: nothing renders from it. `askAdd` is
  // the only caller of `mask_preview`, but the reply can still land after the
  // question it was answering has stopped being the one on screen — a press on
  // Remove, a Cancel, or a second Add before the first reply arrives all move
  // `pending` on without waiting for it. Bumped by every one of those, so a
  // reply whose generation has gone stale is dropped rather than overwriting
  // whatever question replaced it.
  let previews = 0;

  function message(e: unknown) {
    return e instanceof Error ? e.message : String(e);
  }

  async function refresh() {
    const n = ++reads;
    try {
      const list = await listMasks();
      if (n !== reads) return;
      masks = list;
      loadError = null;
    } catch (e) {
      if (n !== reads) return;
      loadError = message(e);
    }
  }

  onMount(refresh);

  function forget() {
    refused = null;
    actionError = null;
    alreadyGone = false;
    alreadyStored = null;
  }

  async function askAdd() {
    // The literal empty string is `validate_mask`'s one deliberate non-error —
    // it previews as two zeros and `add_mask` refuses it — so the blank row is
    // simply not pressable and neither answer is reached. NOT trimmed: `"   "`
    // is a refusal with a sentence of its own, and trimming here would hand a
    // person who typed spaces the wrong one of the two.
    const mask = draft;
    if (mask === '') return;
    forget();
    const n = ++previews;
    pending = { kind: 'checking', mask };
    try {
      const preview = await maskPreview(mask);
      if (n !== previews) return; // a newer question has replaced this one
      pending = { kind: 'add', mask, paths: preview.paths, documents: preview.documents };
    } catch (e) {
      if (n !== previews) return;
      pending = null;
      refused = { mask, of: 'add-check' };
      actionError = message(e);
    }
  }

  // No preview on this side, and it is not an omission: `mask_preview` counts
  // what a mask REMOVES, and what removing one releases is a different set —
  // every file the mask was holding back, including the ones no scan has ever
  // looked at. The cost is stated in words instead, because no count here would
  // be the count of that.
  //
  // 🔴 Fix round 4, F3: those files are not in the index, so the sentence
  // cannot promise they all come back either. With `*.pdf` and `report.*` both
  // stored, removing either leaves `report.pdf` excluded — and "another rule
  // exists" is not "another rule matches the same files", which is why
  // `settings_masks_remove_cost` is one unconditionally hedged sentence rather
  // than two arms this component would have to choose between.
  function askRemove(mask: string) {
    ++previews; // a standing add-preview reply must not land on this question
    forget();
    pending = { kind: 'remove', mask };
  }

  function dismiss() {
    ++previews; // a standing add-preview reply must not resurrect the question just dismissed
    pending = null;
  }

  async function answer() {
    const p = pending;
    if (p === null || p.kind === 'checking') return;
    pending = null;
    try {
      if (p.kind === 'add') {
        // `add_mask` answers WHICH of the two things happened, for the reason
        // `remove_mask` answers its own pair below: "nothing was written
        // because this rule is already here" is a different sentence from
        // "stored", and before this round the command returned neither — the
        // question simply vanished and the screen looked exactly as it had.
        const added = await addMask(p.mask);
        alreadyStored = added.kind === 'alreadyStored' ? added.stored : null;
        draft = '';
      } else {
        // `remove_mask` answers whether a row actually went: a second window may
        // have removed the same mask first, and that is a different sentence.
        alreadyGone = !(await removeMask(p.mask));
      }
    } catch (e) {
      // The check already passed by the time this runs — this is `answer`, not
      // `askAdd` — so a failure here is the STORE's, never the check's.
      refused = { mask: p.mask, of: p.kind === 'add' ? 'add-store' : 'remove' };
      actionError = message(e);
    }
    // After a refusal too: what the person is looking at then is the index as
    // it is now, not the array from before the press.
    await refresh();
  }

  // `void $locale` on every derived sentence: a bare `t()` is evaluated once and
  // keeps its language through a switch.
  const heading = $derived.by(() => { void $locale; return t('settings_masks_heading'); });
  // The three facts none of which follows from the rest of the screen: a mask is
  // global (D-c), so it is not about the folder it is drawn beside; each folder
  // applies it on its OWN next scan, so one scan settles nothing; and letter
  // case does not matter — the mask and the file name are both case-folded and
  // normalised, so `*.PDF` and `*.pdf` are one rule stored as two rows. They are
  // not deduplicated behind the person's back; the sentence says they are the
  // same instead.
  const explainer = $derived.by(() => { void $locale; return t('settings_masks_explainer'); });
  const emptyLabel = $derived.by(() => { void $locale; return t('settings_masks_none'); });
  const addLabel = $derived.by(() => { void $locale; return t('settings_masks_add'); });
  const inputLabel = $derived.by(() => { void $locale; return t('settings_masks_input_label'); });
  const removeLabel = $derived.by(() => { void $locale; return t('settings_masks_remove'); });
  const loadFailedLabel = $derived.by(() => { void $locale; return t('settings_masks_load_failed'); });
  const alreadyGoneLabel = $derived.by(() => {
    void $locale;
    return alreadyGone ? t('settings_masks_already_gone') : null;
  });
  const alreadyStoredLabel = $derived.by(() => {
    void $locale;
    const stored = alreadyStored;
    return stored === null ? null : t('settings_masks_already_stored', { stored });
  });

  const rows = $derived.by(() => {
    void $locale;
    return (masks ?? []).map((mask) => ({
      mask,
      // Named by the mask itself, for `settings_folders_remove_named`'s reason:
      // every row's control carries the same word, and only the accessible name
      // tells two of them apart.
      removeAriaLabel: t('settings_masks_remove_named', { mask }),
    }));
  });

  // 🔴 Whether `answer` spells `mask` some way other than the way it was typed
  // — the question `settings_masks_refused_case_note` exists to explain, asked
  // rather than assumed. Measured live: the note rendered under `sub/*.txt` and
  // under `!notes.txt`, where the answer echoed the mask byte for byte and
  // there was nothing to explain; a caveat shown where nothing changed hints at
  // a change that did not happen.
  //
  // 🔴 **This is a LOCATOR, not the rule, and the difference is the whole
  // reason the line below is allowed to exist here.** `caseless_form`
  // (`mnema-walk`) is the only thing that decides what a mask folds to, and a
  // second copy of it in this file would disagree with the walk at exactly the
  // edges the mask layer was built to pin. So this never folds anything and
  // never decides what a mask matches: it looks for the mask inside a sentence
  // the shell already wrote, ignoring case, and reports whether what it found
  // there is byte-identical to what the person typed. `toLowerCase` is fit for
  // finding and unfit for deciding.
  //
  // 🔴 **It is wrong in BOTH directions, and the list below is open, not
  // closed.** An earlier version of this comment named one blind spot and
  // called the failure harmless; independent review falsified both halves by
  // measurement, so what follows is what has been measured, never a count.
  //
  // Silent where it would have helped, two mechanisms so far, and BOTH
  // examples below were run rather than reasoned — an earlier draft of this
  // paragraph named two that do not reproduce, one of which reproduces the
  // opposite (independent review). A mask only ever reaches this function
  // through a refusal, so an illustration has to be a mask that is actually
  // REFUSED; `Straße*` is accepted and echoed nowhere.
  //
  // A fold that changes length: `[Straße` is refused, and `globset` answers
  // about `'[strasse'` — `ß` folds to two characters, so the answer is longer
  // than what was typed and a lowercase search for `[straße` finds nothing.
  //
  // A fold that normalises: `caseless_form` ends in `.nfc()`, so a mask typed
  // DECOMPOSED — `[`, `R`, `e`, U+0301, the form macOS puts on disk — is
  // answered about as `'[ré'` composed, and `toLowerCase` does not normalise,
  // so it finds nothing either. Written as codepoints on purpose: the composed
  // spelling of the same name is the case where the caveat IS drawn, correctly,
  // and a bare `[Ré` in a comment cannot say which of the two it is.
  //
  // And loud where nothing was respelled: the shell's own sentence opens
  // `file mask "<typed>" …`, so the words `file ` and `mask ` stand in the
  // answer before the mask is ever quoted, and a refused mask whose lowercase
  // form is one of them — `Mask ` with a stray trailing space, which
  // `MaskSurroundingWhitespace` refuses — matches the boilerplate instead of
  // the quotation and draws the caveat.
  //
  // Anchoring the search inside the quoted mask would close that, and it would
  // mean this file reading the shape of the shell's sentence, which is the one
  // thing a locator here must not do. The real fix is the shell answering with
  // the folded spelling instead of the editor guessing at it; booked in the
  // ledger, not attempted here. Until then this decides one caveat and nothing
  // else, which is the whole reason a fallible locator is allowed to exist.
  function answerSpellsItDifferently(answer: string, mask: string): boolean {
    const inAnswer = answer.toLowerCase();
    const wanted = mask.toLowerCase();
    for (let at = inAnswer.indexOf(wanted); at !== -1; at = inAnswer.indexOf(wanted, at + 1)) {
      if (answer.slice(at, at + mask.length) !== mask) return true;
    }
    return false;
  }

  const refusal = $derived.by(() => {
    void $locale;
    const r = refused;
    if (r === null || actionError === null) return null;
    // Branches on WHICH COMMAND WAS CALLED — state this component already
    // holds — never on the error's shape: nothing here may inspect what an
    // error looks like to decide what it means.
    const heading = t(
      r.of === 'add-check' ? 'settings_masks_refused_add'
        : r.of === 'add-store' ? 'settings_masks_refused_store'
        : 'settings_masks_refused_remove',
      { mask: r.mask },
    );
    return {
      heading,
      // The case note belongs to the check only: it explains a folded pattern
      // quoted inside a COMPILE refusal, and neither the add-store path (the
      // check already passed) nor the removal path folds anything. And within
      // the check, only where the answer really carries a second spelling —
      // most of the check's refusals (`/`, a leading `!`, whitespace) quote the
      // mask exactly as typed and have nothing for this sentence to explain.
      note:
        r.of === 'add-check' && answerSpellsItDifferently(actionError, r.mask)
          ? t('settings_masks_refused_case_note')
          : null,
    };
  });

  const question = $derived.by(() => {
    void $locale;
    const p = pending;
    if (p === null) return null;
    if (p.kind === 'checking') return { kind: 'checking' as const, label: t('settings_masks_checking') };
    return {
      kind: 'question' as const,
      heading: t(
        p.kind === 'add' ? 'settings_masks_confirm_add_heading' : 'settings_masks_confirm_remove_heading',
        { mask: p.mask },
      ),
      cost:
        p.kind === 'add'
          // 🔴 The zero has a sentence of its own, and after fix round 7 the
          // reason is the HEDGE rather than the documents clause, which no
          // longer exists: the shared sentence hedges both ways because its
          // number can be too high (a path an in-tree `.gitignore` already
          // covers is charged to this press), and this number is zero and
          // cannot be, so its warning runs one way only.
          // There is no shortcut past the question either: since fix round 4 a
          // preview of zero means this mask takes nothing BEYOND what the
          // stored rules already take — which is also what adding `*.txt` over
          // a stored `*` answers — and the next scan can still take files that
          // never finished indexing.
          ? p.paths === 0
            ? t('settings_masks_add_cost_none')
            : t('settings_masks_add_cost', { paths: p.paths, documents: p.documents })
          : t('settings_masks_remove_cost'),
      confirmLabel: t('settings_masks_confirm'),
      confirmAriaLabel: t(
        p.kind === 'add' ? 'settings_masks_confirm_add_named' : 'settings_masks_confirm_remove_named',
        { mask: p.mask },
      ),
      cancelLabel: t('settings_masks_confirm_cancel'),
      cancelAriaLabel: t('settings_masks_confirm_cancel_named', { mask: p.mask }),
    };
  });
</script>

<div class="masks">
  <h3>{heading}</h3>
  <p>{explainer}</p>
  {#if loadError}
    <p>{loadFailedLabel}</p>
    <p data-testid="masks-load-reason">{loadError}</p>
  {:else if masks !== null && rows.length === 0}
    <p>{emptyLabel}</p>
  {:else}
    <ul>
      {#each rows as { mask, removeAriaLabel } (mask)}
        <li data-testid={`mask-row-${mask}`}>
          <span>{mask}</span>
          <button type="button" aria-label={removeAriaLabel} onclick={() => askRemove(mask)}>{removeLabel}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <!-- The question, and nothing stored until it is answered. -->
  {#if question}
    {@const q = question}
    <div data-testid="mask-confirm">
      {#if q.kind === 'checking'}
        <p>{q.label}</p>
      {:else}
        <p>{q.heading}</p>
        <p data-testid="mask-confirm-cost">{q.cost}</p>
        <button type="button" aria-label={q.confirmAriaLabel} onclick={answer}>{q.confirmLabel}</button>
        <button type="button" aria-label={q.cancelAriaLabel} onclick={dismiss}>{q.cancelLabel}</button>
      {/if}
    </div>
  {/if}
  {#if alreadyGoneLabel}<p data-testid="mask-already-gone">{alreadyGoneLabel}</p>{/if}
  {#if alreadyStoredLabel}<p data-testid="mask-already-stored">{alreadyStoredLabel}</p>{/if}
  {#if refusal}
    <p data-testid="mask-refused-heading">{refusal.heading}</p>
    <!-- Verbatim, and it is the whole of the rejection: nothing here branches on
         an error kind, so this sentence is the only thing that can say which
         rule was refused and why. -->
    <p data-testid="mask-refused-reason">{actionError}</p>
    {#if refusal.note}<p>{refusal.note}</p>{/if}
  {/if}
  <label class="fl" for="mask-draft-input">{inputLabel}</label>
  <input id="mask-draft-input" type="text" bind:value={draft} />
  <!-- Also disabled while `checking`: closes the OTHER route to the same guard
       — nothing stopped a second Add before the first reply landed, queuing
       two `mask_preview` calls on the mutex with no order guarantee between
       them. The generation guard above is what makes either route safe; this
       is what keeps the second call from being placed at all. -->
  <button type="button" disabled={draft === '' || pending?.kind === 'checking'} onclick={askAdd}>{addLabel}</button>
</div>
