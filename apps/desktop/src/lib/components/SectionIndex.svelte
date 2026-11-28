<script lang="ts">
  import { scrollSectionToTop } from '../scroll-section';

  /**
   * The section index of a long page (the host editor, Preferences): the
   * sections in order, the one you are reading marked as current (scroll-spy),
   * and a way to get to any of them with the keyboard.
   *
   * Each item is a button, not a link: the app routes by the URL hash, and a
   * `#section` link would be taken for a page change. Choosing one scrolls the
   * section to the top of its scroller and moves the focus to its heading, so a
   * screen reader starts reading there. The jump is instant: a key may have
   * started it, and nothing a key starts animates.
   *
   * A section is found by the id of its heading; the heading is what labels
   * the `<section>` (`aria-labelledby`), so the same id serves the page, the
   * tests and the palette's "go to section".
   */
  export interface SectionItem {
    /** The id of the section's heading. */
    id: string;
    label: string;
  }

  let {
    items,
    label,
    scroller,
  }: {
    items: SectionItem[];
    /** The accessible name of the navigation. */
    label: string;
    /** The element that scrolls the sections. */
    scroller: HTMLElement | null;
  } = $props();

  /** The user chose an item: it stays current until they scroll away from it. */
  let chosen = $state<string | null>(null);
  /** The scroll an item's own jump causes is not "scrolling away". */
  let holdUntil = 0;
  let seen = $state<string | null>(null);
  const active = $derived(chosen ?? seen ?? items[0]?.id ?? '');

  /** How far under the top of the scroller a heading counts as "reached". */
  const REACH = 32;

  function measure(): void {
    const root = scroller;
    if (!root) return;
    const top = root.getBoundingClientRect().top;
    let current = items[0]?.id ?? null;
    for (const item of items) {
      const heading = document.getElementById(item.id);
      if (!heading) continue;
      if (heading.getBoundingClientRect().top - top <= REACH) current = item.id;
      else break;
    }
    // The last section is often too short to reach the top: at the end of the
    // page it is the one being read.
    if (
      root.scrollHeight > root.clientHeight + 4 &&
      root.scrollTop + root.clientHeight >= root.scrollHeight - 2
    ) {
      for (let index = items.length - 1; index >= 0; index -= 1) {
        if (document.getElementById(items[index].id)) {
          current = items[index].id;
          break;
        }
      }
    }
    seen = current;
  }

  $effect(() => {
    const root = scroller;
    if (!root) return;
    let frame = 0;
    const schedule = (): void => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        if (performance.now() >= holdUntil) chosen = null;
        measure();
      });
    };
    // A user scroll hands the marker back to the scroll position.
    root.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    measure();
    return () => {
      root.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  });

  function go(id: string): void {
    const heading = document.getElementById(id);
    if (!heading) return;
    chosen = id;
    holdUntil = performance.now() + 200;
    const section = heading.closest('section') ?? heading;
    if (scroller) scrollSectionToTop(scroller, section);
    heading.focus({ preventScroll: true });
  }
</script>

<nav class="index" aria-label={label}>
  <ul>
    {#each items as item (item.id)}
      <li>
        <button
          type="button"
          class="item"
          class:on={active === item.id}
          aria-current={active === item.id ? 'location' : undefined}
          onclick={() => go(item.id)}
        >
          {item.label}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .index {
    min-width: 0;
    padding: var(--termru-space-4) var(--termru-space-8) var(--termru-space-16)
      var(--termru-space-20);
    overflow: auto;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    width: 100%;
    height: 30px;
    min-height: 0;
    padding: 0 var(--termru-space-10);
    border-radius: var(--termru-radius-sm);
    background: transparent;
    color: var(--termru-text-2);
    font-weight: 400;
    text-align: left;
    white-space: nowrap;
    /* Only the press moves. The marker follows the page at once: a section is
       often reached with a key, and that is never animated. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .item.on {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  @media (hover: hover) and (pointer: fine) {
    .item:not(.on) {
      transition:
        transform var(--motion-fast) var(--ease-out),
        background-color var(--motion-fast) ease,
        color var(--motion-fast) ease;
    }

    .item:not(.on):hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-raised) 60%, transparent);
      color: var(--termru-text);
    }
  }

  /* No room for a column: the index is one row of items above the page. */
  @container stage (max-width: 760px) {
    .index {
      padding: 0 var(--termru-space-16) var(--termru-space-8);
    }

    ul {
      flex-direction: row;
      gap: 2px;
      overflow-x: auto;
    }

    .item {
      width: auto;
      flex: none;
    }
  }
</style>
