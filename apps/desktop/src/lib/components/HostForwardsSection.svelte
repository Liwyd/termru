<script lang="ts">
  import { tick } from 'svelte';
  import EditorSection from './EditorSection.svelte';
  import Field from './Field.svelte';
  import Icon from './Icon.svelte';
  import Toggle from './Toggle.svelte';
  import type { ForwardRow, HostForm } from '../host-form.svelte';
  import { KIND_LABEL, KIND_LETTER, defaultBind, forwardLine } from '../forwards';
  import { MAX_FORWARDS, forwardRowErrors, isLoopbackBind } from '../validation';
  import type { ForwardKind } from '../types';

  /**
   * `05 / PORT FORWARDS`: local (`-L`), remote (`-R`) and dynamic (`-D`,
   * SOCKS) forwards that start with the session. A forward without a bind
   * address listens on loopback only; any other address is an explicit
   * choice, called out in the row. Each row is checked the way the backend
   * checks it (`validate_forward`).
   */
  let { form }: { form: HostForm } = $props();

  const KINDS: ForwardKind[] = ['local', 'remote', 'dynamic'];
  const error = $derived(form.errorFor('forwards'));

  /** `127.0.0.1:8080 → db:5432` for what the row says so far (the kind letter is shown beside it). */
  function lineOf(row: ForwardRow): string {
    const port = Number(row.bindPortText.trim());
    return forwardLine({
      kind: row.kind,
      bind_address: row.bindAddress.trim() || null,
      bind_port: Number.isInteger(port) && port > 0 ? port : 0,
      target_host: row.targetHost.trim() || null,
      target_port: Number(row.targetPortText.trim()) || null,
      enabled: row.enabled,
    })
      .slice(2)
      .replace(/:0\b/, ':…');
  }

  /** The address that is typed is valid and not loopback: it is reachable from the network. */
  function exposed(row: ForwardRow): boolean {
    const errors = forwardRowErrors(row);
    return (
      row.bindAddress.trim().length > 0 && !errors.bindAddress && !isLoopbackBind(row.bindAddress)
    );
  }

  let addButton = $state<HTMLButtonElement | null>(null);

  async function add(): Promise<void> {
    if (!form.canAddForward) return;
    const key = form.addForward('local');
    await tick();
    document.getElementById(`host-fwd-${key}-kind`)?.focus();
  }

  async function remove(key: number): Promise<void> {
    form.removeForward(key);
    form.touch('forwards');
    await tick();
    addButton?.focus();
  }

  function touch(row: ForwardRow): void {
    form.touchRow(row.key);
    form.touch('forwards');
  }
</script>

<EditorSection
  step="05"
  label="PORT FORWARDS"
  id="sec-forwards"
  title="Forward ports with the session"
  blurb="Open a port on this machine, on the server, or a SOCKS proxy, for as long as the session lives."
>
  {#if form.forwardRows.length === 0}
    <p class="empty">No port forwards. The session opens no ports.</p>
  {/if}

  {#each form.forwardRows as row, index (row.key)}
    {@const id = `host-fwd-${row.key}`}
    <div class="forward" class:off={!row.enabled} role="group" aria-labelledby="{id}-title">
      <div class="head">
        <p class="title" id="{id}-title">
          <span class="kind" aria-hidden="true">{KIND_LETTER[row.kind]}</span>
          <span class="visually-hidden">Forward {index + 1}: </span>
          <code>{lineOf(row)}</code>
        </p>
        <div class="tools">
          <span class="enabled">
            <span id="{id}-enabled-label">Enabled</span>
            <Toggle
              id="{id}-enabled"
              checked={row.enabled}
              labelledby="{id}-enabled-label"
              onchange={(next) => {
                row.enabled = next;
                form.edited('forwards');
              }}
            />
          </span>
          <button
            type="button"
            class="icon quiet"
            aria-label="Remove forward {index + 1}"
            onclick={() => void remove(row.key)}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      </div>

      <div class="grid">
        <Field label="Type" id="{id}-kind">
          {#snippet control(attrs)}
            <select
              {...attrs}
              value={row.kind}
              onchange={(event) =>
                form.setForwardKind(row, event.currentTarget.value as ForwardKind)}
              onblur={() => touch(row)}
            >
              {#each KINDS as kind (kind)}
                <option value={kind}>{KIND_LABEL[kind]}</option>
              {/each}
            </select>
          {/snippet}
        </Field>
        <Field
          label="Bind address"
          id="{id}-bind"
          optional
          error={form.forwardError(index, 'bindAddress')}
          hint={row.bindAddress.trim() === '' ? `Empty is ${defaultBind(row.kind)} only.` : null}
        >
          {#snippet control(attrs)}
            <input
              {...attrs}
              type="text"
              placeholder={defaultBind(row.kind)}
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              bind:value={row.bindAddress}
              oninput={() => form.edited('forwards')}
              onblur={() => touch(row)}
            />
          {/snippet}
        </Field>
        <Field label="Bind port" id="{id}-bind-port" error={form.forwardError(index, 'bindPort')}>
          {#snippet control(attrs)}
            <input
              {...attrs}
              type="text"
              inputmode="numeric"
              placeholder="8080"
              autocomplete="off"
              bind:value={row.bindPortText}
              oninput={() => form.edited('forwards')}
              onblur={() => touch(row)}
            />
          {/snippet}
        </Field>
      </div>

      {#if row.kind === 'dynamic'}
        <p class="note">
          A SOCKS5 proxy: the program that uses it names the target. It has no sign-in, so keep it
          on loopback unless you mean to share it.
        </p>
      {:else}
        <div class="grid target">
          <Field
            label="Target host"
            id="{id}-target"
            error={form.forwardError(index, 'targetHost')}
            hint={row.kind === 'local' ? 'Reached from the server.' : 'Reached from this machine.'}
          >
            {#snippet control(attrs)}
              <input
                {...attrs}
                type="text"
                placeholder={row.kind === 'local' ? 'db.internal' : 'localhost'}
                autocomplete="off"
                autocapitalize="off"
                spellcheck="false"
                bind:value={row.targetHost}
                oninput={() => form.edited('forwards')}
                onblur={() => touch(row)}
              />
            {/snippet}
          </Field>
          <Field
            label="Target port"
            id="{id}-target-port"
            error={form.forwardError(index, 'targetPort')}
          >
            {#snippet control(attrs)}
              <input
                {...attrs}
                type="text"
                inputmode="numeric"
                placeholder="5432"
                autocomplete="off"
                bind:value={row.targetPortText}
                oninput={() => form.edited('forwards')}
                onblur={() => touch(row)}
              />
            {/snippet}
          </Field>
        </div>
      {/if}

      {#if exposed(row)}
        <p class="warning" role="note">
          <Icon name="alert-circle" size={15} />
          <span>
            Not loopback: other machines on the network can use this forward. Each device asks you
            to approve it before it opens.
          </span>
        </p>
      {/if}
    </div>
  {/each}

  {#if error}
    <p class="field-error" id="host-forwards-message" role="alert">{error}</p>
  {/if}

  <div class="foot">
    <button
      type="button"
      id="host-forwards-add"
      bind:this={addButton}
      aria-describedby={error ? 'host-forwards-message' : undefined}
      aria-disabled={form.canAddForward ? undefined : 'true'}
      title={form.canAddForward ? undefined : `A host has at most ${MAX_FORWARDS} port forwards`}
      onclick={() => void add()}
    >
      <Icon name="plus" size={15} />
      Add forward
    </button>
    <p class="note">
      Forwards on loopback start with the session. One that listens on another address needs your
      approval on every device it syncs to.
    </p>
  </div>
</EditorSection>

<style>
  .empty,
  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  .forward {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-3);
    padding: var(--termru-space-4);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-subtle);
    min-width: 0;
    transition: opacity var(--motion-fast) ease;
  }

  .forward.off {
    opacity: 0.72;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-3);
    min-width: 0;
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: var(--termru-space-3);
    margin: 0;
    min-width: 0;
  }

  .kind {
    flex: none;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    font-weight: 600;
    color: var(--termru-accent);
  }

  .title code {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: var(--termru-space-3);
    flex: none;
  }

  .enabled {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-muted);
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--termru-control-h);
    padding: 0;
    color: var(--termru-muted);
  }

  .grid {
    display: grid;
    grid-template-columns: 160px minmax(0, 1fr) 112px;
    gap: var(--termru-space-3);
    align-items: start;
    min-width: 0;
  }

  .grid.target {
    grid-template-columns: minmax(0, 1fr) 112px;
  }

  .warning {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-2);
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-warning);
  }

  .warning :global(svg) {
    flex: none;
    margin-top: 3px;
  }

  .field-error {
    margin: 0;
    color: var(--termru-danger);
    font-size: var(--termru-text-secondary-size);
  }

  .foot {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--termru-space-3);
  }

  .foot button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    padding: 0 var(--termru-space-4);
  }

  .foot button[aria-disabled='true'] {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  @media (max-width: 640px) {
    .grid,
    .grid.target {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
