<script lang="ts">
  import Selection from './Selection.svelte';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { LauncherState, Turn } from './state';

  // §5: the transcript is the conversation — a turn is its question, pushed to
  // the right edge, and its answer, left in the text column with no bubble.
  // Role is told by that alignment alone: no speaker labels ("You" /
  // "Assistant"), and no hint of one here to fall out of step with the sheet.
  let { turns = [], state, onSelected = () => {} }: {
    turns?: Turn[];
    state: LauncherState;
    // Reported upward for the file pane (Task 7); `Selection` needs a listener
    // today, and a turn drawn on its own reports into nothing.
    onSelected?: (selection: AskCitation | Hit | null) => void;
  } = $props();

  // The answer on screen that is not a turn — a failed one — is drawn last with
  // its own question. A turn is drawn once, and identity is how it is
  // recognised (the launcher keeps these as `$state.raw`).
  const items = $derived.by(() => {
    const drawn = turns.map((t) => ({ answer: t.answer, query: t.query }));
    if (
      (state.kind === 'generated' || state.kind === 'citationsOnly')
      && !drawn.some((d) => d.answer === state.answer)
    ) {
      drawn.push({ answer: state.answer, query: state.query });
    }
    return drawn;
  });
</script>

<div class="transcript" data-testid="transcript">
  {#each items as item (item.answer)}
    <div class="exchange" data-testid="turn">
      <Selection answer={item.answer} query={item.query} {onSelected} />
    </div>
  {/each}
</div>
