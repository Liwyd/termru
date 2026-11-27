<script lang="ts">
  import type { ConnectRequest } from '../stores/connect.svelte';
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
    onsubmit: (secrets: SessionSecrets, remember: boolean) => void;
  } = $props();

  let password = $state('');
  let remember = $state(false);

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
    }
  });

  const uid = $props.id();

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (!request) return;
    // An empty field sends no secret at all: the backend then falls back to
    // the credential store (an empty string would be used as the password).
    const secrets: SessionSecrets = password.length > 0 ? { password } : {};
    const keep = remember;
    password = '';
    onsubmit(secrets, keep);
  }

  function cancel(): void {
    password = '';
    oncancel();
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
  <Field
    label="Password"
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
        data-autofocus
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
