<script lang="ts">
  import { prefs } from '../stores/prefs.svelte';
  import { toasts } from '../stores/toasts.svelte';

  let { open = $bindable(false) }: { open?: boolean } = $props();

  // A copy of the preferences, so a change is only applied when the user
  // saves it.
  let draft = $state({
    theme: 'system' as 'light' | 'dark' | 'system',
    font_family: 'monospace',
    font_size: 14,
    scrollback: 10_000,
    cursor_blink: true,
    cursor_style: 'block' as 'block' | 'bar' | 'underline',
    copy_on_select: false,
  });

  $effect(() => {
    if (!open) return;
    draft = {
      theme: prefs.prefs.theme,
      font_family: prefs.prefs.terminal.font_family,
      font_size: prefs.prefs.terminal.font_size,
      scrollback: prefs.prefs.terminal.scrollback,
      cursor_blink: prefs.prefs.terminal.cursor_blink,
      cursor_style: prefs.prefs.terminal.cursor_style,
      copy_on_select: prefs.prefs.terminal.copy_on_select,
    };
  });

  async function save(): Promise<void> {
    await prefs.update({
      ...prefs.prefs,
      theme: draft.theme,
      terminal: {
        ...prefs.prefs.terminal,
        font_family: draft.font_family.trim() || 'monospace',
        font_size: Math.min(72, Math.max(6, draft.font_size)),
        scrollback: Math.min(1_000_000, Math.max(1000, draft.scrollback)),
        cursor_blink: draft.cursor_blink,
        cursor_style: draft.cursor_style,
        copy_on_select: draft.copy_on_select,
      },
    });
    toasts.success('Settings saved.');
    open = false;
  }

  function reset(): void {
    draft.theme = 'system';
    draft.font_family = 'monospace';
    draft.font_size = 14;
    draft.scrollback = 10_000;
    draft.cursor_blink = true;
    draft.cursor_style = 'block';
    draft.copy_on_select = false;
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (open = false)}
  >
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Settings">
      <header>
        <h2>Settings</h2>
        <button type="button" class="ghost" onclick={() => (open = false)} aria-label="Close"
          >✕</button
        >
      </header>

      <section>
        <h3>Appearance</h3>

        <label>
          <span>Theme</span>
          <select bind:value={draft.theme}>
            <option value="system">Follow the system</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>

        <label>
          <span>Font family</span>
          <input type="text" bind:value={draft.font_family} placeholder="monospace" />
        </label>

        <label>
          <span>Font size</span>
          <input type="number" min="6" max="72" bind:value={draft.font_size} />
        </label>

        <label>
          <span>Scrollback (lines)</span>
          <input type="number" min="1000" max="1000000" step="1000" bind:value={draft.scrollback} />
        </label>
      </section>

      <section>
        <h3>Terminal</h3>

        <label>
          <span>Cursor style</span>
          <select bind:value={draft.cursor_style}>
            <option value="block">Block</option>
            <option value="bar">Bar</option>
            <option value="underline">Underline</option>
          </select>
        </label>

        <label class="check">
          <input type="checkbox" bind:checked={draft.cursor_blink} />
          <span>Blinking cursor</span>
        </label>

        <label class="check">
          <input type="checkbox" bind:checked={draft.copy_on_select} />
          <span>Copy the selection automatically</span>
        </label>
      </section>

      <p class="hint">
        These settings follow the <code>preferences</code> record, so they are synced with your other
        devices. Secrets are never part of it.
      </p>

      <div class="actions">
        <button type="button" onclick={reset}>Reset</button>
        <button type="button" onclick={() => (open = false)}>Cancel</button>
        <button type="button" class="primary" onclick={() => void save()}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }

  .dialog {
    width: min(30rem, 94vw);
    max-height: 88vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
    padding: 1.1rem;
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  h3 {
    margin: 0 0 0.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--termru-muted);
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.8rem;
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 0.45rem;
  }

  input,
  select {
    padding: 0.4rem 0.55rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
  }

  input[type='checkbox'] {
    padding: 0;
    width: 0.9rem;
    height: 0.9rem;
  }

  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--termru-muted);
  }

  code {
    font-family: monospace;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
