<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { MenuEntry } from '../menu';
  import { presence } from '../presence.svelte';

  /**
   * A popup menu: the terminal's context menu and the toolbar's More menu.
   *
   * Keyboard: ArrowUp/ArrowDown/Home/End move, Enter or Space chooses,
   * Escape closes and gives the focus back to what had it, Tab leaves the
   * menu. Disabled entries stay reachable so their reason can be read.
   *
   * It scales in from the corner it was opened at (160 ms, 100 ms out). A
   * menu opened from the keyboard (`animate` false) appears without motion.
   */
  let {
    open,
    x,
    y,
    align = 'left',
    items,
    label,
    animate = true,
    onchoose,
    onclose,
  }: {
    open: boolean;
    /** The anchor point, in viewport pixels. */
    x: number;
    y: number;
    /** Which edge of the menu sits on `x`. */
    align?: 'left' | 'right';
    items: MenuEntry[];
    /** The accessible name of the menu. */
    label: string;
    animate?: boolean;
    onchoose: (id: string) => void;
    onclose: () => void;
  } = $props();

  // The exit plays after the caller has dropped its state (a closed menu
  // has no position and no entries): what the menu showed when it was open
  // is kept for the exit.
  interface MenuView {
    x: number;
    y: number;
    align: 'left' | 'right';
    items: MenuEntry[];
    animate: boolean;
  }
  let held = $state.raw<MenuView | null>(null);
  const current = $derived<MenuView>({ x, y, align, items, animate });
  $effect(() => {
    if (open) held = current;
  });
  const view = $derived(open || held === null ? current : held);

  const pop = presence(
    () => open,
    () => view.animate,
  );

  let menu = $state<HTMLDivElement | null>(null);
  let left = $state(0);
  let top = $state(0);
  let origin = $state('top left');
  /** Where the focus was when the menu opened. */
  let opener: HTMLElement | null = null;

  function entries(): HTMLButtonElement[] {
    return menu ? Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')) : [];
  }

  // Remember the opener the moment the menu opens, before it takes the focus.
  $effect(() => {
    if (!open) return;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      const active = document.activeElement;
      const target = opener;
      opener = null;
      if (target?.isConnected && (!active || active === document.body || menu?.contains(active))) {
        target.focus();
      }
    };
  });

  // Place the menu once it is in the page: inside the window, opening
  // towards the room there is, scaling from the corner at the anchor.
  $effect(() => {
    if (!menu || !open) return;
    const box = menu.getBoundingClientRect();
    const margin = 8;
    const { x: ax, y: ay } = view;
    const wantsLeft = view.align === 'right' ? ax - box.width : ax;
    const clampedLeft = Math.max(
      margin,
      Math.min(wantsLeft, window.innerWidth - box.width - margin),
    );
    const flipUp = ay + box.height > window.innerHeight - margin && ay - box.height >= margin;
    const wantsTop = flipUp ? ay - box.height : ay;
    const clampedTop = Math.max(
      margin,
      Math.min(wantsTop, window.innerHeight - box.height - margin),
    );
    left = clampedLeft;
    top = clampedTop;
    const horizontal = clampedLeft + box.width / 2 > ax ? 'left' : 'right';
    origin = `${flipUp ? 'bottom' : 'top'} ${horizontal}`;
  });

  // The menu takes the focus when it is there. Opened from the keyboard, the
  // first available entry is highlighted; opened by a pointer, nothing is
  // highlighted until a key moves in (no ring around an entry nobody chose).
  $effect(() => {
    if (!menu || !open) return;
    const keyboard = !view.animate;
    void tick().then(() => {
      if (keyboard)
        entries()
          .find((button) => button.getAttribute('aria-disabled') !== 'true')
          ?.focus();
      else menu?.focus();
    });
  });

  function onMenuKeyDown(event: KeyboardEvent): void {
    const buttons = entries();
    if (buttons.length === 0) return;
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next: number;
    switch (event.key) {
      case 'ArrowDown':
        next = (index + 1) % buttons.length;
        break;
      case 'ArrowUp':
        // From the menu itself (nothing highlighted) Up goes to the last entry.
        next = index < 0 ? buttons.length - 1 : (index - 1 + buttons.length) % buttons.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = buttons.length - 1;
        break;
      case 'Tab':
        // A menu is left, not tabbed through.
        onclose();
        return;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
    buttons[next]?.focus();
  }

  onMount(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!open || !menu) return;
      if (event.target instanceof Node && menu.contains(event.target)) return;
      onclose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (!open || event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      onclose();
    };
    const dismiss = () => {
      if (open) onclose();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('blur', dismiss);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('blur', dismiss);
    };
  });

  function choose(entry: MenuEntry): void {
    if (entry.disabled) return;
    onchoose(entry.id);
    onclose();
  }
</script>

{#if pop.mounted}
  <div
    class="menu"
    class:shown={pop.shown}
    class:still={!view.animate}
    class:wide={view.items.some((entry) => entry.mono)}
    role="menu"
    tabindex="-1"
    aria-label={label}
    bind:this={menu}
    onkeydown={onMenuKeyDown}
    style:left="{left}px"
    style:top="{top}px"
    style:transform-origin={origin}
  >
    {#each view.items as entry (entry.id)}
      {#if entry.separatorBefore}
        <div class="rule" role="separator"></div>
      {/if}
      <button
        type="button"
        role="menuitem"
        tabindex="-1"
        class="item"
        class:danger={entry.danger}
        aria-disabled={entry.disabled ? 'true' : undefined}
        title={entry.disabled ? entry.reason : undefined}
        onclick={() => choose(entry)}
      >
        <span class="text" class:mono={entry.mono}>
          {entry.label}
          {#if entry.detail}<span class="detail">{entry.detail}</span>{/if}
        </span>
        {#if entry.hint}<span class="hint">{entry.hint}</span>{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 70;
    min-width: 220px;
    max-width: min(360px, calc(100vw - 16px));
    display: flex;
    flex-direction: column;
    padding: 6px;
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-raised);
    box-shadow: var(--termru-shadow-float);
    opacity: 0;
    transform: scale(var(--pop-scale));
    pointer-events: none;
    /* Out is quicker than in. */
    transition:
      opacity var(--motion-fast) var(--ease-out),
      transform var(--motion-fast) var(--ease-out);
  }

  /* A menu of addresses keeps each on one line where it can. */
  .menu.wide {
    max-width: min(480px, calc(100vw - 16px));
  }

  .menu.shown {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    transition:
      opacity var(--motion-context) var(--ease-out),
      transform var(--motion-context) var(--ease-out);
  }

  .menu.still,
  .menu.still.shown {
    transition: none;
  }

  .menu:focus {
    outline: none;
  }

  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-24);
    width: 100%;
    min-height: 34px;
    padding: 0 10px;
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-text);
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: 450;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--motion-fast) ease;
  }

  /* The ring sits inside the entry so the menu's edge does not clip it. */
  .item:focus-visible {
    background: var(--termru-raised-2);
    outline-offset: -2px;
  }

  @media (hover: hover) and (pointer: fine) {
    .item:hover:not([aria-disabled='true']) {
      background: var(--termru-raised-2);
    }
  }

  .item[aria-disabled='true'] {
    color: var(--termru-text-3);
    cursor: default;
  }

  .item.danger {
    color: var(--termru-err-text);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .text.mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  .detail {
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-text-2);
  }

  .hint {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-text-3);
    white-space: nowrap;
  }

  .rule {
    height: 1px;
    margin: 6px 4px;
    background: var(--termru-border);
  }
</style>
