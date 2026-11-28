<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import Field from '../components/Field.svelte';
  import { formatCount } from '../count-line';
  import Icon from '../components/Icon.svelte';
  import InspectorPanel from '../components/InspectorPanel.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import SessionDialog from '../components/SessionDialog.svelte';
  import SplitBody from '../components/SplitBody.svelte';
  import { openFile } from '../ipc-backend';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { intents } from '../stores/intents.svelte';
  import { identities } from '../stores/identities.svelte';
  import { route } from '../stores/route.svelte';
  import { notify } from '../notify.svelte';
  import { MAX_NAME_LEN } from '../validation';
  import type { Host, Identity } from '../types';

  const PAGE = 20;
  const MIN_PASSPHRASE = 5;

  /** The inspector edits the selected identity, or makes a new one. */
  let mode = $state<'edit' | 'create'>('edit');
  /**
   * How the selection last changed: a key never animates the inspector, and
   * neither does the first paint (nothing has been chosen yet).
   */
  let via = $state<'pointer' | 'keyboard'>('keyboard');
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

  // The rows say which algorithm their key is: read the keys of the rows once
  // (the store caches each answer, so a key is never read twice).
  $effect(() => {
    for (const entry of sorted.slice(0, 100)) untrack(() => void identities.ensureKey(entry));
  });

  // The inspector follows the selection (by id: a refreshed list must not wipe what is typed).
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

  /** What the inspector is about: a new value replaces its content. */
  const inspectorKey = $derived(mode === 'create' ? 'new' : (identity?.id ?? 'none'));

  function select(id: string, how: 'pointer' | 'keyboard'): void {
    identities.selectedId = id;
    mode = 'edit';
    via = how;
  }

  /** `detail` is 0 for a click made with the keyboard (Enter, Space). */
  function modality(event: MouseEvent): 'pointer' | 'keyboard' {
    return event.detail > 0 ? 'pointer' : 'keyboard';
  }

  function startCreate(how: 'pointer' | 'keyboard' = 'keyboard'): void {
    mode = 'create';
    via = how;
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

  function cancelForm(): void {
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

  /** What a row says its key is: the algorithm, or why it cannot. */
  function algorithmOf(entry: Identity): { text: string; tone: 'ok' | 'warn' | 'none' } {
    const state = identities.keys[entry.id];
    if (state?.status === 'ready') return { text: state.info.algorithm, tone: 'ok' };
    if (state?.status === 'error') return { text: 'Key unreadable', tone: 'warn' };
    if (state?.status === 'none' || (identities.bindingsLoaded && !identities.bindings[entry.id])) {
      return { text: 'No key file', tone: 'warn' };
    }
    return { text: '—', tone: 'none' };
  }
</script>

<div class="workpage identities">
  <PageHeader title="Identities" count={formatCount(hosts.identities.length)}>
    {#snippet actions()}
      {#if hasBack}
        <button type="button" class="ghost" onclick={() => route.back()}>Back to host</button>
      {/if}
      <button type="button" class="primary new" onclick={(event) => startCreate(modality(event))}>
        <Icon name="plus" size={16} />
        New identity
      </button>
    {/snippet}
  </PageHeader>

  <SplitBody>
    {#snippet main()}
      <div class="list">
        {#if hosts.error}
          <p class="problem" role="alert">
            Could not read your hosts: {hosts.error}
            <button type="button" class="link" onclick={() => void hosts.load()}>Retry</button>
          </p>
        {/if}

        {#if sorted.length === 0}
          <p class="empty">
            No identity yet. Generate a key or add an existing one to sign in without a password.
          </p>
        {:else}
          <nav class="table" aria-label="Identities">
            <div class="head" aria-hidden="true">
              <span></span>
              <span>Identity</span>
              <span class="col-algo">Algorithm</span>
              <span class="col-hosts">Hosts</span>
            </div>
            <ul>
              {#each sorted as entry (entry.id)}
                {@const algorithm = algorithmOf(entry)}
                <li>
                  <button
                    type="button"
                    class="item"
                    class:active={mode === 'edit' && identity?.id === entry.id}
                    aria-current={mode === 'edit' && identity?.id === entry.id ? 'true' : undefined}
                    onclick={(event) => select(entry.id, modality(event))}
                  >
                    <span class="key"><Icon name="key" size={15} /></span>
                    <span class="who">
                      <span class="item-name">{entry.name}</span>
                      <span class="item-sub" title={entry.fingerprint ?? undefined}
                        >{shortFingerprint(entry.fingerprint)}</span
                      >
                    </span>
                    <span class="algo {algorithm.tone} col-algo">{algorithm.text}</span>
                    <span class="col-hosts"
                      ><span class="count">{usage.get(entry.id)?.length ?? 0}</span><span
                        class="sr-only"
                      >
                        {(usage.get(entry.id)?.length ?? 0) === 1 ? 'host uses' : 'hosts use'} it</span
                      ></span
                    >
                  </button>
                </li>
              {/each}
            </ul>
          </nav>
        {/if}
        <p class="note">
          An identity is a name for a key. Only the path of the key file is stored, and it never
          leaves this device.
        </p>
      </div>
    {/snippet}

    {#snippet inspector()}
      <InspectorPanel
        label={mode === 'create' ? 'New identity' : 'Edit identity'}
        contentKey={inspectorKey}
        animate={via === 'pointer'}
      >
        <!-- The buttons change their label; this says it aloud too. -->
        <span class="sr-only" role="status"
          >{copied === 'fingerprint'
            ? 'Fingerprint copied'
            : copied === 'public'
              ? 'Public key copied'
              : ''}</span
        >

        {#if identity && mode === 'edit'}
          <div class="titles">
            <h2 class="insp-title" title={identity.name}>{identity.name}</h2>
            <p class="insp-sub" title={identity.comment ?? undefined}>
              {identity.comment ?? 'Identity'}
            </p>
          </div>

          <section class="insp-group" aria-labelledby="key-file-label">
            <p class="ttl" id="key-file-label">Key file</p>
            {#if keyPath}
              <code class="path mono">{keyPath}</code>
            {:else}
              <p class="insp-note">No key file is bound to this identity on this device.</p>
            {/if}
            <div class="insp-actions wrap">
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
            <p class="insp-note" aria-live="polite">Reading the key…</p>
          {:else if keyState?.status === 'error'}
            <p class="problem" role="alert">{keyState.message}</p>
          {:else if keyState?.status === 'ready'}
            <section class="insp-group" aria-labelledby="fingerprint-label">
              <div class="ttl-row">
                <p class="ttl" id="fingerprint-label">Fingerprint</p>
                <button
                  type="button"
                  class="copy"
                  aria-label="Copy the fingerprint"
                  onclick={() => void copy('fingerprint', keyState.info.fingerprint)}
                >
                  <Icon name={copied === 'fingerprint' ? 'check' : 'copy'} size={14} />
                  {copied === 'fingerprint' ? 'Copied' : 'Copy'}
                </button>
              </div>
              <code class="value mono">{keyState.info.fingerprint}</code>
              <p class="insp-note">
                {keyState.info.algorithm}{keyState.info.encrypted
                  ? ' · protected by a passphrase'
                  : ' · no passphrase'}
              </p>
            </section>

            <section class="insp-group" aria-labelledby="public-label">
              <div class="ttl-row">
                <p class="ttl" id="public-label">Public key</p>
                <button
                  type="button"
                  class="copy"
                  aria-label="Copy the public key"
                  onclick={() => void copy('public', keyState.info.public_key)}
                >
                  <Icon name={copied === 'public' ? 'check' : 'copy'} size={14} />
                  {copied === 'public' ? 'Copied' : 'Copy'}
                </button>
              </div>
              <code class="value public mono">{keyState.info.public_key}</code>
              <p class="insp-note">
                Add this line to <code class="mono">~/.ssh/authorized_keys</code> on the server. It is
                safe to share.
              </p>
            </section>
          {/if}

          <section class="insp-group" aria-labelledby="users-label">
            <p class="ttl" id="users-label">
              <span>Hosts using this identity</span><span class="mono">{users.length}</span>
            </p>
            {#if users.length === 0}
              <p class="insp-note">No host signs in with this identity yet.</p>
            {:else}
              <ul class="users">
                {#each users.slice(0, usersShown) as host (host.id)}
                  <li>
                    <span class="who">
                      <span class="user-name">{host.name}</span>
                      <span class="target">{host.username}@{host.address}:{host.port}</span>
                    </span>
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
          <div class="titles">
            <h2 class="insp-title">New identity</h2>
            <p class="insp-sub">Generate a key, or use one you have</p>
          </div>
          <p class="insp-note">
            Generate a fresh ed25519 key, or point an identity at a key file you already have. The
            private key never leaves this device and is never synced.
          </p>
        {:else}
          <div class="titles">
            <h2 class="insp-title">No identity selected</h2>
            <p class="insp-sub">Identity</p>
          </div>
          <p class="insp-note">Choose an identity in the list, or make a new one.</p>
        {/if}

        {#if mode === 'create' || identity}
          <form onsubmit={(event) => void submit(event)} novalidate>
            {#if mode === 'edit'}<p class="ttl">Details</p>{/if}
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
              <p class="insp-note">
                A new ed25519 key is written to Termru’s key folder, readable by you only. An
                existing file is never overwritten.
              </p>
            {:else if mode === 'create'}
              <p class="insp-note">
                You pick the private key file next. Only its path is stored; the file is not copied
                or read into the database.
              </p>
            {/if}

            {#if formError}<p class="insp-note error form-error" role="alert">{formError}</p>{/if}

            <div class="insp-actions form-actions">
              <button type="button" class="quiet" onclick={cancelForm}>Cancel</button>
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
        {/if}

        {#if identity && mode === 'edit'}
          <section class="insp-group" aria-labelledby="identity-delete-label">
            <p class="ttl" id="identity-delete-label">Delete</p>
            <p class="insp-note">{identities.deleteBody(identity)}</p>
            <button type="button" class="danger quiet delete" onclick={askDelete}>
              <Icon name="trash" size={15} />
              Delete identity
            </button>
          </section>
        {/if}
      </InspectorPanel>
    {/snippet}
  </SplitBody>

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
  .new {
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

  /* The identities: quiet rows, as on the Hosts page. */
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
    padding: var(--termru-space-4) var(--termru-space-16) var(--termru-space-16);
  }

  .table {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .head,
  .item {
    display: grid;
    grid-template-columns: 24px minmax(0, 1.6fr) minmax(0, 1fr) 72px;
    align-items: center;
    column-gap: var(--termru-space-12);
    padding: 0 var(--termru-space-12);
  }

  .head {
    height: 30px;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .col-hosts {
    text-align: right;
  }

  .table ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    width: 100%;
    height: var(--termru-row-h);
    min-height: 0;
    border-radius: var(--termru-radius-row);
    background: transparent;
    color: var(--termru-text);
    font-weight: 400;
    text-align: left;
    /* Only the press moves. The selection changes at once: a row is often
       chosen from the keyboard, and that is never animated. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  /* A row of a list gives under the finger no more than a row does. */
  .item:active:not(:disabled) {
    transform: none;
  }

  .item.active {
    background: var(--termru-raised);
  }

  @media (hover: hover) and (pointer: fine) {
    .item:not(.active) {
      transition: background-color var(--motion-fast) ease;
    }

    .item:not(.active):hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-raised) 60%, transparent);
    }
  }

  .key {
    display: inline-grid;
    place-items: center;
    color: var(--termru-text-3);
  }

  .item.active .key {
    color: var(--termru-accent-text);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .item-name {
    font-size: 13.5px;
    line-height: 20px;
    font-weight: 500;
    letter-spacing: -0.005em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-sub {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    line-height: 16px;
    letter-spacing: -0.01em;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .algo {
    min-width: 0;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .algo.warn {
    font-family: var(--termru-font-ui);
    color: var(--termru-warn-text);
  }

  .algo.none {
    color: var(--termru-text-3);
  }

  .count {
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-2);
    font-variant-numeric: tabular-nums;
  }

  .empty {
    margin: 0;
    padding: var(--termru-space-16) var(--termru-space-12);
    color: var(--termru-text-2);
  }

  .note {
    margin: 0 var(--termru-space-12);
    max-width: 60ch;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  .problem {
    margin: 0 var(--termru-space-12);
    color: var(--termru-err-text);
    overflow-wrap: anywhere;
  }

  /* The inspector. */
  .titles :global(.insp-title) {
    margin: 0;
  }

  .mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
  }

  .path {
    line-height: 20px;
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  .wrap {
    flex-wrap: wrap;
  }

  /* A group's label with its action at the right. */
  .ttl-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-8);
    min-height: 24px;
  }

  .ttl-row .ttl {
    margin: 0;
  }

  .value {
    display: block;
    align-self: stretch;
    padding: var(--termru-space-8) var(--termru-space-12);
    border-radius: var(--termru-radius-input);
    background: var(--termru-base);
    color: var(--termru-text);
    line-height: 18px;
    overflow-wrap: anywhere;
  }

  .value.public {
    max-height: 112px;
    overflow: auto;
    user-select: all;
  }

  .copy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    padding: 0 var(--termru-space-8);
    margin-right: calc(var(--termru-space-8) * -1);
    background: transparent;
    color: var(--termru-text-2);
    font-size: var(--termru-text-secondary-size);
  }

  @media (hover: hover) and (pointer: fine) {
    .copy:hover:not(:disabled) {
      background: var(--termru-raised-2);
      color: var(--termru-text);
    }
  }

  .users {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .users li {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    min-height: 44px;
  }

  .users .who {
    flex: 1;
  }

  .user-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: 16px;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    align-self: flex-start;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--termru-space-4);
  }

  .choice {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    min-height: 30px;
    padding: 0 var(--termru-space-12);
    border-radius: var(--termru-radius-sm);
    color: var(--termru-text-2);
    cursor: pointer;
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .choice.selected {
    background: var(--termru-raised-2);
    color: var(--termru-text);
  }

  .choice:active {
    transform: scale(var(--press-scale, 0.98));
  }

  .choice:has(input:focus-visible) {
    outline: var(--termru-focus-w) solid var(--termru-accent-text);
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

  .muted {
    color: var(--termru-text-2);
  }

  .form-actions {
    justify-content: flex-end;
  }

  .form-actions button {
    padding: 0 var(--termru-space-16);
  }

  .delete {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    margin-left: calc(var(--termru-space-12) * -1);
  }
</style>
