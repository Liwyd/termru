<script lang="ts">
  import { onMount } from 'svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { getTerminal } from '../terminal/registry';
  import { searchDecorations } from '../terminal/theme';
  import SessionIcon from './SessionIcon.svelte';

  /**
   * Find in the terminal's scrollback (`@xterm/addon-search`).
   *
   * Typing searches forward as you go; Enter and Shift+Enter step through the
   * matches; Escape closes it and the terminal gets the keyboard back. The
   * bar appears without motion: the keyboard opens it.
   */
  let { paneId }: { paneId: string } = $props();

  let query = $state('');
  let caseSensitive = $state(false);
  /** The addon's last report: the active match and how many there are. */
  let result = $state<{ index: number; count: number } | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  function options() {
    return {
      caseSensitive,
      incremental: false,
      decorations: searchDecorations(prefs.isDark),
    };
  }

  function search(direction: 'next' | 'previous'): void {
    const handle = getTerminal(paneId);
    if (!handle || handle.disposed) return;
    if (query === '') {
      handle.search.clearDecorations();
      result = null;
      return;
    }
    const found =
      direction === 'next'
        ? handle.search.findNext(query, options())
        : handle.search.findPrevious(query, options());
    // The addon reports counts only when it found something to decorate.
    if (!found) result = { index: -1, count: 0 };
  }

  onMount(() => {
    const handle = getTerminal(paneId);
    const subscription = handle?.search.onDidChangeResults((event) => {
      result = { index: event.resultIndex, count: event.resultCount };
    });
    return () => subscription?.dispose();
  });

  // Opening (or pressing the shortcut again) puts the cursor in the field
  // with the old text selected, ready to type over.
  $effect(() => {
    void terminalUi.searchFocusTick;
    input?.focus();
    input?.select();
  });

  const summary = $derived.by(() => {
    if (query === '' || result === null) return '';
    if (result.count === 0) return 'No results';
    return result.index >= 0 ? `${result.index + 1} of ${result.count}` : `${result.count}+`;
  });

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      terminalUi.closeSearch(paneId);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      search(event.shiftKey ? 'previous' : 'next');
    }
  }
</script>

<div class="find" role="search" aria-label="Find in terminal">
  <label class="field">
    <span class="glyph"><SessionIcon name="search" size={14} /></span>
    <input
      type="text"
      bind:this={input}
      bind:value={query}
      placeholder="Find"
      aria-label="Find in terminal"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      oninput={() => search('next')}
      onkeydown={onKeyDown}
    />
  </label>
  <span class="summary" role="status" aria-live="polite">{summary}</span>
  <button
    type="button"
    class="tool sx-press"
    class:on={caseSensitive}
    aria-pressed={caseSensitive}
    aria-label="Match case"
    title="Match case"
    onclick={() => {
      caseSensitive = !caseSensitive;
      search('next');
    }}
  >
    Aa
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Previous match"
    title="Previous match (Shift+Enter)"
    onclick={() => search('previous')}
  >
    <SessionIcon name="up" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Next match"
    title="Next match (Enter)"
    onclick={() => search('next')}
  >
    <SessionIcon name="down" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Close find"
    title="Close (Escape)"
    onclick={() => terminalUi.closeSearch(paneId)}
  >
    <SessionIcon name="close" />
  </button>
</div>

<style>
  /* The bar is a raised floating panel like a menu: 12 px, the one soft
     shadow, a 1 px edge. */
  .find {
    position: absolute;
    top: var(--termru-space-10);
    right: var(--termru-space-12);
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: calc(100% - 2 * var(--termru-space-12));
    padding: var(--termru-space-4);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-raised);
    box-shadow: var(--termru-shadow-float);
  }

  /* The icon sits inside the field, so the focus ring goes round both. */
  .field {
    display: flex;
    align-items: center;
    gap: var(--termru-space-6);
    min-width: 0;
    padding-left: var(--termru-space-10);
    border-radius: var(--termru-radius-input);
    color: var(--termru-text-3);
  }

  .field:focus-within {
    outline: var(--termru-focus-w) solid var(--termru-accent-text);
    outline-offset: -2px;
  }

  .glyph {
    display: flex;
    flex: none;
  }

  .field input {
    width: 11rem;
    min-width: 0;
    min-height: 32px;
    padding: 0 var(--termru-space-8) 0 0;
    border: none;
    border-radius: var(--termru-radius-input);
    background: transparent;
    color: var(--termru-text);
    font-size: var(--termru-text-control-size);
  }

  .field input:focus-visible {
    outline: none;
  }

  .summary {
    min-width: 4.5rem;
    padding: 0 var(--termru-space-8);
    color: var(--termru-text-2);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    white-space: nowrap;
    text-align: right;
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 32px;
    min-height: 32px;
    padding: 0 6px;
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-text-2);
    font-size: var(--termru-text-secondary-size);
    font-weight: 600;
  }

  .tool.on {
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
  }

  @media (hover: hover) and (pointer: fine) {
    .tool:hover {
      background: var(--termru-side);
      color: var(--termru-text);
      border-color: transparent;
    }
    .tool.on:hover {
      background: var(--termru-accent-soft);
      color: var(--termru-accent-text);
    }
  }
</style>
