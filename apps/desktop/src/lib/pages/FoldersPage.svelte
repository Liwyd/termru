<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { formatCount } from '../count-line';
  import Field from '../components/Field.svelte';
  import Icon from '../components/Icon.svelte';
  import InspectorPanel from '../components/InspectorPanel.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import SessionDialog from '../components/SessionDialog.svelte';
  import SplitBody from '../components/SplitBody.svelte';
  import StatusBadge from '../components/StatusBadge.svelte';
  import {
    buildFolderTree,
    canMoveUnder,
    deleteImpact,
    deleteSentence,
    folderPath,
    hostIsInSubtree,
    parentOptions,
    subtreeIds,
    treeIndent,
  } from '../folders';
  import { errorMessage, hosts } from '../stores/hosts.svelte';
  import { intents } from '../stores/intents.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { notify } from '../notify.svelte';
  import type { Group, Host } from '../types';
  import type { Pane } from '../stores/sessions.svelte';

  /** What the tree selects: every host, the hosts without a folder, or a folder id. */
  type Selection = 'all' | 'unfiled' | (string & {});

  const PAGE = 50;
  const MAX_NAME = 128;

  /** The inspector edits the selected folder, or makes a new one. */
  let mode = $state<'edit' | 'create'>('edit');
  /**
   * How the selection last changed: a key never animates the inspector, and
   * neither does the first paint (nothing has been chosen yet).
   */
  let via = $state<'pointer' | 'keyboard'>('keyboard');
  let name = $state('');
  let parentId = $state('');
  let nameTouched = $state(false);
  let formError = $state<string | null>(null);
  let busy = $state(false);

  let shown = $state(PAGE);
  let picking = $state(false);
  let pickQuery = $state('');

  let deleteOpen = $state(false);
  let deleteTarget = $state<Group | null>(null);

  const tree = $derived(buildFolderTree(hosts.groups, hosts.hosts));
  const byId = $derived(new Map(hosts.groups.map((group) => [group.id, group])));
  /** The selection, falling back to "All hosts" when its folder is gone. */
  const selection = $derived<Selection>(
    hosts.folderSelection === 'all' ||
      hosts.folderSelection === 'unfiled' ||
      byId.has(hosts.folderSelection)
      ? hosts.folderSelection
      : 'all',
  );
  const folder = $derived(
    selection === 'all' || selection === 'unfiled' ? null : (byId.get(selection) ?? null),
  );
  const node = $derived(tree.nodes.find((entry) => entry.group.id === selection) ?? null);

  /** The id of the selected folder: a primitive, so a refreshed list does not retrigger what follows it. */
  const folderId = $derived(folder?.id ?? null);

  const subtree = $derived(folder ? subtreeIds(hosts.groups, folder.id) : null);
  /** The hosts the middle column lists. */
  const listed = $derived.by<Host[]>(() => {
    if (selection === 'all') return hosts.sortedAll;
    if (selection === 'unfiled') {
      return hosts.sortedAll.filter((host) => !host.group_id || !byId.has(host.group_id));
    }
    const inside = subtree;
    return inside ? hosts.sortedAll.filter((host) => hostIsInSubtree(host, inside)) : [];
  });
  /** The hosts that could move here: every host that is not directly in the folder. */
  const available = $derived.by<Host[]>(() => {
    if (selection === 'all') return [];
    if (selection === 'unfiled')
      return hosts.sortedAll.filter((host) => host.group_id && byId.has(host.group_id));
    return hosts.sortedAll.filter((host) => host.group_id !== selection);
  });
  const pickList = $derived.by<Host[]>(() => {
    const needle = pickQuery.trim().toLowerCase();
    const matches = needle
      ? available.filter((host) =>
          `${host.name}\n${host.address}\n${host.username}`.toLowerCase().includes(needle),
        )
      : available;
    return matches.slice(0, PAGE);
  });
  const examples = $derived(available.slice(0, 3));

  const title = $derived(
    selection === 'all' ? 'All hosts' : selection === 'unfiled' ? 'Unfiled' : (folder?.name ?? ''),
  );
  const trail = $derived(
    folder
      ? folderPath(hosts.groups, folder.id).split(' / ').slice(0, -1).join(' / ') || 'Top level'
      : 'Every folder',
  );
  const hostCount = $derived(
    `${formatCount(listed.length)} ${listed.length === 1 ? 'host' : 'hosts'}${node && node.childCount > 0 ? ` · ${formatCount(node.childCount)} ${node.childCount === 1 ? 'subfolder' : 'subfolders'}` : ''}`,
  );

  /** The pane of a running session by host id (the card shows "Connected"). */
  const livePane = $derived.by(() => {
    const map = new Map<string, Pane>();
    for (const pane of sessions.tabs.flatMap((tab) => tab.panes)) {
      const id = pane.host?.id;
      if (!id || pane.status.state === 'closed' || pane.status.state === 'failed') continue;
      if (!map.has(id)) map.set(id, pane);
    }
    return map;
  });

  // The inspector follows the selection: a folder's own values, or a blank form.
  // It follows the folder (its id), not its object: a refreshed list must not
  // wipe what is being typed.
  $effect(() => {
    void folderId;
    const editing = mode === 'edit';
    untrack(() => {
      if (editing && folder) {
        name = folder.name;
        parentId = folder.parent_id ?? '';
      }
      nameTouched = false;
      formError = null;
    });
  });

  // A new selection starts the lists from the top.
  $effect(() => {
    void selection;
    shown = PAGE;
    picking = false;
    pickQuery = '';
  });

  const options = $derived(
    parentOptions(hosts.groups, hosts.hosts, mode === 'edit' ? (folder?.id ?? null) : null),
  );
  const dirty = $derived(
    mode === 'create'
      ? name.trim().length > 0
      : folder !== null && (name.trim() !== folder.name || parentId !== (folder.parent_id ?? '')),
  );
  const nameError = $derived.by(() => {
    const text = name.trim();
    if (text.length === 0) return 'A folder needs a name.';
    if ([...text].length > MAX_NAME) return `A folder name can be at most ${MAX_NAME} characters.`;
    if (/[\u0000-\u001f\u007f-\u009f]/u.test(text))
      return 'A folder name cannot contain control characters.';
    return null;
  });
  const parentVerdict = $derived(
    canMoveUnder(hosts.groups, mode === 'edit' ? (folder?.id ?? null) : null, parentId || null),
  );
  const canEdit = $derived(mode === 'create' || folder !== null);
  /** What the inspector is about: a new value replaces its content. */
  const inspectorKey = $derived(mode === 'create' ? 'new' : `${selection}`);

  function select(next: Selection, how: 'pointer' | 'keyboard'): void {
    hosts.folderSelection = next;
    mode = 'edit';
    via = how;
  }

  /** `detail` is 0 for a click made with the keyboard (Enter, Space). */
  function modality(event: MouseEvent): 'pointer' | 'keyboard' {
    return event.detail > 0 ? 'pointer' : 'keyboard';
  }

  function startCreate(how: 'pointer' | 'keyboard' = 'keyboard'): void {
    mode = 'create';
    via = how;
    name = '';
    // The selected folder is the parent of the new one (frame 5).
    parentId = folder?.id ?? '';
    nameTouched = false;
    formError = null;
    void tick().then(() => document.getElementById('folder-name')?.focus());
  }

  // "New folder" from the palette or the sidebar asks for the form before this
  // page exists: it is served once the page is there (or at once).
  $effect(() => {
    if (intents.take('new-folder')) startCreate();
  });

  function cancelForm(): void {
    mode = 'edit';
    if (folder) {
      name = folder.name;
      parentId = folder.parent_id ?? '';
    }
    nameTouched = false;
    formError = null;
  }

  async function saveFolder(event?: SubmitEvent): Promise<void> {
    event?.preventDefault();
    if (busy) return;
    nameTouched = true;
    formError = null;
    if (nameError) {
      document.getElementById('folder-name')?.focus();
      return;
    }
    if (!parentVerdict.ok) {
      formError = parentVerdict.reason;
      document.getElementById('folder-parent')?.focus();
      return;
    }
    busy = true;
    try {
      if (mode === 'create') {
        const now = new Date().toISOString();
        const created = await hosts.saveGroup({
          id: crypto.randomUUID(),
          name: name.trim(),
          parent_id: parentId || null,
          sort_order: 0,
          created_at: now,
          updated_at: now,
          deleted: false,
        });
        notify({ key: 'folder.created', entityId: created.id, values: { name: created.name } });
        hosts.folderSelection = created.id;
        mode = 'edit';
      } else if (folder) {
        const saved = await hosts.saveGroup({
          ...folder,
          name: name.trim(),
          parent_id: parentId || null,
        });
        notify({ key: 'folder.saved', entityId: saved.id, values: { name: saved.name } });
      }
    } catch (err) {
      // The backend refuses a cycle or too deep a nesting, whatever the page offered.
      formError = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  function askDelete(): void {
    if (!folder) return;
    deleteTarget = folder;
    deleteOpen = true;
  }

  /** What deleting the selected folder would do, said before the question is asked. */
  const consequence = $derived(
    folder ? deleteSentence(folder.name, deleteImpact(hosts.groups, hosts.hosts, folder.id)) : '',
  );

  const deleteText = $derived(
    deleteTarget
      ? deleteSentence(deleteTarget.name, deleteImpact(hosts.groups, hosts.hosts, deleteTarget.id))
      : '',
  );

  async function confirmDelete(): Promise<void> {
    const target = deleteTarget;
    deleteOpen = false;
    if (!target) return;
    const parent = target.parent_id && byId.has(target.parent_id) ? target.parent_id : 'all';
    try {
      await hosts.removeGroup(target.id);
      if (hosts.folderSelection === target.id) hosts.folderSelection = parent;
      notify({ key: 'folder.deleted', entityId: target.id, values: { name: target.name } });
    } catch (err) {
      notify({ key: 'folder.delete-failed', values: { detail: errorMessage(err) } });
    }
  }

  async function move(host: Host, groupId: string | null): Promise<void> {
    if ((host.group_id ?? null) === groupId) return;
    const moved = await hosts.moveHost(host, groupId);
    if (!moved) {
      notify({
        key: 'host.move-failed',
        entityId: host.id,
        values: { name: host.name, detail: hosts.saveError ?? 'The store refused.' },
      });
      return;
    }
    const where = groupId ? folderPath(hosts.groups, groupId) : 'Unfiled';
    notify({
      key: 'host.moved',
      entityId: host.id,
      values: { name: host.name, place: where },
    });
  }

  function folderOf(host: Host): string {
    return host.group_id && byId.has(host.group_id)
      ? folderPath(hosts.groups, host.group_id)
      : 'Unfiled';
  }

  /** `ops@192.0.2.10:22`, with the folder when the list spans several. */
  function targetLine(host: Host): string {
    const address = `${host.username}@${host.address}:${host.port}`;
    const spans = selection === 'all' || (folder !== null && host.group_id !== folder.id);
    return spans ? `${address} · ${folderOf(host)}` : address;
  }

  function onMoveChange(host: Host, event: Event): void {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value;
    select.value = '';
    if (value === '') return;
    void move(host, value === '__none__' ? null : value);
  }

  /** Arrow keys walk the tree; the buttons stay the tab stops. */
  function onTreeKeyDown(event: KeyboardEvent): void {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const items = Array.from(
      (event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('button.item'),
    );
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (index === -1) return;
    event.preventDefault();
    const next = event.key === 'ArrowDown' ? index + 1 : index - 1;
    items[Math.max(0, Math.min(items.length - 1, next))]?.focus();
  }

  const movable = $derived(selection !== 'all');

  // In a narrow window the tree is a short list that scrolls: the folder that
  // is selected (also the one the page opened on) is brought into view, so the
  // tree never shows a selection nobody can see.
  let treeList = $state<HTMLUListElement | null>(null);
  $effect(() => {
    void selection;
    void tree.nodes.length;
    if (!treeList) return;
    const list = treeList;
    void tick().then(() => {
      list.querySelector<HTMLElement>('.item.active')?.scrollIntoView?.({ block: 'nearest' });
    });
  });
</script>

<div class="workpage folders">
  <PageHeader title="Folders" count={formatCount(hosts.groups.length)}>
    {#snippet actions()}
      <button type="button" class="primary new" onclick={(event) => startCreate(modality(event))}>
        <Icon name="folder-plus" size={16} />
        New folder
      </button>
    {/snippet}
  </PageHeader>

  <SplitBody>
    {#snippet main()}
      <div class="cols">
        <!-- The tree: where to look. -->
        <nav class="tree" aria-label="Folders">
          <p class="sect">Folder tree</p>
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <ul onkeydown={onTreeKeyDown} bind:this={treeList}>
            <li>
              <button
                type="button"
                class="item"
                class:active={selection === 'all'}
                aria-current={selection === 'all' ? 'true' : undefined}
                onclick={(event) => select('all', modality(event))}
              >
                <Icon name="layers" size={15} />
                <span class="label">All hosts</span>
                <span class="count">{formatCount(tree.total)}</span>
              </button>
            </li>
            {#each tree.nodes as entry (entry.group.id)}
              <li>
                <button
                  type="button"
                  class="item"
                  class:active={selection === entry.group.id}
                  style:padding-left="calc(var(--termru-space-8) + {treeIndent(entry.depth)}px)"
                  aria-current={selection === entry.group.id ? 'true' : undefined}
                  title={entry.path}
                  onclick={(event) => select(entry.group.id, modality(event))}
                >
                  <Icon name="folder" size={15} />
                  <span class="label">{entry.group.name}</span>
                  <span class="count">{formatCount(entry.count)}</span>
                </button>
              </li>
            {/each}
            <li>
              <button
                type="button"
                class="item"
                class:active={selection === 'unfiled'}
                aria-current={selection === 'unfiled' ? 'true' : undefined}
                onclick={(event) => select('unfiled', modality(event))}
              >
                <Icon name="inbox" size={15} />
                <span class="label">Unfiled</span>
                <span class="count">{formatCount(tree.unfiled)}</span>
              </button>
            </li>
          </ul>
          <p class="note">
            Folders are navigation, not copies. Each host belongs to exactly one folder.
          </p>
        </nav>

        <!-- What is in the selection. -->
        <section class="detail" aria-labelledby="detail-title" aria-busy={hosts.loading}>
          {#if hosts.error}
            <p class="problem" role="alert">
              Could not read your hosts: {hosts.error}
              <button type="button" class="link" onclick={() => void hosts.load()}>Retry</button>
            </p>
          {/if}
          <header class="detail-head">
            <div class="titles">
              <h2 id="detail-title">{title}</h2>
              <p class="trail">{trail}</p>
            </div>
            <p class="count-label">{hostCount}</p>
          </header>

          {#if listed.length > 0}
            <ul class="hostlist" aria-label="Hosts in {title}">
              {#each listed.slice(0, shown) as host (host.id)}
                {@const live = livePane.get(host.id)}
                <li class="host">
                  <span class="who">
                    <span class="host-name">{host.name}</span>
                    <span class="target">{targetLine(host)}</span>
                  </span>
                  {#if live}<StatusBadge pane={live} />{/if}
                  <span class="move-wrap">
                    <select
                      class="move"
                      aria-label="Move {host.name} to another folder"
                      onchange={(event) => onMoveChange(host, event)}
                    >
                      <option value="">Move</option>
                      <option value="__none__" disabled={!host.group_id}>Unfiled (no folder)</option
                      >
                      {#each hosts.folderOptions as option (option.id)}
                        <option value={option.id} disabled={host.group_id === option.id}
                          >{option.label}{host.group_id === option.id ? ' (current)' : ''}</option
                        >
                      {/each}
                    </select>
                  </span>
                </li>
              {/each}
            </ul>
            {#if listed.length > shown}
              <button type="button" class="more" onclick={() => (shown += 100)}>
                Show {formatCount(Math.min(100, listed.length - shown))} more of {formatCount(
                  listed.length - shown,
                )}
              </button>
            {/if}
          {:else if hosts.hosts.length === 0}
            <p class="empty">No hosts yet. Add one on the Hosts page, then file it here.</p>
          {:else if selection === 'all'}
            <p class="empty">No hosts.</p>
          {:else}
            <p class="empty">
              {selection === 'unfiled'
                ? 'Every host is in a folder.'
                : `No hosts in “${title}” yet.`}
            </p>
          {/if}

          {#if movable}
            <div class="pick-bar">
              <p>
                {selection === 'unfiled'
                  ? 'Take a host out of its folder.'
                  : 'Move a host here from another folder.'}
              </p>
              {#if available.length > 0}
                <button
                  type="button"
                  aria-expanded={picking}
                  aria-controls="pick-panel"
                  onclick={() => (picking = !picking)}
                >
                  <Icon name="plus" size={15} />
                  Choose hosts
                </button>
              {/if}
            </div>

            {#if available.length > 0}
              <section class="available" id="pick-panel" aria-labelledby="available-label">
                <p class="sect" id="available-label">
                  Available to move{picking ? '' : ' · examples'}
                </p>
                {#if picking}
                  <input
                    type="search"
                    class="pick-search"
                    aria-label="Search hosts to move"
                    placeholder="Search name, address or username"
                    bind:value={pickQuery}
                  />
                {/if}
                <ul class="pick-list" class:scroll-list={picking}>
                  {#each picking ? pickList : examples as host (host.id)}
                    <li class="pick">
                      <span class="pick-name">{host.name}</span>
                      <span class="pick-from">{folderOf(host)}</span>
                      <button
                        type="button"
                        class="ghost here"
                        aria-label="Move {host.name} here"
                        onclick={() => void move(host, folder ? folder.id : null)}
                      >
                        Move here <Icon name="arrow-right" size={14} />
                      </button>
                    </li>
                  {:else}
                    <li class="pick-empty">No host matches.</li>
                  {/each}
                </ul>
                {#if picking && available.length > pickList.length && pickQuery.trim() === ''}
                  <p class="hint">
                    Showing {pickList.length} of {available.length}. Search to narrow it down.
                  </p>
                {/if}
              </section>
            {/if}
          {:else}
            <p class="hint">Choose a folder in the tree to move hosts into it.</p>
          {/if}

          <p class="note one-folder">
            A host lives in exactly one folder. Moving it here removes it from its current folder;
            its credentials and active sessions are not changed.
          </p>
        </section>
      </div>
    {/snippet}

    {#snippet inspector()}
      <InspectorPanel
        label={mode === 'create' ? 'New folder' : 'Edit folder'}
        contentKey={inspectorKey}
        animate={via === 'pointer'}
      >
        <div class="titles">
          <h2 class="insp-title" title={mode === 'create' ? 'New folder' : title}>
            {mode === 'create' ? 'New folder' : title}
          </h2>
          <p class="insp-sub" title={mode === 'create' ? undefined : trail}>
            {#if mode === 'create'}
              {folder ? `Inside ${folderPath(hosts.groups, folder.id)}` : 'At the top level'}
            {:else if folder}
              {trail}
            {:else}
              A view of your hosts
            {/if}
          </p>
        </div>

        {#if canEdit}
          {#if mode === 'edit' && node}
            <dl class="kv">
              <dt>Hosts</dt>
              <dd>
                {formatCount(node.count)}{node.count !== node.directCount
                  ? ` · ${formatCount(node.directCount)} directly in it`
                  : ''}
              </dd>
              <dt>Subfolders</dt>
              <dd>{formatCount(node.childCount)}</dd>
            </dl>
          {/if}

          <form onsubmit={(event) => void saveFolder(event)} novalidate>
            <Field label="Name" id="folder-name" error={nameTouched ? nameError : null}>
              {#snippet control(attrs)}
                <input
                  {...attrs}
                  type="text"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder="production"
                  bind:value={name}
                  onblur={() => (nameTouched = true)}
                />
              {/snippet}
            </Field>

            <Field
              label="Parent folder"
              id="folder-parent"
              error={parentVerdict.ok ? null : parentVerdict.reason}
              hint="Or choose No parent for a top-level folder."
            >
              {#snippet control(attrs)}
                <select {...attrs} bind:value={parentId}>
                  <option value="">No parent</option>
                  {#each options as option (option.id)}
                    <option
                      value={option.id}
                      disabled={option.disabled}
                      title={option.reason ?? undefined}
                      >{option.label}{option.disabled && option.reason
                        ? ' (unavailable)'
                        : ''}</option
                    >
                  {/each}
                </select>
              {/snippet}
            </Field>

            <p class="insp-note">
              The folder cannot be its own parent or be moved into one of its descendants.
            </p>

            {#if formError}<p class="insp-note error form-error" role="alert">{formError}</p>{/if}

            <div class="insp-actions form-actions">
              <button type="button" class="quiet" onclick={cancelForm}>Cancel</button>
              <button
                type="submit"
                class="primary"
                disabled={busy || (mode === 'edit' && !dirty)}
                title={mode === 'edit' && !dirty && !busy ? 'No changes to save' : undefined}
              >
                {mode === 'create' ? 'Create folder' : 'Save folder'}
              </button>
            </div>
          </form>
        {:else}
          <p class="insp-note">
            {selection === 'all' ? '“All hosts”' : '“Unfiled”'} is a view of your hosts, not a folder.
            Choose a folder in the tree to rename it or move it, or make a new one.
          </p>
        {/if}

        {#if mode === 'create'}
          <p class="insp-note">
            New folder opens this editor with a blank name and the selected folder as its parent.
          </p>
        {/if}

        {#if folder && mode === 'edit'}
          <section class="insp-group danger-zone" aria-labelledby="folder-delete-label">
            <p class="ttl" id="folder-delete-label">Delete</p>
            <p class="insp-note">{consequence}</p>
            <button type="button" class="danger quiet delete" onclick={askDelete}>
              <Icon name="trash" size={15} />
              Delete folder
            </button>
          </section>
        {/if}
      </InspectorPanel>
    {/snippet}
  </SplitBody>

  <SessionDialog
    open={deleteOpen}
    title={deleteTarget ? `Delete “${deleteTarget.name}”?` : 'Delete folder?'}
    description={deleteText}
    onclose={() => (deleteOpen = false)}
  >
    {#snippet actions()}
      <button type="button" class="quiet" data-autofocus onclick={() => (deleteOpen = false)}>
        Cancel
      </button>
      <button type="button" class="danger-fill" onclick={() => void confirmDelete()}
        >Delete folder</button
      >
    {/snippet}
  </SessionDialog>
</div>

<style>
  .new {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  /* The tree, then the hosts of the selection. Each scrolls on its own. */
  .cols {
    display: grid;
    grid-template-columns: 224px minmax(0, 1fr);
    flex: 1;
    min-height: 0;
  }

  .sect {
    margin: 0;
    padding: 0 var(--termru-space-8) var(--termru-space-6);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .tree {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: var(--termru-space-4) var(--termru-space-8) var(--termru-space-16)
      var(--termru-space-20);
    overflow: auto;
  }

  .tree ul {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    min-height: 0;
    padding: 0 var(--termru-space-8);
    border-radius: var(--termru-radius-sm);
    background: transparent;
    color: var(--termru-text-2);
    font-weight: 400;
    text-align: left;
    /* Only the press moves. The selection changes at once: a folder is often
       chosen from the keyboard, and that is never animated. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .item :global(svg) {
    flex: none;
  }

  .item .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex: none;
    font: 500 11px var(--termru-font-mono);
    color: var(--termru-text-3);
    font-variant-numeric: tabular-nums;
  }

  .item.active {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  @media (hover: hover) and (pointer: fine) {
    .item:not(.active) {
      transition:
        transform var(--motion-fast) var(--ease-out),
        background-color var(--motion-fast) ease,
        color var(--motion-fast) ease;
    }

    .item:not(.active):hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-raised) 60%, transparent);
      color: var(--termru-text);
    }
  }

  .note {
    margin: var(--termru-space-16) var(--termru-space-8) 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
    min-height: 0;
    padding: var(--termru-space-4) var(--termru-space-16) var(--termru-space-16);
    overflow: auto;
  }

  .detail-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--termru-space-12);
    padding: 0 var(--termru-space-12);
  }

  .titles {
    min-width: 0;
  }

  .detail-head h2 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .trail {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count-label {
    flex: none;
    margin: 0;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-3);
  }

  /* The hosts: quiet 52 px rows, as on the Hosts page. */
  .hostlist {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .host {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    height: var(--termru-row-h);
    padding: 0 var(--termru-space-8) 0 var(--termru-space-12);
    border-radius: var(--termru-radius-row);
  }

  @media (hover: hover) and (pointer: fine) {
    .host {
      transition: background-color var(--motion-fast) ease;
    }

    .host:hover {
      background: var(--termru-raised);
    }
  }

  .host:focus-within {
    background: var(--termru-raised);
  }

  .who {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .host-name {
    font-size: 13.5px;
    line-height: 20px;
    font-weight: 500;
    letter-spacing: -0.005em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    line-height: 16px;
    letter-spacing: -0.01em;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .move-wrap {
    flex: none;
  }

  /* Move: a quiet control that reads as a button, lit by the row. */
  :global(.workpage) select.move {
    width: 96px;
    min-height: 28px;
    padding: 0 28px 0 var(--termru-space-10);
    border-color: transparent;
    background-color: transparent;
    color: var(--termru-text-2);
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-control-size);
    background-position: right 10px center;
  }

  @media (hover: hover) and (pointer: fine) {
    .host:hover :global(select.move),
    :global(.workpage) select.move:hover:not(:disabled) {
      background-color: var(--termru-raised-2);
      border-color: transparent;
      color: var(--termru-text);
    }
  }

  .more {
    align-self: flex-start;
    margin-left: var(--termru-space-12);
  }

  .empty,
  .hint {
    margin: 0;
    padding: 0 var(--termru-space-12);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  .empty {
    padding-top: var(--termru-space-16);
    padding-bottom: var(--termru-space-16);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-text-2);
  }

  .pick-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-12);
    padding: 0 var(--termru-space-12);
  }

  .pick-bar p {
    margin: 0;
    color: var(--termru-text-2);
  }

  .pick-bar button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .available {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    margin: 0 var(--termru-space-4);
    padding: var(--termru-space-12) var(--termru-space-8) var(--termru-space-8);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-side);
  }

  .available .pick-search {
    margin: 0 var(--termru-space-4);
    width: auto;
  }

  .pick-list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pick-list.scroll-list {
    max-height: 260px;
    overflow: auto;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    min-height: 40px;
    padding: 0 var(--termru-space-4) 0 var(--termru-space-8);
    border-radius: var(--termru-radius-sm);
  }

  .pick-name {
    font-weight: 500;
    flex: none;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pick-from {
    flex: 1;
    min-width: 0;
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pick-empty {
    padding: var(--termru-space-8);
    color: var(--termru-text-3);
  }

  .pick .here {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    flex: none;
    color: var(--termru-text-2);
  }

  @media (hover: hover) and (pointer: fine) {
    .pick .here:hover:not(:disabled) {
      background: var(--termru-raised-2);
      color: var(--termru-text);
    }
  }

  .one-folder {
    margin: var(--termru-space-4) var(--termru-space-12) 0;
    max-width: 60ch;
  }

  .problem {
    margin: 0 var(--termru-space-12);
    color: var(--termru-err-text);
  }

  /* The inspector: the name and where it sits, what it holds, the form. */
  .titles :global(.insp-title) {
    margin: 0;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
  }

  .form-actions {
    justify-content: flex-end;
  }

  .form-actions button {
    padding: 0 var(--termru-space-16);
  }

  .delete {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    margin-left: calc(var(--termru-space-12) * -1);
  }

  /* Room is by the width of the body, not of the window: the stage (the tree,
     the hosts, the inspector) stacks when it is short. */
  @container body (max-width: 1000px) {
    .cols {
      grid-template-columns: 188px minmax(0, 1fr);
    }
  }

  @container body (max-width: 860px) {
    .cols {
      display: flex;
      flex-direction: column;
      flex: none;
      gap: var(--termru-space-12);
    }

    .tree {
      flex: none;
      max-height: 220px;
      padding-right: var(--termru-space-16);
    }

    .detail {
      overflow: visible;
    }
  }
</style>
