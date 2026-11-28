<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../ipc';
  import { listen, type UnlistenFn } from '../ipc-backend';
  import { errorMessage } from '../stores/hosts.svelte';
  import { notify } from '../notify.svelte';
  import type { HostKeyPromptMessage } from '../types';
  import Icon from './Icon.svelte';
  import SessionDialog from './SessionDialog.svelte';

  /**
   * The host-key question (Doc A.05): Termru never trusts a key silently.
   *
   * A host seen for the first time shows where it connects and the
   * fingerprint to compare. A host whose key changed blocks the connection
   * until the user decides: both fingerprints side by side, and a warning
   * that continuing replaces the stored key. The safe answer (Cancel) has
   * the focus, so an Enter by habit does not trust anything.
   *
   * The prompts waiting for an answer, oldest first. A second prompt that
   * arrives while the first is open waits its turn: it must never replace
   * (and so silently drop) a question the user has not answered. Everything
   * the server sent (host, key type, fingerprint) is shown as plain text.
   */
  let queue = $state<HostKeyPromptMessage[]>([]);
  const prompt = $derived(queue[0] ?? null);

  // What the dialog shows while it fades out, after the queue is empty.
  let last = $state.raw<HostKeyPromptMessage | null>(null);
  $effect(() => {
    if (prompt) last = prompt;
  });
  const shown = $derived(prompt ?? last);
  const request = $derived(shown?.request ?? null);
  const changed = $derived(request?.state === 'changed');

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
      if (!accepted) notify({ key: 'prompt.hostkey-inactive' });
    } catch (err) {
      notify({ key: 'prompt.hostkey-failed', values: { detail: errorMessage(err) } });
    }
  }
</script>

<SessionDialog
  open={prompt !== null}
  role="alertdialog"
  wide
  tone={changed ? 'danger' : 'default'}
  backdropCloses={false}
  title={changed ? 'Host key changed' : 'Verify this host'}
  description={request?.state === 'changed'
    ? `The key ${request.host}:${request.port} presents is not the one Termru stored. Connecting is blocked until you decide.`
    : request
      ? `First connection to ${request.host}:${request.port}. Compare the fingerprint with a source you trust before you connect.`
      : undefined}
  onclose={() => void answer(false)}
>
  {#if request?.state === 'unknown'}
    <dl class="facts">
      <div>
        <dt>Host</dt>
        <dd>{request.host}:{request.port}</dd>
      </div>
      <div>
        <dt>Key type</dt>
        <dd>{request.key.key_type}</dd>
      </div>
      <div>
        <dt>Fingerprint</dt>
        <dd class="fingerprint" data-testid="fingerprint-new">{request.key.fingerprint}</dd>
      </div>
    </dl>
    {#if request.other_key_types && request.other_key_types.length > 0}
      <div class="callout warning" data-testid="other-key-types">
        <p class="label">Known under another key type</p>
        <p>
          This host is already known under {request.other_key_types.join(', ')}, but the server now
          presents a {request.key.key_type} key Termru has not seen. That is normal after a server upgrade
          or reinstall. An impostor without the real key would do the same, so check the fingerprint with
          the server’s administrator.
        </p>
      </div>
    {:else}
      <p class="how">
        To check it, run <code>ssh-keygen -lf</code> on the server, or read it in your provider’s console.
      </p>
    {/if}
  {:else if request?.state === 'changed'}
    <dl class="facts">
      <div>
        <dt>Stored key <span class="type">{request.old.key_type}</span></dt>
        <dd class="fingerprint" data-testid="fingerprint-old">{request.old.fingerprint}</dd>
      </div>
      <div>
        <dt>Key now presented <span class="type">{request.new.key_type}</span></dt>
        <dd class="fingerprint new" data-testid="fingerprint-new">{request.new.fingerprint}</dd>
      </div>
    </dl>
    <div class="callout danger">
      <p class="label">Do not continue unless you know why</p>
      <p>
        This can mean the server was rebuilt, or that someone is intercepting the connection.
        Connecting replaces the stored key with the new one.
      </p>
    </div>
  {/if}

  {#if queue.length > 1}
    <p class="more">
      {queue.length - 1} more host key {queue.length === 2 ? 'question' : 'questions'} waiting.
    </p>
  {/if}

  {#snippet actions()}
    <!-- The safe answer takes the focus: an Enter by habit rejects. -->
    <button type="button" class="quiet" data-autofocus onclick={() => answer(false)}>Cancel</button>
    <button
      type="button"
      class="verify"
      class:primary={!changed}
      class:danger-fill={changed}
      onclick={() => answer(true)}
    >
      <Icon name="arrow-up-right" size={16} />
      Verify &amp; connect
    </button>
  {/snippet}
</SessionDialog>

<style>
  .facts {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    margin: 0;
  }

  dt {
    margin-bottom: var(--termru-space-4);
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-text-2);
    font-weight: var(--termru-text-meta-weight);
  }

  .type {
    margin-left: var(--termru-space-8);
    letter-spacing: 0;
    text-transform: none;
    color: var(--termru-text-3);
  }

  dd {
    margin: 0;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    overflow-wrap: anywhere;
  }

  /* The text to compare: boxed, so it reads as one unit. */
  .fingerprint {
    padding: var(--termru-space-12);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-side);
    user-select: all;
  }

  .fingerprint.new {
    border-color: color-mix(in srgb, var(--termru-err-text) 50%, var(--termru-border));
    background: var(--termru-err-soft);
  }

  .how {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  code {
    font-family: var(--termru-font-mono);
    color: var(--termru-text);
  }

  .callout {
    padding: var(--termru-space-12) var(--termru-space-16);
    border-radius: var(--termru-radius-input);
    background: var(--termru-warn-soft);
  }

  .callout.danger {
    background: var(--termru-err-soft);
  }

  .callout p {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    overflow-wrap: anywhere;
  }

  .callout .label {
    margin-bottom: var(--termru-space-4);
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-warn-text);
    font-weight: var(--termru-text-meta-weight);
  }

  .callout.danger .label {
    color: var(--termru-err-text);
  }

  .more {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
  }

  .verify {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
  }
</style>
