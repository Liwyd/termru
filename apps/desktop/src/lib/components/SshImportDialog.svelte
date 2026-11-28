<script lang="ts">
  import { revealed } from '../display-text';
  import { hostPort } from '../forwards';
  import { sshImport, whyNotImportable } from '../stores/ssh-import.svelte';
  import type { SshConfigCandidate } from '../types';
  import Icon from './Icon.svelte';
  import SessionDialog from './SessionDialog.svelte';

  /**
   * Import from `~/.ssh/config` (ADR 0010): lists the hosts of the file with
   * a checkbox each, says which are new, which the library has already and
   * which cannot be imported (and why), shows the jump routes and the key
   * files that will be referenced (never copied), then imports the ticked
   * ones in one step and reports what happened.
   */
  const store = sshImport;

  const uid = $props.id();

  const preview = $derived(store.preview);
  const candidates = $derived(preview?.candidates ?? []);
  const importable = $derived(store.importable);
  const count = $derived(store.selected.length);
  const also = $derived(store.alsoImported);
  const report = $derived(store.report);
  /** There is something to choose from (a file was found and it lists hosts). */
  const choosing = $derived(preview !== null && preview.found && candidates.length > 0);

  const description = $derived(
    store.phase === 'done' ? 'Finished.' : preview ? preview.path : 'Reading your SSH config file.',
  );

  const title = $derived(store.phase === 'done' ? 'Import finished' : 'Import from SSH config');

  function target(candidate: SshConfigCandidate): string {
    const user = candidate.user ? `${candidate.user}@` : '';
    return `${user}${hostPort(candidate.host, candidate.port)}`;
  }

  function status(candidate: SshConfigCandidate): string {
    if (candidate.existing_host_id) return 'In your library';
    if (candidate.problem) return 'Cannot import';
    return 'New';
  }

  const plural = (n: number, noun: string): string => `${n} ${noun}${n === 1 ? '' : 's'}`;

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (store.phase === 'ready' && count > 0) void store.run();
    else if (store.phase === 'done') store.close();
  }
</script>

<SessionDialog
  open={store.open}
  role="dialog"
  xwide
  {title}
  {description}
  monoDescription={store.phase !== 'done'}
  onclose={() => store.close()}
  onsubmit={submit}
>
  {#if store.phase === 'loading'}
    <p class="status" role="status">Reading the file…</p>
  {:else if store.phase === 'error'}
    <p class="problem" role="alert">
      <Icon name="alert-circle" size={15} />
      <span>{store.error ?? 'The file could not be read.'}</span>
    </p>
  {:else if store.phase === 'done' && report}
    <div class="summary" role="status">
      <p class="headline">
        {report.imported.length === 0
          ? 'Nothing was imported.'
          : `Imported ${plural(report.imported.length, 'host')}.`}
      </p>
      {#if report.imported.length > 0}
        <ul class="plain mono">
          {#each report.imported as host (host.id)}
            <li>{revealed(host.name)}</li>
          {/each}
        </ul>
      {/if}
      {#if report.identities_created > 0}
        <p class="note">
          {plural(report.identities_created, 'key file')} referenced as
          {report.identities_created === 1 ? 'an identity' : 'identities'}. The files stay where
          they are.
        </p>
      {/if}
      {#if report.already_present.length > 0}
        <p class="note">
          Already in your library: <span class="mono"
            >{report.already_present.map(revealed).join(', ')}</span
          >.
        </p>
      {/if}
      {#if report.skipped.length > 0}
        <p class="note">Left out:</p>
        <ul class="plain">
          {#each report.skipped as entry (entry.alias)}
            <li><span class="mono">{revealed(entry.alias)}</span> · {revealed(entry.reason)}</li>
          {/each}
        </ul>
      {/if}
      {#each report.warnings as warning, index (index)}
        <p class="warning">
          <Icon name="alert-circle" size={15} /><span>{revealed(warning)}</span>
        </p>
      {/each}
    </div>
  {:else if preview && !preview.found}
    <div class="empty">
      <p class="headline">No SSH config found</p>
      <p class="note">
        Termru looked for <span class="mono">{preview.path}</span> and there is no file there. Nothing
        to import.
      </p>
    </div>
  {:else if preview && candidates.length === 0}
    <div class="empty">
      <p class="headline">No hosts to import</p>
      <p class="note">
        <span class="mono">{preview.path}</span> has no <span class="mono">Host</span> entry that
        names one machine (patterns such as <span class="mono">*</span> are not hosts).
      </p>
    </div>
  {:else if preview}
    <fieldset class="list">
      <legend class="head">
        <span>Hosts in the file</span>
        <span class="tools">
          <span class="count" aria-live="polite">{count} of {importable.length} selected</span>
          <button
            type="button"
            class="link"
            disabled={importable.length === 0 || count === importable.length}
            onclick={() => store.selectAll()}>Select all new</button
          >
          <button
            type="button"
            class="link"
            disabled={count === 0}
            onclick={() => store.selectNone()}>Select none</button
          >
        </span>
      </legend>
      <ul class="rows">
        {#each candidates as candidate (candidate.alias)}
          {@const why = whyNotImportable(candidate)}
          {@const id = `${uid}-${candidate.alias}`}
          <li class="row" class:off={why !== null}>
            <input
              id="{id}-box"
              type="checkbox"
              checked={store.selected.includes(candidate.alias)}
              disabled={why !== null}
              aria-describedby="{id}-detail"
              onchange={() => store.toggle(candidate.alias)}
            />
            <label for="{id}-box" class="body">
              <span class="line">
                <span class="alias">{revealed(candidate.alias)}</span>
                <span class="chip" class:new={why === null}>{status(candidate)}</span>
              </span>
              <span class="target mono">{revealed(target(candidate))}</span>
            </label>
            <div class="detail" id="{id}-detail">
              {#if why}
                <p class="why">{revealed(why)}</p>
              {/if}
              {#if candidate.jump_chain.length > 0}
                <p class="meta">
                  <span class="label">Via</span>
                  <span class="mono">{candidate.jump_chain.map(revealed).join(' → ')}</span>
                </p>
              {/if}
              {#each candidate.identity_files as file (file)}
                <p class="meta">
                  <span class="label">Key</span>
                  <span class="mono">{revealed(file)}</span>
                </p>
              {/each}
              {#each candidate.warnings as warning, index (index)}
                <p class="warning">
                  <Icon name="alert-circle" size={14} /><span>{revealed(warning)}</span>
                </p>
              {/each}
            </div>
          </li>
        {/each}
      </ul>
    </fieldset>

    {#if preview.warnings.length > 0}
      <div class="file-warnings">
        {#each preview.warnings as warning, index (index)}
          <p class="warning">
            <Icon name="alert-circle" size={14} /><span>{revealed(warning)}</span>
          </p>
        {/each}
      </div>
    {/if}

    <p class="note">
      Key files are only referenced by their path: Termru never reads or copies them. A host that
      another one jumps through is imported with it.
      {#if also.length > 0}
        <strong class="also"
          >Also imports <span class="mono">{also.map(revealed).join(', ')}</span>.</strong
        >
      {/if}
    </p>

    {#if store.error}
      <p class="problem" role="alert">
        <Icon name="alert-circle" size={15} />
        <span>{store.error}</span>
      </p>
    {/if}
  {/if}

  {#snippet actions()}
    {#if store.phase === 'done'}
      <button type="submit" class="primary" data-autofocus>Done</button>
    {:else}
      <button type="button" class="quiet" onclick={() => store.close()}>
        {store.phase === 'ready' && !choosing ? 'Close' : 'Cancel'}
      </button>
      {#if store.phase === 'error'}
        <button type="button" class="primary" data-autofocus onclick={() => void store.show()}>
          Try again
        </button>
      {:else if choosing && (store.phase === 'ready' || store.phase === 'importing')}
        <button
          type="submit"
          class="primary"
          data-autofocus
          disabled={count === 0 || store.phase === 'importing'}
          title={store.phase === 'importing'
            ? 'Importing…'
            : count === 0
              ? 'Tick at least one host to import'
              : undefined}
        >
          {store.phase === 'importing'
            ? 'Importing…'
            : count === 0
              ? 'Import'
              : `Import ${plural(count, 'host')}`}
        </button>
      {/if}
    {/if}
  {/snippet}
</SessionDialog>

<style>
  .status,
  .note {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  .headline {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: 550;
  }

  .empty,
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
  }

  ul.plain {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .list {
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-12);
    width: 100%;
    padding: 0 0 var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 550;
  }

  .tools {
    display: inline-flex;
    align-items: baseline;
    gap: var(--termru-space-12);
    font-weight: 400;
  }

  .count {
    color: var(--termru-text-2);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: min(46vh, 420px);
    overflow: auto;
    border-top: 1px solid var(--termru-border);
  }

  .row {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr);
    column-gap: var(--termru-space-12);
    row-gap: var(--termru-space-4);
    padding: var(--termru-space-12) 0;
    border-bottom: 1px solid var(--termru-border);
  }

  .row input {
    margin: 3px 0 0;
    width: 15px;
    height: 15px;
  }

  .row.off .body {
    color: var(--termru-text-2);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    cursor: pointer;
  }

  .row.off .body {
    cursor: default;
  }

  .line {
    display: flex;
    align-items: baseline;
    gap: var(--termru-space-12);
  }

  .alias {
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: 550;
    overflow-wrap: anywhere;
  }

  .chip {
    flex: none;
    padding: 0 var(--termru-space-8);
    border: 1px solid var(--termru-border);
    border-radius: 999px;
    font-size: var(--termru-text-meta-size);
    line-height: 18px;
    color: var(--termru-text-2);
  }

  /* A host that can be imported: a neutral chip. Champagne is the dialog's one
     primary button and the ticks. */
  .chip.new {
    border-color: transparent;
    background: var(--termru-raised-2);
    color: var(--termru-text);
  }

  .target {
    overflow-wrap: anywhere;
    color: var(--termru-text-2);
  }

  .detail {
    grid-column: 2;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .detail p {
    margin: 0;
  }

  .why {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .meta {
    display: flex;
    gap: var(--termru-space-12);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    overflow-wrap: anywhere;
  }

  .label {
    flex: none;
    width: 2.5rem;
    color: var(--termru-text-2);
  }

  .warning {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-warn-text);
  }

  .warning :global(svg) {
    flex: none;
    margin-top: 3px;
  }

  .file-warnings {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-4);
  }

  .also {
    display: block;
    margin-top: var(--termru-space-4);
    font-weight: 550;
    color: var(--termru-text);
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
