<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { api } from '../ipc';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { Identity } from '../types';

  let { open: visible = $bindable(false) }: { open?: boolean } = $props();

  let name = $state('');
  let comment = $state('');
  let bindings = $state<Record<string, string>>({});
  let error = $state<string | null>(null);
  let busy = $state(false);

  // The bindings are device-local: they say which key file each identity
  // uses on this machine.
  $effect(() => {
    if (!visible) return;
    void loadBindings();
  });

  async function loadBindings(): Promise<void> {
    try {
      const list = (await api.identityBindings()) ?? [];
      const map: Record<string, string> = {};
      for (const binding of list) map[binding.identityId] = binding.keyPath;
      bindings = map;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  function close(): void {
    visible = false;
    name = '';
    comment = '';
    error = null;
  }

  async function create(): Promise<void> {
    if (name.trim().length === 0) {
      error = 'The name of the identity is required.';
      return;
    }
    busy = true;
    error = null;
    const now = new Date().toISOString();
    const identity: Identity = {
      id: crypto.randomUUID(),
      name: name.trim(),
      comment: comment.trim() ? comment.trim() : null,
      fingerprint: null,
      created_at: now,
      updated_at: now,
      deleted: false,
    };
    try {
      await api.identitySave(identity);
      await hosts.load();
      name = '';
      comment = '';
      toasts.success(`Created ${identity.name}`);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  /** Opens the native file picker and binds the chosen key. */
  async function chooseKey(identity: Identity): Promise<void> {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: 'Choose a private key file',
      });
      if (typeof selected !== 'string' || selected.length === 0) return;
      await api.identityBind(identity.id, selected);
      bindings = { ...bindings, [identity.id]: selected };
      toasts.success(`Bound ${identity.name}`);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function unbind(identity: Identity): Promise<void> {
    try {
      await api.identityUnbind(identity.id);
      const next = { ...bindings };
      delete next[identity.id];
      bindings = next;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function remove(identity: Identity): Promise<void> {
    try {
      await api.identityDelete(identity.id);
      await hosts.load();
      const next = { ...bindings };
      delete next[identity.id];
      bindings = next;
    } catch (err) {
      error = errorMessage(err);
    }
  }
</script>

{#if visible}
  <div
    class="backdrop"
    role="presentation"
    onclick={(event) => event.target === event.currentTarget && close()}
  >
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Identities">
      <h2>Identities</h2>

      <p class="hint">
        An identity is a name for a key. The key file itself stays on this machine: only its path is
        stored, and it is never part of the sync.
      </p>

      <ul class="identities">
        {#each hosts.identities as identity (identity.id)}
          <li>
            <div class="name">
              <strong>{identity.name}</strong>
              {#if identity.comment}
                <span class="comment">{identity.comment}</span>
              {/if}
            </div>
            <code class="path">{bindings[identity.id] ?? 'no key bound'}</code>
            <div class="actions">
              <button type="button" onclick={() => void chooseKey(identity)}>Choose key…</button>
              {#if bindings[identity.id]}
                <button type="button" class="ghost" onclick={() => void unbind(identity)}
                  >Unbind</button
                >
              {/if}
              <button type="button" class="ghost" onclick={() => void remove(identity)}
                >Delete</button
              >
            </div>
          </li>
        {:else}
          <li class="empty">No identity yet.</li>
        {/each}
      </ul>

      <form
        onsubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <h3>New identity</h3>
        <label>
          Name
          <input bind:value={name} required maxlength="120" placeholder="work laptop" />
        </label>
        <label>
          Comment
          <input bind:value={comment} maxlength="240" placeholder="optional" />
        </label>
        <div class="actions">
          <button type="submit" class="primary" disabled={busy}>Create</button>
        </div>
      </form>

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      <div class="footer">
        <button type="button" onclick={close}>Close</button>
      </div>
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
    z-index: 45;
  }

  .dialog {
    width: min(34rem, 92vw);
    max-height: 88vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    padding: 1.1rem;
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  h3 {
    margin: 0 0 0.3rem;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--termru-muted);
  }

  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--termru-muted);
  }

  .identities {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .identities li {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.5rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
  }

  .identities .empty {
    border: none;
    color: var(--termru-muted);
    font-size: 0.8rem;
  }

  .name {
    display: flex;
    gap: 0.4rem;
    align-items: baseline;
    font-size: 0.85rem;
  }

  .comment {
    font-size: 0.75rem;
    color: var(--termru-muted);
  }

  .path {
    font-size: 0.72rem;
    color: var(--termru-muted);
    word-break: break-all;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    border-top: 1px solid var(--termru-border);
    padding-top: 0.7rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.78rem;
    color: var(--termru-muted);
  }

  input {
    padding: 0.4rem 0.5rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font: inherit;
  }

  .actions {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
  }

  .error {
    margin: 0;
    color: var(--termru-danger);
    font-size: 0.8rem;
  }
</style>
