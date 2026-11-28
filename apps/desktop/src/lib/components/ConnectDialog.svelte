<script lang="ts">
  import { tick } from 'svelte';
  import type { ConnectRequest } from '../stores/connect.svelte';
  import type { HopAnswers } from '../stores/sessions.svelte';
  import type { SessionSecrets } from '../types';
  import Field from './Field.svelte';
  import Icon from './Icon.svelte';
  import SessionDialog from './SessionDialog.svelte';

  /**
   * Asks for the password of a host before a connection starts. It is only
   * opened for a password host with nothing stored: a key's passphrase and
   * the questions of a keyboard-interactive server are asked by the backend
   * when they come up (`AuthPromptDialog`), and an agent needs no question.
   *
   * When the route to the host passes jump hosts that sign in with a password
   * nobody has given (nothing in the credential store), the dialog has one
   * field for each, labelled with the hop's name, and a box per hop to keep
   * its password in the credential store. Those travel as `hop_secrets`.
   *
   * It stays mounted so it can play its exit; `request` is null while it is
   * closed.
   */
  let {
    request,
    oncancel,
    onsubmit,
  }: {
    request: ConnectRequest | null;
    oncancel: () => void;
    onsubmit: (secrets: SessionSecrets, remember: boolean, hops?: HopAnswers) => void;
  } = $props();

  let password = $state('');
  let remember = $state(false);
  /** What was typed for each jump host, and whether to keep it, by host id. */
  let hopPasswords = $state<Record<string, string>>({});
  let hopKeep = $state<Record<string, boolean>>({});
  /** Hops that were submitted without the password they need. */
  let hopMissing = $state<Record<string, boolean>>({});

  // What the dialog shows while it fades out, after the request is gone.
  let last = $state.raw<ConnectRequest | null>(null);
  $effect(() => {
    if (request) last = request;
  });
  const shown = $derived(request ?? last);

  // Every request starts with an empty field: a password never carries over
  // from the host before, and never stays in the page once the dialog closes.
  $effect(() => {
    if (request) {
      password = '';
      remember = false;
      hopPasswords = {};
      hopKeep = {};
      hopMissing = {};
    }
  });

  const uid = $props.id();

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (!request) return;
    // A jump host whose password is nowhere cannot be connected through: say
    // which one, instead of failing a connection later.
    const missing = Object.fromEntries(
      request.hops
        .filter((hop) => !hop.stored && !(hopPasswords[hop.host.id] ?? ''))
        .map((hop) => [hop.host.id, true]),
    );
    if (Object.keys(missing).length > 0) {
      hopMissing = missing;
      const first = request.hops.find((hop) => missing[hop.host.id]);
      void tick().then(() => document.getElementById(`${uid}-hop-${first?.host.id}`)?.focus());
      return;
    }
    // An empty field sends no secret at all: the backend then falls back to
    // the credential store (an empty string would be used as the password).
    const secrets: SessionSecrets = password.length > 0 ? { password } : {};
    const keep = remember;
    const hops: HopAnswers = { secrets: {}, remember: [] };
    for (const hop of request.hops) {
      const typed = hopPasswords[hop.host.id] ?? '';
      if (typed.length === 0) continue;
      hops.secrets[hop.host.id] = { password: typed };
      if (hopKeep[hop.host.id]) hops.remember.push(hop.host.id);
    }
    password = '';
    hopPasswords = {};
    onsubmit(secrets, keep, hops);
  }

  function cancel(): void {
    password = '';
    hopPasswords = {};
    oncancel();
  }

  function target(host: { username: string; address: string; port: number }): string {
    return `${host.username}@${host.address}${host.port === 22 ? '' : `:${host.port}`}`;
  }
</script>

<SessionDialog
  open={request !== null}
  role="dialog"
  title={shown ? `Connect to ${shown.host.name}` : 'Connect'}
  description={shown
    ? `${shown.host.username}@${shown.host.address}:${shown.host.port}`
    : undefined}
  monoDescription
  onclose={cancel}
  onsubmit={submit}
>
  {#each shown?.hops ?? [] as hop, index (hop.host.id)}
    <Field
      label="Password for {hop.host.name}"
      id="{uid}-hop-{hop.host.id}"
      error={hopMissing[hop.host.id] ? `Enter the password for ${hop.host.name}.` : null}
      hint={hop.stored
        ? `A password for ${hop.host.name} is saved on this device. Leave this empty to use it.`
        : `Jump host ${target(hop.host)}. Used for this connection only.`}
    >
      {#snippet control(attrs)}
        <input
          {...attrs}
          type="password"
          bind:value={hopPasswords[hop.host.id]}
          oninput={() => {
            if (hopMissing[hop.host.id]) hopMissing = { ...hopMissing, [hop.host.id]: false };
          }}
          autocomplete="off"
          data-autofocus={index === 0 && !shown?.askTarget ? true : undefined}
        />
      {/snippet}
    </Field>
    <label class="remember">
      <input type="checkbox" bind:checked={hopKeep[hop.host.id]} />
      <span>
        Save {hop.host.name}’s password in this device’s credential store
        <span class="note">Otherwise Termru asks every time.</span>
      </span>
    </label>
  {/each}

  {#if shown?.askTarget ?? true}
    <Field
      label={shown && shown.hops.length > 0 ? `Password for ${shown.host.name}` : 'Password'}
      id="{uid}-password"
      hint={shown?.secretStored
        ? 'A password is saved on this device. Leave this empty to use it.'
        : 'Used for this connection only.'}
    >
      {#snippet control(attrs)}
        <input
          {...attrs}
          type="password"
          bind:value={password}
          autocomplete="current-password"
          data-autofocus={shown?.hops.length ? undefined : true}
        />
      {/snippet}
    </Field>

    <label class="remember">
      <input type="checkbox" bind:checked={remember} />
      <span>
        Save in this device’s credential store
        <span class="note">Otherwise Termru asks every time.</span>
      </span>
    </label>
  {/if}

  {#snippet actions()}
    <button type="button" class="quiet" onclick={cancel}>Cancel</button>
    <button type="submit" class="primary connect">
      <Icon name="arrow-up-right" size={16} />
      Connect
    </button>
  {/snippet}
</SessionDialog>

<style>
  .remember {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-2);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .remember input {
    margin: 2px 0 0;
  }

  .note {
    display: block;
    color: var(--termru-muted);
  }

  .connect {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
  }
</style>
