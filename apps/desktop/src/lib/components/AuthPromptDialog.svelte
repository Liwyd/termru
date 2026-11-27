<script lang="ts">
  import { onMount } from 'svelte';
  import { focusTrap } from '../actions/focus-trap';
  import { api } from '../ipc';
  import { listen, type UnlistenFn } from '../ipc-backend';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import type { AuthPromptMessage } from '../types';

  /**
   * The questions of the authentication phase: the rounds of a
   * keyboard-interactive server (one-time codes, PAM prompts) and the
   * passphrase of an encrypted key.
   *
   * The questions waiting for an answer, oldest first. A second question
   * that arrives while the first is open waits its turn: it must never
   * replace (and so silently drop) a question the user has not answered.
   */
  let queue = $state<AuthPromptMessage[]>([]);
  const current = $derived(queue[0] ?? null);

  /** The answers typed so far, one per prompt of the current question. */
  let answers = $state<string[]>([]);
  let remember = $state(false);
  /** The question the fields above belong to. */
  let shownId: string | null = null;

  // Fresh fields for every question (a second round must not start with the
  // answers of the first).
  $effect.pre(() => {
    const id = current?.id ?? null;
    if (id === shownId) return;
    shownId = id;
    answers = current ? current.request.prompts.map(() => '') : [];
    remember = false;
  });

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

    subscribe<AuthPromptMessage>('auth-prompt', (incoming) => {
      if (queue.some((entry) => entry.id === incoming.id)) return;
      queue = [...queue, incoming];
    });
    // The question ended without this dialog: it timed out, its connection
    // was closed, or another window answered it.
    subscribe<{ id: string }>('auth-prompt-closed', ({ id }) => {
      queue = queue.filter((entry) => entry.id !== id);
    });

    return () => {
      cancelled = true;
      for (const fn of unlisteners) fn();
    };
  });

  const passphrase = $derived(
    current?.request.kind.type === 'passphrase' ? current.request.kind : null,
  );
  /** A passphrase can be remembered when we know whose it is. */
  const canRemember = $derived(
    passphrase?.identity != null && current?.host_id != null && answers.length === 1,
  );
  const hostName = $derived.by(() => {
    const hostId = current?.host_id ?? null;
    return hostId ? (hosts.hosts.find((host) => host.id === hostId)?.name ?? null) : null;
  });
  const title = $derived.by(() => {
    if (!current) return '';
    if (passphrase) return 'Passphrase required';
    return current.request.name.trim() || 'Authentication required';
  });

  async function answer(values: string[] | null): Promise<void> {
    const question = current;
    if (!question) return;
    // Read before the question leaves the queue: what is derived from the
    // current question is about the next one afterwards.
    const rememberFor =
      values !== null && canRemember && remember && values[0] && question.host_id
        ? { sessionId: question.session_id, hostId: question.host_id, value: values[0] }
        : null;
    queue = queue.slice(1);
    // Kept until the connection authenticated (until then nobody knows that
    // the passphrase is right). Stashed before the answer goes out: the
    // status that settles it can arrive before this call returns.
    if (rememberFor) {
      sessions.stashPassphrase(rememberFor.sessionId, rememberFor.hostId, rememberFor.value);
    }
    try {
      const delivered = await api.authPromptResolve(question.id, values);
      if (!delivered) {
        if (rememberFor) sessions.discardPassphrase(rememberFor.sessionId);
        toasts.info('The authentication question is no longer active.');
      }
    } catch (err) {
      if (rememberFor) sessions.discardPassphrase(rememberFor.sessionId);
      toasts.error(`Could not answer the authentication question: ${errorMessage(err)}`);
    }
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    void answer([...answers]);
  }
</script>

{#if current}
  {#key current.id}
    <div class="backdrop" role="presentation">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="auth-prompt-title"
        aria-describedby={current.request.instructions.trim()
          ? 'auth-prompt-instructions'
          : undefined}
        use:focusTrap={{ onescape: () => void answer(null) }}
      >
        <form onsubmit={submit}>
          <h2 id="auth-prompt-title">{title}</h2>
          {#if hostName}
            <p class="target">{hostName}</p>
          {/if}
          {#if current.request.instructions.trim()}
            <p class="instructions" id="auth-prompt-instructions">
              {current.request.instructions}
            </p>
          {/if}
          {#if passphrase?.retry}
            <p class="hint danger" role="alert">
              That passphrase did not unlock the key. Try again.
            </p>
          {/if}

          {#each current.request.prompts as prompt, index (index)}
            <label>
              {prompt.text}
              <input
                type={prompt.echo ? 'text' : 'password'}
                bind:value={answers[index]}
                autocomplete="off"
                autocapitalize="off"
                spellcheck="false"
                data-autofocus={index === 0 ? true : undefined}
              />
            </label>
          {/each}

          {#if canRemember}
            <label class="checkbox">
              <input type="checkbox" bind:checked={remember} />
              <span>Remember in the system credential store</span>
            </label>
          {/if}

          {#if queue.length > 1}
            <p class="hint">
              {queue.length - 1} more {queue.length === 2 ? 'question' : 'questions'} waiting.
            </p>
          {/if}

          <div class="actions">
            <button type="button" onclick={() => answer(null)}>Cancel</button>
            <button type="submit" class="primary">
              {passphrase ? 'Unlock' : 'Continue'}
            </button>
          </div>
        </form>
      </div>
    </div>
  {/key}
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

  /* Text the server sent: keep its line breaks, never let it overflow. */
  .instructions {
    margin: 0;
    font-size: 0.8rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
    color: var(--termru-muted);
    overflow-wrap: anywhere;
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

  .hint.danger {
    color: var(--termru-danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
