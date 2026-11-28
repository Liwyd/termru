<script lang="ts">
  import { tick } from 'svelte';
  import Field from './Field.svelte';
  import Icon from './Icon.svelte';
  import type { HostForm } from '../host-form.svelte';
  import { MAX_ENV_VARS, MAX_STARTUP_COMMAND_LEN } from '../validation';
  import type { AuthKind } from '../types';

  /**
   * What `06 / ADVANCED` holds besides the terminal type and the keepalive
   * (data model v2, ADR 0010): environment variables, the startup command,
   * agent forwarding and the order the authentication methods are tried in.
   *
   * The startup command, agent forwarding and the environment travel with
   * sync, so they act on every device only once that device's user has
   * approved them: the hints say so.
   */
  let { form }: { form: HostForm } = $props();

  const AUTH_LABEL: Record<AuthKind, string> = {
    password: 'Password',
    private_key: 'Identity key',
    agent: 'SSH agent',
    keyboard_interactive: 'Keyboard-interactive',
  };
  const ALL_KINDS: AuthKind[] = ['password', 'private_key', 'agent', 'keyboard_interactive'];

  const envError = $derived(form.errorFor('env'));
  const startupError = $derived(form.errorFor('startup'));
  const authError = $derived(form.errorFor('authOrder'));
  const remainingKinds = $derived(
    ALL_KINDS.filter((kind) => !(form.authOrder ?? []).includes(kind)),
  );
  let addEnvButton = $state<HTMLButtonElement | null>(null);

  async function addEnv(): Promise<void> {
    if (!form.canAddEnv) return;
    const key = form.addEnv();
    await tick();
    document.getElementById(`host-env-${key}-name`)?.focus();
  }

  async function removeEnv(key: number): Promise<void> {
    form.removeEnv(key);
    form.touch('env');
    await tick();
    addEnvButton?.focus();
  }

  function touchEnv(key: number): void {
    form.touchRow(key);
    form.touch('env');
  }

  async function moveAuth(kind: AuthKind, direction: -1 | 1): Promise<void> {
    form.moveAuth(kind, direction);
    await tick();
    const row = document.querySelector<HTMLElement>(`[data-auth="${kind}"]`);
    const same = row?.querySelector<HTMLElement>(
      `[data-move="${direction === -1 ? 'up' : 'down'}"]`,
    );
    const other = row?.querySelector<HTMLElement>(
      `[data-move="${direction === -1 ? 'down' : 'up'}"]`,
    );
    (same?.getAttribute('aria-disabled') === 'true' ? other : same)?.focus();
  }

  /** The picker adds the method it was set to and goes back to its prompt. */
  function addAuth(select: HTMLSelectElement): void {
    const kind = select.value as AuthKind | '';
    select.value = '';
    if (kind !== '') form.addAuth(kind);
  }
</script>

<fieldset class="group env">
  <legend>Environment variables <span class="optional">· optional</span></legend>
  {#each form.envRows as row, index (row.key)}
    {@const id = `host-env-${row.key}`}
    {@const nameError = form.envError(index, 'name')}
    {@const valueError = form.envError(index, 'value')}
    <div class="env-row" role="group" aria-label="Variable {index + 1}">
      <div class="cell">
        <input
          id="{id}-name"
          type="text"
          class="mono"
          aria-label="Name of variable {index + 1}"
          aria-invalid={nameError ? 'true' : undefined}
          aria-describedby={nameError ? `${id}-name-error` : undefined}
          placeholder="NAME"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          bind:value={row.name}
          oninput={() => form.edited('env')}
          onblur={() => touchEnv(row.key)}
        />
        {#if nameError}<p class="field-error" id="{id}-name-error">{nameError}</p>{/if}
      </div>
      <div class="cell">
        <input
          id="{id}-value"
          type="text"
          class="mono"
          aria-label="Value of variable {index + 1}"
          aria-invalid={valueError ? 'true' : undefined}
          aria-describedby={valueError ? `${id}-value-error` : undefined}
          placeholder="value"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          bind:value={row.value}
          oninput={() => form.edited('env')}
          onblur={() => touchEnv(row.key)}
        />
        {#if valueError}<p class="field-error" id="{id}-value-error">{valueError}</p>{/if}
      </div>
      <button
        type="button"
        class="icon quiet"
        aria-label="Remove variable {index + 1}"
        onclick={() => void removeEnv(row.key)}
      >
        <Icon name="trash" size={15} />
      </button>
    </div>
  {/each}
  {#if envError}<p class="field-error" id="host-env-message" role="alert">{envError}</p>{/if}
  <div class="foot">
    <button
      type="button"
      id="host-env-add"
      bind:this={addEnvButton}
      aria-describedby={envError ? 'host-env-message' : undefined}
      aria-disabled={form.canAddEnv ? undefined : 'true'}
      title={form.canAddEnv ? undefined : `A host has at most ${MAX_ENV_VARS} variables`}
      onclick={() => void addEnv()}
    >
      <Icon name="plus" size={15} />
      Add variable
    </button>
    <p class="note">
      Sent to the server before the shell starts; it keeps only the names it accepts. Values are
      synced in plain text: keep secrets out of them.
    </p>
  </div>
</fieldset>

<Field
  label="Startup command"
  id="host-startup"
  optional
  error={startupError}
  hint={`Typed into the shell once it starts, then Enter. One line, up to ${MAX_STARTUP_COMMAND_LEN} characters. Other devices ask for your approval before it runs.`}
>
  {#snippet control(attrs)}
    <input
      {...attrs}
      type="text"
      class="mono"
      placeholder="tmux new -A -s main"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      bind:value={form.startupCommand}
      oninput={() => form.edited('startup')}
      onblur={() => form.touch('startup')}
    />
  {/snippet}
</Field>

<div class="agent">
  <label class="check">
    <input type="checkbox" bind:checked={form.agentForward} />
    <span>
      Forward my SSH agent
      <span class="note block" id="host-agent-note">
        While you are connected, the server's administrators, and anyone who can read the socket it
        creates, can use your agent's keys. Turn it on only for servers you trust.
      </span>
    </span>
  </label>
</div>

<fieldset class="group order">
  <legend>Authentication order</legend>
  <div class="choices" role="radiogroup" aria-label="Authentication order">
    <label class="choice" class:selected={form.authOrder === null}>
      <input
        type="radio"
        name="auth-order"
        value="automatic"
        checked={form.authOrder === null}
        onchange={() => form.setCustomAuthOrder(false)}
      />
      <span class="mark" aria-hidden="true">{form.authOrder === null ? '●' : '○'}</span>
      Automatic
    </label>
    <label class="choice" class:selected={form.authOrder !== null}>
      <input
        type="radio"
        name="auth-order"
        value="custom"
        checked={form.authOrder !== null}
        onchange={() => form.setCustomAuthOrder(true)}
      />
      <span class="mark" aria-hidden="true">{form.authOrder !== null ? '●' : '○'}</span>
      Custom order
    </label>
  </div>
  {#if form.authOrder === null}
    <p class="note">
      The method chosen under Authentication first, then whatever question the server asks.
    </p>
  {:else}
    <ol class="order-list" aria-label="Methods, in the order they are tried">
      {#each form.authOrder as kind, index (kind)}
        <li data-auth={kind}>
          <span class="n" aria-hidden="true">{index + 1}</span>
          <span class="label">{AUTH_LABEL[kind]}</span>
          <span class="tools">
            <button
              type="button"
              class="icon quiet"
              data-move="up"
              aria-label="Try {AUTH_LABEL[kind]} earlier"
              aria-disabled={index === 0 ? 'true' : undefined}
              onclick={() => {
                if (index > 0) void moveAuth(kind, -1);
              }}
            >
              <span class="arrow up" aria-hidden="true"><Icon name="arrow-down" size={15} /></span>
            </button>
            <button
              type="button"
              class="icon quiet"
              data-move="down"
              aria-label="Try {AUTH_LABEL[kind]} later"
              aria-disabled={index === (form.authOrder?.length ?? 0) - 1 ? 'true' : undefined}
              onclick={() => {
                if (index < (form.authOrder?.length ?? 0) - 1) void moveAuth(kind, 1);
              }}
            >
              <Icon name="arrow-down" size={15} />
            </button>
            <button
              type="button"
              class="icon quiet"
              aria-label="Remove {AUTH_LABEL[kind]} from the order"
              onclick={() => form.removeAuth(kind)}
            >
              <Icon name="x" size={15} />
            </button>
          </span>
        </li>
      {/each}
    </ol>
    {#if remainingKinds.length > 0}
      <div class="add-auth">
        <label class="sr-label" for="host-auth-add">Add a method</label>
        <select
          id="host-auth-add"
          aria-invalid={authError ? 'true' : undefined}
          aria-describedby={authError ? 'host-authorder-message' : undefined}
          onchange={(event) => addAuth(event.currentTarget)}
        >
          <option value="">Add a method…</option>
          {#each remainingKinds as kind (kind)}
            <option value={kind}>{AUTH_LABEL[kind]}</option>
          {/each}
        </select>
      </div>
    {/if}
    {#if authError}
      <p class="field-error" id="host-authorder-message" role="alert">{authError}</p>
    {/if}
    <p class="note">
      Only the methods listed are tried, in this order. One the host has nothing for (no identity,
      no password) is skipped.
    </p>
  {/if}
</fieldset>

<style>
  .group {
    margin: 0;
    padding: 0;
    border: none;
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    min-width: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 550;
  }

  .optional {
    margin-left: var(--termru-space-4);
    font-weight: 400;
    color: var(--termru-text-2);
  }

  .mono {
    font-family: var(--termru-font-mono);
  }

  .env-row {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) auto;
    gap: var(--termru-space-12);
    align-items: start;
  }

  .cell {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-4);
    min-width: 0;
  }

  .cell input {
    width: 100%;
    min-width: 0;
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

  .field-error {
    margin: 0;
    color: var(--termru-err-text);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .foot {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--termru-space-12);
  }

  .foot button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    padding: 0 var(--termru-space-16);
  }

  .foot button[aria-disabled='true'] {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .note.block {
    display: block;
    margin-top: var(--termru-space-4);
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text);
    cursor: pointer;
  }

  .check input {
    margin: 2px 0 0;
    width: 15px;
    height: 15px;
  }

  /* The same radio row as the authentication method above. */
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--termru-space-8);
  }

  .choice {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-16);
    border: 1px solid transparent;
    border-radius: var(--termru-radius-button);
    font-size: var(--termru-text-control-size);
    font-weight: var(--termru-text-control-weight);
    color: var(--termru-text-2);
    cursor: pointer;
    transition:
      transform var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) ease,
      border-color var(--motion-fast) ease,
      color var(--motion-fast) ease;
  }

  .choice.selected {
    border-color: var(--termru-border);
    background: var(--termru-side);
    color: var(--termru-text);
  }

  .choice:active {
    transform: scale(var(--press-scale, 0.97));
  }

  .choice:has(input:focus-visible) {
    outline: var(--termru-focus-w) solid var(--termru-accent);
    outline-offset: var(--termru-focus-offset);
  }

  .choice input {
    position: absolute;
    inset: 0;
    opacity: 0;
    margin: 0;
    cursor: pointer;
  }

  .mark {
    font-size: 11px;
    color: var(--termru-text);
  }

  .order-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--termru-border);
  }

  .order-list li {
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--termru-space-12);
    min-height: 44px;
    border-bottom: 1px solid var(--termru-border);
  }

  .n {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-accent-text);
  }

  .tools {
    display: flex;
    gap: var(--termru-space-4);
  }

  .add-auth select {
    width: 100%;
    max-width: 280px;
  }

  .sr-label {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  @media (max-width: 640px) {
    .env-row {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .env-row .cell:nth-child(2) {
      grid-column: 1;
    }
  }
</style>
