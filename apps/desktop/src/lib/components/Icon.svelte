<script lang="ts">
  /**
   * The small line icons of the work pages (folders, identities, the
   * editor). One 24-unit grid, a 1.6 stroke, `currentColor`: they take the
   * colour of the text they sit next to. Decorative by default; the control
   * that holds one carries the accessible name.
   */
  export type IconName =
    | 'folder'
    | 'folder-plus'
    | 'layers'
    | 'inbox'
    | 'plus'
    | 'key'
    | 'copy'
    | 'check'
    | 'terminal'
    | 'arrow-up-right'
    | 'arrow-right'
    | 'chevron-down'
    | 'trash'
    | 'x'
    | 'check-circle'
    | 'alert-circle'
    | 'info'
    | 'refresh'
    | 'search'
    | 'arrow-down'
    | 'server'
    | 'star'
    | 'clock'
    | 'gear'
    | 'more'
    | 'panel-left'
    | 'panel-right'
    | 'import'
    | 'sort'
    | 'arrow-left';

  const PATHS: Record<IconName, string> = {
    folder: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',
    'folder-plus':
      'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 10.5v5M9.5 13h5',
    layers: 'M12 3.5 3.5 8 12 12.5 20.5 8zM3.5 12 12 16.5 20.5 12M3.5 16 12 20.5 20.5 16',
    inbox: 'M3.5 13 6 5.5h12L20.5 13v5a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1zM3.5 13H8l1 3h6l1-3h4.5',
    plus: 'M12 5v14M5 12h14',
    key: 'M4 15a4 4 0 1 0 8 0 4 4 0 1 0-8 0zM11 12l9-9M16 7l3 3',
    copy: 'M9 9.5a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1zM5 15V6a1 1 0 0 1 1-1h9',
    check: 'M5 12.5 9.5 17 19 7.5',
    terminal: 'M5 8l4 4-4 4M12 17h7',
    'arrow-up-right': 'M7 17 17 7M8 7h9v9',
    'arrow-right': 'M5 12h14M13 6l6 6-6 6',
    'chevron-down': 'M6 9l6 6 6-6',
    trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4.5h6V7',
    x: 'M6 6l12 12M18 6 6 18',
    'check-circle': 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12.4l2.8 2.8L16.2 9.6',
    'alert-circle': 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7.6v5.4M12 16.3v.1',
    info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5.4M12 7.8v.1',
    refresh: 'M20 11a8 8 0 0 0-14.4-3.6M4.5 4.5v4h4M4 13a8 8 0 0 0 14.4 3.6M19.5 19.5v-4h-4',
    search: 'M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20',
    'arrow-down': 'M12 5v14M6 13l6 6 6-6',
    server:
      'M5 4h14a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM5 13h14a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2zM7 7.5h.01M7 16.5h.01',
    star: 'm12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z',
    clock: 'M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17zM12 7.5V12l3 2',
    gear: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.4-2.3.9a7 7 0 0 0-2-1.2L14.2 3H9.8l-.4 2.6a7 7 0 0 0-2 1.2l-2.3-.9-2 3.4 2 1.5A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.5 2 3.4 2.3-.9a7 7 0 0 0 2 1.2l.4 2.6h4.4l.4-2.6a7 7 0 0 0 2-1.2l2.3.9 2-3.4-2-1.5c.1-.4.1-.8.1-1.2z',
    more: 'M5 12a1 1 0 1 0 2 0 1 1 0 1 0-2 0M11 12a1 1 0 1 0 2 0 1 1 0 1 0-2 0M17 12a1 1 0 1 0 2 0 1 1 0 1 0-2 0',
    'panel-left':
      'M5.5 4.5h13a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2zM9.5 4.5v15',
    'panel-right':
      'M5.5 4.5h13a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2zM14.5 4.5v15',
    import: 'M12 4v10M8 10.5l4 4 4-4M5 19h14',
    'arrow-left': 'M19 12H5M11 6l-6 6 6 6',
    sort: 'M8 5v14M4.5 15.5 8 19l3.5-3.5M16 19V5M12.5 8.5 16 5l3.5 3.5',
  };

  let {
    name,
    size = 16,
    filled = false,
  }: { name: IconName; size?: number; filled?: boolean } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width="1.6"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  <path d={PATHS[name]} />
</svg>

<style>
  .icon {
    flex: none;
  }
</style>
