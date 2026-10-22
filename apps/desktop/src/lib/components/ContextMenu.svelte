<script lang="ts">
  import { onMount } from 'svelte';

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

  // The menu closes on the next click outside, on Escape, and when the
  // window is resized.
  onMount(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!menu) return;
      if (event.target instanceof Node && menu.contains(event.target)) return;
      onclose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onclose();
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
    bind:this={menu}
    style={menu ? `left: ${position(menu).left}px; top: ${position(menu).top}px` : ''}
  >
    {#each items as item (item.id)}
      <button
        type="button"
        role="menuitem"
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
