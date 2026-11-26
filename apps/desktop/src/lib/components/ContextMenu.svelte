<script lang="ts">
  import { onMount, tick } from 'svelte';

  export interface MenuItem {
    id: string;
    label: string;
    /** A destructive entry (the delete). */
    danger?: boolean;
    disabled?: boolean;
  }

  let {
    open,
    x,
    y,
    items,
    onchoose,
    onclose,
  }: {
    open: boolean;
    x: number;
    y: number;
    items: MenuItem[];
    onchoose: (id: string) => void;
    onclose: () => void;
  } = $props();

  let menu = $state<HTMLDivElement | null>(null);

  /** The enabled entries, in order. */
  function entries(): HTMLButtonElement[] {
    return menu
      ? Array.from(
          menu.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)'),
        )
      : [];
  }

  // On open the first entry takes the focus; on close the focus goes back
  // to where it was (the row or the ⋯ button that opened the menu).
  $effect(() => {
    if (!open) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void tick().then(() => entries()[0]?.focus());
    return () => {
      const active = document.activeElement;
      if (opener?.isConnected && (!active || active === document.body || menu?.contains(active))) {
        opener.focus();
      }
    };
  });

  function onMenuKeyDown(event: KeyboardEvent): void {
    const items = entries();
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null = null;
    switch (event.key) {
      case 'ArrowDown':
        next = (index + 1) % items.length;
        break;
      case 'ArrowUp':
        next = (index - 1 + items.length) % items.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = items.length - 1;
        break;
      case 'Tab':
        // A menu is left, not tabbed through.
        onclose();
        return;
      default:
        return;
    }
    event.preventDefault();
    items[next]?.focus();
  }

  // The menu closes on the next click outside, on Escape, and when the
  // window is resized.
  onMount(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!menu) return;
      if (event.target instanceof Node && menu.contains(event.target)) return;
      onclose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !open) return;
      event.preventDefault();
      onclose();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('resize', onclose);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('resize', onclose);
    };
  });

  // The menu never leaves the window: it is moved back inside when it would
  // overflow.
  function position(node: HTMLDivElement): { left: number; top: number } {
    const box = node.getBoundingClientRect();
    const left = Math.min(x, window.innerWidth - box.width - 8);
    const top = Math.min(y, window.innerHeight - box.height - 8);
    return { left: Math.max(8, left), top: Math.max(8, top) };
  }
</script>

{#if open}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menu}
    onkeydown={onMenuKeyDown}
    style={menu ? `left: ${position(menu).left}px; top: ${position(menu).top}px` : ''}
  >
    {#each items as item (item.id)}
      <button
        type="button"
        role="menuitem"
        tabindex="-1"
        class:danger={item.danger}
        disabled={item.disabled}
        onclick={() => {
          onchoose(item.id);
          onclose();
        }}
      >
        {item.label}
      </button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 60;
    min-width: 10rem;
    display: flex;
    flex-direction: column;
    padding: 0.2rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    background: var(--termru-surface);
    box-shadow: 0 6px 18px rgba(2, 6, 23, 0.35);
  }

  button {
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--termru-radius);
    color: var(--termru-fg);
    cursor: pointer;
    font: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.5rem;
  }

  button:hover:not(:disabled) {
    background: var(--termru-bg);
  }

  button.danger {
    color: var(--termru-danger);
  }

  button:disabled {
    color: var(--termru-muted);
    cursor: default;
  }
</style>
