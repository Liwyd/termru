<script lang="ts">
  /**
   * The line icons of the Sessions page: 16px, 1.4px stroke, round caps, the
   * colour of the text around them. They are decoration: the button that
   * holds one carries the name.
   */
  export type IconName =
    | 'split-h'
    | 'split-v'
    | 'duplicate'
    | 'detach'
    | 'plus'
    | 'close'
    | 'expand'
    | 'collapse'
    | 'caret'
    | 'up'
    | 'down'
    | 'search'
    | 'more'
    | 'forward'
    | 'alert';

  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.4"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === 'split-h'}
    <rect x="2.5" y="2.5" width="11" height="11" rx="2" />
    <path d="M2.5 8h11" />
  {:else if name === 'split-v'}
    <rect x="2.5" y="2.5" width="11" height="11" rx="2" />
    <path d="M8 2.5v11" />
  {:else if name === 'duplicate'}
    <rect x="5.5" y="5.5" width="8" height="8" rx="1.8" />
    <path d="M10.5 3.6V3.5A1.5 1.5 0 0 0 9 2H3.5A1.5 1.5 0 0 0 2 3.5V9a1.5 1.5 0 0 0 1.5 1.5h.1" />
  {:else if name === 'detach'}
    <path d="M9.5 2.5h4v4" />
    <path d="M13.5 2.5 7.5 8.5" />
    <path d="M12 9.5v2A1.5 1.5 0 0 1 10.5 13h-6A1.5 1.5 0 0 1 3 11.5v-6A1.5 1.5 0 0 1 4.5 4h2" />
  {:else if name === 'plus'}
    <path d="M8 3v10M3 8h10" />
  {:else if name === 'close'}
    <path d="m4 4 8 8M12 4l-8 8" />
  {:else if name === 'expand'}
    <path d="M9.5 2.5h4v4" />
    <path d="M13.5 2.5 9 7" />
    <path d="M6.5 13.5h-4v-4" />
    <path d="M2.5 13.5 7 9" />
  {:else if name === 'collapse'}
    <path d="M13.5 6.5h-4v-4" />
    <path d="m9.5 6.5 4-4" />
    <path d="M2.5 9.5h4v4" />
    <path d="m6.5 9.5-4 4" />
  {:else if name === 'caret'}
    <path d="m4.5 6.5 3.5 3.5 3.5-3.5" />
  {:else if name === 'up'}
    <path d="m4 9.5 4-4 4 4" />
  {:else if name === 'down'}
    <path d="m4 6.5 4 4 4-4" />
  {:else if name === 'search'}
    <circle cx="7" cy="7" r="4.2" />
    <path d="m10.2 10.2 3.3 3.3" />
  {:else if name === 'more'}
    <circle cx="3.5" cy="8" r=".6" />
    <circle cx="8" cy="8" r=".6" />
    <circle cx="12.5" cy="8" r=".6" />
  {:else if name === 'forward'}
    <path d="M2.5 5.5h10M10 3l2.5 2.5L10 8" />
    <path d="M13.5 10.5h-10M6 8l-2.5 2.5L6 13" />
  {:else if name === 'alert'}
    <path d="M8 2.2 14 12.8H2z" />
    <path d="M8 6.5v2.8M8 11.1v.1" />
  {/if}
</svg>

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
