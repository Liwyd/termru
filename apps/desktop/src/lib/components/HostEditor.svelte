<script lang="ts">
  import { hosts } from '../stores/hosts.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { AuthMethod, Host } from '../types';

  let {
    onmanageidentities,
  }: {
    onmanageidentities: () => void;
  } = $props();

  const AUTH_TYPES: { value: AuthMethod['type']; label: string }[] = [
    { value: 'password', label: 'Password' },
    { value: 'private_key', label: 'Private key' },
    { value: 'agent', label: 'SSH agent' },
  ];

  let draft = $state<Host | null>(null);
  let authType = $state<AuthMethod['type']>('password');
  let identityId = $state<string>('');
  let saving = $state(false);
  let formError = $state<string | null>(null);

  // Open the editor whenever the store opens it (existing host or new one).
  $effect(() => {
    if (!hosts.editorOpen) {
      draft = null;
      return;
    }
    const target = hosts.editing;
    draft = target ? { ...target, description: target.description ?? '' } : newHost();
    authType = target?.auth.type ?? 'password';
    identityId = target?.auth.type === 'private_key' ? target.auth.identity_id : '';
  });

  function close(): void {
    hosts.editorOpen = false;
    hosts.editing = null;
    draft = null;
    formError = null;
  }

  function newHost(): Host {
    const now = new Date().toISOString();
    return {
      id: crypto.randomUUID(),
      name: '',
      address: '',
      port: 22,
      username: '',
      auth: { type: 'password' },
      group_id: null,
      tags: [],
      favorite: false,
      description: '',
      term: 'xterm-256color',
      keepalive_seconds: 0,
      sort_order: 0,
      created_at: now,
      updated_at: now,
      deleted: false,
    };
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!draft) return;
    formError = null;

    if (draft.name.trim().length === 0) {
      formError = 'Name is required.';
      return;
    }
    if (draft.address.trim().length === 0) {
      formError = 'Address is required.';
      return;
    }
    if (draft.username.trim().length === 0) {
      formError = 'Username is required.';
      return;
    }
    if (draft.port < 1 || draft.port > 65535) {
      formError = 'Port must be between 1 and 65535.';
      return;
    }
    if (authType === 'private_key' && identityId.length === 0) {
      formError = 'Select an identity for key authentication.';
      return;
    }

    const auth: AuthMethod =
      authType === 'private_key'
        ? { type: 'private_key', identity_id: identityId }
        : authType === 'agent'
          ? { type: 'agent' }
          : { type: 'password' };

    const payload: Host = {
      ...draft,
      auth,
      name: draft.name.trim(),
      address: draft.address.trim(),
      username: draft.username.trim(),
      description: draft.description?.trim() ? draft.description.trim() : null,
    };
    saving = true;
    const saved = await hosts.save(payload);
    saving = false;
    if (saved) {
      toasts.success(`Saved ${saved.name}`);
      close();
    } else {
      formError = hosts.error ?? 'Could not save the host.';
    }
  }
</script>

{#if draft}
  <div
    class="backdrop"
    role="presentation"
    onclick={(event) => {
      if (event.target === event.currentTarget) close();
    }}
  >
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Host details">
      <form onsubmit={save}>
        <h2>{hosts.editing ? 'Edit host' : 'New host'}</h2>

        <label>
          Name
          <input bind:value={draft.name} required maxlength="120" />
        </label>

        <label>
          Address
          <input bind:value={draft.address} required placeholder="example.com" />
        </label>

        <div class="row">
          <label>
            Port
            <input type="number" bind:value={draft.port} min="1" max="65535" required />
          </label>
          <label>
            Username
            <input bind:value={draft.username} required />
          </label>
        </div>

        <label>
          Authentication
          <select bind:value={authType}>
            {#each AUTH_TYPES as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        {#if authType === 'private_key'}
          <label>
            Identity
            <select bind:value={identityId}>
              <option value="">Select an identity…</option>
              {#each hosts.identities as identity (identity.id)}
                <option value={identity.id}>{identity.name}</option>
              {/each}
            </select>
          </label>
          {#if hosts.identities.length === 0}
            <p class="hint">No identities yet — create one and bind a key file to it.</p>
          {/if}
          <button type="button" class="link" onclick={onmanageidentities}>Manage identities…</button
          >
        {/if}

        <div class="row">
          <label>
            Terminal type
            <input bind:value={draft.term} />
          </label>
          <label>
            Keepalive (s)
            <input type="number" bind:value={draft.keepalive_seconds} min="0" max="3600" />
          </label>
        </div>

        <label>
          Description
          <input bind:value={draft.description} placeholder="optional" />
        </label>

        {#if formError}
          <p class="error" role="alert">{formError}</p>
        {/if}

        <div class="actions">
          <button type="button" onclick={close}>Cancel</button>
          <button type="submit" class="primary" disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
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
    z-index: 40;
  }

  .dialog {
    width: min(34rem, 92vw);
    max-height: 90vh;
    overflow-y: auto;
    padding: 1.1rem;
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  h2 {
    margin: 0 0 0.2rem;
    font-size: 1rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
    color: var(--termru-muted);
  }

  input,
  select {
    padding: 0.4rem 0.5rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font: inherit;
  }

  .row {
    display: flex;
    gap: 0.6rem;
  }

  .row label {
    flex: 1;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.3rem;
  }

  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--termru-muted);
  }

  .link {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--termru-accent);
    text-decoration: underline;
    font-size: 0.78rem;
    cursor: pointer;
  }

  .error {
    margin: 0;
    color: var(--termru-danger);
    font-size: 0.8rem;
  }
</style>
