<script lang="ts">
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { api } from '../ipc';
  import { toasts } from '../stores/toasts.svelte';
  import type { HostKeyPromptRequest } from '../types';

  interface PromptMessage {
    id: string;
    request: HostKeyPromptRequest;
  }

  let prompt = $state<PromptMessage | null>(null);

  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    void listen<PromptMessage>('host-key-prompt', (event) => {
      prompt = event.payload;
    }).then((fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  async function answer(trust: boolean): Promise<void> {
    if (!prompt) return;
    const current = prompt;
    prompt = null;
    const accepted = await api.hostKeyPromptResolve(current.id, trust);
    if (!accepted) {
      toasts.info('The host-key prompt is no longer active.');
    }
  }
</script>

{#if prompt}
  <div class="backdrop" role="presentation">
    <div class="dialog" role="alertdialog" aria-modal="true" aria-label="Host key confirmation">
      {#if prompt.request.state === 'unknown'}
        <h2>Unknown host key</h2>
        <p class="target">{prompt.request.host}:{prompt.request.port}</p>
        <dl>
          <dt>Type</dt>
          <dd>{prompt.request.key.key_type}</dd>
          <dt>Fingerprint</dt>
          <dd class="mono">{prompt.request.key.fingerprint}</dd>
        </dl>
        <p class="hint">
          The server key is not known yet. Verify the fingerprint with the server administrator
          before trusting it.
        </p>
      {:else}
        <h2 class="danger">Host key changed</h2>
        <p class="target">{prompt.request.host}:{prompt.request.port}</p>
        <dl>
          <dt>Old</dt>
          <dd class="mono">{prompt.request.old.fingerprint}</dd>
          <dt>New</dt>
          <dd class="mono danger">{prompt.request.new.fingerprint}</dd>
        </dl>
        <p class="hint danger">
          The server key differs from the stored one. This can indicate a man-in-the-middle attack
          or a rebuilt server. Only continue if you know why it changed.
        </p>
      {/if}

      <div class="actions">
        <button type="button" onclick={() => answer(false)}>Reject</button>
        <button type="button" class="danger" onclick={() => answer(true)}>Trust</button>
      </div>
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
    z-index: 50;
  }

  .dialog {
    width: min(30rem, 92vw);
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 1.1rem;
    border-radius: var(--termru-radius-lg);
    border: 1px solid var(--termru-danger);
    background: var(--termru-surface);
    color: var(--termru-fg);
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  h2.danger {
    color: var(--termru-danger);
  }

  .target {
    margin: 0;
    font-size: 0.8rem;
    color: var(--termru-muted);
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.2rem 0.6rem;
    margin: 0;
    font-size: 0.82rem;
  }

  dt {
    color: var(--termru-muted);
  }

  dd {
    margin: 0;
  }

  .mono {
    font-family: monospace;
    word-break: break-all;
  }

  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--termru-muted);
  }

  .hint.danger {
    color: var(--termru-danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
