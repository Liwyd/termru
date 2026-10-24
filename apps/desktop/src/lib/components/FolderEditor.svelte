<script lang="ts">
  import { api } from '../ipc';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { Group } from '../types';
  import ContextConfirm from './ContextConfirm.svelte';

  let { open: visible = $bindable(false) }: { open?: boolean } = $props();

  let name = $state('');
  let parentId = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let confirmOpen = $state(false);
  let pendingDelete = $state<Group | null>(null);

  function close(): void {
    visible = false;
    name = '';
    parentId = '';
    error = null;
  }

  async function create(): Promise<void> {
    if (name.trim().length === 0) {
      error = 'The name of the folder is required.';
      return;
    }
    busy = true;
    error = null;
    const now = new Date().toISOString();
    const group: Group = {
      id: crypto.randomUUID(),
      name: name.trim(),
      parent_id: parentId.length > 0 ? parentId : null,
      sort_order: 0,
      created_at: now,
      updated_at: now,
      deleted: false,
    };
    try {
      await api.groupSave(group);
      await hosts.load();
      name = '';
      parentId = '';
      toasts.success(`Created ${group.name}`);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function rename(group: Group): Promise<void> {
    const next = window.prompt('New name of the folder', group.name);
    if (next === null) return;
    const trimmed = next.trim();
    if (trimmed.length === 0 || trimmed === group.name) return;
    try {
      await api.groupSave({ ...group, name: trimmed });
      await hosts.load();
    } catch (err) {
      error = errorMessage(err);
    }
  }

  function remove(group: Group): void {
    pendingDelete = group;
    confirmOpen = true;
  }

  async function confirmedDelete(): Promise<void> {
    const group = pendingDelete;
    pendingDelete = null;
    if (!group) return;
    try {
      await api.groupDelete(group.id);
      await hosts.load();
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
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Folders">
      <h2>Folders</h2>

      <p class="hint">
        A folder groups the hosts in the sidebar. Folders can be nested; a host lives in exactly one
        folder.
      </p>

      <ul>
        {#each hosts.folderOptions as option (option.id)}
          {@const group = hosts.groups.find((entry) => entry.id === option.id)}
          {#if group}
            <li>
              <span class="label">{option.label}</span>
              <div class="actions">
                <button type="button" onclick={() => void rename(group)}>Rename</button>
                <button type="button" class="ghost" onclick={() => void remove(group)}
                  >Delete</button
                >
              </div>
            </li>
          {/if}
        {:else}
          <li class="empty">No folder yet.</li>
        {/each}
      </ul>

      <form
        onsubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <h3>New folder</h3>
        <label>
          Name
          <input bind:value={name} required maxlength="120" placeholder="production" />
        </label>
        <label>
          Parent
          <select bind:value={parentId}>
            <option value="">No parent</option>
            {#each hosts.folderOptions as option (option.id)}
              <option value={option.id}>{option.label}</option>
            {/each}
          </select>
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

<ContextConfirm
  bind:open={confirmOpen}
  title={pendingDelete ? `Delete the folder “${pendingDelete.name}”?` : 'Delete folder?'}
  body="The hosts stay, without a folder."
  onconfirm={() => void confirmedDelete()}
/>

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
    width: min(32rem, 92vw);
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

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }

  li.empty {
    color: var(--termru-muted);
    font-size: 0.8rem;
  }

  .label {
    flex: 1;
  }

  .actions {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
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

  input,
  select {
    padding: 0.4rem 0.5rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font: inherit;
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
