<script lang="ts">
  import type { Host, SessionSecrets } from '../types';

  let {
    host,
    oncancel,
    onsubmit,
    secretStored = false,
  }: {
    host: Host;
    oncancel: () => void;
    onsubmit: (secrets: SessionSecrets, remember: boolean) => void;
    secretStored?: boolean;
  } = $props();

  let password = $state('');
  let passphrase = $state('');
  let remember = $state(false);
  let busy = $state(false);

  const needsPassword = $derived(host.auth.type === 'password');
  const needsPassphrase = $derived(host.auth.type === 'private_key');

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    busy = true;
    const secrets: SessionSecrets = {};
    if (needsPassword) secrets.password = password;
    if (needsPassphrase) secrets.passphrase = passphrase.length > 0 ? passphrase : undefined;
    onsubmit(secrets, remember);
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(event) => {
    if (event.target === event.currentTarget) oncancel();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Connect to host">
    <form onsubmit={submit}>
      <h2>Connect to {host.name}</h2>
      <p class="target">{host.username}@{host.address}:{host.port}</p>

      {#if needsPassword}
        <label>
          Password
          <input type="password" bind:value={password} autocomplete="current-password" />
        </label>
      {/if}

      {#if needsPassphrase}
        <label>
          Passphrase for the private key
          <input type="password" bind:value={passphrase} autocomplete="off" />
        </label>
      {/if}

      {#if needsPassword || needsPassphrase}
        <label class="checkbox">
          <input type="checkbox" bind:checked={remember} />
          <span>Remember in the system credential store</span>
        </label>
        {#if secretStored}
          <p class="hint">A secret is already stored for this host.</p>
        {/if}
      {/if}

      <p class="hint">
        {#if needsPassword}
          The password is used for this connection only and is never stored on disk.
        {:else if needsPassphrase}
          The passphrase is used for this connection only and is never stored on disk.
        {:else}
          This host uses your running SSH agent.
        {/if}
      </p>

      <div class="actions">
        <button type="button" onclick={oncancel}>Cancel</button>
        <button type="submit" class="primary" disabled={busy}>Connect</button>
      </div>
    </form>
  </div>
</div>

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
    width: min(26rem, 92vw);
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
    margin: 0;
    font-size: 1rem;
  }

  .target {
    margin: 0;
    font-size: 0.8rem;
    color: var(--termru-muted);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
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

  .checkbox {
    flex-direction: row;
    align-items: center;
    gap: 0.4rem;
    color: var(--termru-fg);
    font-size: 0.8rem;
  }

  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--termru-muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
