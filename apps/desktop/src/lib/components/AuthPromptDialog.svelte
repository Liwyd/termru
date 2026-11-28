<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../ipc';
  import { listen, type UnlistenFn } from '../ipc-backend';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { notify } from '../notify.svelte';
  import type { AuthPromptMessage } from '../types';
  import Field from './Field.svelte';
  import SessionDialog from './SessionDialog.svelte';

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

  // What the dialog shows while it fades out, after the last question.
  let last = $state.raw<AuthPromptMessage | null>(null);
  $effect(() => {
    if (current) last = current;
  });
  const shown = $derived(current ?? last);

  const passphrase = $derived(
    shown?.request.kind.type === 'passphrase' ? shown.request.kind : null,
  );
  /** A passphrase can be remembered when we know whose it is. */
  const canRemember = $derived(
    passphrase?.identity != null && shown?.host_id != null && answers.length === 1,
  );
  const hostName = $derived.by(() => {
    const hostId = shown?.host_id ?? null;
    return hostId ? (hosts.hosts.find((host) => host.id === hostId)?.name ?? null) : null;
  });
  // The title is text the server may have chosen: it is shown as it is, as text.
  const title = $derived.by(() => {
    if (!shown) return 'Sign in';
    if (passphrase) return 'Passphrase required';
    return shown.request.name.trim() || 'Authentication required';
  });
  const subtitle = $derived.by(() => {
    if (passphrase?.identity) {
      return hostName
        ? `Unlock the key of ${passphrase.identity.name} to connect to ${hostName}.`
        : `Unlock the key of ${passphrase.identity.name}.`;
    }
    if (passphrase)
      return hostName ? `Unlock the key to connect to ${hostName}.` : 'Unlock the key.';
    return hostName ? `${hostName} asks for more than a password.` : undefined;
  });

  /** A new question: the first field takes the focus (the dialog stays mounted). */
  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[data-autofocus]')?.focus();
  }

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
        notify({ key: 'prompt.auth-inactive' });
      }
    } catch (err) {
      if (rememberFor) sessions.discardPassphrase(rememberFor.sessionId);
      notify({ key: 'prompt.auth-failed', values: { detail: errorMessage(err) } });
    }
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    void answer([...answers]);
  }
</script>

<SessionDialog
  open={current !== null}
  role="dialog"
  {title}
  description={subtitle}
  backdropCloses={false}
  onclose={() => void answer(null)}
  onsubmit={submit}
>
  {#if shown}
    {#key shown.id}
      <div class="question" use:focusFirst>
        {#if shown.request.instructions.trim()}
          <p class="instructions">{shown.request.instructions}</p>
        {/if}

        <!-- A wrong passphrase is announced at once, and the field points at it. -->
        {#if passphrase?.retry}
          <p class="retry" id="auth-retry" role="alert">
            That passphrase did not unlock the key. Try again.
          </p>
        {/if}

        {#each shown.request.prompts as prompt, index (index)}
          <Field label={prompt.text} id="auth-answer-{index}">
            {#snippet control(attrs)}
              <input
                {...attrs}
                aria-invalid={passphrase?.retry && index === 0 ? 'true' : undefined}
                aria-describedby={passphrase?.retry && index === 0 ? 'auth-retry' : undefined}
                type={prompt.echo ? 'text' : 'password'}
                bind:value={answers[index]}
                autocomplete="off"
                autocapitalize="off"
                spellcheck="false"
                data-autofocus={index === 0 ? true : undefined}
              />
            {/snippet}
          </Field>
        {/each}

        {#if canRemember}
          <label class="remember">
            <input type="checkbox" bind:checked={remember} />
            <span>Save in this device’s credential store</span>
          </label>
        {/if}
      </div>
    {/key}
  {/if}

  {#if queue.length > 1}
    <p class="more">
      {queue.length - 1} more {queue.length === 2 ? 'question' : 'questions'} waiting.
    </p>
  {/if}

  {#snippet actions()}
    <button type="button" class="quiet" onclick={() => answer(null)}>Cancel</button>
    <button type="submit" class="primary">{passphrase ? 'Unlock' : 'Continue'}</button>
  {/snippet}
</SessionDialog>

<style>
  .question {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
  }

  /* Text the server sent: keep its line breaks, never let it overflow. */
  .instructions {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .retry {
    margin: 0;
    padding: var(--termru-space-12) var(--termru-space-16);
    border-left: 2px solid var(--termru-err-text);
    background: var(--termru-err-soft);
    color: var(--termru-err-text);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .remember {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .more {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
  }
</style>
