<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { appear } from '../actions/appear';
  import Field from '../components/Field.svelte';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { listen } from '../ipc-backend';
  import { formatLastSession } from '../last-session';
  import { network } from '../stores/network.svelte';
  import { afterFirstPaint } from '../settled.svelte';
  import { sync } from '../stores/sync.svelte';
  import type { ConflictView } from '../types';
  import { emailError, serverUrlError } from '../validation';

  /**
   * The Sync page. Sync is optional: signed out, the page is the sign-in
   * form; signed in, it is a status block, the conflicts to decide, this
   * device, and the account. The status always says what is true (up to
   * date, changes waiting, last cycle failed, offline) with a word and a
   * glyph, never a colour alone. Being offline never means an SSH session is
   * down, and the page says so.
   *
   * What the backend gives: the status, the settings, the open conflicts and
   * Sync now / sign in / sign out. It gives no list of the account's other
   * devices yet, so the Devices section shows this device and says so.
   */

  /** The form that opens while the page is open moves in; one there at the start does not. */
  const page = afterFirstPaint();

  $effect(() => {
    void sync.load();
  });

  // A cycle that ran in the background (or in another window) changed the
  // last sync time, the waiting changes or the conflicts: read them again.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen('sync-applied', () => void sync.load()).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  // ---- the form ------------------------------------------------------------

  let serverUrl = $state('');
  let email = $state('');
  let password = $state('');
  let deviceName = $state('');
  /** Which fields the user has left (their errors show from then on). */
  let touched = $state<Record<string, boolean>>({});
  /** The signed-in page shows the form only when asked. */
  let configuring = $state(false);
  /** The user typed in the form: a status reload must not overwrite it. */
  let dirty = false;

  // Fill the form from the stored settings; a reload (a sync, a conflict
  // resolved) must not wipe what is being typed.
  $effect(() => {
    const settings = sync.status?.settings;
    untrack(() => {
      if (dirty) return;
      serverUrl = settings?.server_url ?? '';
      email = settings?.email ?? '';
      deviceName = settings?.device_name ?? '';
    });
  });

  const errors = $derived({
    server: serverUrlError(serverUrl),
    email: emailError(email),
    password: password.length === 0 ? 'Password is required.' : null,
  });

  function shown(field: 'server' | 'email' | 'password'): string | null {
    return touched[field] ? errors[field] : null;
  }

  function touch(field: string): void {
    touched = { ...touched, [field]: true };
  }

  async function signIn(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    touched = { server: true, email: true, password: true };
    const first = (['server', 'email', 'password'] as const).find((field) => errors[field]);
    if (first) {
      document.getElementById(`sync-${first}`)?.focus();
      return;
    }
    const ok = await sync.signIn(serverUrl.trim(), email.trim(), password, deviceName.trim());
    if (ok) {
      password = '';
      touched = {};
      dirty = false;
      configuring = false;
    }
  }

  function signOut(): void {
    password = '';
    touched = {};
    dirty = false;
    configuring = false;
    void sync.signOut();
  }

  // ---- the status ----------------------------------------------------------

  type Tone = 'ok' | 'working' | 'attention' | 'failed' | 'offline';

  interface StatusLine {
    tone: Tone;
    label: string;
    detail: string;
  }

  function plural(count: number, noun: string): string {
    return `${count} ${noun}${count === 1 ? '' : 's'}`;
  }

  const lastSync = $derived(
    sync.status?.last_sync_at ? formatLastSession(Date.parse(sync.status.last_sync_at)) : 'Never',
  );

  /** One honest line: the most important true thing about the sync. */
  const line = $derived.by((): StatusLine => {
    if (sync.busy) return { tone: 'working', label: 'Syncing', detail: 'Talking to the server' };
    if (!network.online) {
      return {
        tone: 'offline',
        label: 'Sync offline',
        detail: `Last known state: ${lastSync}. Local work and SSH sessions keep running.`,
      };
    }
    if (sync.status?.last_error) {
      return { tone: 'failed', label: 'Last sync failed', detail: sync.status.last_error };
    }
    if (sync.conflictCount > 0) {
      return {
        tone: 'attention',
        label: `${plural(sync.conflictCount, 'conflict')} to decide`,
        detail: 'Both this device and the server changed the same record. Nothing is overwritten.',
      };
    }
    if (sync.pending > 0) {
      return {
        tone: 'working',
        label: `${plural(sync.pending, 'change')} waiting`,
        detail: 'They are sent on the next sync',
      };
    }
    return { tone: 'ok', label: 'Up to date', detail: 'Nothing is waiting to be sent' };
  });

  const offline = $derived(sync.signedIn && !network.online);
  const syncTitle = $derived(
    sync.busy
      ? 'A sync is running.'
      : offline
        ? 'Offline. Termru syncs again when the network is back.'
        : undefined,
  );

  // ---- conflicts -----------------------------------------------------------

  const KINDS: Record<string, string> = {
    host: 'Host',
    group: 'Folder',
    identity: 'Identity',
    tag: 'Tag',
    preferences: 'Preferences',
  };

  /** Fields that always differ between two revisions of a record. */
  const BOOKKEEPING = new Set(['updated_at', 'created_at', 'server_rev', 'dirty', 'id']);

  function recordName(conflict: ConflictView): string {
    for (const copy of [conflict.local, conflict.remote]) {
      if (copy && typeof copy === 'object' && 'name' in copy) {
        const name = (copy as { name?: unknown }).name;
        if (typeof name === 'string' && name.length > 0) return name;
      }
    }
    return conflict.kind === 'preferences' ? 'Preferences' : conflict.record_id;
  }

  function show(value: unknown): string {
    if (value === undefined) return '—';
    const text = typeof value === 'string' ? value : JSON.stringify(value);
    return text.length > 64 ? `${text.slice(0, 63)}…` : text;
  }

  interface Difference {
    field: string;
    local: string;
    server: string;
  }

  /** The fields whose values differ; empty when one side was deleted. */
  function differences(conflict: ConflictView): Difference[] {
    const local = conflict.local as Record<string, unknown> | null;
    const remote = conflict.remote as Record<string, unknown> | null;
    if (!local || !remote || typeof local !== 'object' || typeof remote !== 'object') return [];
    const keys = new Set([...Object.keys(local), ...Object.keys(remote)]);
    const out: Difference[] = [];
    for (const key of keys) {
      if (BOOKKEEPING.has(key)) continue;
      if (JSON.stringify(local[key]) === JSON.stringify(remote[key])) continue;
      out.push({ field: key, local: show(local[key]), server: show(remote[key]) });
    }
    return out;
  }

  const MAX_DIFFERENCES = 4;
</script>

<div class="workpage syncpage">
  <PageHeader title="Sync">
    {#snippet actions()}
      {#if sync.signedIn}
        <button
          type="button"
          class="primary sync-now"
          disabled={sync.busy || offline}
          title={syncTitle}
          onclick={() => void sync.syncNow()}
        >
          <Icon name="refresh" size={16} />
          {sync.busy ? 'Syncing…' : 'Sync now'}
        </button>
      {/if}
    {/snippet}
  </PageHeader>

  <div class="scroll">
    <div class="column">
      {#if sync.signedIn}
        <section class="block" aria-labelledby="status-label">
          <h2 class="sr-only" id="status-label">Status</h2>
          <div class="status {line.tone}" role="status" aria-live="polite">
            <p class="state"><span class="glyph" aria-hidden="true"></span>{line.label}</p>
            <p class="state-detail">{line.detail}</p>
          </div>
          <dl class="facts">
            <div>
              <dt>Last sync</dt>
              <dd>{lastSync}</dd>
            </div>
            <div>
              <dt>Server</dt>
              <dd class="mono">{sync.status?.settings.server_url}</dd>
            </div>
            <div>
              <dt>Account</dt>
              <dd class="mono">{sync.status?.settings.email}</dd>
            </div>
            <div>
              <dt>Waiting to send</dt>
              <dd>{plural(sync.pending, 'change')}</dd>
            </div>
            <div>
              <dt>Conflicts</dt>
              <dd>{sync.conflictCount}</dd>
            </div>
          </dl>
        </section>

        {#if sync.conflicts.length > 0}
          <section aria-labelledby="conflicts-label" class="group conflicts">
            <h2 class="label" id="conflicts-label">Conflicts</h2>
            <p class="note">
              Choose which copy to keep. The other one is replaced; nothing is overwritten until you
              choose.
            </p>
            <ul>
              {#each sync.conflicts as conflict (conflict.record_id)}
                {@const changed = differences(conflict)}
                <li>
                  <p class="record">
                    <span class="record-name">{recordName(conflict)}</span>
                    <span class="kind">{KINDS[conflict.kind] ?? conflict.kind}</span>
                  </p>
                  {#if conflict.remote === null}
                    <p class="note">Deleted on the server, changed on this device.</p>
                  {:else if conflict.local === null}
                    <p class="note">Deleted on this device, changed on the server.</p>
                  {:else if changed.length > 0}
                    <table class="diff">
                      <thead>
                        <tr><th>Field</th><th>This device</th><th>Server</th></tr>
                      </thead>
                      <tbody>
                        {#each changed.slice(0, MAX_DIFFERENCES) as row (row.field)}
                          <tr>
                            <th scope="row">{row.field}</th>
                            <td>{row.local}</td>
                            <td>{row.server}</td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                    {#if changed.length > MAX_DIFFERENCES}
                      <p class="note">
                        and {changed.length - MAX_DIFFERENCES} more field{changed.length -
                          MAX_DIFFERENCES ===
                        1
                          ? ''
                          : 's'}
                      </p>
                    {/if}
                  {/if}
                  <div class="choices">
                    <button
                      type="button"
                      disabled={sync.busy}
                      title={sync.busy ? 'A sync is running.' : undefined}
                      onclick={() => void sync.resolve(conflict.record_id, false)}
                    >
                      Take the server’s copy
                    </button>
                    <button
                      type="button"
                      disabled={sync.busy}
                      title={sync.busy ? 'A sync is running.' : undefined}
                      onclick={() => void sync.resolve(conflict.record_id, true)}
                    >
                      Keep this device’s copy
                    </button>
                  </div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        <section aria-labelledby="devices-label" class="group">
          <h2 class="label" id="devices-label">Devices</h2>
          <ul class="devices">
            <li>
              <span class="device-name">
                {sync.status?.settings.device_name || 'This device'}
                <span class="this">This device</span>
              </span>
              <span class="mono device-id">{sync.status?.settings.device_id}</span>
            </li>
          </ul>
          <p class="note">
            The desktop app cannot list your other devices yet. To revoke one, use the server’s
            device list.
          </p>
        </section>

        <section aria-labelledby="account-actions-label" class="group">
          <h2 class="label" id="account-actions-label">Account</h2>
          <div class="account-actions">
            <button
              type="button"
              aria-expanded={configuring}
              aria-controls="sync-account-form"
              onclick={() => (configuring = !configuring)}
            >
              {configuring ? 'Hide the form' : 'Change server or account'}
            </button>
            <button type="button" class="danger quiet" disabled={sync.busy} onclick={signOut}>
              Sign out
            </button>
          </div>
          <p class="note">
            Signing out forgets the token and the server address. Hosts and preferences stay on this
            device.
          </p>
        </section>
      {/if}

      {#if !sync.signedIn || configuring}
        <section
          class="group"
          id="sync-account-form"
          aria-labelledby="account-label"
          use:appear={{ enabled: page.value }}
        >
          <h2 class="label" id="account-label">
            {sync.signedIn ? 'Change server or account' : 'Sign in'}
          </h2>
          {#if !sync.signedIn}
            <p class="lead">Connect to a sync server</p>
            <p class="note">
              Sign in to a Termru sync server to keep hosts, folders and preferences in step.
              Passwords and keys are never uploaded.
            </p>
            <p class="note">
              Sync is off. Hosts, folders, identities and preferences are saved on this device only.
            </p>
          {/if}
          <form class="form" onsubmit={signIn} oninput={() => (dirty = true)} novalidate>
            <Field label="Server address" id="sync-server" error={shown('server')}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="url"
                  placeholder="https://sync.example.com"
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={serverUrl}
                  onblur={() => touch('server')}
                />
              {/snippet}
            </Field>
            <div class="pair">
              <Field label="Email" id="sync-email" error={shown('email')}>
                {#snippet control(attrs)}
                  <input
                    {...attrs}
                    type="email"
                    autocomplete="username"
                    spellcheck="false"
                    bind:value={email}
                    onblur={() => touch('email')}
                  />
                {/snippet}
              </Field>
              <Field
                label="Password"
                id="sync-password"
                error={shown('password')}
                hint="Used to sign in once. Termru keeps a token in this device’s credential store, not the password."
              >
                {#snippet control(attrs)}
                  <input
                    {...attrs}
                    type="password"
                    autocomplete="current-password"
                    bind:value={password}
                    onblur={() => touch('password')}
                  />
                {/snippet}
              </Field>
            </div>
            <Field
              label="Device name"
              id="sync-device"
              optional
              hint="Shown in the server’s device list"
            >
              {#snippet control(attrs)}
                <input {...attrs} type="text" placeholder="my laptop" bind:value={deviceName} />
              {/snippet}
            </Field>

            {#if sync.error && !sync.signedIn}
              <p class="problem" role="alert">{sync.error}</p>
            {/if}

            <div class="form-actions">
              {#if sync.signedIn}
                <button type="button" class="quiet" onclick={() => (configuring = false)}>
                  Cancel
                </button>
              {/if}
              <button type="submit" class="primary" disabled={sync.busy}>
                {sync.busy ? 'Signing in…' : 'Sign in'}
              </button>
            </div>
          </form>
        </section>
      {/if}

      <section class="lists" aria-label="What syncs">
        <div class="list">
          <h2 class="label">What syncs</h2>
          <ul>
            <li>Hosts and folders</li>
            <li>Identities (names, comments, fingerprints)</li>
            <li>Preferences</li>
          </ul>
        </div>
        <div class="list">
          <h2 class="label">Stays on this device</h2>
          <ul>
            <li>Passwords and key passphrases</li>
            <li>Private keys and their paths</li>
            <li>Host key decisions</li>
          </ul>
        </div>
        <p class="note wide">Local work and SSH sessions never wait for sync</p>
      </section>
    </div>
  </div>
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x) var(--termru-space-40);
  }

  .column {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-28);
    max-width: 720px;
    min-width: 0;
  }

  .sync-now {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    min-width: 0;
  }

  /* A group's label: sentence case, 11 px. */
  .label {
    margin: 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .lead {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
    letter-spacing: -0.015em;
  }

  .note {
    margin: 0;
    max-width: 62ch;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  /* ---- the status: one panel, a word and what stands behind it ---- */

  .block {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-20);
    padding: var(--termru-space-20) var(--termru-space-24);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-side);
  }

  .state {
    display: flex;
    align-items: center;
    gap: var(--termru-space-10);
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
    letter-spacing: -0.015em;
    color: var(--termru-live-text);
  }

  .working .state {
    color: var(--termru-text);
  }

  .attention .state,
  .offline .state {
    color: var(--termru-warn-text);
  }

  .failed .state {
    color: var(--termru-err-text);
  }

  .state-detail {
    margin: var(--termru-space-4) 0 0;
    max-width: 60ch;
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  /* A shape per state besides the colour: a dot when up to date, a ring while
     it works, a diamond when it needs a decision or failed, a bar offline. */
  .glyph {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }

  .working .glyph {
    background: transparent;
    border: 1.5px solid currentColor;
  }

  .attention .glyph,
  .failed .glyph {
    border-radius: 1px;
    transform: rotate(45deg) scale(0.9);
  }

  .offline .glyph {
    width: 9px;
    height: 2px;
    border-radius: 1px;
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-10);
    margin: 0;
  }

  .facts > div {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: var(--termru-space-12);
  }

  dt {
    color: var(--termru-text-3);
  }

  dd {
    margin: 0;
    min-width: 0;
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  /* ---- conflicts ---- */

  .conflicts ul,
  .devices {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .conflicts li {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    padding: var(--termru-space-16) var(--termru-space-20);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-side);
  }

  .record {
    display: flex;
    align-items: baseline;
    gap: var(--termru-space-10);
    margin: 0;
    min-width: 0;
  }

  .record-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kind {
    flex: none;
    height: 20px;
    padding: 0 var(--termru-space-8);
    border-radius: 6px;
    background: var(--termru-raised);
    color: var(--termru-text-2);
    font-size: var(--termru-text-secondary-size);
    line-height: 20px;
  }

  .diff {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .diff th,
  .diff td {
    padding: var(--termru-space-4) var(--termru-space-12) var(--termru-space-4) 0;
    text-align: left;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  .diff thead th {
    font-size: var(--termru-text-meta-size);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .diff tbody th {
    font-family: var(--termru-font-mono);
    font-weight: 500;
    color: var(--termru-text-2);
  }

  .diff td {
    font-family: var(--termru-font-mono);
    color: var(--termru-text);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--termru-space-8);
  }

  /* ---- devices ---- */

  .devices li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--termru-space-16);
    min-width: 0;
  }

  .device-name {
    display: inline-flex;
    align-items: baseline;
    gap: var(--termru-space-10);
    min-width: 0;
    font-weight: 500;
  }

  .this {
    height: 20px;
    padding: 0 var(--termru-space-8);
    border-radius: 6px;
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
    font-size: var(--termru-text-secondary-size);
    font-weight: 400;
    line-height: 20px;
  }

  .device-id {
    color: var(--termru-text-3);
  }

  /* ---- account ---- */

  .account-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--termru-space-8);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    margin-top: var(--termru-space-8);
  }

  .pair {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--termru-space-16);
    align-items: start;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--termru-space-8);
  }

  .form-actions button {
    padding: 0 var(--termru-space-16);
  }

  .problem {
    margin: 0;
    color: var(--termru-err-text);
    overflow-wrap: anywhere;
  }

  /* ---- what syncs ---- */

  .lists {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--termru-space-12) var(--termru-space-24);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    min-width: 0;
  }

  .list ul {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-4);
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--termru-text-2);
  }

  .wide {
    grid-column: 1 / -1;
  }

  @media (max-width: 760px) {
    .pair,
    .lists {
      grid-template-columns: minmax(0, 1fr);
    }

    .facts > div {
      grid-template-columns: 96px minmax(0, 1fr);
    }
  }
</style>
