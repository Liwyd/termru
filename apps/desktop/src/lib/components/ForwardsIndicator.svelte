<script lang="ts">
  import { forwardRows, type ForwardRow } from '../forwards';
  import type { MenuEntry } from '../menu';
  import { notify } from '../notify.svelte';
  import type { Pane } from '../stores/sessions.svelte';
  import { execApproval } from '../stores/exec-approval.svelte';
  import { copyText } from '../terminal/links';
  import PopMenu from './PopMenu.svelte';

  /**
   * What became of the port forwards of the session in this pane, in the
   * toolbar: `2 forwards`, or `1 of 2 failed` / `1 needs approval` when
   * something is wrong. It opens a menu with one line per forward
   * (`L 127.0.0.1:8080 → db:5432 · listening`, or the reason it failed).
   * Fed by the `forward` updates of the session (and `SessionInfo.forwards`
   * when a view attaches).
   */
  let { pane }: { pane: Pane | null } = $props();

  const rows = $derived(
    forwardRows(pane?.host ?? null, pane?.forwards, pane?.execPending === true),
  );
  const failed = $derived(rows.filter((row) => row.status.state === 'failed').length);
  const blocked = $derived(rows.filter((row) => row.status.state === 'blocked').length);
  const up = $derived(rows.filter((row) => row.status.state === 'listening').length);

  /** The state of the whole set: what the glyph and the colour stand for. */
  const tone = $derived<'bad' | 'warn' | 'ok' | 'wait'>(
    failed > 0 ? 'bad' : blocked > 0 ? 'warn' : up === rows.length ? 'ok' : 'wait',
  );
  const glyph = $derived.by(() => ({ bad: '▲', warn: '▲', ok: '■', wait: '□' })[tone]);

  const label = $derived(
    failed > 0
      ? `${failed} of ${rows.length} failed`
      : blocked > 0
        ? `${blocked} need${blocked === 1 ? 's' : ''} approval`
        : `${rows.length} ${rows.length === 1 ? 'forward' : 'forwards'}`,
  );

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
    <span class="glyph" aria-hidden="true">{glyph}</span>
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
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    min-height: 28px;
    padding: 0 var(--termru-space-3);
    border: 1px solid var(--termru-border);
    border-radius: 999px;
    background: transparent;
    color: var(--termru-muted);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    white-space: nowrap;
  }

  .glyph {
    font-size: 8px;
    color: var(--termru-accent);
  }

  .chip.bad {
    border-color: var(--termru-danger);
    color: var(--termru-danger);
  }

  .chip.bad .glyph {
    color: var(--termru-danger);
  }

  .chip.warn {
    border-color: var(--termru-warning);
    color: var(--termru-warning);
  }

  .chip.warn .glyph {
    color: var(--termru-warning);
  }

  @media (hover: hover) and (pointer: fine) {
    .chip:hover {
      background: var(--termru-subtle);
    }
  }

  .chip[aria-expanded='true'] {
    background: var(--termru-subtle);
  }
</style>
