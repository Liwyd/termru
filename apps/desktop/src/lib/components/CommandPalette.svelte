<script lang="ts">
  import { tick } from 'svelte';
  import { focusTrap } from '../actions/focus-trap';
  import { palette, type PaletteEntry } from '../stores/palette.svelte';
  import Icon from './Icon.svelte';

  /**
   * The command palette: the one place that reaches everything. Sections when
   * the field is empty, a ranked list when it is not; the keyboard does all of
   * it (arrows move, Enter runs, Shift+Enter runs the second action, Escape
   * closes). It opens and closes at once: a key opens it, and a key that
   * opens a thing does not animate it.
   */
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (palette.open) {
      // Focus the input as soon as the palette opens.
      queueMicrotask(() => input?.focus());
    }
  });

  const results = $derived(palette.results);
  const searching = $derived(palette.query.trim().length > 0);
  const current = $derived<PaletteEntry | undefined>(results[palette.selected]);

  /** The list as drawn: a heading before the first entry of each section (when not searching). */
  const rows = $derived.by(() => {
    const out: (
      { kind: 'heading'; group: string } | { kind: 'entry'; entry: PaletteEntry; index: number }
    )[] = [];
    let previous: string | null = null;
    results.forEach((entry, index) => {
      if (!searching && entry.group !== previous) out.push({ kind: 'heading', group: entry.group });
      previous = entry.group;
      out.push({ kind: 'entry', entry, index });
    });
    return out;
  });

  // The highlighted entry stays on screen while the arrows move.
  $effect(() => {
    const index = palette.selected;
    void results;
    void tick().then(() => {
      list?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView?.({
        block: 'nearest',
      });
    });
  });

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      palette.move(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      palette.move(-1);
    } else if (event.key === 'Home' && palette.query === '') {
      event.preventDefault();
      palette.selected = 0;
    } else if (event.key === 'End' && palette.query === '') {
      event.preventDefault();
      palette.selected = Math.max(0, results.length - 1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      palette.choose(event.shiftKey);
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
    <div
      class="palette"
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      use:focusTrap={{ onescape: () => palette.hide(), initialFocus: 'input' }}
    >
      <div class="field">
        <Icon name="search" size={16} />
        <input
          bind:this={input}
          bind:value={palette.query}
          placeholder="Search hosts, pages and actions…"
          aria-label="Command"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-autocomplete="list"
          aria-activedescendant={current ? `palette-option-${palette.selected}` : undefined}
          autocomplete="off"
          spellcheck="false"
          onkeydown={onKeyDown}
        />
        <kbd>Esc</kbd>
      </div>

      {#if results.length === 0}
        <p class="empty">Nothing matches “{palette.query.trim()}”</p>
      {:else}
        <div class="list" id="palette-list" role="listbox" aria-label="Results" bind:this={list}>
          {#each rows as row, position (row.kind === 'heading' ? `h:${row.group}:${position}` : row.entry.id)}
            {#if row.kind === 'heading'}
              <div class="heading" role="presentation">{row.group}</div>
            {:else}
              <div
                class="option"
                class:selected={row.index === palette.selected}
                id="palette-option-{row.index}"
                data-index={row.index}
                role="option"
                aria-selected={row.index === palette.selected}
                tabindex="-1"
                onclick={() => {
                  palette.selected = row.index;
                  palette.choose();
                }}
                onkeydown={() => {}}
              >
                <span class="title">{row.entry.title}</span>
                {#if row.entry.detail}<span class="detail">{row.entry.detail}</span>{/if}
                {#if row.entry.accelerator}
                  <kbd>{row.entry.accelerator}</kbd>
                {:else if searching}
                  <span class="section">{row.entry.group}</span>
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      {/if}

      <div class="foot" aria-hidden="true">
        <span><kbd>↑↓</kbd> move</span>
        {#if current}<span><kbd>↵</kbd> {current.verb ?? 'Run'}</span>{/if}
        {#if current?.alt}<span><kbd>⇧↵</kbd> {current.alt.verb}</span>{/if}
        <span class="grow"></span>
        <span><kbd>Esc</kbd> close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    background: var(--termru-scrim);
    z-index: 70;
  }

  .palette {
    display: flex;
    flex-direction: column;
    width: min(40rem, 92vw);
    max-height: min(34rem, 76vh);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-raised);
    color: var(--termru-text);
    overflow: hidden;
    box-shadow: var(--termru-shadow-float);
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--termru-space-10);
    padding: 0 var(--termru-space-16);
    height: 52px;
    flex: none;
    border-bottom: 1px solid var(--termru-line);
    color: var(--termru-text-3);
  }

  input {
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--termru-text);
    font: inherit;
    font-size: 14px;
  }

  input::placeholder {
    color: var(--termru-text-3);
  }

  input:focus {
    outline: none;
  }

  .list {
    flex: 1;
    min-height: 0;
    padding: var(--termru-space-6);
    overflow-y: auto;
  }

  .heading {
    padding: var(--termru-space-10) var(--termru-space-10) var(--termru-space-4);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .option {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    min-height: 34px;
    padding: 0 var(--termru-space-10);
    border-radius: var(--termru-radius-sm);
    cursor: default;
  }

  .option.selected {
    background: var(--termru-raised-2);
  }

  .title {
    flex: none;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    color: var(--termru-text-3);
  }

  .option > :last-child {
    margin-left: auto;
    flex: none;
  }

  .option.selected kbd {
    background: var(--termru-raised);
  }

  .section {
    font-size: var(--termru-text-meta-size);
    color: var(--termru-text-3);
  }

  .empty {
    margin: 0;
    padding: var(--termru-space-16);
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--termru-space-16);
    flex: none;
    height: 34px;
    padding: 0 var(--termru-space-16);
    border-top: 1px solid var(--termru-line);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-text-3);
  }

  .foot .grow {
    flex: 1;
  }
</style>
