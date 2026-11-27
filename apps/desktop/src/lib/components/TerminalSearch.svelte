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
  .find {
    position: absolute;
    top: var(--termru-space-2);
    right: var(--termru-space-4);
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: calc(100% - 2 * var(--termru-space-4));
    padding: 4px 4px 4px var(--termru-space-3);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: 10px;
    background: var(--termru-surface);
    box-shadow: var(--termru-shadow-float);
  }

  .glyph {
    display: flex;
    color: var(--termru-faint);
  }

  input {
    width: 12rem;
    min-width: 0;
    min-height: 32px;
    padding: 0 var(--termru-space-2);
    border: none;
    border-radius: var(--termru-radius-input);
    background: transparent;
    color: var(--termru-fg);
    font-size: var(--termru-text-control-size);
  }

  /* The ring stays; it sits inside the field so the bar does not clip it. */
  input:focus-visible {
    outline-offset: -2px;
  }

  .summary {
    min-width: 4.5rem;
    padding: 0 var(--termru-space-2);
    color: var(--termru-muted);
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
    color: var(--termru-muted);
    font-size: var(--termru-text-secondary-size);
    font-weight: 600;
  }

  .tool.on {
    background: var(--termru-accent-soft);
    color: var(--termru-accent);
  }

  @media (hover: hover) and (pointer: fine) {
    .tool:hover {
      background: var(--termru-subtle);
      color: var(--termru-fg);
      border-color: transparent;
    }
    .tool.on:hover {
      background: var(--termru-accent-soft);
      color: var(--termru-accent);
    }
  }
</style>
