<script lang="ts">
  import { tick } from 'svelte';
  import EditorSection from './EditorSection.svelte';
  import Field from './Field.svelte';
  import Icon from './Icon.svelte';
  import type { HostForm } from '../host-form.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { MAX_JUMP_HOPS } from '../validation';
  import type { Host } from '../types';

  /**
   * The Jump hosts section: the ordered list of hosts a connection passes through
   * on the way (a bastion), the route that results (`you → bastion → app`)
   * and what is wrong with it. Each hop is another host of the library and
   * keeps its own address, key and host-key decision (ADR 0010).
   */
  let { form }: { form: HostForm } = $props();

  const byId = $derived(new Map(hosts.hosts.map((host) => [host.id, host])));

  /** Hosts that can still be added: not this host, not deleted, not on the route yet. */
  const candidates = $derived(
    hosts.hosts
      .filter(
        (host) => !host.deleted && host.id !== form.hostId && !form.jumpChain.includes(host.id),
      )
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  function target(host: Host): string {
    const port = host.port === 22 ? '' : `:${host.port}`;
    return `${host.username}@${host.address}${port}`;
  }

  const nameOf = (id: string): string => byId.get(id)?.name ?? 'missing host';

  /** `you → gateway → bastion → app`: the route as the form resolves it. */
  const routeLine = $derived.by(() => {
    const route = form.route;
    if (!route || !route.ok) return null;
    const names = route.route.map(nameOf);
    return ['you', ...names, form.name.trim() || 'this host'].join(' → ');
  });

  /** Hops the route passes that the chain does not list (a hop's own jump hosts). */
  const implied = $derived.by(() => {
    const route = form.route;
    if (!route || !route.ok) return [];
    return route.route.filter((id) => !form.jumpChain.includes(id)).map(nameOf);
  });

  const error = $derived(form.errorFor('jump'));

  /** The picker adds the host it was set to and goes back to its prompt. */
  function add(select: HTMLSelectElement): void {
    const id = select.value;
    select.value = '';
    if (id.length > 0) form.addHop(id);
  }

  /** Moves a hop and keeps the keyboard focus on it (the rows are re-ordered in the page). */
  async function move(id: string, direction: -1 | 1): Promise<void> {
    form.moveHop(id, direction);
    await tick();
    const row = document.querySelector<HTMLElement>(`[data-hop="${CSS.escape(id)}"]`);
    const same = row?.querySelector<HTMLElement>(
      `[data-move="${direction === -1 ? 'up' : 'down'}"]`,
    );
    const other = row?.querySelector<HTMLElement>(
      `[data-move="${direction === -1 ? 'down' : 'up'}"]`,
    );
    // At the end of the list the button that was pressed has nowhere to go.
    (same?.getAttribute('aria-disabled') === 'true' ? other : same)?.focus();
  }
</script>

<EditorSection
  id="sec-jump"
  title="Jump hosts"
  blurb="Pass through a bastion or gateway first. Each jump host is a host of your library and keeps its own sign-in."
>
  {#if form.jumpChain.length > 0}
    <ol class="hops" aria-label="Jump hosts, in order">
      {#each form.jumpChain as id, index (id)}
        {@const hop = byId.get(id)}
        <li class="hop" data-hop={id}>
          <span class="n" aria-hidden="true">{index + 1}</span>
          <div class="who">
            <span class="name" class:missing={!hop}>{hop ? hop.name : 'Missing host'}</span>
            <span class="where">{hop ? target(hop) : 'It was deleted or has not synced yet'}</span>
          </div>
          <div class="tools">
            <button
              type="button"
              class="icon quiet"
              data-move="up"
              aria-label="Move {nameOf(id)} earlier"
              aria-disabled={index === 0 ? 'true' : undefined}
              title={index === 0 ? 'It is the first hop' : undefined}
              onclick={() => {
                if (index > 0) void move(id, -1);
              }}
            >
              <span class="arrow up" aria-hidden="true"><Icon name="arrow-down" size={15} /></span>
            </button>
            <button
              type="button"
              class="icon quiet"
              data-move="down"
              aria-label="Move {nameOf(id)} later"
              aria-disabled={index === form.jumpChain.length - 1 ? 'true' : undefined}
              title={index === form.jumpChain.length - 1 ? 'It is the last hop' : undefined}
              onclick={() => {
                if (index < form.jumpChain.length - 1) void move(id, 1);
              }}
            >
              <Icon name="arrow-down" size={15} />
            </button>
            <button
              type="button"
              class="icon quiet"
              aria-label="Remove {nameOf(id)} from the route"
              onclick={() => form.removeHop(id)}
            >
              <Icon name="x" size={15} />
            </button>
          </div>
        </li>
      {/each}
    </ol>
  {/if}

  <Field
    label="Add a jump host"
    id="host-jump-add"
    {error}
    hint={form.jumpChain.length === 0
      ? 'Without one, Termru connects straight to the address.'
      : null}
  >
    {#snippet control(attrs)}
      <div class="add">
        <select
          {...attrs}
          disabled={!form.canAddHop || candidates.length === 0}
          onchange={(event) => add(event.currentTarget)}
          onblur={() => form.touch('jump')}
        >
          <option value="">
            {!form.canAddHop
              ? `A route has at most ${MAX_JUMP_HOPS} hops`
              : candidates.length === 0
                ? 'No other hosts to add'
                : 'Choose a host…'}
          </option>
          {#each candidates as host (host.id)}
            <option value={host.id}>{host.name} · {target(host)}</option>
          {/each}
        </select>
      </div>
    {/snippet}
  </Field>

  {#if routeLine}
    <p class="route" aria-live="polite">
      <span class="muted">Route</span>
      <code>{routeLine}</code>
    </p>
    {#if implied.length > 0}
      <p class="note">
        {implied.join(', ')}
        {implied.length === 1 ? 'is' : 'are'} on the route too: the first jump host is reached through
        {implied.length === 1 ? 'it' : 'them'}.
      </p>
    {/if}
  {/if}
</EditorSection>

<style>
  .hops {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* A hop is a quiet row of the page: no line, a soft fill. */
  .hop {
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--termru-space-12);
    min-height: 56px;
    padding: var(--termru-space-8) var(--termru-space-8) var(--termru-space-8)
      var(--termru-space-12);
    border-radius: var(--termru-radius-row);
    background: var(--termru-side);
  }

  .n {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-accent-text);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: 550;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name.missing {
    color: var(--termru-warn-text);
  }

  .where {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tools {
    display: flex;
    gap: var(--termru-space-4);
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--termru-control-h);
    padding: 0;
    color: var(--termru-text-2);
  }

  .icon[aria-disabled='true'] {
    color: var(--termru-text-3);
    cursor: default;
  }

  .arrow.up {
    display: inline-flex;
    transform: rotate(180deg);
  }

  .add select {
    width: 100%;
  }

  .route {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--termru-space-12);
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .route code {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    overflow-wrap: anywhere;
  }

  .muted {
    color: var(--termru-text-2);
  }

  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }
</style>
