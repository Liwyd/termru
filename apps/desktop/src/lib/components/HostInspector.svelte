<script lang="ts">
  import { api } from '../ipc';
  import { authLabel } from '../auth-label';
  import { forwardRows, hasExecFields, KIND_LETTER } from '../forwards';
  import { hostMenuItems, type HostActionId } from '../host-actions';
  import { resolveRoute } from '../jump';
  import { keepaliveLabel } from '../validation';
  import { execApproval } from '../stores/exec-approval.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import type { Pane } from '../stores/sessions.svelte';
  import type { AuthKind, Host } from '../types';
  import Icon from './Icon.svelte';
  import PopMenu from './PopMenu.svelte';

  /**
   * The inspector of the hosts page: everything about the selected host, so the
   * list can stay quiet. Who and where (title, address), what to do (open,
   * edit, more), how a connection gets there (the route through jump hosts, the
   * sign-in and its second step, the keepalive), the forwards with what became
   * of them in the running session, the tags, what happens on connect (the
   * environment, the startup command, agent forwarding) and whether this
   * device has approved the synced settings that act.
   */
  let {
    host,
    live,
    pane,
    animate,
    onconnect,
    onopen,
    onedit,
    ondelete,
    ontogglefavorite,
  }: {
    host: Host;
    /** How many panes hold a session of this host right now. */
    live: number;
    /** The pane whose session reports the forwards (the first live one). */
    pane: Pane | null;
    /** The content fades in (a click chose this host); a key never animates it. */
    animate: boolean;
    onconnect: (host: Host) => void;
    onopen: (host: Host) => void;
    onedit: (host: Host) => void;
    ondelete: (host: Host) => void;
    ontogglefavorite: (host: Host) => void;
  } = $props();

  const AUTH_KIND_LABEL: Record<AuthKind, string> = {
    password: 'Password',
    private_key: 'Identity key',
    agent: 'SSH agent',
    keyboard_interactive: 'Keyboard-interactive',
  };

  const address = $derived(`${host.username}@${host.address}:${host.port}`);
  const isLive = $derived(live > 0);

  // --- route ---------------------------------------------------------------
  const route = $derived(
    resolveRoute(host.id, host.name, host.jump_chain ?? [], (id) =>
      hosts.hosts.find((entry) => entry.id === id),
    ),
  );
  /** The hop names, first hop first (a hop that is gone shows as `missing`). */
  const hops = $derived(
    route.ok
      ? route.route.map(
          (id) => hosts.hosts.find((entry) => entry.id === id)?.name ?? 'missing host',
        )
      : [],
  );

  // --- sign-in -------------------------------------------------------------
  const signIn = $derived(
    authLabel(host, hosts.identities, hosts.passwordStored[host.id] === true),
  );
  const order = $derived(
    host.auth_order ? host.auth_order.map((kind) => AUTH_KIND_LABEL[kind]).join(' → ') : null,
  );

  // --- folder, keepalive, tags --------------------------------------------
  const folder = $derived(
    host.group_id
      ? (hosts.folderOptions.find((option) => option.id === host.group_id)?.label ?? 'Unfiled')
      : 'Unfiled',
  );
  const tagNames = $derived(
    host.tags
      .map((id) => hosts.tags.find((tag) => tag.id === id)?.name)
      .filter((name): name is string => name !== undefined),
  );

  // --- forwards ------------------------------------------------------------
  const forwards = $derived(
    forwardRows(host, isLive ? pane?.forwards : [], pane?.execPending === true),
  );
  const listening = $derived(forwards.filter((row) => row.status.state === 'listening').length);

  function forwardStatus(row: (typeof forwards)[number]): {
    text: string;
    tone: string;
    why: string;
  } {
    if (!isLive) return { text: 'Starts on connect', tone: 'idle', why: '' };
    switch (row.status.state) {
      case 'listening':
        return { text: 'Listening', tone: 'live', why: '' };
      case 'failed':
        return { text: 'Failed', tone: 'err', why: row.status.reason };
      case 'blocked':
        return {
          text: 'Not approved',
          tone: 'warn',
          why: 'Left out until you approve the synced settings.',
        };
      default:
        return { text: 'Starting…', tone: 'idle', why: '' };
    }
  }

  // --- on connect ----------------------------------------------------------
  const envCount = $derived(host.env?.length ?? 0);
  const startup = $derived(host.startup_command?.trim() ? host.startup_command.trim() : null);
  const agent = $derived(host.agent_forward === true);
  const onConnect = $derived(envCount > 0 || startup !== null || agent);

  // --- approval of the synced settings that act ---------------------------
  type Approval = 'none' | 'checking' | 'approved' | 'pending' | 'unknown';
  let approval = $state<Approval>('none');
  /** Counts reads, so a slow answer for a host that is no longer shown is ignored. */
  let reads = 0;

  $effect(() => {
    // A new host, an edited host, or the review dialog closing (it may have
    // approved): ask the backend what this device has approved.
    const id = host.id;
    void host.updated_at;
    void execApproval.open;
    const token = ++reads;
    if (!hasExecFields(host)) {
      approval = 'none';
      return;
    }
    approval = 'checking';
    void api
      .hostExecReview(id)
      .then((review) => {
        if (token !== reads) return;
        approval = review?.pending ? 'pending' : review ? 'approved' : 'unknown';
      })
      .catch(() => {
        if (token === reads) approval = 'unknown';
      });
  });

  // --- the more menu -------------------------------------------------------
  let menu = $state<{ x: number; y: number; animate: boolean } | null>(null);

  function toggleMenu(event: MouseEvent): void {
    if (menu) {
      menu = null;
      return;
    }
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    // `detail` is 0 for a click made with the keyboard: that opens without motion.
    menu = { x: box.right, y: box.bottom + 6, animate: event.detail > 0 };
  }

  function choose(id: string): void {
    switch (id as HostActionId) {
      case 'open':
        onopen(host);
        break;
      case 'connect':
        onconnect(host);
        break;
      case 'edit':
        onedit(host);
        break;
      case 'favorite':
        ontogglefavorite(host);
        break;
      case 'delete':
        ondelete(host);
        break;
    }
  }
</script>

<aside class="insp" aria-label="Host details">
  {#key host.id}
    <div class="content" class:fade={animate}>
      <div class="titlebar">
        <div class="titles">
          <h2 title={host.name}>{host.name}</h2>
          <div class="sub mono" title={address}>{address}</div>
        </div>
        <button
          type="button"
          class="star ghost"
          class:favorite={host.favorite}
          aria-label={host.favorite
            ? `Remove ${host.name} from favorites`
            : `Add ${host.name} to favorites`}
          aria-pressed={host.favorite}
          onclick={() => ontogglefavorite(host)}
        >
          <Icon name="star" size={16} filled={host.favorite} />
        </button>
      </div>

      <div class="acts">
        <button
          type="button"
          class="primary open"
          onclick={() => (isLive ? onopen(host) : onconnect(host))}
        >
          {isLive ? 'Open session' : 'Connect'}
          <kbd aria-hidden="true">↵</kbd>
        </button>
        <button type="button" onclick={() => onedit(host)}>Edit</button>
        <button
          type="button"
          class="icon"
          aria-label="More actions for {host.name}"
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          onclick={toggleMenu}
        >
          <Icon name="more" size={16} />
        </button>
      </div>

      {#if approval === 'pending'}
        <div class="note" role="group" aria-label="Synced settings need approval">
          <p class="note-title">Synced settings are off on this device</p>
          <p class="note-text">
            This host connects without its startup command, environment, agent forwarding or exposed
            forwards until you review them.
          </p>
          <button type="button" onclick={() => void execApproval.show(host.id)}>Review…</button>
        </div>
      {/if}

      <dl class="kv">
        <dt>Route</dt>
        <dd class="route">
          {#if route.ok}
            <span>you</span>
            {#each hops as hop, index (index)}
              <Icon name="arrow-right" size={12} />
              <b>{hop}</b>
            {/each}
            <Icon name="arrow-right" size={12} />
            <b>{host.name}</b>
          {:else}
            <span class="bad">{route.error}</span>
          {/if}
        </dd>
        <dt>Sign-in</dt>
        <dd>{signIn}</dd>
        <dt>Then</dt>
        <dd>One-time code if asked</dd>
        {#if order}
          <dt>Order</dt>
          <dd>{order}</dd>
        {/if}
        <dt>Folder</dt>
        <dd class="wrap">{folder}</dd>
        <dt>Keepalive</dt>
        <dd>{keepaliveLabel(host.keepalive_seconds)}</dd>
      </dl>

      {#if forwards.length > 0}
        <section aria-labelledby="insp-forwards">
          <h3 class="ttl" id="insp-forwards">
            <span>Forwards</span>
            <span>{isLive ? `${listening} listening` : `${forwards.length} configured`}</span>
          </h3>
          <ul class="fwds">
            {#each forwards as row (row.id)}
              {@const status = forwardStatus(row)}
              <li class="fwd">
                <span class="k" aria-hidden="true">{KIND_LETTER[row.forward.kind]}</span>
                <span class="line mono" title={row.line}>{row.line.slice(2)}</span>
                <span class="s {status.tone}" title={status.why || undefined}>{status.text}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if tagNames.length > 0}
        <section aria-labelledby="insp-tags">
          <h3 class="ttl" id="insp-tags"><span>Tags</span></h3>
          <ul class="tags">
            {#each tagNames as name (name)}<li class="tag">{name}</li>{/each}
          </ul>
        </section>
      {/if}

      {#if onConnect || approval === 'approved'}
        <section aria-labelledby="insp-connect">
          <h3 class="ttl" id="insp-connect"><span>On connect</span></h3>
          <dl class="kv">
            {#if envCount > 0}
              <dt>Environment</dt>
              <dd>{envCount} {envCount === 1 ? 'variable' : 'variables'}</dd>
            {/if}
            {#if startup}
              <dt>Startup</dt>
              <dd class="mono clip" title={startup}>{startup}</dd>
            {/if}
            {#if agent}
              <dt>Agent</dt>
              <dd class="warn">Forwarded to the server</dd>
            {/if}
            {#if approval === 'approved'}
              <dt>Approval</dt>
              <dd>
                Approved on this device
                <button type="button" class="link" onclick={() => void execApproval.show(host.id)}
                  >Review</button
                >
              </dd>
            {/if}
          </dl>
        </section>
      {/if}

      {#if host.description}
        <section aria-labelledby="insp-notes">
          <h3 class="ttl" id="insp-notes"><span>Notes</span></h3>
          <p class="notes">{host.description}</p>
        </section>
      {/if}
    </div>
  {/key}
</aside>

<PopMenu
  open={menu !== null}
  x={menu?.x ?? 0}
  y={menu?.y ?? 0}
  align="right"
  animate={menu?.animate ?? true}
  label="Actions for {host.name}"
  items={hostMenuItems(host, isLive)}
  onchoose={choose}
  onclose={() => (menu = null)}
/>

<style>
  .insp {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    margin: 0 var(--termru-space-10) var(--termru-space-10) 0;
    padding: var(--termru-space-20) var(--termru-space-20) var(--termru-space-16);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-side);
    overflow: auto;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-20);
    min-width: 0;
  }

  /* The content of a host a click chose fades in (120 ms); a key never does. */
  .content.fade {
    animation: inspector-in var(--motion-context) var(--ease-out) both;
  }

  @keyframes inspector-in {
    from {
      opacity: 0;
    }
  }

  .titlebar {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
  }

  .titles {
    flex: 1;
    min-width: 0;
  }

  h2 {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
    letter-spacing: -0.015em;
    color: var(--termru-text);
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    letter-spacing: -0.01em;
  }

  .sub {
    margin-top: 2px;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .star {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    min-height: 0;
    padding: 0;
    margin: -2px -6px 0 0;
    color: var(--termru-text-3);
  }

  .star.favorite {
    color: var(--termru-accent-text);
  }

  .acts {
    display: flex;
    gap: var(--termru-space-6);
  }

  .acts button {
    background: var(--termru-raised);
    color: var(--termru-text);
    justify-content: center;
  }

  .acts .primary {
    background: var(--termru-accent);
    color: var(--termru-accent-ink);
  }

  .open {
    flex: 1;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    white-space: nowrap;
  }

  .open kbd {
    background: rgba(0, 0, 0, 0.12);
    color: inherit;
  }

  .acts .icon {
    width: 32px;
    padding: 0;
    display: inline-grid;
    place-items: center;
  }

  .note {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--termru-space-6);
    padding: var(--termru-space-12) var(--termru-space-12) var(--termru-space-12);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-warn-soft);
  }

  .note p {
    margin: 0;
  }

  .note-title {
    font-weight: 500;
    color: var(--termru-warn-text);
  }

  .note-text {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .note button {
    margin-top: var(--termru-space-4);
    background: var(--termru-raised);
  }

  .kv {
    display: grid;
    grid-template-columns: 92px minmax(0, 1fr);
    gap: 10px var(--termru-space-12);
    margin: 0;
  }

  .kv dt {
    color: var(--termru-text-3);
  }

  .kv dd {
    margin: 0;
    min-width: 0;
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  .kv dd.wrap {
    overflow-wrap: anywhere;
  }

  .kv dd.clip {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    overflow-wrap: normal;
  }

  .kv dd.warn {
    color: var(--termru-warn-text);
  }

  .route {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-2);
  }

  .route b {
    font-weight: 500;
    color: var(--termru-text);
    overflow-wrap: anywhere;
  }

  .route :global(svg) {
    flex: none;
  }

  .route .bad {
    font-family: var(--termru-font-ui);
    font-weight: 400;
    color: var(--termru-warn-text);
  }

  .link {
    min-height: 0;
    padding: 0 0 0 var(--termru-space-6);
    border-radius: 3px;
    background: none;
    color: var(--termru-accent-text);
    text-decoration: underline;
    text-underline-offset: 2px;
    font-weight: 400;
  }

  .ttl {
    display: flex;
    justify-content: space-between;
    gap: var(--termru-space-8);
    margin: 0 0 var(--termru-space-8);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .fwds,
  .tags {
    display: flex;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .fwds {
    flex-direction: column;
  }

  .fwd {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 30px;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-2);
  }

  .fwd .k {
    display: grid;
    place-items: center;
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 5px;
    background: var(--termru-raised);
    color: var(--termru-text);
    font-size: 10.5px;
  }

  .fwd .line {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fwd .s {
    flex: none;
    margin-left: auto;
    font-family: var(--termru-font-ui);
    font-size: 12px;
    color: var(--termru-text-3);
  }

  .fwd .s.live {
    color: var(--termru-live-text);
  }

  .fwd .s.warn {
    color: var(--termru-warn-text);
  }

  .fwd .s.err {
    color: var(--termru-err-text);
  }

  .tags {
    flex-wrap: wrap;
    gap: 6px;
  }

  .tag {
    display: inline-flex;
    align-items: center;
    max-width: 100%;
    height: 22px;
    padding: 0 8px;
    border-radius: 6px;
    background: var(--termru-raised);
    color: var(--termru-text-2);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notes {
    display: -webkit-box;
    -webkit-line-clamp: 6;
    line-clamp: 6;
    -webkit-box-orient: vertical;
    margin: 0;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--termru-text-2);
  }
</style>
