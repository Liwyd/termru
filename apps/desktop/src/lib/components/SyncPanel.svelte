<script lang="ts">
  import { sync } from '../stores/sync.svelte';
  import type { ConflictView } from '../types';

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let serverUrl = $state('');
  let email = $state('');
  let password = $state('');
  let deviceName = $state('');

  // Fill the form from the stored settings whenever the panel opens.
  $effect(() => {
    if (!open) return;
    const status = sync.status;
    serverUrl = status?.settings.server_url ?? '';
    email = status?.settings.email ?? '';
    deviceName = status?.settings.device_name ?? '';
  });

  async function signIn(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const ok = await sync.signIn(serverUrl, email, password, deviceName);
    if (ok) password = '';
  }

  /** A readable label for a record in the conflict list. */
  function label(conflict: ConflictView): string {
    const payload = conflict.local ?? conflict.remote;
    if (payload && typeof payload === 'object' && 'name' in payload) {
      const name = (payload as { name?: unknown }).name;
      if (typeof name === 'string') return name;
    }
    return conflict.record_id;
  }

  /** The field that differs between the two copies. */
  function difference(conflict: ConflictView): string | null {
    const local = conflict.local as Record<string, unknown> | null;
    const remote = conflict.remote as Record<string, unknown> | null;
    if (!local || !remote) return null;
    for (const key of Object.keys(local)) {
      if (key === 'updated_at' || key === 'server_rev' || key === 'dirty') continue;
      const a = JSON.stringify(local[key]);
      const b = JSON.stringify(remote[key]);
      if (a !== b) return key;
    }
    return null;
  }

  function formatTime(value: string | null): string {
    if (!value) return 'never';
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (open = false)}
  >
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Cloud sync">
      <header>
        <h2>Cloud sync</h2>
        <button type="button" class="ghost" onclick={() => (open = false)} aria-label="Close"
          >✕</button
        >
      </header>

      {#if sync.signedIn}
        <dl class="status">
          <dt>Server</dt>
          <dd class="mono">{sync.status?.settings.server_url}</dd>
          <dt>Account</dt>
          <dd>{sync.status?.settings.email}</dd>
          <dt>Device</dt>
          <dd>{sync.status?.settings.device_name || sync.status?.settings.device_id}</dd>
          <dt>Last sync</dt>
          <dd>{formatTime(sync.status?.last_sync_at ?? null)}</dd>
          <dt>Pending</dt>
          <dd>{sync.pending} change(s)</dd>
          <dt>Conflicts</dt>
          <dd class:danger={sync.conflictCount > 0}>{sync.conflictCount}</dd>
        </dl>

        {#if sync.error}
          <p class="hint error">{sync.error}</p>
        {/if}

        <div class="actions">
          <button
            type="button"
            onclick={() => {
              password = '';
              void sync.signOut();
            }}
            disabled={sync.busy}
          >
            Sign out
          </button>
          <button
            type="button"
            class="primary"
            onclick={() => void sync.syncNow()}
            disabled={sync.busy}
          >
            {sync.busy ? 'Syncing…' : 'Sync now'}
          </button>
        </div>
      {:else}
        <p class="hint">
          Sign in to a Termru sync server to keep your hosts in sync across devices. Secrets are
          never uploaded — they stay in this device's credential store.
        </p>

        <form onsubmit={signIn}>
          <label>
            <span>Server URL</span>
            <input
              type="url"
              required
              placeholder="https://sync.example.com"
              bind:value={serverUrl}
            />
          </label>
          <label>
            <span>Email</span>
            <input type="email" required autocomplete="username" bind:value={email} />
          </label>
          <label>
            <span>Password</span>
            <input type="password" required autocomplete="current-password" bind:value={password} />
          </label>
          <label>
            <span>Device name</span>
            <input type="text" placeholder="my laptop" bind:value={deviceName} />
          </label>

          {#if sync.error}
            <p class="hint error">{sync.error}</p>
          {/if}

          <div class="actions">
            <button type="button" onclick={() => (open = false)}>Cancel</button>
            <button type="submit" class="primary" disabled={sync.busy}>
              {sync.busy ? 'Signing in…' : 'Sign in'}
            </button>
          </div>
        </form>
      {/if}

      {#if sync.conflicts.length > 0}
        <section class="conflicts">
          <h3>Conflicts</h3>
          <p class="hint">
            Both this device and the server changed the same record. Choose which copy to keep —
            nothing is overwritten silently.
          </p>
          <ul>
            {#each sync.conflicts as conflict (conflict.record_id)}
              <li>
                <div class="conflict-head">
                  <strong>{label(conflict)}</strong>
                  <span class="kind">{conflict.kind} · rev {conflict.server_rev}</span>
                </div>
                {#if conflict.remote === null}
                  <p class="hint">The record was deleted on the server.</p>
                {:else if difference(conflict)}
                  <p class="hint">Different field: <code>{difference(conflict)}</code></p>
                {/if}
                <div class="actions">
                  <button
                    type="button"
                    onclick={() => void sync.resolve(conflict.record_id, false)}
                    disabled={sync.busy}
                  >
                    Take server copy
                  </button>
                  <button
                    type="button"
                    class="primary"
                    onclick={() => void sync.resolve(conflict.record_id, true)}
                    disabled={sync.busy}
                  >
                    Keep local copy
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }

  .dialog {
    width: min(34rem, 94vw);
    max-height: 88vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    padding: 1.1rem;
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  h3 {
    margin: 0;
    font-size: 0.85rem;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.8rem;
  }

  input {
    padding: 0.4rem 0.55rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
  }

  dl.status {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.2rem 0.6rem;
    margin: 0;
    font-size: 0.82rem;
  }

  dt {
    color: var(--termru-muted);
  }

  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  dd.danger {
    color: var(--termru-danger);
    font-weight: 600;
  }

  .mono {
    font-family: monospace;
  }

  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--termru-muted);
  }

  .hint.error {
    color: var(--termru-danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  .conflicts ul {
    list-style: none;
    margin: 0.4rem 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .conflicts li {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.5rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
  }

  .conflict-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .kind {
    font-size: 0.72rem;
    color: var(--termru-muted);
  }

  code {
    font-family: monospace;
  }
</style>
