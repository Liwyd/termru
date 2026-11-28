<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Field from '../components/Field.svelte';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { listen } from '../ipc-backend';
  import { formatLastSession } from '../last-session';
  import { network } from '../stores/network.svelte';
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
    <div class="columns">
      <div class="main">
        {#if sync.signedIn}
          <section aria-labelledby="status-label">
            <p class="wp-label" id="status-label">Status</p>
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
            <section aria-labelledby="conflicts-label" class="conflicts">
              <p class="wp-label" id="conflicts-label">Conflicts</p>
              <p class="note">
                Choose which copy to keep. The other one is replaced; nothing is overwritten until
                you choose.
              </p>
              <ul>
                {#each sync.conflicts as conflict (conflict.record_id)}
                  {@const changed = differences(conflict)}
                  <li>
                    <div class="conflict-head">
                      <p class="record">
                        <span class="record-name">{recordName(conflict)}</span>
                        <span class="kind">{KINDS[conflict.kind] ?? conflict.kind}</span>
                      </p>
                    </div>
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
                        class="primary"
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

          <section aria-labelledby="devices-label">
            <p class="wp-label" id="devices-label">Devices</p>
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
        {/if}

        {#if !sync.signedIn || configuring}
          <section aria-labelledby="account-label">
            <p class="wp-label" id="account-label">
              {sync.signedIn ? 'CHANGE SERVER OR ACCOUNT' : 'SIGN IN'}
            </p>
            {#if !sync.signedIn}
              <h2>Connect to a sync server</h2>
              <p class="note lead">
                Sign in to a Termru sync server to keep hosts, folders and preferences in step.
                Passwords and keys are never uploaded.
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
      </div>

      <aside class="rail" aria-label="About sync">
        {#if sync.signedIn}
          <p class="wp-label">Account</p>
          <div class="account-actions">
            <button
              type="button"
              aria-expanded={configuring}
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
          <hr />
        {:else}
          <div class="callout">
            <p class="wp-label accent">Local workspace</p>
            <p>
              Sync is off. Hosts, folders, identities and preferences are saved on this device only.
            </p>
          </div>
        {/if}

        <p class="wp-label">What syncs</p>
        <ul class="plain">
          <li>Hosts and folders</li>
          <li>Identities (names, comments, fingerprints)</li>
          <li>Preferences</li>
        </ul>

        <p class="wp-label">Stays on this device</p>
        <ul class="plain">
          <li>Passwords and key passphrases</li>
          <li>Private keys and their paths</li>
          <li>Host key decisions</li>
        </ul>

        <p class="note">Local work and SSH sessions never wait for sync</p>
      </aside>
    </div>
  </div>
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x) var(--termru-space-32);
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-24);
  }

  .sync-now {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    padding: 0 var(--termru-space-24);
  }

  /* Main column | hairline | rail. */
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 360px;
    flex: 1;
    min-height: 0;
  }

  .main {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-32);
    padding-right: var(--termru-space-48);
    min-width: 0;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    padding-left: var(--termru-space-32);
    border-left: 1px solid var(--termru-border);
    min-width: 0;
    align-self: start;
  }

  .wp-label {
    margin-bottom: var(--termru-space-12);
  }

  .rail .wp-label {
    margin: var(--termru-space-12) 0 0;
  }

  .rail .wp-label:first-child {
    margin-top: 0;
  }

  h2 {
    margin: 0 0 var(--termru-space-8);
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
  }

  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .conflicts .note {
    margin-bottom: var(--termru-space-12);
  }

  .note.lead {
    margin-bottom: var(--termru-space-16);
    max-width: 52ch;
  }

  .mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    overflow-wrap: anywhere;
  }

  /* ---- status ---- */

  .status {
    padding: var(--termru-space-16);
    border-left: 2px solid var(--termru-accent);
    background: var(--termru-accent-soft);
  }

  .status.working {
    border-left-color: var(--termru-text-2);
    background: var(--termru-side);
  }

  .status.attention,
  .status.offline {
    border-left-color: var(--termru-warn-text);
    background: var(--termru-warn-soft);
  }

  .status.failed {
    border-left-color: var(--termru-err-text);
    background: var(--termru-err-soft);
  }

  .state {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 600;
  }

  .ok .state {
    color: var(--termru-accent-text);
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
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  /* Glyph per state, as on the session badge: ■ ok, □ working, ◆ failed or
     needs a decision, ▬ offline. */
  .glyph {
    flex: none;
    width: 8px;
    height: 8px;
    background: currentColor;
  }

  .working .glyph {
    background: transparent;
    border: 1.5px solid currentColor;
  }

  .attention .glyph,
  .failed .glyph {
    transform: rotate(45deg) scale(0.9);
  }

  .offline .glyph {
    width: 9px;
    height: 2px;
  }

  .facts {
    margin: var(--termru-space-16) 0 0;
  }

  .facts > div {
    display: grid;
    grid-template-columns: 160px minmax(0, 1fr);
    gap: var(--termru-space-16);
    padding: var(--termru-space-12) 0;
    border-bottom: 1px solid var(--termru-border);
  }

  dt {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  dd {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-secondary-lh);
    overflow-wrap: anywhere;
  }

  dd.mono {
    font-size: var(--termru-text-secondary-size);
  }

  /* ---- conflicts ---- */

  .conflicts ul,
  .devices {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    margin: 0 0 var(--termru-space-12);
    padding: 0;
    list-style: none;
  }

  .conflicts li {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    padding: var(--termru-space-16);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
  }

  .record {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--termru-space-12);
    margin: 0;
  }

  .record-name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .kind {
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-text-2);
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
    padding: var(--termru-space-8) var(--termru-space-12) var(--termru-space-8) 0;
    text-align: left;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  .diff thead th {
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-2);
    border-bottom: 1px solid var(--termru-border);
  }

  .diff tbody th {
    font-weight: 600;
  }

  .diff td {
    font-family: var(--termru-font-mono);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--termru-space-12);
  }

  /* ---- devices ---- */

  .devices li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--termru-space-12);
    padding: var(--termru-space-12) 0;
    border-bottom: 1px solid var(--termru-border);
  }

  .device-name {
    display: inline-flex;
    align-items: baseline;
    gap: var(--termru-space-12);
    font-weight: 550;
  }

  .this {
    padding: 2px 8px;
    border-radius: var(--termru-radius-input);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: 550;
  }

  .device-id {
    color: var(--termru-text-2);
  }

  /* ---- form ---- */

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    max-width: 640px;
  }

  .pair {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--termru-space-16);
  }

  .problem {
    margin: 0;
    padding: var(--termru-space-12) var(--termru-space-16);
    border-left: 2px solid var(--termru-err-text);
    background: var(--termru-err-soft);
    color: var(--termru-err-text);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    overflow-wrap: anywhere;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--termru-space-12);
    padding-top: var(--termru-space-12);
  }

  /* ---- rail ---- */

  .account-actions {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--termru-space-8);
  }

  hr {
    width: 100%;
    margin: var(--termru-space-12) 0 0;
    border: none;
    border-top: 1px solid var(--termru-border);
  }

  .callout {
    padding: var(--termru-space-16);
    border-left: 2px solid var(--termru-accent);
    background: var(--termru-side);
  }

  .callout p {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .callout .wp-label.accent {
    margin-bottom: var(--termru-space-4);
    color: var(--termru-accent-text);
  }

  .plain {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--termru-text-secondary-size);
    line-height: 26px;
    color: var(--termru-text);
  }

  @media (max-width: 960px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--termru-space-32);
    }

    .main {
      padding-right: 0;
    }

    .rail {
      padding-left: 0;
      padding-top: var(--termru-space-24);
      border-left: none;
      border-top: 1px solid var(--termru-border);
    }

    .pair {
      grid-template-columns: minmax(0, 1fr);
    }

    .facts > div {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--termru-space-4);
    }
  }
</style>
