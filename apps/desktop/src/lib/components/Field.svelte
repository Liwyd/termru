<script lang="ts">
  import type { Snippet } from 'svelte';

  /** What the control of a field needs to be wired to its label and messages. */
  export interface FieldAttrs {
    id: string;
    'aria-invalid'?: 'true';
    'aria-describedby'?: string;
  }

  /**
   * A labelled form field: the label, the control, and under it the hint or
   * the error. The error replaces the hint and is linked to the control with
   * `aria-describedby`, so a screen reader reads it with the field; the
   * message area is a polite live region, so an error that appears on blur
   * is announced as well.
   */
  let {
    label,
    id,
    error = null,
    hint = null,
    optional = false,
    control,
  }: {
    label: string;
    id: string;
    error?: string | null;
    hint?: string | null;
    optional?: boolean;
    control: Snippet<[FieldAttrs]>;
  } = $props();

  const messageId = $derived(`${id}-message`);
  const attrs = $derived<FieldAttrs>({
    id,
    'aria-invalid': error ? 'true' : undefined,
    'aria-describedby': error || hint ? messageId : undefined,
  });
</script>

<div class="field">
  <label for={id}
    >{label}{#if optional}<span class="optional">· optional</span>{/if}</label
  >
  {@render control(attrs)}
  <div class="message" aria-live="polite">
    {#if error}
      <p class="error" id={messageId}>{error}</p>
    {:else if hint}
      <p class="hint" id={messageId}>{hint}</p>
    {/if}
  </div>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  label {
    margin-bottom: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 550;
    color: var(--termru-text);
  }

  .optional {
    margin-left: var(--termru-space-4);
    font-weight: 400;
    color: var(--termru-text-2);
  }

  /* The live region stays in the page (a region that is created together
     with its message is not announced); empty, it takes no room. */
  .message p {
    margin: var(--termru-space-8) 0 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .hint {
    color: var(--termru-text-2);
  }

  .error {
    color: var(--termru-err-text);
  }
</style>
