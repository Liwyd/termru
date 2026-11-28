<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * The inspector panel of the management pages (Folders, Identities): the
   * same 340 px raised panel the Hosts page shows its selected host in. What
   * it holds belongs to one thing (the selected folder, the selected
   * identity, a form for a new one); `contentKey` says which, and when it
   * changes the content is replaced and, for a click, cross-fades in (120 ms).
   * A key never animates it.
   *
   * It also owns the look of what goes in it, so every inspector reads alike:
   * `.insp-title` and `.insp-sub` (who this is), `.ttl` (a group's label, a
   * sentence-case 11 px line), `.kv` (label and value rows), `.insp-note` (a
   * quiet sentence), `.insp-actions` (the buttons under the title) and
   * `.insp-group` (a labelled group).
   */
  let {
    label,
    contentKey,
    animate = false,
    children,
  }: {
    /** The accessible name of the panel. */
    label: string;
    /** What the content is about: a new value replaces the content. */
    contentKey: string;
    /** The content fades in (a click chose it). */
    animate?: boolean;
    children: Snippet;
  } = $props();
</script>

<aside class="insp" aria-label={label}>
  {#key contentKey}
    <div class="content" class:fade={animate}>
      {@render children()}
    </div>
  {/key}
</aside>

<style>
  .insp {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    margin: 0 var(--termru-space-10) var(--termru-space-10) 0;
    padding: var(--termru-space-20) var(--termru-space-20) var(--termru-space-16);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-side);
    overflow: auto;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-20);
    min-width: 0;
  }

  /* The content a click chose fades in (120 ms); a key never does. */
  .content.fade {
    animation: inspector-in var(--motion-context) var(--ease-out) both;
  }

  @keyframes inspector-in {
    from {
      opacity: 0;
    }
  }

  .insp :global(.insp-title) {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
    letter-spacing: -0.015em;
    color: var(--termru-text);
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .insp :global(.insp-sub) {
    margin: 2px 0 0;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .insp :global(.mono) {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    letter-spacing: -0.01em;
  }

  .insp :global(.insp-group) {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  /* A group's label: sentence case, 11 px, the same line the Hosts inspector
     puts over its forwards and tags. */
  .insp :global(.ttl) {
    display: flex;
    justify-content: space-between;
    gap: var(--termru-space-8);
    margin: 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .insp :global(.insp-note) {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  .insp :global(.insp-note.error) {
    color: var(--termru-err-text);
  }

  .insp :global(.insp-note.warn) {
    color: var(--termru-warn-text);
  }

  .insp :global(.kv) {
    display: grid;
    grid-template-columns: 92px minmax(0, 1fr);
    gap: 10px var(--termru-space-12);
    margin: 0;
  }

  .insp :global(.kv dt) {
    color: var(--termru-text-3);
  }

  .insp :global(.kv dd) {
    margin: 0;
    min-width: 0;
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  .insp :global(.insp-actions) {
    display: flex;
    gap: var(--termru-space-6);
  }

  .insp :global(.insp-actions button) {
    background: var(--termru-raised);
    color: var(--termru-text);
    justify-content: center;
  }

  .insp :global(.insp-actions button.primary) {
    background: var(--termru-accent);
    color: var(--termru-accent-ink);
  }

  .insp :global(.insp-actions button.quiet) {
    background: transparent;
    color: var(--termru-text-2);
  }

  .insp :global(.insp-actions button.danger) {
    color: var(--termru-err-text);
  }
</style>
