<script lang="ts">
  import { onMount } from 'svelte';
  import { focusTrap } from '../actions/focus-trap';
  import { api } from '../ipc';
  import { listen, type UnlistenFn } from '../ipc-backend';
  import { errorMessage } from '../stores/hosts.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { HostKeyPromptMessage } from '../types';

  /**
   * The prompts waiting for an answer, oldest first. A second prompt that
   * arrives while the first is open waits its turn: it must never replace
   * (and so silently drop) a question the user has not answered.
   */
  let queue = $state<HostKeyPromptMessage[]>([]);
  const prompt = $derived(queue[0] ?? null);

  onMount(() => {
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    /** Subscribes, unless the dialog was torn down before the listener arrived. */
    function subscribe<T>(event: string, handler: (payload: T) => void): void {
      void listen<T>(event, (received) => handler(received.payload)).then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisteners.push(fn);
      });
    }

    subscribe<HostKeyPromptMessage>('host-key-prompt', (incoming) => {
      if (queue.some((entry) => entry.id === incoming.id)) return;
      queue = [...queue, incoming];
    });
    // The question ended without this dialog: it timed out, its connection
    // was closed, or another window answered it.
    subscribe<{ id: string }>('host-key-prompt-closed', ({ id }) => {
      queue = queue.filter((entry) => entry.id !== id);
    });

    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
    };
  });

  async function answer(trust: boolean): Promise<void> {
    const current = prompt;
    if (!current) return;
    queue = queue.slice(1);
    try {
      const accepted = await api.hostKeyPromptResolve(current.id, trust);
      if (!accepted) {
        toasts.info('The host-key prompt is no longer active.');
      }
    } catch (err) {
      toasts.error(`Could not answer the host-key prompt: ${errorMessage(err)}`);
    }
  }
</script>

{#if prompt}
  <div class="backdrop" role="presentation">
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-label="Host key confirmation"
      use:focusTrap={{ onescape: () => void answer(false) }}
    >
      {#if prompt.request.state === 'unknown'}
        <h2>Unknown host key</h2>
        <p class="target">{prompt.request.host}:{prompt.request.port}</p>
        <dl>
          <dt>Type</dt>
          <dd>{prompt.request.key.key_type}</dd>
          <dt>Fingerprint</dt>
          <dd class="mono">{prompt.request.key.fingerprint}</dd>
        </dl>
        {#if prompt.request.other_key_types && prompt.request.other_key_types.length > 0}
          <p class="hint danger" data-testid="other-key-types">
            This host is already known under another key type ({prompt.request.other_key_types.join(
              ', ',
            )}), but the server now presents a {prompt.request.key.key_type} key that was never seen before.
            That is normal after a server upgrade or reinstall, and it is also what an impostor without
            the real key would do. Verify the fingerprint with the server administrator.
          </p>
        {:else}
          <p class="hint">
            The server key is not known yet. Verify the fingerprint with the server administrator
            before trusting it.
          </p>
        {/if}
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

      {#if queue.length > 1}
        <p class="hint">
          {queue.length - 1} more host key {queue.length === 2 ? 'question' : 'questions'} waiting.
        </p>
      {/if}

      <div class="actions">
        <!-- The safe answer takes the focus: an Enter by habit rejects. -->
        <button type="button" data-autofocus onclick={() => answer(false)}>Reject</button>
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
