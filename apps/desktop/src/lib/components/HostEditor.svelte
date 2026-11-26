<script lang="ts">
  import { untrack } from 'svelte';
  import { focusTrap } from '../actions/focus-trap';
  import { api } from '../ipc';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { route, type HostEditDraft } from '../stores/route.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { AuthMethod, Host } from '../types';

  /**
   * The host editor. Mounted, it is open: the route (`route.editHost`, null
   * for a new host) is the only state that says what it edits.
   */
  let {
    onmanageidentities,
    onclose,
    embedded = false,
  }: {
    /** Leaves for the identities page; the unsaved form is handed over. */
    onmanageidentities: (draft: HostEditDraft) => void;
    /** Called when the editor is saved or cancelled. */
    onclose?: () => void;
    /** Renders as a page section instead of a modal dialog. */
    embedded?: boolean;
  } = $props();

  const AUTH_TYPES: { value: AuthMethod['type']; label: string }[] = [
    { value: 'password', label: 'Password' },
    { value: 'private_key', label: 'Private key' },
    { value: 'agent', label: 'SSH agent' },
  ];

  const DEFAULT_PORT = 22;
  const MAX_KEEPALIVE = 3600;

  let draft = $state<Host | null>(null);
  let authType = $state<AuthMethod['type']>('password');
  let identityId = $state<string>('');
  let groupId = $state<string>('');
  let selectedTags = $state<string[]>([]);
  let newTag = $state('');
  let saving = $state(false);
  let formError = $state<string | null>(null);
  // The numbers are edited as text: a cleared number input would bind `null`
  // and the backend refuses a `null` port.
  let portText = $state(String(DEFAULT_PORT));
  let keepaliveText = $state('0');

  /** The port, or null when the text is not a valid port (empty = default). */
  function parsePort(text: string): number | null {
    const trimmed = text.trim();
    if (trimmed === '') return DEFAULT_PORT;
    if (!/^\d+$/.test(trimmed)) return null;
    const value = Number(trimmed);
    return value >= 1 && value <= 65535 ? value : null;
  }

  /** The keepalive in seconds, or null when invalid (empty = 0). */
  function parseKeepalive(text: string): number | null {
    const trimmed = text.trim();
    if (trimmed === '') return 0;
    if (!/^\d+$/.test(trimmed)) return null;
    const value = Number(trimmed);
    return value <= MAX_KEEPALIVE ? value : null;
  }

  const portError = $derived(
    parsePort(portText) === null ? 'Port must be a whole number from 1 to 65535.' : null,
  );
  const keepaliveError = $derived(
    parseKeepalive(keepaliveText) === null
      ? `Keepalive must be a whole number from 0 to ${MAX_KEEPALIVE} seconds.`
      : null,
  );

  // Fill the form from the route: the host being edited, a new host, or the
  // draft that was parked while the user visited the identities page.
  $effect(() => {
    const target = route.editHost;
    untrack(() => load(target));
  });

  function load(target: Host | null): void {
    const parked = route.draft;
    if (parked) {
      route.draft = null;
      draft = { ...parked.host };
      authType = parked.authType;
      identityId = parked.identityId;
      groupId = parked.groupId;
      selectedTags = [...parked.tags];
    } else {
      draft = target ? { ...target, description: target.description ?? '' } : newHost();
      authType = target?.auth.type ?? 'password';
      identityId = target?.auth.type === 'private_key' ? target.auth.identity_id : '';
      groupId = target?.group_id ?? '';
      selectedTags = [...(target?.tags ?? [])];
    }
    portText = String(draft.port);
    keepaliveText = String(draft.keepalive_seconds ?? 0);
    newTag = '';
    formError = null;
  }

  /** Saved or cancelled: whoever mounted the editor takes it away. */
  function close(): void {
    formError = null;
    onclose?.();
  }

  /** The form as it is now, to park it while another page is open. */
  function snapshot(): HostEditDraft {
    const current = draft as Host;
    return {
      host: {
        ...current,
        port: parsePort(portText) ?? DEFAULT_PORT,
        keepalive_seconds: parseKeepalive(keepaliveText) ?? 0,
      },
      authType,
      identityId,
      groupId,
      tags: [...selectedTags],
    };
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

  /** Creates a tag and puts it on the host. */
  async function addTag(): Promise<void> {
    const name = newTag.trim();
    if (name.length === 0) return;
    formError = null;
    const now = new Date().toISOString();
    try {
      const created = await api.tagSave({
        id: crypto.randomUUID(),
        name,
        color: null,
        created_at: now,
        updated_at: now,
        deleted: false,
      });
      if (!selectedTags.includes(created.id)) selectedTags = [...selectedTags, created.id];
      newTag = '';
      await hosts.load();
    } catch (err) {
      formError = errorMessage(err);
    }
  }

  function toggleTag(id: string): void {
    selectedTags = selectedTags.includes(id)
      ? selectedTags.filter((entry) => entry !== id)
      : [...selectedTags, id];
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
    const port = parsePort(portText);
    if (port === null) {
      formError = portError;
      return;
    }
    const keepalive = parseKeepalive(keepaliveText);
    if (keepalive === null) {
      formError = keepaliveError;
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
      port,
      keepalive_seconds: keepalive,
      auth,
      name: draft.name.trim(),
      address: draft.address.trim(),
      username: draft.username.trim(),
      description: draft.description?.trim() ? draft.description.trim() : null,
      group_id: groupId.length > 0 ? groupId : null,
      tags: [...selectedTags],
    };
    saving = true;
    const saved = await hosts.save(payload);
    saving = false;
    if (saved) {
      toasts.success(`Saved ${saved.name}`);
      close();
    } else {
      formError = hosts.saveError ?? 'Could not save the host.';
    }
  }
</script>

{#snippet panel()}
  <div
    class="dialog"
    class:embedded
    role={embedded ? 'region' : 'dialog'}
    aria-modal={embedded ? undefined : 'true'}
    aria-label="Host details"
    use:focusTrap={{ enabled: !embedded, onescape: close }}
  >
    {#if draft}
      <form onsubmit={save} novalidate>
        <h2>{route.editHost ? 'Edit host' : 'New host'}</h2>

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
            <input
              inputmode="numeric"
              bind:value={portText}
              placeholder={String(DEFAULT_PORT)}
              aria-invalid={portError !== null}
              aria-describedby={portError ? 'port-error' : undefined}
              onblur={() => {
                if (portText.trim() === '') portText = String(DEFAULT_PORT);
              }}
            />
            {#if portError}
              <span class="field-error" id="port-error" role="alert">{portError}</span>
            {/if}
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
          <button type="button" class="link" onclick={() => onmanageidentities(snapshot())}
            >Manage identities…</button
          >
        {/if}

        <div class="row">
          <label>
            Terminal type
            <input bind:value={draft.term} />
          </label>
          <label>
            Keepalive (s)
            <input
              inputmode="numeric"
              bind:value={keepaliveText}
              placeholder="0"
              aria-invalid={keepaliveError !== null}
              aria-describedby={keepaliveError ? 'keepalive-error' : undefined}
              onblur={() => {
                if (keepaliveText.trim() === '') keepaliveText = '0';
              }}
            />
            {#if keepaliveError}
              <span class="field-error" id="keepalive-error" role="alert">{keepaliveError}</span>
            {/if}
          </label>
        </div>

        <label>
          Folder
          <select bind:value={groupId}>
            <option value="">No folder</option>
            {#each hosts.folderOptions as option (option.id)}
              <option value={option.id}>{option.label}</option>
            {/each}
          </select>
        </label>

        <fieldset>
          <legend>Tags</legend>
          {#if hosts.tags.length > 0}
            <div class="tags">
              {#each hosts.tags as tag (tag.id)}
                <label class="check">
                  <input
                    type="checkbox"
                    checked={selectedTags.includes(tag.id)}
                    onchange={() => toggleTag(tag.id)}
                  />
                  <span>{tag.name}</span>
                </label>
              {/each}
            </div>
          {/if}
          <div class="new-tag">
            <input bind:value={newTag} placeholder="New tag" maxlength="60" />
            <button type="button" onclick={() => void addTag()}>Add</button>
          </div>
        </fieldset>

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
    {/if}
  </div>
{/snippet}

{#if draft}
  {#if embedded}
    {@render panel()}
  {:else}
    <div
      class="backdrop"
      role="presentation"
      onclick={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      {@render panel()}
    </div>
  {/if}
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

  form {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .field-error {
    color: var(--termru-danger);
    font-size: 0.75rem;
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

  fieldset {
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    padding: 0.45rem 0.6rem 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  legend {
    font-size: 0.75rem;
    color: var(--termru-muted);
    padding: 0 0.3rem;
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .check {
    flex-direction: row;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.78rem;
    color: var(--termru-fg);
  }

  .new-tag {
    display: flex;
    gap: 0.4rem;
  }

  .new-tag input {
    flex: 1;
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
