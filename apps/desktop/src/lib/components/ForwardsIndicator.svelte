<script lang="ts">
  import { forwardRows, forwardsSummary, type ForwardRow } from '../forwards';
  import type { MenuEntry } from '../menu';
  import { notify } from '../notify.svelte';
  import type { Pane } from '../stores/sessions.svelte';
  import { execApproval } from '../stores/exec-approval.svelte';
  import { copyText } from '../terminal/links';
  import PopMenu from './PopMenu.svelte';
  import SessionIcon from './SessionIcon.svelte';

  /**
   * What became of the port forwards of the session in this pane, in the
   * toolbar: a champagne-soft chip, `2 forwards`, or `1 of 2 failed` /
   * `1 needs approval` (in the colour of the trouble) when something is wrong.
   * It opens a menu with one line per forward
   * (`L 127.0.0.1:8080 → db:5432 · listening`, or the reason it failed).
   * Fed by the `forward` updates of the session (and `SessionInfo.forwards`
   * when a view attaches).
   */
  let { pane }: { pane: Pane | null } = $props();

  const rows = $derived(
    forwardRows(pane?.host ?? null, pane?.forwards, pane?.execPending === true),
  );
  /** The state of the whole set: what the icon, the colour and the words stand for. */
  const summary = $derived(forwardsSummary(rows));
  const tone = $derived(summary.tone);
  const label = $derived(summary.label);

  let menu = $state<{ x: number; y: number; animate: boolean } | null>(null);

  function toggle(event: MouseEvent): void {
    if (menu) {
      menu = null;
      return;
    }
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    // `detail` is 0 for a click made with the keyboard: that opens without motion.
    menu = { x: box.left, y: box.bottom + 6, animate: event.detail > 0 };
  }

  function entryFor(row: ForwardRow): MenuEntry {
    const base = { id: `forward-${row.id}`, label: row.line, mono: true };
    switch (row.status.state) {
      case 'listening':
        return { ...base, hint: 'listening' };
      case 'failed':
        return {
          ...base,
          hint: 'failed',
          detail: `failed: ${row.status.reason}`,
          disabled: true,
          reason: `failed: ${row.status.reason}`,
        };
      case 'blocked':
        return {
          ...base,
          hint: 'not approved',
          detail: 'Left out until you approve the synced settings.',
          disabled: true,
          reason: 'Left out until you approve the synced settings.',
        };
      default:
        return {
          ...base,
          hint: 'starting…',
          disabled: true,
          reason: 'The forward starts once the session is connected.',
        };
    }
  }

  const items = $derived.by((): MenuEntry[] => {
    const list = rows.map(entryFor);
    if (pane?.execPending) {
      list.push({
        id: 'review',
        label: 'Review synced settings…',
        separatorBefore: true,
      });
    }
    return list;
  });

  function choose(id: string): void {
    if (id === 'review') {
      if (pane?.host) void execApproval.show(pane.host.id);
      return;
    }
    const row = rows.find((entry) => `forward-${entry.id}` === id);
    if (!row || row.status.state !== 'listening') return;
    // The address that is listening: where a program on this machine connects.
    const address = `${row.status.address}:${row.status.port}`;
    void copyText(address).then((ok) =>
      ok
        ? notify({ key: 'forward.address-copied', values: { place: address } })
        : notify({ key: 'clipboard.copy-failed' }),
    );
  }
</script>

{#if rows.length > 0}
  <button
    type="button"
    class="chip sx-press"
    class:bad={tone === 'bad'}
    class:warn={tone === 'warn'}
    aria-haspopup="menu"
    aria-expanded={menu !== null}
    title="Port forwards of this session"
    onclick={toggle}
  >
    <SessionIcon name={tone === 'bad' || tone === 'warn' ? 'alert' : 'forward'} size={13} />
    {label}
  </button>

  <PopMenu
    open={menu !== null}
    x={menu?.x ?? 0}
    y={menu?.y ?? 0}
    animate={menu?.animate ?? true}
    label="Port forwards"
    {items}
    onchoose={choose}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  /* The chip: mono 11 on the soft champagne; trouble takes the warn or error
     tint. It is the one coloured thing in the toolbar, so it is the first
     thing the eye finds when a forward needs attention. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    margin-right: var(--termru-space-6);
    padding: 0 9px;
    border: none;
    border-radius: var(--termru-radius-sm);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    white-space: nowrap;
  }

  .chip.bad {
    background: var(--termru-err-soft);
    color: var(--termru-err-text);
  }

  .chip.warn {
    background: var(--termru-warn-soft);
    color: var(--termru-warn-text);
  }

  @media (hover: hover) and (pointer: fine) {
    button.chip:hover:not(:disabled) {
      background-color: color-mix(in srgb, var(--termru-accent) 18%, transparent);
    }

    button.chip.bad:hover:not(:disabled) {
      background-color: color-mix(in srgb, var(--termru-err) 20%, transparent);
    }

    button.chip.warn:hover:not(:disabled) {
      background-color: color-mix(in srgb, var(--termru-warn) 20%, transparent);
    }
  }

  .chip[aria-expanded='true'] {
    background-color: color-mix(in srgb, var(--termru-accent) 18%, transparent);
  }
</style>
