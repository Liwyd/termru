<script lang="ts">
  import { autofocus } from '../actions/autofocus';
  import { focusTrap } from '../actions/focus-trap';
  import { api } from '../ipc';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { Group } from '../types';
  import ContextConfirm from './ContextConfirm.svelte';

  let {
    open: visible = $bindable(false),
    embedded = false,
    onclose,
  }: {
    /** Modal mode: whether the dialog is shown. */
    open?: boolean;
    /** Renders as a page section (no backdrop, always shown). */
    embedded?: boolean;
    /** Called when the editor is closed (page mode: go back). */
    onclose?: () => void;
  } = $props();

  let name = $state('');
  let parentId = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let confirmOpen = $state(false);
  let pendingDelete = $state<Group | null>(null);
  /** The folder being renamed inline, and the text typed so far. */
  let renamingId = $state<string | null>(null);
  let renameText = $state('');

  function close(): void {
    visible = false;
    name = '';
    parentId = '';
    error = null;
    renamingId = null;
    onclose?.();
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

  function startRename(group: Group): void {
    renamingId = group.id;
    renameText = group.name;
    error = null;
  }

  function cancelRename(): void {
    renamingId = null;
    renameText = '';
  }

  async function commitRename(group: Group): Promise<void> {
    const trimmed = renameText.trim();
    cancelRename();
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

{#snippet panel()}
  <div
    class="dialog"
    class:embedded
    role={embedded ? 'region' : 'dialog'}
    aria-modal={embedded ? undefined : 'true'}
    aria-label="Folders"
    use:focusTrap={{
      enabled: !embedded,
      // Escape first leaves an inline rename, then closes the dialog.
      onescape: () => (renamingId !== null ? cancelRename() : close()),
    }}
  >
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
            {#if renamingId === group.id}
              <form
                class="rename"
                onsubmit={(event) => {
                  event.preventDefault();
                  void commitRename(group);
                }}
              >
                <input
                  bind:value={renameText}
                  aria-label={`New name of ${group.name}`}
                  maxlength="120"
                  use:autofocus
                  onkeydown={(event) => {
                    // In page mode there is no dialog trap to handle Escape.
                    if (event.key === 'Escape') cancelRename();
                  }}
                />
                <button type="submit" class="primary">Save</button>
                <button type="button" onclick={cancelRename}>Cancel</button>
              </form>
            {:else}
              <span class="label">{option.label}</span>
              <div class="actions">
                <button type="button" onclick={() => startRename(group)}>Rename</button>
                <button type="button" class="ghost" onclick={() => void remove(group)}
                  >Delete</button
                >
              </div>
            {/if}
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
{/snippet}

{#if embedded}
  {@render panel()}
{:else if visible}
  <div
    class="backdrop"
    role="presentation"
    onclick={(event) => event.target === event.currentTarget && close()}
  >
    {@render panel()}
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

  /* Page mode: in flow, scrolls itself and keeps the inset of every page. */
  .dialog.embedded {
    width: auto;
    max-width: none;
    max-height: none;
    height: 100%;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x);
    border: none;
    border-radius: 0;
    background: transparent;
  }

  .dialog.embedded > * {
    width: 100%;
    max-width: 42rem;
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

  form.rename {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 0.4rem;
    flex: 1;
    border: none;
    padding: 0;
  }

  form.rename input {
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
