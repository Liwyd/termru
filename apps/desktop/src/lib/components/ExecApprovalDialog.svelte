<script lang="ts">
  import { revealed } from '../display-text';
  import { forwardLine } from '../forwards';
  import { execApproval } from '../stores/exec-approval.svelte';
  import SessionDialog from './SessionDialog.svelte';
  import Icon from './Icon.svelte';

  /**
   * The review of the settings of a host that arrived through sync and that
   * act when it is connected to (ADR 0010): the startup command, agent
   * forwarding, environment variables and forwards that listen on another
   * address than loopback. It lists exactly what the connection left out,
   * word for word (text that could disguise itself is written out), and
   * approves exactly that: the backend refuses the approval when the fields
   * changed after the list was read.
   */
  const store = execApproval;

  // What the dialog shows while it fades out, after the review is gone.
  const review = $derived(store.current);
  const pending = $derived(review?.pending === true);
  const fields = $derived(review?.fields ?? null);
  const nothing = $derived(
    fields !== null &&
      fields.startup_command === null &&
      !fields.agent_forward &&
      fields.env.length === 0 &&
      fields.forwards.length === 0,
  );

  const description = $derived(
    review
      ? pending
        ? `Synced settings for ${review.host_name} need your approval on this device.`
        : `The settings of ${review.host_name} are approved on this device.`
      : 'The settings of the host could not be read.',
  );

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (pending) void store.approve();
    else store.close();
  }
</script>

<SessionDialog
  open={store.open}
  role="dialog"
  wide
  title={pending ? 'Approve synced settings' : 'Synced settings'}
  {description}
  onclose={() => store.close()}
  onsubmit={submit}
>
  {#if review && fields}
    {#if pending}
      <p class="lead">
        These came with the host from sync (or from another device) and act when you connect: they
        type into the shell, share your agent or open ports. Until you approve them, connections to
        this host run without them.
      </p>
    {:else if nothing}
      <p class="lead">This host asks for nothing that needs approval.</p>
    {:else}
      <p class="lead">
        You approved these. Connect again to use them: a session that is already open keeps running
        without them.
      </p>
    {/if}

    <dl class="fields">
      {#if fields.startup_command !== null}
        <div class="field">
          <dt>Startup command</dt>
          <dd><code class="block">{revealed(fields.startup_command)}</code></dd>
          <dd class="why">Typed into the shell when it starts, followed by Enter.</dd>
        </div>
      {/if}
      {#if fields.agent_forward}
        <div class="field">
          <dt>Agent forwarding</dt>
          <dd>On</dd>
          <dd class="why">
            The server's administrators can use your agent's keys while you are connected.
          </dd>
        </div>
      {/if}
      {#if fields.env.length > 0}
        <div class="field">
          <dt>Environment variables</dt>
          {#each fields.env as variable (variable.name)}
            <dd><code>{revealed(variable.name)}={revealed(variable.value)}</code></dd>
          {/each}
        </div>
      {/if}
      {#if fields.forwards.length > 0}
        <div class="field">
          <dt>Forwards that are not loopback only</dt>
          {#each fields.forwards as forward, index (index)}
            <dd><code>{revealed(forwardLine(forward))}</code></dd>
          {/each}
          <dd class="why">Other machines on the network can reach these.</dd>
        </div>
      {/if}
    </dl>

    {#if pending}
      <p class="small">
        Forwards that listen on loopback only still run. Saving the host in the editor of this
        device approves what it shows.
      </p>
    {/if}
  {/if}

  {#if store.error}
    <p class="problem" role="alert">
      <Icon name="alert-circle" size={15} />
      <span>{store.error}</span>
    </p>
  {/if}

  {#snippet actions()}
    <button type="button" class="quiet" onclick={() => store.close()}>
      {pending ? 'Not now' : 'Close'}
    </button>
    {#if pending}
      <button
        type="submit"
        class="primary"
        data-autofocus
        disabled={store.busy}
        title={store.busy ? 'Approving…' : undefined}
      >
        {store.busy ? 'Approving…' : 'Approve on this device'}
      </button>
    {/if}
  {/snippet}
</SessionDialog>

<style>
  .lead,
  .small {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    margin: 0;
    padding: var(--termru-space-12) 0;
    border-top: 1px solid var(--termru-border);
    border-bottom: 1px solid var(--termru-border);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-4);
    min-width: 0;
  }

  dt {
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-text-2);
    font-weight: var(--termru-text-meta-weight);
  }

  dd {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    overflow-wrap: anywhere;
  }

  dd.why {
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-2);
  }

  code {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  code.block {
    display: block;
    padding: var(--termru-space-8) var(--termru-space-12);
    border-radius: var(--termru-radius-input);
    background: var(--termru-side);
    white-space: pre-wrap;
  }

  .problem {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-err-text);
  }

  .problem :global(svg) {
    flex: none;
    margin-top: 3px;
  }
</style>
