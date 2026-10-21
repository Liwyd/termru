<script lang="ts">
  import { palette } from '../stores/palette.svelte';

  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (palette.open) {
      // Focus the input as soon as the palette opens.
      queueMicrotask(() => input?.focus());
    }
  });

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      palette.move(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      palette.move(-1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      palette.choose();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      palette.hide();
    }
  }
</script>

{#if palette.open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(event) => {
      if (event.target === event.currentTarget) palette.hide();
    }}
  >
    <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
      <input
        bind:this={input}
        bind:value={palette.query}
        placeholder="Type a command…"
        aria-label="Command"
        onkeydown={onKeyDown}
      />

      {#if palette.results.length === 0}
        <p class="empty">No matching command</p>
      {:else}
        <ul>
          {#each palette.results as entry, index (entry.id)}
            <li>
              <button
                type="button"
                class:selected={index === palette.selected}
                onclick={() => {
                  palette.selected = index;
                  palette.choose();
                }}
              >
                <span class="title">{entry.title}</span>
                <span class="group">{entry.group}</span>
                {#if entry.accelerator}
                  <kbd>{entry.accelerator}</kbd>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.55);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    z-index: 70;
  }

  .palette {
    width: min(38rem, 92vw);
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
    overflow: hidden;
    box-shadow: 0 18px 48px rgba(2, 6, 23, 0.45);
  }

  input {
    width: 100%;
    padding: 0.7rem 0.9rem;
    border: none;
    border-bottom: 1px solid var(--termru-border);
    background: transparent;
    color: var(--termru-fg);
    font: inherit;
    font-size: 0.95rem;
  }

  input:focus {
    outline: none;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0.3rem;
    max-height: 22rem;
    overflow-y: auto;
  }

  button {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    border: none;
    background: none;
    color: inherit;
    padding: 0.4rem 0.5rem;
    border-radius: var(--termru-radius);
    text-align: left;
  }

  button.selected {
    background: var(--termru-bg);
  }

  .title {
    flex: 1;
  }

  .group {
    font-size: 0.7rem;
    color: var(--termru-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  kbd {
    font-family: monospace;
    font-size: 0.72rem;
    color: var(--termru-muted);
    border: 1px solid var(--termru-border);
    border-radius: 4px;
    padding: 0 0.25rem;
  }

  .empty {
    margin: 0;
    padding: 0.8rem 0.9rem;
    color: var(--termru-muted);
    font-size: 0.85rem;
  }
</style>
