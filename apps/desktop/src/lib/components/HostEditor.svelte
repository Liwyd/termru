<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import EditorSection from './EditorSection.svelte';
  import Field from './Field.svelte';
  import HostAdvancedFields from './HostAdvancedFields.svelte';
  import HostForwardsSection from './HostForwardsSection.svelte';
  import HostJumpSection from './HostJumpSection.svelte';
  import Icon from './Icon.svelte';
  import PageHeader from './PageHeader.svelte';
  import SessionDialog from './SessionDialog.svelte';
  import { hasExecFields, sshForwardFlag } from '../forwards';
  import { HostForm } from '../host-form.svelte';
  import { api } from '../ipc';
  import { currentWindow } from '../ipc-backend';
  import { execApproval } from '../stores/exec-approval.svelte';
  import { connectFlow } from '../stores/connect.svelte';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { identities } from '../stores/identities.svelte';
  import { route } from '../stores/route.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { notify } from '../notify.svelte';
  import { unsaved } from '../stores/unsaved.svelte';
  import {
    describeAddress,
    keepaliveLabel,
    parseKeepalive,
    parsePort,
    type HostField,
  } from '../validation';
  import type { Host } from '../types';

  const AUTH_CHOICES = [
    { value: 'password', label: 'Password' },
    { value: 'private_key', label: 'Identity' },
    { value: 'agent', label: 'SSH agent' },
  ] as const;

  /**
   * The form of this editor. The page mounts the editor once per
   * `route.edit()`, so a new host (or another one) is a fresh form. A form
   * that was parked while the identities page was open comes back as it was
   * left.
   */
  const form = new HostForm(route.editHost, {
    draft: route.draft,
    // The route through the jump hosts is resolved against the library.
    lookup: (id) => {
      const found = hosts.hosts.find((entry) => entry.id === id && !entry.deleted);
      return found ? { name: found.name, jump_chain: found.jump_chain ?? [] } : undefined;
    },
  });

  onMount(() => {
    // The parked draft was handed over; it is not kept a second time.
    route.draft = null;
    if (form.isNew) document.getElementById('host-name')?.focus();
  });

  let saving = $state(false);
  /** A refusal that belongs to no field (a failing disk), shown in the footer. */
  let formError = $state<string | null>(null);
  let revealPassword = $state(false);
  let newTag = $state('');
  let tagError = $state<string | null>(null);

  /**
   * What the credential store holds for the saved host: null while unknown.
   * A password belongs to the host; a key passphrase to its identity.
   */
  let credential = $state<{ password: boolean | null; passphrase: boolean | null }>({
    password: null,
    passphrase: null,
  });
  let forgetting = $state(false);

  // Ask the credential store once. The answer is the baseline of the "save in
  // the credential store" box, not an edit.
  onMount(() => {
    let cancelled = false;
    const saved = route.editHost;
    if (form.isNew || !saved) {
      credential = { password: false, passphrase: false };
      return;
    }
    const ask = (kind: 'password' | 'passphrase'): Promise<boolean> =>
      api.secretHas(saved.id, kind).then(
        (stored) => stored === true,
        () => false,
      );
    if (saved.auth.type === 'password') {
      void ask('password').then((stored) => {
        if (cancelled) return;
        credential = { ...credential, password: stored };
        hosts.markPasswordStored(saved.id, stored);
        form.adoptStored(stored);
      });
    } else {
      credential = { ...credential, password: false };
    }
    if (saved.auth.type === 'private_key') {
      void ask('passphrase').then((stored) => {
        if (!cancelled) credential = { ...credential, passphrase: stored };
      });
    } else {
      credential = { ...credential, passphrase: false };
    }
    return () => {
      cancelled = true;
    };
  });

  // A host that came through sync may carry a startup command, agent
  // forwarding, an environment or an exposed forward this device has not
  // approved. The editor shows them, and saving is the user's yes: say so.
  let execPending = $state(false);
  /** The hash of the fields the review showed: what a Save without changes approves. */
  let execHash = $state<string | null>(null);
  onMount(() => {
    const saved = route.editHost;
    if (form.isNew || !saved || !hasExecFields(saved)) return;
    let cancelled = false;
    void api.hostExecReview(saved.id).then(
      (review) => {
        if (cancelled) return;
        execPending = review?.pending === true;
        execHash = review?.hash ?? null;
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  });

  // The key an identity is bound to: its file and fingerprint are shown under
  // the picker and in the rail.
  $effect(() => {
    void identities.loadBindings().catch(() => undefined);
  });
  const identity = $derived(hosts.identities.find((entry) => entry.id === form.identityId) ?? null);
  $effect(() => {
    const picked = identity;
    if (form.authType === 'private_key' && picked) untrack(() => void identities.ensureKey(picked));
  });
  const keyPath = $derived(identity ? (identities.bindings[identity.id] ?? null) : null);
  const keyState = $derived(identity ? identities.keys[identity.id] : undefined);

  // An unsaved form is held against everything that would drop it: a page
  // change (see `route`) and the window closing.
  $effect(() => unsaved.hold(() => form.dirty));

  $effect(() => {
    let cancelled = false;
    let remove: (() => void) | undefined;
    let allow = false;
    void currentWindow()
      .then(async (win) => {
        const off = await win.onCloseRequested((event) => {
          if (allow || !form.dirty) return;
          event.preventDefault();
          unsaved.ask(() => {
            // The window asks again once it is told to close; this time it may.
            allow = true;
            void win.close();
          }, 'close');
        });
        if (cancelled) off();
        else remove = off;
      })
      // Outside the app (a browser tab, a test) there is no window to hold back.
      .catch(() => undefined);
    return () => {
      cancelled = true;
      remove?.();
    };
  });

  // The text of the question outlives the question for the length of the
  // dialog's exit, so it does not change while it fades.
  let lastKind = $state<'leave' | 'close'>('leave');
  $effect(() => {
    if (unsaved.question) lastKind = unsaved.question.kind;
  });

  const heading = $derived(form.isNew ? 'New connection' : 'Edit connection');
  function folderLabel(groupId: string): string {
    return hosts.folderOptions.find((option) => option.id === groupId)?.label ?? '';
  }

  const addressNote = $derived(
    form.errorFor('address') === null && form.touched.address
      ? describeAddress(form.address)
      : null,
  );

  const keepaliveNote = $derived.by(() => {
    const seconds = parseKeepalive(form.keepaliveText);
    if (seconds === null) return null;
    return seconds === 0
      ? `Empty or 0 is ${keepaliveLabel(0)}: a probe goes out when the connection is quiet.`
      : `A probe goes out every ${seconds} s when the connection is quiet.`;
  });

  // The rail: what the host will look like on the command line, and what it
  // signs in with.
  const preview = $derived.by(() => {
    const user = form.username.trim() || 'user';
    const address = form.address.trim() || 'host';
    const port = parsePort(form.portText);
    const portPart = port !== null && port !== 22 ? ` -p ${port}` : '';
    return `ssh${flags}${' '}${user}@${address}${portPart}`;
  });
  // What the new sections add to the command: `-J`, `-L`/`-R`/`-D`, `-A`.
  const flags = $derived.by(() => {
    let text = '';
    const route = form.route;
    if (route?.ok && route.route.length > 0) {
      const hops = route.route.flatMap((id) => {
        const hop = hosts.hosts.find((entry) => entry.id === id);
        return hop
          ? [`${hop.username}@${hop.address}${hop.port === 22 ? '' : `:${hop.port}`}`]
          : [];
      });
      if (hops.length > 0) text += ` -J ${hops.join(',')}`;
    }
    for (const row of form.forwardRows) {
      if (!row.enabled) continue;
      const flag = sshForwardFlag(row);
      if (flag) text += ` ${flag}`;
    }
    if (form.agentForward) text += ' -A';
    return text;
  });
  const authSummary = $derived.by(() => {
    if (form.authType === 'agent') return 'SSH agent authentication';
    if (form.authType === 'private_key') {
      return identity ? `Identity · ${identity.name}` : 'Identity · none chosen yet';
    }
    return 'Password authentication';
  });
  const whereSummary = $derived(
    `${folderLabel(form.groupId) || 'Unfiled'} / ${form.name.trim() || 'untitled'}`,
  );
  const firstIdentity = $derived(hosts.identities[0]?.name ?? null);

  const summary = $derived(
    form.submitted && form.errorCount > 0
      ? `Fix ${form.errorCount} ${form.errorCount === 1 ? 'field' : 'fields'} before saving.`
      : null,
  );

  /** Moves the focus to the field of an error. */
  async function focusField(field: HostField): Promise<void> {
    await tick();
    const ids: Record<HostField, string> = {
      name: 'host-name',
      address: 'host-address',
      port: 'host-port',
      username: 'host-username',
      identity: 'host-identity',
      term: 'host-term',
      keepalive: 'host-keepalive',
      description: 'host-description',
      jump: 'host-jump-add',
      forwards: 'host-forwards-add',
      env: 'host-env-add',
      startup: 'host-startup',
      authOrder: 'host-auth-add',
    };
    // A list of rows: the first row with an error, else the section's own control.
    const rows: Partial<Record<HostField, string>> = {
      forwards: '[aria-labelledby="sec-forwards"] [aria-invalid="true"]',
      env: '[aria-labelledby="sec-advanced"] [id^="host-env-"][aria-invalid="true"]',
    };
    const selector = rows[field];
    const broken = selector ? document.querySelector<HTMLElement>(selector) : null;
    (broken ?? document.getElementById(ids[field]))?.focus();
  }

  async function save(connect: boolean): Promise<void> {
    if (saving) return;
    formError = null;
    form.submit();
    if (form.errorCount > 0) {
      if (form.firstInvalid) await focusField(form.firstInvalid);
      return;
    }
    // Nothing changed on a host that exists: there is nothing to write. Its
    // synced settings that are not approved here are the exception: Save is
    // the yes to what the page shows, so it approves them (the backend checks
    // that they are still the ones the review listed).
    if (!form.dirty && !form.isNew) {
      const existing = route.editHost;
      if (existing && execPending && execHash) {
        try {
          await api.hostExecApprove(existing.id, execHash);
          execPending = false;
          notify({
            key: 'host.exec-approved',
            entityId: existing.id,
            values: { name: existing.name },
          });
        } catch (err) {
          formError = errorMessage(err);
          return;
        }
      }
      if (connect && existing) void connectFlow.request(existing);
      else route.back();
      return;
    }
    saving = true;
    let saved: Host | null = null;
    try {
      saved = await hosts.save(form.toHost(), { approveExec: true });
      if (!saved) {
        const message = hosts.saveError ?? 'Could not save the host.';
        if (form.refuse(message)) {
          const field = form.firstInvalid ?? (Object.keys(form.serverErrors)[0] as HostField);
          if (field) await focusField(field);
        } else {
          formError = message;
        }
        return;
      }
      const typed = form.password;
      const note = await applyCredential(saved);
      form.markSaved(saved);
      if (note) {
        notify({
          key: 'host.credential-not-stored',
          entityId: saved.id,
          values: { name: saved.name, detail: note },
        });
      } else {
        notify({ key: 'host.saved', entityId: saved.id, values: { name: saved.name } });
      }
      if (connect) {
        if (typed.length > 0 && !form.savePassword && saved.auth.type === 'password') {
          // A password that is not kept is used once, from memory.
          sessions.open(saved, { password: typed }, false);
          route.go('sessions');
        } else {
          void connectFlow.request(saved);
        }
      } else {
        route.back();
      }
    } finally {
      saving = false;
    }
  }

  /**
   * Puts the typed password in the credential store (or takes the stored one
   * out when the box was unticked). A store that is not there does not undo
   * the save: the host is saved and the note says what did not happen.
   */
  async function applyCredential(saved: Host): Promise<string | null> {
    if (saved.auth.type !== 'password') return null;
    try {
      if (form.savePassword && form.password.length > 0) {
        await api.secretSet(saved.id, 'password', form.password);
        hosts.markPasswordStored(saved.id, true);
        credential = { ...credential, password: true };
      } else if (!form.savePassword && credential.password === true) {
        await api.secretDelete(saved.id, 'password');
        hosts.markPasswordStored(saved.id, false);
        credential = { ...credential, password: false };
      }
    } catch (err) {
      // Only the reason: the notice owns the sentence around it.
      return errorMessage(err);
    }
    return null;
  }

  /** "Forget": removes the secret this device keeps for the host. */
  async function forget(kind: 'password' | 'passphrase'): Promise<void> {
    const saved = route.editHost;
    if (!saved || forgetting) return;
    forgetting = true;
    try {
      await api.secretDelete(saved.id, kind);
      if (kind === 'password') {
        credential = { ...credential, password: false };
        hosts.markPasswordStored(saved.id, false);
        form.adoptStored(false);
        notify({ key: 'secret.password-forgotten' });
      } else {
        credential = { ...credential, passphrase: false };
        notify({ key: 'secret.passphrase-forgotten' });
      }
    } catch (err) {
      notify({ key: 'secret.forget-failed', values: { detail: errorMessage(err) } });
    } finally {
      forgetting = false;
    }
  }

  async function addTag(): Promise<void> {
    const name = newTag.trim();
    tagError = null;
    if (name.length === 0) return;
    const existing = hosts.tags.find((tag) => tag.name.toLowerCase() === name.toLowerCase());
    if (existing) {
      if (!form.tags.includes(existing.id)) form.toggleTag(existing.id);
      newTag = '';
      return;
    }
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
      hosts.tags = [...hosts.tags, created];
      if (!form.tags.includes(created.id)) form.toggleTag(created.id);
      newTag = '';
    } catch (err) {
      tagError = errorMessage(err);
    }
  }

  function manageIdentities(): void {
    route.leaveEditor(form.toDraft(), 'identities');
  }
</script>

<div class="workpage editor">
  <PageHeader
    title={heading}
    count={form.isNew ? null : form.name.trim() || 'Untitled'}
    back={{ label: 'Back to hosts', onclick: () => route.back() }}
  >
    {#snippet actions()}
      <div class="state" aria-live="polite">
        {#if form.dirty}
          <span class="pill"><span class="glyph" aria-hidden="true">■</span> Unsaved changes</span>
        {/if}
      </div>
    {/snippet}
  </PageHeader>

  <div class="scroll">
    <form
      id="host-form"
      class="layout"
      novalidate
      onsubmit={(event) => {
        event.preventDefault();
        void save(false);
      }}
    >
      <div class="sections">
        <EditorSection
          step="01"
          label="CONNECTION"
          id="sec-connection"
          title="Where to connect"
          blurb="Use an IP address or hostname. SSH uses port 22 by default."
        >
          <div class="row two">
            <Field label="Name" id="host-name" error={form.errorFor('name')}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={form.name}
                  oninput={() => form.edited('name')}
                  onblur={() => form.touch('name')}
                />
              {/snippet}
            </Field>
            <Field label="Username" id="host-username" error={form.errorFor('username')}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  bind:value={form.username}
                  oninput={() => form.edited('username')}
                  onblur={() => form.touch('username')}
                />
              {/snippet}
            </Field>
          </div>
          <div class="row address">
            <Field
              label="Address"
              id="host-address"
              error={form.errorFor('address')}
              hint={addressNote ? `✓ ${addressNote}` : null}
            >
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  placeholder="example.com"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  bind:value={form.address}
                  oninput={() => form.edited('address')}
                  onblur={() => form.touch('address')}
                />
              {/snippet}
            </Field>
            <Field label="Port" id="host-port" error={form.errorFor('port')}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  inputmode="numeric"
                  placeholder="22"
                  autocomplete="off"
                  bind:value={form.portText}
                  oninput={() => form.edited('port')}
                  onblur={() => {
                    if (form.portText.trim() === '') form.portText = '22';
                    form.touch('port');
                  }}
                />
              {/snippet}
            </Field>
          </div>
        </EditorSection>

        <EditorSection
          step="02"
          label="AUTHENTICATION"
          id="sec-auth"
          title="How to sign in"
          blurb="Choose a password, a local identity or your SSH agent. Nothing secret is synced."
        >
          <div class="choices" role="radiogroup" aria-label="Authentication method">
            {#each AUTH_CHOICES as choice (choice.value)}
              <label class="choice" class:selected={form.authType === choice.value}>
                <input type="radio" name="auth" value={choice.value} bind:group={form.authType} />
                <span class="mark" aria-hidden="true"
                  >{form.authType === choice.value ? '●' : '○'}</span
                >
                {choice.label}
              </label>
            {/each}
          </div>

          {#if form.authType === 'password'}
            <Field label="Password" id="host-password">
              {#snippet control(attrs)}
                <div class="secret">
                  <input
                    {...attrs}
                    type={revealPassword ? 'text' : 'password'}
                    autocomplete="new-password"
                    placeholder={credential.password === true
                      ? 'Saved on this device · type to replace'
                      : 'Asked on connect unless you type one'}
                    bind:value={form.password}
                  />
                  <button
                    type="button"
                    class="reveal quiet"
                    aria-label={revealPassword ? 'Hide password' : 'Show password'}
                    onclick={() => (revealPassword = !revealPassword)}
                  >
                    {revealPassword ? 'Hide' : 'Show'}
                  </button>
                </div>
              {/snippet}
            </Field>
            <label class="check">
              <input type="checkbox" bind:checked={form.savePassword} />
              <span
                >Save in this device’s credential store
                <span class="muted">· Otherwise ask on connect</span></span
              >
            </label>
          {:else if form.authType === 'private_key'}
            <Field label="Identity" id="host-identity" error={form.errorFor('identity')}>
              {#snippet control(attrs)}
                <select
                  {...attrs}
                  bind:value={form.identityId}
                  onchange={() => form.edited('identity')}
                  onblur={() => form.touch('identity')}
                >
                  <option value="">Select an identity…</option>
                  {#each hosts.identities as entry (entry.id)}
                    <option value={entry.id}>{entry.name}</option>
                  {/each}
                </select>
              {/snippet}
            </Field>
            {#if identity}
              <p class="keyline">
                {#if keyPath}
                  <span class="muted">Key file</span> <code>{keyPath}</code>
                {:else}
                  <span class="warn">No key file is bound to this identity on this device.</span>
                {/if}
                {#if keyState?.status === 'ready'}
                  <br /><span class="muted">Fingerprint</span>
                  <code>{keyState.info.fingerprint}</code>
                {/if}
              </p>
            {:else if hosts.identities.length === 0}
              <p class="blurb">No identities yet. Create one, or generate a key, in Identities.</p>
            {/if}
            <button type="button" class="link" onclick={manageIdentities}>Manage identities…</button
            >
          {:else}
            <p class="blurb">
              Termru asks your running SSH agent to sign. No key and no password is stored for this
              host.
            </p>
          {/if}
          <p class="blurb">
            Servers that ask for a one-time code or a second factor (keyboard-interactive) are
            handled automatically: Termru asks when the server does.
          </p>
        </EditorSection>

        <EditorSection
          step="03"
          label="ORGANIZATION"
          id="sec-org"
          title="Make it easy to find"
          blurb="A host belongs to one folder. Favorites and tags are independent."
        >
          <div class="row folder">
            <Field label="Folder" id="host-folder">
              {#snippet control(attrs)}
                <select {...attrs} bind:value={form.groupId}>
                  <option value="">No folder</option>
                  {#each hosts.folderOptions as option (option.id)}
                    <option value={option.id}>{option.label}</option>
                  {/each}
                </select>
              {/snippet}
            </Field>
            <button
              type="button"
              class="favorite"
              aria-pressed={form.favorite}
              onclick={() => (form.favorite = !form.favorite)}
            >
              <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true" focusable="false">
                <path
                  d="M10 2.2l2.36 4.97 5.44.7-4 3.76 1.02 5.4L10 14.38 5.18 17.03l1.02-5.4-4-3.76 5.44-.7z"
                  fill={form.favorite ? 'currentColor' : 'none'}
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linejoin="round"
                />
              </svg>
              {form.favorite ? 'Favorited' : 'Favorite'}
            </button>
          </div>

          <fieldset class="tags">
            <legend>Tags</legend>
            {#if hosts.tags.length > 0}
              <div class="chips">
                {#each hosts.tags as tag (tag.id)}
                  <button
                    type="button"
                    class="chip"
                    class:on={form.tags.includes(tag.id)}
                    aria-pressed={form.tags.includes(tag.id)}
                    onclick={() => form.toggleTag(tag.id)}
                  >
                    {#if form.tags.includes(tag.id)}<Icon name="check" size={12} />{/if}
                    {tag.name}
                  </button>
                {/each}
              </div>
            {/if}
            <div class="new-tag">
              <input
                type="text"
                aria-label="New tag"
                aria-invalid={tagError ? 'true' : undefined}
                aria-describedby={tagError ? 'host-tag-error' : undefined}
                placeholder="New tag"
                maxlength="64"
                bind:value={newTag}
                onkeydown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    void addTag();
                  }
                }}
              />
              <button type="button" onclick={() => void addTag()}>Add tag</button>
            </div>
            {#if tagError}<p class="field-error" id="host-tag-error" role="alert">
                {tagError}
              </p>{/if}
          </fieldset>

          <Field
            label="Comment"
            id="host-description"
            optional
            error={form.errorFor('description')}
          >
            {#snippet control(attrs)}
              <input
                {...attrs}
                type="text"
                autocomplete="off"
                bind:value={form.description}
                oninput={() => form.edited('description')}
                onblur={() => form.touch('description')}
              />
            {/snippet}
          </Field>
        </EditorSection>

        <HostJumpSection {form} />

        <HostForwardsSection {form} />

        <EditorSection
          step="06"
          label="ADVANCED"
          id="sec-advanced"
          title="Terminal, session and sign-in"
          blurb="The defaults suit most servers. Change them only for a reason."
        >
          <div class="row two">
            <Field label="Terminal type" id="host-term" error={form.errorFor('term')}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  bind:value={form.term}
                  oninput={() => form.edited('term')}
                  onblur={() => form.touch('term')}
                />
              {/snippet}
            </Field>
            <Field
              label="Keepalive (seconds)"
              id="host-keepalive"
              error={form.errorFor('keepalive')}
              hint={keepaliveNote}
            >
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  inputmode="numeric"
                  placeholder={keepaliveLabel(0)}
                  autocomplete="off"
                  bind:value={form.keepaliveText}
                  oninput={() => form.edited('keepalive')}
                  onblur={() => form.touch('keepalive')}
                />
              {/snippet}
            </Field>
          </div>
          <HostAdvancedFields {form} />
        </EditorSection>
      </div>

      <aside class="rail" aria-label="Summary of this host">
        <section class="credential" aria-labelledby="rail-credential">
          <p class="wp-label accent" id="rail-credential">Local credential</p>
          {#if form.authType === 'password'}
            {#if credential.password === true}
              <p>
                A password is stored on this device. It is used when you connect and never appears
                in logs, toasts or sync payloads.
              </p>
              <button type="button" disabled={forgetting} onclick={() => void forget('password')}>
                Forget password
              </button>
            {:else}
              <p>
                {credential.password === null
                  ? 'Checking the credential store…'
                  : 'No password is stored for this host on this device. It is asked for when you connect.'}
              </p>
            {/if}
          {:else if form.authType === 'private_key'}
            {#if !identity}
              <p>Choose an identity to see which key file it uses on this device.</p>
            {:else if keyPath}
              <p>
                Key file <code>{keyPath}</code>.
                {#if keyState?.status === 'ready' && keyState.info.encrypted}
                  The key is protected by a passphrase.
                {/if}
              </p>
              {#if credential.passphrase === true}
                <p>The passphrase is stored on this device.</p>
                <button
                  type="button"
                  disabled={forgetting}
                  onclick={() => void forget('passphrase')}
                >
                  Forget passphrase
                </button>
              {/if}
            {:else}
              <p>
                No key file is bound to “{identity.name}” on this device. Bind one in Identities.
              </p>
            {/if}
          {:else}
            <p>Authentication goes through your SSH agent. Termru stores nothing for this host.</p>
          {/if}
        </section>

        {#if execPending}
          <section class="credential approval" aria-labelledby="rail-approval">
            <p class="wp-label accent" id="rail-approval">Not approved on this device</p>
            <p>
              This host has a startup command, agent forwarding, environment variables or a forward
              on another address that came from sync. Connections leave them out until you approve
              them. Saving approves exactly what this page shows.
            </p>
            <button type="button" onclick={() => void execApproval.show(form.hostId)}>
              Review them…
            </button>
          </section>
        {/if}

        <section class="preview" aria-labelledby="rail-preview">
          <p class="wp-label" id="rail-preview">Connection preview</p>
          <code class="command">{preview}</code>
          <p class="muted">{authSummary}</p>
          <p class="muted where">{whereSummary}</p>
        </section>

        <hr />

        {#if form.authType === 'password' && firstIdentity}
          <p class="note">
            Using a key instead? Choose Identity to pick {firstIdentity} or add a local key in Identities.
          </p>
        {/if}
        <p class="note small">
          Save updates this host. Save &amp; connect opens a new session without replacing your
          active sessions.
        </p>
      </aside>
    </form>
  </div>

  <footer class="footer">
    <button type="button" class="quiet" onclick={() => route.back()}>Cancel</button>
    <p class="message" role="alert">{formError ?? summary ?? ''}</p>
    <div class="buttons">
      <button
        type="button"
        disabled={saving}
        title={saving ? 'Saving…' : undefined}
        onclick={() => void save(false)}
      >
        {saving ? 'Saving…' : 'Save host'}
      </button>
      <button
        type="button"
        class="primary"
        disabled={saving}
        title={saving ? 'Saving…' : undefined}
        onclick={() => void save(true)}
      >
        <Icon name="arrow-up-right" size={15} />
        Save &amp; connect
      </button>
    </div>
  </footer>

  <SessionDialog
    open={unsaved.question !== null}
    title="Discard unsaved changes?"
    description={lastKind === 'close'
      ? 'This host has changes that are not saved. Closing the window discards them.'
      : 'This host has changes that are not saved. Leaving this page discards them.'}
    onclose={() => unsaved.keep()}
  >
    {#snippet actions()}
      <button type="button" class="quiet" data-autofocus onclick={() => unsaved.keep()}>
        Keep editing
      </button>
      <button type="button" class="danger-fill" onclick={() => unsaved.discard()}>
        {lastKind === 'close' ? 'Discard and close' : 'Discard changes'}
      </button>
    {/snippet}
  </SessionDialog>
</div>

<style>
  .editor {
    min-height: 0;
  }

  /* The page scrolls; the footer stays where Save is reachable. */
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x) var(--termru-space-32);
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-24);
  }

  .state {
    min-height: 28px;
    display: flex;
    align-items: center;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    padding: 4px var(--termru-space-12);
    border-radius: var(--termru-radius-input);
    background: var(--termru-warn-soft);
    color: var(--termru-warn-text);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 550;
    transition: opacity var(--motion-context) var(--ease-out);

    @starting-style {
      opacity: 0;
    }
  }

  .glyph {
    font-size: 8px;
  }

  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 336px;
    column-gap: var(--termru-space-48);
    align-items: start;
    min-width: 0;
  }

  .sections {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .blurb {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    color: var(--termru-text-2);
  }

  .row {
    display: grid;
    gap: var(--termru-space-16);
    align-items: start;
    min-width: 0;
  }

  .row.two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .row.address {
    grid-template-columns: minmax(0, 1fr) 112px;
  }

  .row.folder {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
  }

  .row.folder .favorite {
    margin-bottom: 0;
  }

  /* The auth method: a radio group that reads as a row of choices. */
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
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-16);
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

  .mark {
    font-size: 11px;
    color: var(--termru-text);
  }

  .secret {
    position: relative;
  }

  .secret input {
    padding-right: 72px;
  }

  .reveal {
    position: absolute;
    top: 50%;
    right: var(--termru-space-4);
    transform: translateY(-50%);
    min-height: 32px;
    padding: 0 var(--termru-space-12);
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
  }

  .secret .reveal:active:not(:disabled) {
    transform: translateY(-50%) scale(var(--press-scale, 0.97));
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text);
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

  .warn {
    color: var(--termru-warn-text);
  }

  .keyline {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: 22px;
    overflow-wrap: anywhere;
  }

  code {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  .favorite {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    min-height: var(--termru-input-h);
    border-color: transparent;
    background: none;
  }

  .favorite[aria-pressed='true'] {
    color: var(--termru-accent-text);
  }

  fieldset.tags {
    margin: 0;
    padding: 0;
    border: none;
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    min-width: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 550;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--termru-space-8);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-4);
    min-height: 30px;
    padding: 0 var(--termru-space-12);
    border-radius: 999px;
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
  }

  .chip.on {
    border-color: var(--termru-accent);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
  }

  .new-tag {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--termru-space-8);
  }

  .field-error {
    margin: 0;
    color: var(--termru-err-text);
    font-size: var(--termru-text-secondary-size);
  }

  /* The right rail: quiet, on the subtle surface, mono labels. */
  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-24);
    min-width: 0;
  }

  .rail p {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  .credential {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--termru-space-12);
    padding: var(--termru-space-16) var(--termru-space-24);
    border-left: 2px solid var(--termru-border);
    background: var(--termru-side);
  }

  /* The card's title is a sans label, not a mono one (frame 4). */
  .credential .accent {
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-secondary-size);
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--termru-accent-text);
  }

  .preview {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
  }

  .command {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  hr {
    width: 100%;
    margin: 0;
    border: none;
    border-top: 1px solid var(--termru-border);
  }

  .rail .note.small {
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
  }

  .footer {
    display: flex;
    align-items: center;
    gap: var(--termru-space-16);
    margin: 0 var(--termru-page-inset-x);
    padding: var(--termru-space-16) 0;
    border-top: 1px solid var(--termru-border);
  }

  .message {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-err-text);
    overflow-wrap: anywhere;
  }

  .buttons {
    display: flex;
    gap: var(--termru-space-12);
  }

  .buttons button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    padding: 0 var(--termru-space-16);
  }

  @media (max-width: 1180px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      row-gap: var(--termru-space-32);
    }
  }

  @media (max-width: 860px) {
    .row.two {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  @media (max-width: 560px) {
    .footer {
      flex-wrap: wrap;
    }

    .message {
      order: -1;
      flex-basis: 100%;
    }
  }
</style>
