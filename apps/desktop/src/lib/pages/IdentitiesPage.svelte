<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { appear } from '../actions/appear';
  import Field from '../components/Field.svelte';
  import { formatCount } from '../count-line';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import SessionDialog from '../components/SessionDialog.svelte';
  import { openFile } from '../ipc-backend';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { intents } from '../stores/intents.svelte';
  import { identities } from '../stores/identities.svelte';
  import { route } from '../stores/route.svelte';
  import { notify } from '../notify.svelte';
  import { afterFirstPaint } from '../settled.svelte';
  import { MAX_NAME_LEN } from '../validation';
  import type { Host, Identity } from '../types';

  const PAGE = 20;
  const MIN_PASSPHRASE = 5;

  /** The rail edits the selected identity, or makes a new one. */
  let mode = $state<'edit' | 'create'>('edit');
  /** The form of a rail that opens while the page is open moves in; one there at the start does not. */
  const page = afterFirstPaint();
  let source = $state<'generate' | 'existing'>('generate');
  let name = $state('');
  let comment = $state('');
  let passphrase = $state('');
  let remember = $state(false);
  let nameTouched = $state(false);
  let passphraseTouched = $state(false);
  let formError = $state<string | null>(null);
  let busy = $state(false);
  let usersShown = $state(PAGE);
  /** What was copied last, for the "Copied" state of its button. */
  let copied = $state<'fingerprint' | 'public' | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  let deleteOpen = $state(false);
  let deleteTarget = $state<Identity | null>(null);

  onMount(() => {
    void identities
      .loadBindings()
      .catch((err) =>
        notify({ key: 'identity.load-failed', values: { detail: errorMessage(err) } }),
      );
    return () => clearTimeout(copiedTimer);
  });

  /** Hosts per identity, counted in one pass. */
  const usage = $derived.by(() => {
    const map = new Map<string, Host[]>();
    for (const host of hosts.hosts) {
      if (host.auth.type !== 'private_key') continue;
      const list = map.get(host.auth.identity_id);
      if (list) list.push(host);
      else map.set(host.auth.identity_id, [host]);
    }
    return map;
  });

  const sorted = $derived(
    [...hosts.identities].sort((a, b) => a.name.localeCompare(b.name) || (a.id < b.id ? -1 : 1)),
  );
  const identity = $derived(
    identities.selectedId
      ? (hosts.identities.find((entry) => entry.id === identities.selectedId) ?? null)
      : null,
  );
  /** The id of the selected identity: a primitive, so a refreshed list does not retrigger what follows it. */
  const identityId = $derived(identity?.id ?? null);
  const users = $derived(identity ? (usage.get(identity.id) ?? []) : []);
  const keyState = $derived(identity ? identities.keys[identity.id] : undefined);
  const keyPath = $derived(identity ? (identities.bindings[identity.id] ?? null) : null);

  // The first identity is selected when there is one and nothing is chosen.
  $effect(() => {
    if (identities.selectedId === null || !identity) {
      if (mode === 'edit') identities.selectedId = sorted[0]?.id ?? null;
    }
  });

  // The key of the selected identity is read once and cached.
  $effect(() => {
    const picked = identity;
    if (picked) untrack(() => void identities.ensureKey(picked));
  });

  // The rail follows the selection (by id: a refreshed list must not wipe what is typed).
  $effect(() => {
    void identityId;
    const editing = mode === 'edit';
    untrack(() => {
      if (editing && identity) {
        name = identity.name;
        comment = identity.comment ?? '';
      }
      nameTouched = false;
      formError = null;
      usersShown = PAGE;
    });
  });

  const dirty = $derived(
    mode === 'create'
      ? name.trim().length > 0
      : identity !== null &&
          (name.trim() !== identity.name || comment.trim() !== (identity.comment ?? '')),
  );
  const nameError = $derived.by(() => {
    const text = name.trim();
    if (text.length === 0) return 'An identity needs a name.';
    if ([...text].length > MAX_NAME_LEN) {
      return `An identity name can be at most ${MAX_NAME_LEN} characters.`;
    }
    if (/[\u0000-\u001f\u007f-\u009f]/u.test(text)) {
      return 'An identity name cannot contain control characters.';
    }
    return null;
  });
  const passphraseError = $derived(
    mode === 'create' &&
      source === 'generate' &&
      passphrase.length > 0 &&
      [...passphrase].length < MIN_PASSPHRASE
      ? `A passphrase needs at least ${MIN_PASSPHRASE} characters. Leave it empty for a key without one.`
      : null,
  );
  const hasBack = $derived(route.draft !== null);

  function select(id: string): void {
    identities.selectedId = id;
    mode = 'edit';
  }

  function startCreate(): void {
    mode = 'create';
    source = 'generate';
    name = '';
    comment = '';
    passphrase = '';
    remember = false;
    nameTouched = false;
    passphraseTouched = false;
    formError = null;
    void tick().then(() => document.getElementById('identity-name')?.focus());
  }

  // "New identity" from the palette asks for the form before this page exists:
  // it is served once the page is there (or at once).
  $effect(() => {
    if (intents.take('new-identity')) startCreate();
  });

  function cancelRail(): void {
    mode = 'edit';
    if (identity) {
      name = identity.name;
      comment = identity.comment ?? '';
    }
    passphrase = '';
    nameTouched = false;
    formError = null;
  }

  /** A host editor that was parked (to add an identity) gets the new one picked. */
  function offerToParkedEditor(made: Identity): void {
    const draft = route.draft;
    if (draft && draft.authType === 'private_key' && draft.identityId === '') {
      route.draft = { ...draft, identityId: made.id };
    }
  }

  async function submit(event?: SubmitEvent): Promise<void> {
    event?.preventDefault();
    if (busy) return;
    nameTouched = true;
    passphraseTouched = true;
    formError = null;
    if (nameError) {
      document.getElementById('identity-name')?.focus();
      return;
    }
    if (mode === 'create' && passphraseError) {
      document.getElementById('identity-passphrase')?.focus();
      return;
    }
    busy = true;
    try {
      if (mode === 'edit' && identity) {
        const saved = await identities.rename(identity, name, comment);
        notify({ key: 'identity.saved', entityId: saved.id, values: { name: saved.name } });
      } else if (source === 'generate') {
        const made = await identities.generate({
          name,
          comment,
          passphrase,
          rememberPassphrase: remember,
        });
        notify({
          key: 'identity.generated',
          entityId: made.identity.id,
          values: { name: made.identity.name },
        });
        if (made.passphrase_note) {
          notify({ key: 'identity.passphrase-note', values: { detail: made.passphrase_note } });
        }
        offerToParkedEditor(made.identity);
        passphrase = '';
        identities.selectedId = made.identity.id;
        mode = 'edit';
      } else {
        // An existing key: the file is chosen first, so no empty identity is left behind.
        const picked = await openFile({ title: 'Choose a private key file' });
        if (typeof picked !== 'string' || picked.length === 0) return;
        const created = await identities.create(name, comment);
        try {
          await identities.bind(created, picked);
        } catch (err) {
          notify({
            key: 'identity.created-unbound',
            entityId: created.id,
            values: { name: created.name, detail: errorMessage(err) },
          });
        }
        notify({ key: 'identity.created', entityId: created.id, values: { name: created.name } });
        offerToParkedEditor(created);
        identities.selectedId = created.id;
        mode = 'edit';
      }
    } catch (err) {
      formError = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function chooseKey(target: Identity): Promise<void> {
    try {
      const picked = await openFile({ title: 'Choose a private key file' });
      if (typeof picked !== 'string' || picked.length === 0) return;
      await identities.bind(target, picked);
      notify({ key: 'identity.bound', entityId: target.id, values: { name: target.name } });
    } catch (err) {
      notify({ key: 'identity.bind-failed', values: { detail: errorMessage(err) } });
    }
  }

  async function unbind(target: Identity): Promise<void> {
    try {
      await identities.unbind(target);
      notify({ key: 'identity.unbound', entityId: target.id, values: { name: target.name } });
    } catch (err) {
      notify({ key: 'identity.unbind-failed', values: { detail: errorMessage(err) } });
    }
  }

  async function copy(kind: 'fingerprint' | 'public', text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      notify({ key: 'clipboard.copy-failed' });
      return;
    }
    copied = kind;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied = null), 1600);
  }

  function askDelete(): void {
    if (!identity) return;
    deleteTarget = identity;
    deleteOpen = true;
  }

  const deleteText = $derived(deleteTarget ? identities.deleteBody(deleteTarget) : '');

  async function confirmDelete(): Promise<void> {
    const target = deleteTarget;
    deleteOpen = false;
    if (!target) return;
    try {
      await identities.remove(target);
      if (identities.selectedId === target.id) identities.selectedId = null;
      notify({ key: 'identity.deleted', entityId: target.id, values: { name: target.name } });
    } catch (err) {
      notify({ key: 'identity.delete-failed', values: { detail: errorMessage(err) } });
    }
  }

  function shortFingerprint(value: string | null): string {
    return value ? `${value.slice(0, 14)}…` : 'No fingerprint yet';
  }

  function hostLabel(count: number): string {
    return `${count} ${count === 1 ? 'host' : 'hosts'}`;
  }
</script>

<div class="workpage identities">
  <PageHeader title="Identities" count={formatCount(hosts.identities.length)}>
    {#snippet actions()}
      {#if hasBack}
        <button type="button" class="ghost" onclick={() => route.back()}>Back to host</button>
      {/if}
      <button type="button" class="primary new" onclick={startCreate}>
        <Icon name="plus" size={16} />
        New identity
      </button>
    {/snippet}
  </PageHeader>

  <div class="scroll">
    <div class="columns">
      <!-- Column 1: the identities. -->
      <nav class="list" aria-label="Identities">
        <p class="wp-label">Identities</p>
        {#if sorted.length === 0}
          <p class="empty-list">
            No identity yet. Generate a key or add an existing one to sign in without a password.
          </p>
        {:else}
          <ul>
            {#each sorted as entry (entry.id)}
              <li>
                <button
                  type="button"
                  class="item"
                  class:active={mode === 'edit' && identity?.id === entry.id}
                  aria-current={mode === 'edit' && identity?.id === entry.id ? 'true' : undefined}
                  onclick={() => select(entry.id)}
                >
                  <Icon name="key" size={17} />
                  <span class="who">
                    <span class="item-name">{entry.name}</span>
                    <span class="item-sub">{shortFingerprint(entry.fingerprint)}</span>
                  </span>
                  <span class="count">{usage.get(entry.id)?.length ?? 0}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <p class="footnote">
          An identity is a name for a key. Only the path of the key file is stored, and it never
          leaves this device.
        </p>
      </nav>

      <!-- Column 2: the selected identity. -->
      <section class="detail" aria-labelledby="detail-title">
        <!-- The buttons change their label; this says it aloud too. -->
        <span class="sr-only" role="status"
          >{copied === 'fingerprint'
            ? 'Fingerprint copied'
            : copied === 'public'
              ? 'Public key copied'
              : ''}</span
        >
        {#if hosts.error}
          <p class="problem" role="alert">
            Could not read your hosts: {hosts.error}
            <button type="button" class="link" onclick={() => void hosts.load()}>Retry</button>
          </p>
        {/if}
        {#if identity && mode === 'edit'}
          <header class="detail-head">
            <div class="titles">
              <p class="trail">{identity.comment ?? 'Identity'}</p>
              <h2 id="detail-title">{identity.name}</h2>
            </div>
            <p class="wp-label count-label">{hostLabel(users.length).toUpperCase()} USING</p>
          </header>

          <section class="block" aria-labelledby="key-file-label">
            <p class="wp-label" id="key-file-label">Key file</p>
            {#if keyPath}
              <code class="path">{keyPath}</code>
            {:else}
              <p class="muted">No key file is bound to this identity on this device.</p>
            {/if}
            <div class="inline">
              <button type="button" onclick={() => void chooseKey(identity)}>
                {keyPath ? 'Choose another key…' : 'Choose key…'}
              </button>
              {#if keyPath}
                <button type="button" class="quiet" onclick={() => void unbind(identity)}>
                  Unbind
                </button>
              {/if}
            </div>
          </section>

          {#if keyState?.status === 'loading'}
            <p class="muted" aria-live="polite">Reading the key…</p>
          {:else if keyState?.status === 'error'}
            <p class="problem" role="alert">{keyState.message}</p>
          {:else if keyState?.status === 'ready'}
            <section class="block" aria-labelledby="fingerprint-label">
              <p class="wp-label" id="fingerprint-label">Fingerprint</p>
              <div class="copyline">
                <code class="value">{keyState.info.fingerprint}</code>
                <button
                  type="button"
                  class="copy"
                  aria-label="Copy the fingerprint"
                  onclick={() => void copy('fingerprint', keyState.info.fingerprint)}
                >
                  <Icon name={copied === 'fingerprint' ? 'check' : 'copy'} size={15} />
                  {copied === 'fingerprint' ? 'Copied' : 'Copy'}
                </button>
              </div>
              <p class="muted small">
                {keyState.info.algorithm}{keyState.info.encrypted
                  ? ' · protected by a passphrase'
                  : ' · no passphrase'}
              </p>
            </section>

            <section class="block" aria-labelledby="public-label">
              <p class="wp-label" id="public-label">Public key</p>
              <div class="copyline top">
                <code class="value public">{keyState.info.public_key}</code>
                <button
                  type="button"
                  class="copy"
                  aria-label="Copy the public key"
                  onclick={() => void copy('public', keyState.info.public_key)}
                >
                  <Icon name={copied === 'public' ? 'check' : 'copy'} size={15} />
                  {copied === 'public' ? 'Copied' : 'Copy'}
                </button>
              </div>
              <p class="muted small">
                Add this line to <code>~/.ssh/authorized_keys</code> on the server. It is safe to share.
              </p>
            </section>
          {/if}

          <section class="block" aria-labelledby="users-label">
            <p class="wp-label" id="users-label">Hosts using this identity</p>
            {#if users.length === 0}
              <p class="muted">No host signs in with this identity yet.</p>
            {:else}
              <ul class="users">
                {#each users.slice(0, usersShown) as host (host.id)}
                  <li>
                    <span class="user-name">{host.name}</span>
                    <span class="target">{host.username}@{host.address}:{host.port}</span>
                    <button type="button" class="link" onclick={() => route.edit(host)}>
                      Edit<span class="sr-only"> {host.name}</span>
                    </button>
                  </li>
                {/each}
              </ul>
              {#if users.length > usersShown}
                <button type="button" class="more" onclick={() => (usersShown += 50)}>
                  Show {Math.min(50, users.length - usersShown)} more of {users.length - usersShown}
                </button>
              {/if}
            {/if}
          </section>
        {:else if mode === 'create'}
          <header class="detail-head">
            <div class="titles">
              <p class="trail">Identity</p>
              <h2 id="detail-title">New identity</h2>
            </div>
          </header>
          <p class="muted">
            Generate a fresh ed25519 key, or point an identity at a key file you already have. The
            private key never leaves this device and is never synced.
          </p>
        {:else}
          <header class="detail-head">
            <div class="titles">
              <p class="trail">Identity</p>
              <h2 id="detail-title">No identity selected</h2>
            </div>
          </header>
          <p class="muted">Choose an identity in the list, or make a new one.</p>
        {/if}
      </section>

      <!-- Column 3: the rail. -->
      <aside class="rail" aria-label={mode === 'create' ? 'New identity' : 'Edit identity'}>
        <p class="wp-label accent">{mode === 'create' ? 'NEW IDENTITY' : 'EDIT IDENTITY'}</p>

        {#if mode === 'create' || identity}
          <form
            onsubmit={(event) => void submit(event)}
            novalidate
            use:appear={{ enabled: page.value }}
          >
            {#if mode === 'create'}
              <div class="choices" role="radiogroup" aria-label="Where the key comes from">
                <label class="choice" class:selected={source === 'generate'}>
                  <input type="radio" name="source" value="generate" bind:group={source} />
                  <span aria-hidden="true">{source === 'generate' ? '●' : '○'}</span> Generate a key
                </label>
                <label class="choice" class:selected={source === 'existing'}>
                  <input type="radio" name="source" value="existing" bind:group={source} />
                  <span aria-hidden="true">{source === 'existing' ? '●' : '○'}</span> Use a key file
                </label>
              </div>
            {/if}

            <Field label="Name" id="identity-name" error={nameTouched ? nameError : null}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder="work laptop"
                  bind:value={name}
                  onblur={() => (nameTouched = true)}
                />
              {/snippet}
            </Field>

            <Field label="Comment" id="identity-comment" optional>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  maxlength="240"
                  bind:value={comment}
                />
              {/snippet}
            </Field>

            {#if mode === 'create' && source === 'generate'}
              <Field
                label="Passphrase"
                id="identity-passphrase"
                optional
                error={passphraseTouched ? passphraseError : null}
                hint="Protects the key file if someone copies it. Leave empty for none."
              >
                {#snippet control(attrs)}
                  <input
                    {...attrs}
                    type="password"
                    autocomplete="new-password"
                    bind:value={passphrase}
                    onblur={() => (passphraseTouched = true)}
                  />
                {/snippet}
              </Field>
              {#if passphrase.length > 0}
                <label class="check">
                  <input type="checkbox" bind:checked={remember} />
                  <span
                    >Remember the passphrase on this device
                    <span class="muted">· Otherwise ask on connect</span></span
                  >
                </label>
              {/if}
              <p class="note">
                A new ed25519 key is written to Termru’s key folder, readable by you only. An
                existing file is never overwritten.
              </p>
            {:else if mode === 'create'}
              <p class="note">
                You pick the private key file next. Only its path is stored; the file is not copied
                or read into the database.
              </p>
            {/if}

            {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}

            <div class="rail-actions">
              <button type="button" class="quiet" onclick={cancelRail}>Cancel</button>
              <button
                type="submit"
                class="primary"
                disabled={busy || (mode === 'edit' && !dirty)}
                title={mode === 'edit' && !dirty && !busy ? 'No changes to save' : undefined}
              >
                {#if mode === 'edit'}
                  Save identity
                {:else if source === 'generate'}
                  {busy ? 'Generating…' : 'Generate key'}
                {:else}
                  Choose key file…
                {/if}
              </button>
            </div>
          </form>
        {:else}
          <p class="note">Choose an identity to rename it, or make a new one.</p>
        {/if}

        {#if identity && mode === 'edit'}
          <hr />
          <button type="button" class="danger quiet delete" onclick={askDelete}>
            <Icon name="trash" size={15} />
            Delete identity
          </button>
        {/if}
      </aside>
    </div>
  </div>

  <SessionDialog
    open={deleteOpen}
    title={deleteTarget ? `Delete “${deleteTarget.name}”?` : 'Delete identity?'}
    description={deleteText}
    onclose={() => (deleteOpen = false)}
  >
    {#snippet actions()}
      <button type="button" class="quiet" data-autofocus onclick={() => (deleteOpen = false)}>
        Cancel
      </button>
      <button type="button" class="danger-fill" onclick={() => void confirmDelete()}
        >Delete identity</button
      >
    {/snippet}
  </SessionDialog>
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

  .new {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    padding: 0 var(--termru-space-24);
  }

  .columns {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr) 392px;
    flex: 1;
    min-height: 0;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    padding-right: var(--termru-space-24);
    border-right: 1px solid var(--termru-border);
    min-width: 0;
  }

  .list ul {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    width: 100%;
    min-height: 54px;
    padding: 0 var(--termru-space-12);
    border: none;
    border-radius: var(--termru-radius-button);
    background: none;
    color: var(--termru-text);
    text-align: left;
  }

  .item :global(.icon) {
    color: var(--termru-text-2);
  }

  .who {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .item-name {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-sub {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: 400;
    color: var(--termru-text-2);
  }

  .count {
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
    font-variant-numeric: tabular-nums;
  }

  /* A row of a list is not a button that gives under the finger (it would
     shrink the whole row), and it needs a hover of its own. */
  .item.item:active:not(:disabled) {
    transform: none;
  }

  @media (hover: hover) and (pointer: fine) {
    .item:hover:not(.active) {
      background-color: var(--termru-side);
    }
  }

  .item.active {
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
  }

  .item.active :global(.icon),
  .item.active .count,
  .item.active .item-sub {
    color: var(--termru-accent-text);
  }

  .empty-list,
  .footnote {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .footnote {
    padding-top: var(--termru-space-16);
    border-top: 1px solid var(--termru-border);
  }

  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-24);
    padding: 0 var(--termru-space-32);
    min-width: 0;
  }

  .detail-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--termru-space-16);
  }

  .titles {
    min-width: 0;
  }

  .trail {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  h2 {
    margin: 0;
    font-size: 24px;
    line-height: 36px;
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .count-label {
    flex: none;
    padding-bottom: var(--termru-space-8);
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    padding-top: var(--termru-space-24);
    border-top: 1px solid var(--termru-border);
    min-width: 0;
  }

  .block:first-of-type {
    border-top: none;
    padding-top: 0;
  }

  .muted {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    color: var(--termru-text-2);
  }

  .muted.small {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .problem {
    margin: 0;
    padding: var(--termru-space-12) var(--termru-space-16);
    border-left: 2px solid var(--termru-err-text);
    background: var(--termru-err-soft);
    color: var(--termru-err-text);
    font-size: var(--termru-text-control-size);
    overflow-wrap: anywhere;
  }

  code {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  .path {
    line-height: 20px;
    overflow-wrap: anywhere;
  }

  .inline {
    display: flex;
    gap: var(--termru-space-8);
    flex-wrap: wrap;
  }

  .copyline {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    min-width: 0;
  }

  .copyline.top {
    align-items: flex-start;
  }

  .value {
    flex: 1;
    min-width: 0;
    padding: var(--termru-space-12);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-field);
    line-height: 20px;
    overflow-wrap: anywhere;
  }

  .value.public {
    max-height: 120px;
    overflow: auto;
    user-select: all;
  }

  .copy {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    flex: none;
    min-width: 88px;
    justify-content: center;
  }

  .users {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .users li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr) auto;
    align-items: center;
    gap: var(--termru-space-16);
    min-height: 48px;
    border-bottom: 1px solid var(--termru-border);
  }

  .user-name {
    font-size: var(--termru-text-body-size);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    align-self: flex-start;
  }

  .users li .link.link {
    text-decoration: none;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    padding-left: 28px;
    border-left: 1px solid var(--termru-border);
    min-width: 0;
  }

  .rail .accent {
    color: var(--termru-accent-text);
  }

  .rail form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
  }

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
    min-height: 36px;
    padding: 0 var(--termru-space-12);
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

  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .check input {
    margin: 2px 0 0;
    width: 15px;
    height: 15px;
  }

  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .form-error {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-err-text);
    overflow-wrap: anywhere;
  }

  .rail-actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
  }

  .rail-actions .primary {
    padding: 0 var(--termru-space-24);
  }

  hr {
    width: 100%;
    margin: var(--termru-space-8) 0;
    border: none;
    border-top: 1px solid var(--termru-border);
  }

  .delete {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
  }

  @media (max-width: 1180px) {
    .columns {
      grid-template-columns: 240px minmax(0, 1fr);
      row-gap: var(--termru-space-32);
    }

    .rail {
      grid-column: 1 / -1;
      padding: var(--termru-space-24) 0 0;
      border-left: none;
      border-top: 1px solid var(--termru-border);
    }

    .detail {
      padding-right: 0;
    }
  }

  @media (max-width: 760px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }

    .list {
      padding-right: 0;
      padding-bottom: var(--termru-space-24);
      border-right: none;
      border-bottom: 1px solid var(--termru-border);
    }

    .list ul {
      max-height: 260px;
      overflow: auto;
    }

    .detail {
      padding: 0;
    }

    .users li {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .target {
      display: none;
    }
  }
</style>
