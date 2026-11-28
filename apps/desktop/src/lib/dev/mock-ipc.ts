/**
 * DEV ONLY: an in-memory fake of the Tauri backend.
 *
 * `ipc-backend.ts` loads this module (by a dynamic import behind
 * `import.meta.env.DEV`) when the dev server is opened with `?mock`, so the
 * whole UI runs in a plain browser with realistic data. It is never part of
 * a production bundle.
 *
 * URL flags (combine with `&`):
 *   ?mock            the fixtures of the design frames
 *   &empty           no hosts and no recent hosts (the empty state)
 *   &error           `host_list` rejects (the error state)
 *   &demo            two tabs are restored and connected on startup:
 *                    atlas-prod in two panes, and relay-edge
 *   &theme=dark      the dark theme (or `light`)
 *   &edit[=name]     opens the host editor on start (default staging-api;
 *                    `&edit=new` opens it empty), because `#host-edit`
 *                    alone bounces back to the hosts page
 *   &folder=Europe   selects that folder on the folders page (by name)
 *   &identity=work-ed25519  selects that identity on the identities page
 *   &hostkey         an unknown-host-key prompt appears shortly after start
 *                    (`&hostkey=other`: the host is known under other key types;
 *                    `&hostkey=changed`: the stored key differs from the one presented)
 *   &connect[=archive]
 *                    the connect dialog opens for that host (default archive,
 *                    a password host with nothing stored)
 *   &sync[=conflicts|error]
 *                    this device is signed in to a sync server (`conflicts`:
 *                    with two conflicts to resolve, `error`: the last cycle failed)
 *   &authprompt      a keyboard-interactive question appears shortly after
 *                    start (`&authprompt=passphrase`: a key passphrase)
 *   &many[=N]        N more hosts (default 10 000, at most 100 000), in a few
 *                    folders, with recent connections on the first few: the
 *                    scroll, filter and window of a very large library
 *   &v2              data model v2 on top of the fixtures: a `bastion` host
 *                    (password, nothing stored) that atlas-prod jumps
 *                    through, four port forwards (one fails when its session
 *                    opens), environment variables and a startup command,
 *                    all approved on this device
 *   &approval        like `v2`, but the synced startup command, environment
 *                    and the exposed forward of atlas-prod are NOT approved on
 *                    this device: its sessions open without them and say so
 *   &review[=atlas-prod]
 *                    the approval review dialog opens for that host on start
 *                    (implies `approval`)
 *   &menu            the forwards menu of the toolbar opens as soon as a session
 *                    shows one (use with `&demo&v2` on `#sessions`)
 *   &palette[=text]  the command palette opens on start (with `text` typed in)
 *   &import          the "Import from SSH config" dialog opens on start
 *   &long            the worst realistic data on top of the fixtures: names
 *                    at the 128 character limit, a folder five levels deep with
 *                    long names, long identity names and comments, a 2 000
 *                    character comment, twelve tags, a one-letter host, a
 *                    non-Latin name, two hosts with the same name
 *
 * Every command of `ipc.ts` is implemented here; `dev-harness.test.ts`
 * fails when one is missing. Return shapes follow `types.ts` and the Rust
 * DTOs; session updates have the exact JSON shape of `SessionUpdate`
 * (`state/sessions.rs`), including the base64 `data` payload.
 */

import type {
  EventCallback,
  InvokeArgs,
  OpenFileOptions,
  UnlistenFn,
  WindowHandle,
} from '../ipc-backend';
import { execFieldsOf, hasExecFields } from '../forwards';
import { resolveRoute } from '../jump';
import { isIpv4Literal, isIpv6, isLoopbackBind } from '../validation';
import type {
  AuthPromptMessage,
  ConflictView,
  Group,
  Host,
  HostKeyPromptMessage,
  HostExecReview,
  HostKeyPromptRequest,
  GeneratedIdentity,
  Identity,
  IdentityBinding,
  Preferences,
  PublicKeyInfo,
  RecentHost,
  ForwardUpdate,
  SessionInfo,
  SessionStatus,
  SessionUpdate,
  SshConfigImportReport,
  SshConfigPreview,
  SyncOutcome,
  SyncStatus,
  Tag,
  ThemePreference,
  WorkspaceDoc,
} from '../types';

// ---------------------------------------------------------------------------
// Flags

export interface MockFlags {
  empty: boolean;
  error: boolean;
  demo: boolean;
  /** The host name the editor opens on (`new` = an empty editor), or null. */
  edit: string | null;
  /** The folder (by name) the folders page starts on, or null. */
  folder: string | null;
  /** The identity (by name) the identities page starts on, or null. */
  identity: string | null;
  hostkey: boolean;
  /** The host-key prompt says the host is known under other key types. */
  hostkeyOtherTypes: boolean;
  /** The host-key prompt is the "key changed" question. */
  hostkeyChanged: boolean;
  /** The host (by name) whose connect dialog opens on start, or null. */
  connect: string | null;
  /** Signed in to a sync server from the start (`conflicts`, `error`: with trouble). */
  sync: 'in' | 'conflicts' | 'error' | null;
  /** An authentication question appears on start (`kbd` or `passphrase`). */
  authprompt: 'kbd' | 'passphrase' | null;
  theme: ThemePreference | null;
  /** How many generated hosts to add (0 = none). */
  many: number;
  /** The worst-case fixtures are added. */
  long: boolean;
  /** Data model v2 fixtures (a bastion, forwards, environment, startup command). */
  v2: boolean;
  /** The synced exec-like fields of atlas-prod are not approved on this device (implies `v2`). */
  approval: boolean;
  /** The host (by name) whose approval review opens on start, or null. */
  review: string | null;
  /** The SSH config import dialog opens on start. */
  importConfig: boolean;
  /** The forwards menu of the Sessions toolbar opens once there is one. */
  menu: boolean;
  /** The command palette opens on start, with this text typed (empty: none), or null. */
  palette: string | null;
}

function readFlags(): MockFlags {
  const params = new URLSearchParams(typeof location === 'undefined' ? '' : location.search);
  const theme = params.get('theme');
  return {
    empty: params.has('empty'),
    error: params.has('error'),
    demo: params.has('demo'),
    edit: params.has('edit') ? params.get('edit') || 'staging-api' : null,
    folder: params.get('folder') || null,
    identity: params.get('identity') || null,
    hostkey: params.has('hostkey'),
    hostkeyOtherTypes: params.get('hostkey') === 'other',
    hostkeyChanged: params.get('hostkey') === 'changed',
    connect: params.has('connect') ? params.get('connect') || 'archive' : null,
    sync: params.has('sync')
      ? params.get('sync') === 'conflicts'
        ? 'conflicts'
        : params.get('sync') === 'error'
          ? 'error'
          : 'in'
      : null,
    authprompt: params.has('authprompt')
      ? params.get('authprompt') === 'passphrase'
        ? 'passphrase'
        : 'kbd'
      : null,
    theme: theme === 'dark' || theme === 'light' || theme === 'system' ? theme : null,
    many: params.has('many')
      ? Math.min(100_000, Math.max(1, Number.parseInt(params.get('many') || '', 10) || 10_000))
      : 0,
    long: params.has('long'),
    approval: params.has('approval') || params.has('review'),
    v2: params.has('v2') || params.has('approval') || params.has('review'),
    review: params.has('review') ? params.get('review') || 'atlas-prod' : null,
    importConfig: params.has('import'),
    menu: params.has('menu'),
    palette: params.has('palette') ? (params.get('palette') ?? '') : null,
  };
}

// ---------------------------------------------------------------------------
// Errors

/** What a rejected Tauri command carries: the serialized `CommandError`. */
function commandError(message: string): never {
  throw { message };
}

// ---------------------------------------------------------------------------
// Fixtures (the data of the design frames)

/** A stable, UUID-shaped id for a fixture. */
function fixtureId(n: number): string {
  return `00000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
}

const ID = {
  atlas: fixtureId(0x101),
  relay: fixtureId(0x102),
  staging: fixtureId(0x103),
  build: fixtureId(0x104),
  archive: fixtureId(0x105),
  bastion: fixtureId(0x106),
  production: fixtureId(0x201),
  europe: fixtureId(0x202),
  edge: fixtureId(0x203),
  sandbox: fixtureId(0x204),
  work: fixtureId(0x301),
  buildKey: fixtureId(0x302),
};

const CREATED = '2026-09-14T08:30:00Z';
const UPDATED = '2026-10-02T11:05:00Z';

function fixtureHost(
  n: number,
  fields: Pick<Host, 'id' | 'name' | 'address' | 'port' | 'username' | 'auth' | 'group_id'> &
    Partial<Host>,
): Host {
  return {
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 30,
    sort_order: n,
    created_at: CREATED,
    updated_at: UPDATED,
    deleted: false,
    ...fields,
  };
}

function fixtureGroup(n: number, id: string, name: string, parent: string | null): Group {
  return {
    id,
    name,
    parent_id: parent,
    sort_order: n,
    created_at: CREATED,
    updated_at: CREATED,
    deleted: false,
  };
}

function fixtureIdentity(id: string, name: string, comment: string, fingerprint: string): Identity {
  return {
    id,
    name,
    comment,
    fingerprint,
    created_at: CREATED,
    updated_at: CREATED,
    deleted: false,
  };
}

function fixtureHosts(): Host[] {
  return [
    fixtureHost(0, {
      id: ID.atlas,
      name: 'atlas-prod',
      address: '192.0.2.10',
      port: 22,
      username: 'ops',
      auth: { type: 'private_key', identity_id: ID.work },
      group_id: ID.europe,
      favorite: true,
      description: 'Atlas API and workers',
    }),
    fixtureHost(1, {
      id: ID.relay,
      name: 'relay-edge',
      address: '192.0.2.20',
      port: 22,
      username: 'ops',
      auth: { type: 'private_key', identity_id: ID.work },
      group_id: ID.edge,
      favorite: true,
      description: 'Edge relay',
    }),
    fixtureHost(2, {
      id: ID.staging,
      name: 'staging-api',
      address: '198.51.100.12',
      port: 22,
      username: 'dev',
      auth: { type: 'password' },
      group_id: ID.sandbox,
      favorite: true,
      description: 'Pre-release API · shared test environment',
    }),
    fixtureHost(3, {
      id: ID.build,
      name: 'build-runner',
      address: '198.51.100.24',
      port: 2222,
      username: 'ci',
      auth: { type: 'private_key', identity_id: ID.buildKey },
      group_id: ID.sandbox,
    }),
    fixtureHost(4, {
      id: ID.archive,
      name: 'archive',
      address: '203.0.113.30',
      port: 22,
      username: 'ops',
      auth: { type: 'password' },
      group_id: null,
    }),
  ];
}

/**
 * `?v2`: the data model of ADR 0010 on top of the fixtures. atlas-prod is
 * reached through a password-only bastion (nothing stored for it, so the
 * connect dialog asks), forwards three ports and a SOCKS proxy (one of them
 * binds all interfaces, one is refused), sets two variables and starts tmux.
 */
function withV2(hosts: Host[]): Host[] {
  const bastion = fixtureHost(5, {
    id: ID.bastion,
    name: 'bastion',
    address: 'bastion.corp.example',
    port: 2222,
    username: 'ops',
    auth: { type: 'password' },
    group_id: null,
    description: 'Jump host for the production network',
  });
  return [
    ...hosts.map((host) =>
      host.id === ID.atlas
        ? {
            ...host,
            jump_chain: [ID.bastion],
            forwards: [
              {
                kind: 'local' as const,
                bind_address: '127.0.0.1',
                bind_port: 5433,
                target_host: 'db.internal',
                target_port: 5432,
                enabled: true,
              },
              {
                kind: 'dynamic' as const,
                bind_port: 1080,
                enabled: true,
              },
              {
                kind: 'local' as const,
                bind_address: '0.0.0.0',
                bind_port: 9090,
                target_host: 'metrics.internal',
                target_port: 9090,
                enabled: true,
              },
              {
                kind: 'remote' as const,
                bind_port: 80,
                target_host: 'localhost',
                target_port: 3000,
                enabled: true,
              },
            ],
            env: [
              { name: 'LANG', value: 'en_US.UTF-8' },
              { name: 'TERMRU_PROFILE', value: 'ops' },
            ],
            startup_command: 'tmux new -A -s atlas',
            agent_forward: false,
          }
        : host,
    ),
    bastion,
  ];
}

function fixtureGroups(): Group[] {
  return [
    fixtureGroup(0, ID.production, 'Production', null),
    fixtureGroup(1, ID.europe, 'Europe', ID.production),
    fixtureGroup(2, ID.edge, 'Edge', ID.production),
    fixtureGroup(3, ID.sandbox, 'Sandbox', null),
  ];
}

function fixtureIdentities(): Identity[] {
  return [
    fixtureIdentity(
      ID.work,
      'work-ed25519',
      'Work laptop key',
      'SHA256:Zk3qN0yQm1v8cTt2uRk5GxJ7aWb4HsD9eLp6OiXnYfE',
    ),
    fixtureIdentity(
      ID.buildKey,
      'build-ed25519',
      'CI runner deploy key',
      'SHA256:b7Qd2Fh9UoPz4LmEw1KsVx8TnYc3RjA6GiHt5MaOe0u',
    ),
  ];
}

function defaultPreferences(): Preferences {
  return {
    theme: 'dark',
    terminal: {
      font_family: 'Geist Mono, monospace',
      font_size: 13,
      scrollback: 10_000,
      cursor_blink: true,
      cursor_style: 'block',
      copy_on_select: false,
      line_height: 1.3,
      confirm_multiline_paste: true,
    },
    keybindings: {},
    reduced_motion: 'system',
    focus_visibility: false,
    routine_notices: true,
  };
}

function signedOutSync(): SyncStatus {
  return {
    settings: {
      server_url: '',
      email: '',
      device_name: 'Linux workstation',
      device_id: fixtureId(0x401),
    },
    signed_in: false,
    pending: 0,
    conflicts: 0,
    cursor: 0,
    last_sync_at: null,
    last_error: null,
  };
}

/** This device already signed in (`&sync`): the state of the Sync page with an account. */
function seedSync(flags: MockFlags): SyncStatus {
  const out = signedOutSync();
  if (!flags.sync) return out;
  const conflicts = flags.sync === 'conflicts' ? 2 : 0;
  return {
    ...out,
    settings: flags.long
      ? {
          server_url:
            'https://sync.platform.northwind-industries-holdings.example.com/termru/v1/workspaces/platform-infrastructure',
          email: 'bartholomew.fitzgerald@northwind-industries-holdings.example.com',
          device_name: 'Aleksandra’s ThinkPad X1 Carbon Gen 11 (platform infrastructure team)',
          device_id: out.settings.device_id,
        }
      : {
          server_url: 'https://sync.atlas.example',
          email: 'ops@atlas.example',
          device_name: 'Linux workstation',
          device_id: out.settings.device_id,
        },
    signed_in: true,
    pending: flags.sync === 'conflicts' ? 3 : 0,
    conflicts,
    cursor: 142,
    last_sync_at: new Date(todayAt(9, 18)).toISOString(),
    last_error:
      flags.sync === 'error'
        ? 'could not reach https://sync.atlas.example (connection refused)'
        : null,
  };
}

/** Two records that changed on this device and on the server (`&sync=conflicts`). */
function fixtureConflicts(): ConflictView[] {
  const at = new Date(todayAt(9, 12)).toISOString();
  return [
    {
      record_id: ID.staging,
      kind: 'host',
      server_rev: 141,
      observed_at: at,
      local: { id: ID.staging, name: 'staging-api', address: '198.51.100.12', port: 22 },
      remote: { id: ID.staging, name: 'staging-api', address: '198.51.100.40', port: 2222 },
    },
    {
      record_id: ID.archive,
      kind: 'host',
      server_rev: 139,
      observed_at: at,
      local: { id: ID.archive, name: 'archive', description: 'Cold storage' },
      remote: null,
    },
  ];
}

/** Today at the given local time (ms since the epoch). */
function todayAt(hours: number, minutes: number): number {
  const date = new Date();
  date.setHours(hours, minutes, 0, 0);
  return date.getTime();
}

/** Yesterday in the afternoon. */
function yesterday(): number {
  const date = new Date();
  date.setDate(date.getDate() - 1);
  date.setHours(17, 42, 0, 0);
  return date.getTime();
}

/** The latest 18 October that is not in the future. */
function eighteenthOfOctober(): number {
  const now = new Date();
  const date = new Date(now.getFullYear(), 9, 18, 14, 5, 0, 0);
  if (date.getTime() > now.getTime()) date.setFullYear(now.getFullYear() - 1);
  return date.getTime();
}

/** The restored layout of `?demo`: atlas-prod in two panes, and relay-edge. */
function demoLayout(): WorkspaceDoc {
  const pane = (n: number) => fixtureId(0x500 + n);
  return {
    version: 1,
    windows: [
      {
        id: 'window-main',
        tabs: [
          {
            id: fixtureId(0x601),
            title: 'atlas-prod',
            root: {
              split: {
                direction: 'columns',
                ratio: 0.5,
                first: { leaf: { pane: pane(1), host_id: ID.atlas } },
                second: { leaf: { pane: pane(2), host_id: ID.atlas } },
              },
            },
            active_pane: pane(1),
          },
          {
            id: fixtureId(0x602),
            title: 'relay-edge',
            root: { leaf: { pane: pane(3), host_id: ID.relay } },
            active_pane: pane(3),
          },
        ],
        active_tab: fixtureId(0x601),
      },
    ],
    active_window: 'window-main',
    last_route: 'sessions',
  };
}

// ---------------------------------------------------------------------------
// Stress fixtures (`&long`, `&many`)

interface LongFixtures {
  hosts: Host[];
  groups: Group[];
  identities: Identity[];
  tags: Tag[];
  recents: [string, number][];
}

/**
 * The worst data a real library produces, within the limits the backend
 * accepts (`validation.ts`: name 128, address 253, username 128, comment
 * 2 000, 32 tags a host, folders nested at most 8 deep).
 */
function longFixtures(): LongFixtures {
  const n = (offset: number) => fixtureId(0x1000 + offset);
  const groups = [
    fixtureGroup(10, n(1), 'Northwind Industries Holdings', null),
    fixtureGroup(11, n(2), 'Platform Infrastructure and Site Reliability', n(1)),
    fixtureGroup(12, n(3), 'Production (Frankfurt, eu-central-1)', n(2)),
    fixtureGroup(13, n(4), 'Customer-facing services — blue/green canary', n(3)),
    fixtureGroup(14, n(5), 'Legacy billing reconciliation (decommission pending Q3)', n(4)),
    fixtureGroup(15, n(6), '東京リージョン', null),
  ];
  const identities = [
    fixtureIdentity(
      n(20),
      'northwind-platform-infrastructure-deploy-ed25519-2026',
      'Deploy key for the platform infrastructure team’s production bastion hosts (rotated every quarter, see runbook 17)',
      'SHA256:Zk3qN0yQm1v8cTt2uRk5GxJ7aWb4HsD9eLp6OiXnYfE',
    ),
    fixtureIdentity(n(21), 'k', '', 'SHA256:b7Qd2Fh9UoPz4LmEw1KsVx8TnYc3RjA6GiHt5MaOe0u'),
  ];
  const now = new Date().toISOString();
  const tags: Tag[] = [
    'design',
    'frontend',
    'q3',
    'urgent',
    'needs-review',
    'customer-feedback-from-enterprise-onboarding',
    'eu',
    'pci',
    'blue',
    'green',
    'canary',
    'on-call',
  ].map((name, index) => ({
    id: n(100 + index),
    name,
    color: null,
    created_at: now,
    updated_at: now,
    deleted: false,
  }));
  const longName =
    'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment-primary';
  const limitName =
    `${'analytics-warehouse-replica-'.repeat(5)}primary-eu-central-1-blue-green`.slice(0, 128);
  const hosts = [
    fixtureHost(100, {
      id: n(40),
      name: longName,
      address:
        'ip-10-128-44-203.eu-central-1.compute.internal.northwind-industries-holdings.example.com',
      port: 2222,
      username: 'svc-deploy-automation',
      auth: { type: 'private_key', identity_id: n(20) },
      group_id: n(5),
      favorite: true,
      tags: tags.map((tag) => tag.id),
      description: 'Primary reporting service for the customer-facing dashboards. '
        .repeat(31)
        .slice(0, 2000),
    }),
    fixtureHost(101, {
      id: n(41),
      name: limitName,
      address:
        'a-very-long-hostname-with-many-labels.region-one.zone-b.platform.internal.northwind-industries-holdings.example.test',
      port: 22,
      username: 'svc-platform-infrastructure-deploy-automation-readonly-metrics-exporter',
      auth: { type: 'agent' },
      group_id: n(4),
    }),
    fixtureHost(102, {
      id: n(42),
      name: 'x',
      address: 'a.test',
      port: 22,
      username: 'u',
      auth: { type: 'password' },
      group_id: null,
    }),
    fixtureHost(103, {
      id: n(43),
      name: 'Åsa’s staging 東京',
      address: '2001:db8:85a3:8d3:1319:8a2e:370:7348',
      port: 65535,
      username: 'ops',
      auth: { type: 'private_key', identity_id: n(21) },
      group_id: n(6),
    }),
    // Two hosts with the same name: the address is what tells them apart.
    fixtureHost(104, {
      id: n(44),
      name: 'archive',
      address: '203.0.113.31',
      port: 22,
      username: 'ops',
      auth: { type: 'password' },
      group_id: null,
    }),
  ];
  const recents: [string, number][] = [
    [n(40), todayAt(8, 2)],
    [n(41), yesterday()],
    [n(43), eighteenthOfOctober()],
  ];
  return { hosts, groups, identities, tags, recents };
}

/** `count` generated hosts, in the naming pattern of a real fleet. */
function bulkHosts(count: number): { hosts: Host[]; recents: [string, number][] } {
  const regions = ['eu', 'us', 'ap'];
  const roles = ['web', 'api', 'db', 'cache', 'worker', 'batch'];
  const folders = [ID.production, ID.europe, ID.edge, ID.sandbox, null];
  const hosts: Host[] = [];
  const recents: [string, number][] = [];
  const base = Date.now() - 3 * 24 * 3_600_000;
  for (let i = 0; i < count; i++) {
    const id = fixtureId(0x100000 + i);
    hosts.push(
      fixtureHost(1000 + i, {
        id,
        name: `${roles[i % roles.length]}-${regions[i % regions.length]}-${String(i).padStart(5, '0')}`,
        address: `10.${(i >> 8) & 255}.${i & 255}.${(i * 7) % 250}`,
        port: i % 17 === 0 ? 2222 : 22,
        username: i % 5 === 0 ? 'ops' : 'deploy',
        auth: i % 3 === 0 ? { type: 'password' } : { type: 'agent' },
        group_id: folders[i % folders.length],
        favorite: i % 97 === 0,
      }),
    );
    if (i < 40) recents.push([id, base + i * 3_600_000]);
  }
  return { hosts, recents };
}

// ---------------------------------------------------------------------------
// State

interface MockState {
  hosts: Host[];
  groups: Group[];
  tags: Tag[];
  identities: Identity[];
  /** identity id -> key file path (device-local). */
  bindings: Map<string, string>;
  /** identity id -> the key a generated identity was made with. */
  keyInfo: Map<string, PublicKeyInfo>;
  /** host id -> last connection time (ms). */
  recents: Map<string, number>;
  /** `${host id}:${kind}` of the secrets "in the credential store". */
  secrets: Set<string>;
  prefs: Preferences;
  sync: SyncStatus;
  conflicts: ConflictView[];
  layout: WorkspaceDoc | null;
  /** host id -> the hash of the exec-like fields this device approved (device-local). */
  execApproved: Map<string, string>;
}

function seed(flags: MockFlags): MockState {
  const prefs = defaultPreferences();
  if (flags.theme) prefs.theme = flags.theme;
  const hosts = flags.empty ? [] : flags.v2 ? withV2(fixtureHosts()) : fixtureHosts();
  const groups = fixtureGroups();
  const identities = fixtureIdentities();
  const tags: Tag[] = [];
  const recents = new Map<string, number>();
  if (!flags.empty) {
    recents.set(ID.staging, todayAt(9, 18));
    recents.set(ID.build, yesterday());
    recents.set(ID.archive, eighteenthOfOctober());
    if (flags.long) {
      const extra = longFixtures();
      hosts.push(...extra.hosts);
      groups.push(...extra.groups);
      identities.push(...extra.identities);
      tags.push(...extra.tags);
      for (const [id, at] of extra.recents) recents.set(id, at);
    }
    if (flags.many > 0) {
      const bulk = bulkHosts(flags.many);
      hosts.push(...bulk.hosts);
      for (const [id, at] of bulk.recents) recents.set(id, at);
    }
  }
  return {
    hosts,
    groups,
    tags,
    identities,
    bindings: new Map([
      [ID.work, '/home/demo/.ssh/work_ed25519'],
      [ID.buildKey, '/home/demo/.ssh/build_ed25519'],
    ]),
    keyInfo: new Map(),
    recents,
    // The password of staging-api is "stored on this device"; archive asks.
    secrets: new Set(flags.empty ? [] : [`${ID.staging}:password`]),
    prefs,
    sync: seedSync(flags),
    conflicts: flags.sync === 'conflicts' ? fixtureConflicts() : [],
    layout: flags.demo ? demoLayout() : null,
    // What was saved here is approved: every host starts that way, unless the
    // harness plays "these arrived through sync".
    execApproved: new Map(
      hosts
        .filter((host) => !(flags.approval && host.id === ID.atlas))
        .map((host) => [host.id, mockExecHash(host)]),
    ),
  };
}

/** The mock's stand-in for `Host::exec_hash`: the same fields, a different hash function. */
function mockExecHash(host: Host): string {
  const text = JSON.stringify(execFieldsOf(host));
  let hash = 5381;
  for (let index = 0; index < text.length; index += 1) {
    hash = ((hash << 5) + hash + text.charCodeAt(index)) | 0;
  }
  return `mock-${(hash >>> 0).toString(16).padStart(8, '0')}-${text.length}`;
}

function execStatusOf(host: Host): { hash: string; approved: boolean } {
  const hash = mockExecHash(host);
  return { hash, approved: !hasExecFields(host) || state.execApproved.get(host.id) === hash };
}

let flags: MockFlags = readFlags();
let state: MockState = seed(flags);

// ---------------------------------------------------------------------------
// Channels and events

/** What the session code needs from a channel argument. */
interface ChannelLike {
  onmessage: (update: SessionUpdate) => void;
}

let nextChannelId = 1;

/** The mock of Tauri's `Channel`: `onmessage` plus the id sent to a command. */
export class Channel<T = unknown> {
  readonly id = nextChannelId++;
  onmessage: (response: T) => void;

  constructor(onmessage?: (response: T) => void) {
    this.onmessage = onmessage ?? (() => {});
  }

  toJSON(): string {
    return `__CHANNEL__:${this.id}`;
  }
}

const listeners = new Map<string, Set<EventCallback<never>>>();
let nextEventId = 1;
let hostKeyPromptScheduled = false;
let authPromptScheduled = false;

/** Pending host-key and authentication prompts, by id. */
const pendingPrompts = new Set<string>();

/** The mock of `listen`. */
export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const set = listeners.get(event) ?? new Set<EventCallback<never>>();
  set.add(handler as EventCallback<never>);
  listeners.set(event, set);
  if (event === 'host-key-prompt' && flags.hostkey && !hostKeyPromptScheduled) {
    hostKeyPromptScheduled = true;
    setTimeout(() => {
      const id = `mock-prompt-${nextEventId}`;
      const request: HostKeyPromptRequest = flags.hostkeyChanged
        ? {
            state: 'changed',
            host: '192.0.2.10',
            port: 22,
            old: {
              key_type: 'ssh-ed25519',
              fingerprint: 'SHA256:Zk3qN0yQm1v8cTt2uRk5GxJ7aWb4HsD9eLp6OiXnYfE',
            },
            new: {
              key_type: 'ssh-ed25519',
              fingerprint: 'SHA256:Vq8m2nT0cYb5LhZr7WdEa1KsXo4GuPj9RfIt3NxMe6A',
            },
          }
        : {
            state: 'unknown',
            host: '192.0.2.10',
            port: 22,
            key: {
              key_type: 'ssh-ed25519',
              fingerprint: 'SHA256:Vq8m2nT0cYb5LhZr7WdEa1KsXo4GuPj9RfIt3NxMe6A',
            },
            ...(flags.hostkeyOtherTypes
              ? { other_key_types: ['ssh-rsa', 'ecdsa-sha2-nistp256'] }
              : {}),
          };
      pendingPrompts.add(id);
      const message: HostKeyPromptMessage = { id, session_id: 'mock-session', request };
      emit('host-key-prompt', message);
    }, 600);
  }
  if (event === 'auth-prompt' && flags.authprompt && !authPromptScheduled) {
    authPromptScheduled = true;
    setTimeout(() => {
      const id = `mock-auth-prompt-${nextEventId}`;
      const message: AuthPromptMessage = {
        id,
        session_id: 'mock-session',
        host_id: ID.atlas,
        request:
          flags.authprompt === 'passphrase'
            ? {
                kind: {
                  type: 'passphrase',
                  identity: { id: ID.work, name: 'work laptop' },
                  retry: false,
                },
                name: '',
                instructions: '',
                prompts: [{ text: 'Passphrase for work laptop', echo: false }],
              }
            : {
                kind: { type: 'keyboard_interactive' },
                name: 'Two-factor authentication',
                instructions: 'Enter the code from your authenticator app.',
                prompts: [
                  { text: 'Verification code: ', echo: true },
                  { text: 'PIN: ', echo: false },
                ],
              },
      };
      pendingPrompts.add(id);
      emit('auth-prompt', message);
    }, 600);
  }
  return Promise.resolve(() => {
    set.delete(handler as EventCallback<never>);
  });
}

/** Delivers an event to its listeners (the mock of a backend `emit`). */
function emit(event: string, payload: unknown): void {
  for (const handler of listeners.get(event) ?? []) {
    (handler as EventCallback<unknown>)({ event, id: nextEventId++, payload });
  }
}

// ---------------------------------------------------------------------------
// Sessions

/** How a session behaves once it is connected. */
type SessionVariant = 'bash' | 'journal' | 'relay' | 'generic';

interface MockSession {
  id: string;
  hostId: string;
  variant: SessionVariant;
  status: SessionStatus;
  views: ChannelLike[];
  /** Everything sent so far (the replay buffer of `session_attach`). */
  buffer: string;
  /** The current shell line (what the user has typed). */
  line: string;
  cwd: string;
  /** The `journalctl -f` stream: it ignores input until Ctrl C. */
  following: boolean;
  timers: Set<ReturnType<typeof setTimeout>>;
  /** What became of the port forwards (cleared by every status but `ready`). */
  forwards: ForwardUpdate[];
  /** The session runs without the synced exec-like fields (not approved here). */
  execPending: boolean;
}

const sessions = new Map<string, MockSession>();
let nextSessionId = 1;
/** How many sessions each host has opened; it picks the transcript. */
const openCounts = new Map<string, number>();

/** The variants a host cycles through, one per session it opens. */
function variantsFor(host: Host): SessionVariant[] {
  if (host.id === ID.atlas) return ['bash', 'journal'];
  if (host.id === ID.relay) return ['relay'];
  return ['generic'];
}

function shortName(host: Host): string {
  return host.name.split('-')[0] || host.name;
}

function ansi(code: string, text: string): string {
  return `\x1b[${code}m${text}\x1b[0m`;
}

/** The shell prompt: `ops@atlas /srv/atlas $ ` with the frame's colours. */
function shellPrompt(host: Host, cwd: string): string {
  return `${ansi('1;36', `${host.username}@${shortName(host)}`)} ${ansi('37', cwd)} $ `;
}

/** The scripted output of a session, in chunks. */
function transcript(session: MockSession, host: Host): string[] {
  const p = (cwd: string) => shellPrompt(host, cwd);
  const ok = (text: string) => `${ansi('90', '[ok]')} ${text}\r\n`;
  switch (session.variant) {
    case 'bash':
      session.cwd = '/srv/atlas';
      return [
        '\r\n',
        `${p('~')}cd /srv/atlas\r\n`,
        `${p('/srv/atlas')}./deploy.sh --check\r\n`,
        `${ansi('37', 'Checking release 1.8.4…')}\r\n`,
        ok('Environment configuration'),
        ok('Database connection'),
        ok('Service health endpoint'),
        ok('Disk space: 28G available'),
        `${ansi('32', 'All checks passed. No changes made.')}\r\n\r\n`,
        `${p('/srv/atlas')}git -C current log -1 --oneline\r\n`,
        `${ansi('35', 'a6b8d21')} fix: handle empty job queue\r\n\r\n`,
        p('/srv/atlas'),
      ];
    case 'journal': {
      const line = (time: string, text: string, color?: string) =>
        `${color ? ansi(color, `${time} atlas node[1428]: ${text}`) : `${time} atlas node[1428]: ${text}`}\r\n`;
      session.following = true;
      return [
        '\r\n',
        `${p('~')}journalctl -u atlas-api -n 12 -f\r\n`,
        '-- Logs begin at Fri 2026-10-23 09:00:00 UTC --\r\n',
        line('09:38:02', 'Starting Atlas API'),
        line('09:38:02', '√ Database ready', '32'),
        line('09:38:02', '√ Listening on :3000', '32'),
        line('09:42:08', 'GET /health 200 2ms'),
        line('09:42:15', 'GET /v1/jobs 200 12ms'),
        line('09:43:01', 'GET /health 200 1ms'),
        line('09:44:09', 'WARN slow query 204ms', '33'),
        line('09:44:10', 'GET /v1/jobs 200 14ms'),
        line('09:45:01', 'GET /health 200 1ms'),
        `\r\n${ansi('90', 'Following new entries · Ctrl C to stop')}\r\n`,
      ];
    }
    case 'relay':
      session.cwd = '~';
      return [
        '\r\n',
        `${p('~')}systemctl status relay-edge --no-pager\r\n`,
        `${ansi('32', '●')} relay-edge.service - Relay edge proxy\r\n`,
        '     Loaded: loaded (/etc/systemd/system/relay-edge.service; enabled)\r\n',
        `     Active: ${ansi('32', 'active (running)')} since Fri 2026-10-23 06:12:40 UTC\r\n`,
        '   Main PID: 912 (relayd)\r\n',
        '      Tasks: 8 (limit: 4591)\r\n',
        '     Memory: 61.4M\r\n\r\n',
        `${p('~')}`,
      ];
    default:
      session.cwd = '~';
      return [
        `${ansi('37', 'Last login: Fri Oct 23 09:42:16 2026 from 192.0.2.50')}\r\n`,
        'Welcome to Ubuntu 24.04.1 LTS (GNU/Linux 6.8.0 x86_64)\r\n\r\n',
        p('~'),
      ];
  }
}

function toBase64(text: string): string {
  let binary = '';
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function send(session: MockSession, update: SessionUpdate): void {
  for (const view of session.views) view.onmessage(update);
}

function sendData(session: MockSession, text: string): void {
  if (text.length === 0) return;
  session.buffer += text;
  send(session, { kind: 'data', base64: toBase64(text) });
}

function setStatus(session: MockSession, status: SessionStatus): void {
  session.status = status;
  // The forwards live and die with the connection.
  if (status.state !== 'ready') session.forwards = [];
  send(session, { kind: 'status', status });
}

/**
 * What the engine reports once the session is ready: every enabled forward
 * is listening, except one on a privileged port (refused). A session that
 * lacks the approval of this device skips the forwards that are exposed.
 */
function startForwards(session: MockSession, host: Host): void {
  (host.forwards ?? []).forEach((forward, id) => {
    if (!forward.enabled) return;
    if (session.execPending && !isLoopbackBind(forward.bind_address)) return;
    const update: ForwardUpdate =
      forward.bind_port < 1024
        ? {
            state: 'failed',
            id,
            kind: forward.kind,
            reason:
              forward.kind === 'remote'
                ? 'the server refused the forward (administratively prohibited)'
                : 'Permission denied (os error 13)',
          }
        : {
            state: 'listening',
            id,
            kind: forward.kind,
            address:
              forward.bind_address ?? (forward.kind === 'remote' ? 'localhost' : '127.0.0.1'),
            port: forward.bind_port,
          };
    session.forwards = [...session.forwards.filter((entry) => entry.id !== id), update];
    send(session, { kind: 'forward', forward: update });
  });
}

function later(session: MockSession, ms: number, run: () => void): void {
  const timer = setTimeout(() => {
    session.timers.delete(timer);
    run();
  }, ms);
  session.timers.add(timer);
}

function hostOf(id: string): Host {
  const host = state.hosts.find((entry) => entry.id === id && !entry.deleted);
  if (!host) return commandError(`no host with id ${id}`);
  return host;
}

function sessionOf(id: string): MockSession {
  const session = sessions.get(id);
  if (!session) return commandError(`no session with id ${id}`);
  return session;
}

function createSession(host: Host, variant: SessionVariant): MockSession {
  const session: MockSession = {
    id: `mock-session-${nextSessionId++}`,
    hostId: host.id,
    variant,
    status: { state: 'connecting' },
    views: [],
    buffer: '',
    line: '',
    cwd: '~',
    following: false,
    timers: new Set(),
    forwards: [],
    execPending: false,
  };
  sessions.set(session.id, session);
  return session;
}

/** Connecting, authenticating, ready, then the scripted output. */
function startSession(session: MockSession): void {
  const host = hostOf(session.hostId);
  session.line = '';
  session.following = false;
  setStatusLater(session, 0, { state: 'connecting' });
  setStatusLater(session, 120, { state: 'authenticating' });
  later(session, 320, () => {
    setStatus(session, { state: 'ready' });
    state.recents.set(host.id, Date.now());
    later(session, 40, () => startForwards(session, host));
  });
  transcript(session, host).forEach((chunk, index) => {
    later(session, 420 + index * 35, () => sendData(session, chunk));
  });
}

function setStatusLater(session: MockSession, ms: number, status: SessionStatus): void {
  later(session, ms, () => setStatus(session, status));
}

function openSession(
  hostId: string,
  view: ChannelLike,
  hopSecrets: Record<string, { password?: string }> = {},
  inherited = false,
): MockSession {
  const host = hostOf(hostId);
  // Like the backend: a jump host that signs in with a password needs one,
  // typed now or in the credential store.
  for (const hopId of inherited ? [] : (host.jump_chain ?? [])) {
    const hop = state.hosts.find((entry) => entry.id === hopId);
    if (hop?.auth.type === 'password' && !hopSecrets[hopId]?.password) {
      if (!state.secrets.has(`${hopId}:password`)) {
        commandError(`jump host: host "${hop.name}" requires a password`);
      }
    }
  }
  const variants = variantsFor(host);
  const count = openCounts.get(host.id) ?? 0;
  openCounts.set(host.id, count + 1);
  const session = createSession(host, variants[count % variants.length]);
  session.views.push(view);
  if (!execStatusOf(host).approved) {
    session.execPending = true;
    send(session, { kind: 'exec_pending' });
  }
  startSession(session);
  return session;
}

function closeSession(session: MockSession): void {
  for (const timer of session.timers) clearTimeout(timer);
  session.timers.clear();
  sessions.delete(session.id);
}

/** Absolute path of `cd <target>` from `cwd` (`~` is the home directory). */
function resolvePath(cwd: string, target: string): string {
  if (target === '' || target === '~') return '~';
  const parts = target.startsWith('/') ? [] : cwd === '~' ? [] : cwd.split('/').filter(Boolean);
  for (const part of target.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') parts.pop();
    else parts.push(part);
  }
  return parts.length === 0 ? '/' : `/${parts.join('/')}`;
}

/** The output of a typed command (with terminal line endings). */
function runCommand(session: MockSession, host: Host, input: string): string {
  const [name = '', ...rest] = input.trim().split(/\s+/);
  switch (name) {
    case '':
      return '';
    case 'cd':
      session.cwd = resolvePath(session.cwd, rest[0] ?? '');
      return '';
    case 'pwd':
      return `${session.cwd === '~' ? `/home/${host.username}` : session.cwd}\r\n`;
    case 'whoami':
      return `${host.username}\r\n`;
    case 'hostname':
      return `${shortName(host)}\r\n`;
    case 'echo':
      return `${rest.join(' ')}\r\n`;
    case 'date':
      return `${new Date().toUTCString()}\r\n`;
    case 'clear':
      return '\x1b[2J\x1b[H';
    case 'ls':
      return (
        [
          ansi('34', 'current/'),
          ansi('34', 'releases/'),
          'compose.yml',
          ansi('32', 'deploy.sh'),
        ].join('  ') + '\r\n'
      );
    default:
      return `bash: ${name}: command not found\r\n`;
  }
}

/** The typed bytes of `session_write`: echo them like a shell would. */
function typeInto(session: MockSession, bytes: number[]): void {
  const host = hostOf(session.hostId);
  // Arrow keys and other escape sequences are not echoed.
  const text = new TextDecoder()
    .decode(new Uint8Array(bytes))
    .replace(/\x1b\[[0-9;]*[A-Za-z~]/g, '');
  for (const char of text) {
    if (session.following) {
      if (char === '\x03') {
        session.following = false;
        sendData(session, `^C\r\n${shellPrompt(host, session.cwd)}`);
      }
      continue;
    }
    if (char === '\r' || char === '\n') {
      const command = session.line;
      session.line = '';
      sendData(session, '\r\n');
      if (/^(exit|logout)$/.test(command.trim())) {
        sendData(session, 'logout\r\n');
        send(session, { kind: 'exit', code: 0 });
        setStatus(session, { state: 'closed', reason: 'the remote shell exited' });
        return;
      }
      sendData(session, runCommand(session, host, command) + shellPrompt(host, session.cwd));
    } else if (char === '\x7f' || char === '\b') {
      if (session.line.length > 0) {
        session.line = session.line.slice(0, -1);
        sendData(session, '\b \b');
      }
    } else if (char === '\x03') {
      session.line = '';
      sendData(session, `^C\r\n${shellPrompt(host, session.cwd)}`);
    } else if (char >= ' ') {
      session.line += char;
      sendData(session, char);
    }
  }
}

/**
 * The startup flags that act on the app's stores (`?demo`, `?edit`). They run
 * once the app has loaded its hosts and asked for the layout, which is the
 * last step of its startup. The stores are imported on demand: a static
 * import would be a cycle with the seam, which awaits this module.
 */
let startupScheduled = false;

function scheduleStartup(): void {
  const acts =
    flags.demo ||
    flags.edit !== null ||
    flags.folder !== null ||
    flags.identity !== null ||
    flags.connect !== null ||
    flags.review !== null ||
    flags.importConfig ||
    flags.palette !== null ||
    flags.menu;
  if (startupScheduled || !acts) return;
  startupScheduled = true;
  // After the app has applied the layout it just received.
  setTimeout(() => {
    void (async () => {
      if (flags.demo) await connectRestoredPanes();
      if (flags.edit !== null) await openEditor(flags.edit);
      if (flags.connect !== null) await openConnect(flags.connect);
      if (flags.review !== null) await openReview(flags.review);
      if (flags.importConfig) (await import('../stores/ssh-import.svelte')).sshImport.show();
      if (flags.menu) openForwardsMenu();
      if (flags.palette !== null) await openPalette(flags.palette);
      await selectEntities();
    })();
  }, 0);
}

/**
 * A session that is already connected, with its whole transcript in the
 * replay buffer and no view yet: the terminal that shows it attaches (like
 * the window of a moved tab) and receives the replay.
 */
function connectedSession(host: Host): MockSession {
  const variants = variantsFor(host);
  const count = openCounts.get(host.id) ?? 0;
  openCounts.set(host.id, count + 1);
  const session = createSession(host, variants[count % variants.length]);
  session.status = { state: 'ready' };
  session.execPending = !execStatusOf(host).approved;
  startForwards(session, host);
  for (const chunk of transcript(session, host)) sendData(session, chunk);
  return session;
}

/**
 * `?demo`: the restored panes count as connected, so the hosts page shows
 * live sessions before the Sessions page is ever opened (the terminals
 * attach to them when that page mounts). Like a real connection, they also
 * make their hosts the most recent ones (the order of frame 1).
 */
async function connectRestoredPanes(): Promise<void> {
  const [{ sessions: store }, { hosts }] = await Promise.all([
    import('../stores/sessions.svelte'),
    import('../stores/hosts.svelte'),
  ]);
  const connectedAt = new Map([
    [ID.atlas, todayAt(9, 20)],
    [ID.relay, todayAt(9, 19)],
  ]);
  for (const tab of store.tabs) {
    for (const pane of tab.panes) {
      if (!pane.host || !pane.restored) continue;
      const host = state.hosts.find((entry) => entry.id === pane.host?.id);
      if (!host) continue;
      const session = connectedSession(host);
      state.recents.set(host.id, connectedAt.get(host.id) ?? todayAt(9, 20));
      pane.sessionId = session.id;
      pane.restored = false;
      store.applyStatus(pane, { state: 'ready' });
      // A restored pane waits for the user; this one was "connected" behind
      // the scenes, so its terminal is asked to show the session.
      pane.generation += 1;
    }
  }
  await hosts.refreshRecents();
}

/** `?folder` and `?identity`: the folders and identities pages start on that entry. */
async function selectEntities(): Promise<void> {
  const [{ hosts }, { identities }] = await Promise.all([
    import('../stores/hosts.svelte'),
    import('../stores/identities.svelte'),
  ]);
  if (flags.folder !== null) {
    const found = hosts.groups.find((group) => group.name === flags.folder);
    if (found) hosts.folderSelection = found.id;
  }
  if (flags.identity !== null) {
    const found = hosts.identities.find((identity) => identity.name === flags.identity);
    if (found) identities.selectedId = found.id;
  }
}

/** `?palette`: presses the palette chord of the platform, then types the text. */
async function openPalette(text: string): Promise<void> {
  const { palette } = await import('../stores/palette.svelte');
  window.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'P', ctrlKey: true, shiftKey: true, bubbles: true }),
  );
  if (text) palette.query = text;
}

/** `?connect`: starts a connection, which asks for the password of a password host. */
async function openConnect(name: string): Promise<void> {
  const [{ hosts }, { connectFlow }] = await Promise.all([
    import('../stores/hosts.svelte'),
    import('../stores/connect.svelte'),
  ]);
  const host = hosts.hosts.find((entry) => entry.name === name);
  if (host) await connectFlow.request(host);
}

/**
 * `?menu`: clicks the forwards chip of the toolbar once the session has
 * reported what became of its forwards and the layout has settled (a menu
 * closes when the window is resized, which the terminals do while they fit).
 */
function openForwardsMenu(): void {
  let tries = 0;
  let settledAt = 0;
  const timer = setInterval(() => {
    tries += 1;
    const chip = document.querySelector<HTMLElement>(
      'button[title="Port forwards of this session"]',
    );
    const reported = /failed|approval/.test(chip?.textContent ?? '');
    if (chip && reported) {
      settledAt = settledAt || Date.now();
      if (Date.now() - settledAt > 1200) {
        clearInterval(timer);
        chip.click();
      }
    } else if (tries > 100) {
      clearInterval(timer);
    }
  }, 100);
}

/** `?review`: opens the approval review of a host. */
async function openReview(name: string): Promise<void> {
  const [{ hosts }, { execApproval }] = await Promise.all([
    import('../stores/hosts.svelte'),
    import('../stores/exec-approval.svelte'),
  ]);
  const host = hosts.hosts.find((entry) => entry.name === name);
  if (host) await execApproval.show(host.id);
}

/** `?edit`: opens the host editor, as the "Edit" row action does. */
async function openEditor(name: string): Promise<void> {
  const [{ hosts }, { route }] = await Promise.all([
    import('../stores/hosts.svelte'),
    import('../stores/route.svelte'),
  ]);
  const host = name === 'new' ? null : (hosts.hosts.find((entry) => entry.name === name) ?? null);
  if (name !== 'new' && !host) return;
  route.edit(host);
}

// ---------------------------------------------------------------------------
// Commands

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

/** A required argument of a command. */
function arg<T>(args: Args, key: string): T {
  if (!(key in args)) return commandError(`missing argument \`${key}\``);
  return args[key] as T;
}

function secretKey(hostId: string, kind: string): string {
  if (kind !== 'password' && kind !== 'passphrase') {
    return commandError(`unknown secret kind '${kind}'`);
  }
  return `${hostId}:${kind}`;
}

function validateGeometry(cols: number, rows: number): void {
  if (!(cols >= 2 && cols <= 500) || !(rows >= 1 && rows <= 500)) {
    commandError(`invalid terminal size ${cols}x${rows} (allowed: 2-500 columns, 1-500 rows)`);
  }
}

/** Inserts or replaces a record by id. */
function upsert<T extends { id: string }>(list: T[], item: T): T[] {
  return list.some((entry) => entry.id === item.id)
    ? list.map((entry) => (entry.id === item.id ? item : entry))
    : [...list, item];
}

function now(): string {
  return new Date().toISOString();
}

/**
 * The refusals of `termru-core`'s `Host::validate`, in its words, so the
 * editor's inline errors and the mapping of a backend message onto a field
 * can be tried in the harness.
 */
function validateHostLikeBackend(host: Host): void {
  if (host.name.trim().length === 0) commandError('name must not be empty');
  if ([...host.name].length > 128) commandError('name must be at most 128 characters');
  // Letters, digits, `.-_:` and, for IPv6, brackets and a `%zone` (`fe80::1%eth0`).
  if (!/^[A-Za-z0-9._:%[\]-]+$/.test(host.address)) {
    commandError(
      "address contains invalid characters (allowed: letters, digits, '.', '-', '_', ':')",
    );
  }
  if (host.address.startsWith('-') || host.address.endsWith('-')) {
    commandError("address must not start or end with '-'");
  }
  if (host.port === 0) commandError('port must be between 1 and 65535');
  if (!/^[A-Za-z0-9._@-]+$/.test(host.username)) {
    commandError(
      "username contains invalid characters (allowed: letters, digits, '.', '-', '_', '@')",
    );
  }
  if (!/^[A-Za-z0-9._-]+$/.test(host.term)) {
    commandError("term contains invalid characters (allowed: letters, digits, '-', '_', '.')");
  }
  if (host.keepalive_seconds > 3600) {
    commandError('keepalive interval must be 0 (automatic) or at most 3600 seconds');
  }
  validateV2LikeBackend(host);
}

/** The v2 rules of `validate.rs` and of the store's route check, in their words. */
function validateV2LikeBackend(host: Host): void {
  const chain = host.jump_chain ?? [];
  if (chain.length > 8) commandError('a jump chain may have at most 8 hops');
  chain.forEach((hop, index) => {
    if (hop === host.id) commandError('a host cannot be its own jump host');
    if (chain.indexOf(hop) !== index) commandError('a jump chain lists the same host twice');
  });
  if (chain.length > 0) {
    const route = resolveRoute(host.id, host.name, chain, (id) => {
      const found = state.hosts.find((entry) => entry.id === id && !entry.deleted);
      return found ? { name: found.name, jump_chain: found.jump_chain ?? [] } : undefined;
    });
    // The store's wording is the core's, which starts lower case and has no full stop.
    if (!route.ok)
      commandError(route.error.replace(/^./, (c) => c.toLowerCase()).replace(/\.$/, ''));
  }

  const forwards = host.forwards ?? [];
  if (forwards.length > 32) commandError('a host may have at most 32 port forwards');
  const claimed = new Set<string>();
  for (const forward of forwards) {
    if (forward.bind_port === 0) commandError('forward bind port must be between 1 and 65535');
    const bind = forward.bind_address;
    if (bind) {
      const bare = bind.replace(/^\[|\]$/g, '').split('%')[0];
      const literal = bare.toLowerCase() === 'localhost' || isIpv4Literal(bare) || isIpv6(bare);
      if (forward.kind !== 'remote' && !literal) {
        commandError('a local listener needs an IP address (or localhost) as its bind address');
      }
    }
    if (forward.kind === 'dynamic') {
      if (forward.target_host || forward.target_port) {
        commandError('a dynamic (SOCKS) forward has no target: the client of the proxy names it');
      }
    } else {
      if (!forward.target_host) commandError(`a ${forward.kind} forward needs a target host`);
      if (!forward.target_port) {
        commandError(`a ${forward.kind} forward needs a target port between 1 and 65535`);
      }
    }
    const key = `${forward.kind === 'remote' ? 'r' : 'l'}|${bind ?? ''}|${forward.bind_port}`;
    if (claimed.has(key)) {
      commandError(`two forwards listen on ${bind ?? 'localhost'}:${forward.bind_port}`);
    }
    claimed.add(key);
  }

  const env = host.env ?? [];
  if (env.length > 32) commandError('a host may have at most 32 environment variables');
  const names = new Set<string>();
  for (const variable of env) {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(variable.name)) {
      commandError(
        `environment variable name "${variable.name}" is invalid (use letters, digits and '_', not starting with a digit)`,
      );
    }
    if (names.has(variable.name)) {
      commandError(`environment variable ${variable.name} is listed twice`);
    }
    names.add(variable.name);
  }

  if (host.startup_command != null) {
    if (host.startup_command.trim().length === 0) {
      commandError('startup command must not be empty (leave it unset instead)');
    }
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u0008\u000a-\u001f\u007f-\u009f]/u.test(host.startup_command)) {
      commandError('startup command must be a single line without control characters');
    }
  }
  if (host.auth_order != null && host.auth_order.length === 0) {
    commandError(
      'the authentication order must name at least one method (leave it unset for the default)',
    );
  }
}

/** What the harness offers to import (no real `~/.ssh/config` here). */
function sshConfigSample(): Omit<SshConfigPreview['candidates'][number], 'existing_host_id'>[] {
  return [
    {
      alias: 'bastion',
      host: 'bastion.corp.example',
      user: 'ops',
      port: 2222,
      identity_files: ['/home/demo/.ssh/work_ed25519'],
      jump_chain: [],
      warnings: [],
      problem: null,
    },
    {
      alias: 'app',
      host: 'app.corp.example',
      user: 'deploy',
      port: 22,
      identity_files: ['/home/demo/.ssh/work_ed25519'],
      jump_chain: ['bastion'],
      warnings: [],
      problem: null,
    },
    {
      alias: 'legacy',
      host: 'bad host',
      user: 'root',
      port: 22,
      identity_files: [],
      jump_chain: [],
      warnings: ['uses ProxyCommand, which cannot be imported; the host is imported without it'],
      problem: 'address contains invalid characters',
    },
  ];
}

/** The folder rules of the store's `validate_parent`, in its words. */
function validateGroupLikeBackend(groups: Group[], group: Group): void {
  if (group.name.trim().length === 0) commandError('group name must not be empty');
  if (group.parent_id === group.id) commandError('a group cannot be its own parent');
  if (!group.parent_id) return;
  const byId = new Map(groups.map((entry) => [entry.id, entry]));
  let next: string | null = group.parent_id;
  let depth = 1;
  while (next && depth <= 8) {
    const current = byId.get(next);
    if (!current) return;
    if (current.parent_id === group.id) commandError('group hierarchy contains a cycle');
    next = current.parent_id;
    if (!next) return;
    depth += 1;
  }
  commandError('group nesting exceeds the maximum depth of 8');
}

const BASE64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';

/** A stable pseudo-random base64 string from a seed (fake key material). */
function fakeBase64(seed: string, length: number): string {
  let h = 2166136261;
  for (const char of seed) h = Math.imul(h ^ char.charCodeAt(0), 16777619);
  let out = '';
  for (let i = 0; i < length; i += 1) {
    h = Math.imul(h ^ (h >>> 15), 2246822519) + i;
    out += BASE64[(h >>> 0) % BASE64.length];
  }
  return out;
}

/** What the bound key file of an identity says (fake, but stable). */
function keyInfoFor(identity: Identity, keyPath: string): PublicKeyInfo {
  const known = state.keyInfo.get(identity.id);
  if (known) return known;
  return {
    public_key: `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI${fakeBase64(keyPath, 43)} ${identity.name}`,
    fingerprint: identity.fingerprint ?? `SHA256:${fakeBase64(`fp:${keyPath}`, 43)}`,
    algorithm: 'ssh-ed25519',
    encrypted: false,
    key_path: keyPath,
  };
}

const handlers: Record<string, Handler> = {
  app_info: () => ({ name: 'termru-desktop', version: '0.2.0-mock' }),

  // Hosts
  host_list: () => {
    if (flags.error) {
      return commandError('could not read the host store: database is locked (mock error)');
    }
    return state.hosts.filter((host) => !host.deleted);
  },
  recent_list: (): RecentHost[] =>
    [...state.recents]
      .flatMap(([id, lastUsedAt]) => {
        const host = state.hosts.find((entry) => entry.id === id && !entry.deleted);
        return host ? [{ host, last_used_at: lastUsedAt }] : [];
      })
      .sort((a, b) => b.last_used_at - a.last_used_at),
  host_save: (args) => {
    const host = { ...arg<Host>(args, 'host'), updated_at: now() };
    validateHostLikeBackend(host);
    state.hosts = upsert(state.hosts, host);
    // Only a save from the editor approves; any other save leaves it alone.
    if (args.approveExec === true) state.execApproved.set(host.id, mockExecHash(host));
    return host;
  },
  host_exec_review: (args): HostExecReview => {
    const host = hostOf(arg<string>(args, 'id'));
    const status = execStatusOf(host);
    return {
      host_id: host.id,
      host_name: host.name,
      hash: status.hash,
      pending: !status.approved,
      fields: execFieldsOf(host),
    };
  },
  host_exec_approve: (args) => {
    const host = hostOf(arg<string>(args, 'id'));
    if (mockExecHash(host) !== arg<string>(args, 'hash')) {
      return commandError(
        'The synced settings of this host changed after you opened the review. Review them again.',
      );
    }
    state.execApproved.set(host.id, mockExecHash(host));
    return null;
  },
  host_delete: (args) => {
    const id = arg<string>(args, 'id');
    state.hosts = state.hosts.filter((host) => host.id !== id);
    state.recents.delete(id);
    return null;
  },
  host_set_favorite: (args) => {
    const id = arg<string>(args, 'id');
    const favorite = arg<boolean>(args, 'favorite');
    state.hosts = state.hosts.map((host) => (host.id === id ? { ...host, favorite } : host));
    return null;
  },

  // Folders and tags
  group_list: () => state.groups.filter((group) => !group.deleted),
  group_save: (args) => {
    const group = { ...arg<Group>(args, 'group'), updated_at: now() };
    validateGroupLikeBackend(state.groups, group);
    state.groups = upsert(state.groups, group);
    return group;
  },
  group_delete: (args) => {
    // Like the store: the hosts and the subfolders move up to the parent.
    const id = arg<string>(args, 'id');
    const parent = state.groups.find((group) => group.id === id)?.parent_id ?? null;
    state.groups = state.groups
      .filter((group) => group.id !== id)
      .map((group) => (group.parent_id === id ? { ...group, parent_id: parent } : group));
    state.hosts = state.hosts.map((host) =>
      host.group_id === id ? { ...host, group_id: parent } : host,
    );
    return null;
  },
  tag_list: () => state.tags.filter((tag) => !tag.deleted),
  tag_save: (args) => {
    const tag = { ...arg<Tag>(args, 'tag'), updated_at: now() };
    state.tags = upsert(state.tags, tag);
    return tag;
  },
  tag_delete: (args) => {
    const id = arg<string>(args, 'id');
    state.tags = state.tags.filter((tag) => tag.id !== id);
    state.hosts = state.hosts.map((host) => ({
      ...host,
      tags: host.tags.filter((tagId) => tagId !== id),
    }));
    return null;
  },

  // Identities
  identity_list: () => state.identities.filter((identity) => !identity.deleted),
  identity_save: (args) => {
    const identity = { ...arg<Identity>(args, 'identity'), updated_at: now() };
    state.identities = upsert(state.identities, identity);
    return identity;
  },
  identity_delete: (args) => {
    const id = arg<string>(args, 'id');
    state.identities = state.identities.filter((identity) => identity.id !== id);
    state.bindings.delete(id);
    return null;
  },
  identity_bind: (args) => {
    const keyPath = arg<string>(args, 'keyPath').trim();
    if (keyPath.length === 0) return commandError('the key path is empty');
    state.bindings.set(arg<string>(args, 'id'), keyPath);
    return null;
  },
  identity_unbind: (args) => {
    state.bindings.delete(arg<string>(args, 'id'));
    return null;
  },
  identity_generate: (args): GeneratedIdentity => {
    const name = arg<string>(args, 'name').trim();
    if (name.length === 0) return commandError('identity name must not be empty');
    const passphrase = (args.passphrase as string | null | undefined) || null;
    if (passphrase !== null && [...passphrase].length < 5) {
      return commandError(
        'a passphrase needs at least 5 characters; leave it empty for a key without one',
      );
    }
    const id = crypto.randomUUID();
    const comment = (args.comment as string | null | undefined)?.trim() || null;
    const keyPath = `/home/demo/.local/share/com.liwyd.termru/keys/id_ed25519-${id}`;
    const fingerprint = `SHA256:${fakeBase64(id, 43)}`;
    const publicKey = `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI${fakeBase64(`pub:${id}`, 43)} ${name}`;
    const stamp = now();
    const identity: Identity = {
      id,
      name,
      comment,
      fingerprint,
      created_at: stamp,
      updated_at: stamp,
      deleted: false,
    };
    state.identities = [...state.identities, identity];
    state.bindings.set(id, keyPath);
    state.keyInfo.set(id, {
      public_key: publicKey,
      fingerprint,
      algorithm: 'ssh-ed25519',
      encrypted: passphrase !== null,
      key_path: keyPath,
    });
    const remember = passphrase !== null && args.rememberPassphrase === true;
    return {
      identity,
      key_path: keyPath,
      public_key: publicKey,
      fingerprint,
      passphrase_remembered: remember,
      passphrase_note: null,
    };
  },
  identity_public_key: (args): PublicKeyInfo | null => {
    const id = arg<string>(args, 'id');
    const keyPath = state.bindings.get(id);
    if (!keyPath) return null;
    const identity = state.identities.find((entry) => entry.id === id);
    if (!identity) return commandError(`no identity with id ${id}`);
    return keyInfoFor(identity, keyPath);
  },
  identity_bindings: (): IdentityBinding[] =>
    [...state.bindings]
      .map(([identityId, keyPath]) => ({ identityId, keyPath }))
      .sort((a, b) => a.identityId.localeCompare(b.identityId)),

  // Preferences
  prefs_get: () => state.prefs,
  prefs_set: (args) => {
    state.prefs = arg<Preferences>(args, 'prefs');
    return null;
  },

  // Sessions
  session_open: (args) => {
    const options = arg<{
      cols: number;
      rows: number;
      hop_secrets?: Record<string, { password?: string }>;
    }>(args, 'options');
    validateGeometry(options.cols, options.rows);
    return openSession(
      arg<string>(args, 'hostId'),
      arg<ChannelLike>(args, 'channel'),
      options.hop_secrets,
    ).id;
  },
  session_retry: (args) => {
    const geometry = arg<{ cols: number; rows: number }>(args, 'geometry');
    validateGeometry(geometry.cols, geometry.rows);
    const session = sessionOf(arg<string>(args, 'id'));
    session.views.push(arg<ChannelLike>(args, 'channel'));
    for (const timer of session.timers) clearTimeout(timer);
    session.timers.clear();
    startSession(session);
    return null;
  },
  session_duplicate: (args) => {
    const geometry = arg<{ cols: number; rows: number }>(args, 'geometry');
    validateGeometry(geometry.cols, geometry.rows);
    const source = sessionOf(arg<string>(args, 'id'));
    // A duplicate re-uses what the original was opened with, hop secrets included.
    return openSession(source.hostId, arg<ChannelLike>(args, 'channel'), undefined, true).id;
  },
  session_close: (args) => {
    closeSession(sessionOf(arg<string>(args, 'id')));
    return null;
  },
  session_detach: (args) => {
    // Releases the views of the caller; the session keeps running.
    sessionOf(arg<string>(args, 'id')).views = [];
    return null;
  },
  session_write: (args) => {
    typeInto(sessionOf(arg<string>(args, 'id')), arg<number[]>(args, 'data'));
    return null;
  },
  session_resize: (args) => {
    validateGeometry(arg<number>(args, 'cols'), arg<number>(args, 'rows'));
    sessionOf(arg<string>(args, 'id'));
    return null;
  },
  session_list: (): SessionInfo[] =>
    [...sessions.values()].map((session) => ({
      id: session.id,
      host_id: session.hostId,
      status: session.status,
      ...(session.forwards.length > 0 ? { forwards: session.forwards } : {}),
      ...(session.execPending ? { exec_pending: true } : {}),
    })),
  session_attach: (args) => {
    const session = sessionOf(arg<string>(args, 'id'));
    const view = arg<ChannelLike>(args, 'channel');
    // The replay buffer first, then the status, then the live stream.
    if (session.buffer.length > 0)
      view.onmessage({ kind: 'data', base64: toBase64(session.buffer) });
    view.onmessage({ kind: 'status', status: session.status });
    for (const forward of session.forwards) view.onmessage({ kind: 'forward', forward });
    if (session.execPending) view.onmessage({ kind: 'exec_pending' });
    session.views.push(view);
    return null;
  },
  host_key_prompt_resolve: (args) => pendingPrompts.delete(arg<string>(args, 'id')),
  auth_prompt_resolve: (args) => pendingPrompts.delete(arg<string>(args, 'id')),

  // Windows
  window_open_session: (args) => {
    const session = sessionOf(arg<string>(args, 'id'));
    if (session.status.state !== 'ready') {
      return commandError('only a connected session can be moved to a new window');
    }
    return `session-${session.id}`;
  },
  window_close: () => null,

  // Layout
  layout_save: (args) => {
    state.layout = arg<WorkspaceDoc>(args, 'doc');
    return null;
  },
  layout_load: () => {
    scheduleStartup();
    return state.layout;
  },

  // Secrets
  secret_set: (args) => {
    if (arg<string>(args, 'value').length === 0) {
      return commandError('refusing to store an empty secret');
    }
    state.secrets.add(secretKey(arg<string>(args, 'hostId'), arg<string>(args, 'kind')));
    return null;
  },
  secret_delete: (args) => {
    state.secrets.delete(secretKey(arg<string>(args, 'hostId'), arg<string>(args, 'kind')));
    return null;
  },
  secret_has: (args) =>
    state.secrets.has(secretKey(arg<string>(args, 'hostId'), arg<string>(args, 'kind'))),

  // Importing from ~/.ssh/config (fixed sample: the harness has no file system)
  ssh_config_preview: (): SshConfigPreview => ({
    found: true,
    path: '/home/demo/.ssh/config',
    candidates: sshConfigSample().map((candidate) => ({
      ...candidate,
      existing_host_id:
        state.hosts.find(
          (host) =>
            !host.deleted &&
            host.address === candidate.host &&
            host.port === candidate.port &&
            host.username === candidate.user,
        )?.id ?? null,
    })),
    warnings: ['/home/demo/.ssh/config:31: a Match block was skipped'],
  }),
  ssh_config_import: (args): SshConfigImportReport => {
    const wanted = new Set(arg<string[]>(args, 'selection'));
    const imported: SshConfigImportReport['imported'] = [];
    const skipped: SshConfigImportReport['skipped'] = [];
    const already: string[] = [];
    for (const candidate of sshConfigSample()) {
      if (!wanted.has(candidate.alias)) continue;
      if (candidate.problem) {
        skipped.push({ alias: candidate.alias, reason: candidate.problem });
        continue;
      }
      const exists = state.hosts.some(
        (host) =>
          !host.deleted &&
          host.address === candidate.host &&
          host.port === candidate.port &&
          host.username === candidate.user,
      );
      if (exists) {
        already.push(candidate.alias);
        continue;
      }
      const id = crypto.randomUUID();
      state.hosts = upsert(
        state.hosts,
        fixtureHost(state.hosts.length, {
          id,
          name: candidate.alias,
          address: candidate.host,
          port: candidate.port,
          username: candidate.user ?? 'demo',
          auth: { type: 'agent' },
          group_id: null,
        }),
      );
      imported.push({ id, name: candidate.alias });
    }
    return {
      imported,
      identities_created: 0,
      skipped,
      already_present: already,
      warnings: [],
    };
  },

  // Sync
  sync_status: () => state.sync,
  sync_sign_in: (args) => {
    const serverUrl = arg<string>(args, 'serverUrl').trim();
    const email = arg<string>(args, 'email').trim();
    if (
      serverUrl.length === 0 ||
      email.length === 0 ||
      arg<string>(args, 'password').length === 0
    ) {
      return commandError('the server, the email and the password are required');
    }
    state.sync = {
      ...state.sync,
      settings: {
        ...state.sync.settings,
        server_url: serverUrl,
        email,
        device_name: arg<string>(args, 'deviceName'),
      },
      signed_in: true,
      last_sync_at: now(),
      last_error: null,
    };
    return state.sync;
  },
  sync_sign_out: () => {
    state.sync = {
      ...signedOutSync(),
      settings: { ...signedOutSync().settings, device_name: state.sync.settings.device_name },
    };
    state.conflicts = [];
    return null;
  },
  sync_now: (): SyncOutcome => {
    if (!state.sync.signed_in) return commandError('not signed in to a sync server');
    const cursor = state.sync.cursor + 1;
    state.sync = { ...state.sync, pending: 0, cursor, last_sync_at: now(), last_error: null };
    return { pushed: 0, pulled: 0, applied: 0, conflicts: state.conflicts.length, cursor };
  },
  sync_conflicts: () => state.conflicts,
  sync_resolve_conflict: (args) => {
    const recordId = arg<string>(args, 'recordId');
    state.conflicts = state.conflicts.filter((conflict) => conflict.record_id !== recordId);
    state.sync = { ...state.sync, conflicts: state.conflicts.length };
    return null;
  },
};

/** The names of the commands the mock implements. */
export const mockCommands: readonly string[] = Object.keys(handlers);

/**
 * The arguments as the backend would receive them: serialized, so the UI
 * never shares an object (or a reactive proxy) with the "backend". Channels
 * are live objects and pass through.
 */
function receive(args: InvokeArgs | undefined): Args {
  if (!args || Array.isArray(args) || args instanceof ArrayBuffer || args instanceof Uint8Array) {
    return {};
  }
  const input: Args = {};
  for (const [key, value] of Object.entries(args)) {
    input[key] =
      value instanceof Channel || value === undefined
        ? value
        : (JSON.parse(JSON.stringify(value)) as unknown);
  }
  return input;
}

/** The mock of `invoke`. */
export async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  const handler = handlers[cmd];
  if (!handler) throw new Error(`unknown command: ${cmd}`);
  // A real IPC call is asynchronous even when the answer is immediate.
  await Promise.resolve();
  // The answer is serialized too (a command returning `()` gives `null`).
  return structuredClone(handler(receive(args))) as T;
}

/** The mock of the single-file picker. */
export async function openFile(_options: OpenFileOptions = {}): Promise<string | null> {
  await Promise.resolve();
  return '/home/demo/.ssh/id_ed25519';
}

/** The addresses the mock "opened in the browser" (for the harness and tests). */
export const openedUrls: string[] = [];

/** The mock of the opener plugin: it remembers the address and opens nothing. */
export async function openUrl(url: string): Promise<void> {
  await Promise.resolve();
  openedUrls.push(url);
}

/** The mock of the window: the title bar controls do nothing. */
export async function currentWindow(): Promise<WindowHandle> {
  return {
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: async () => {},
    // A browser tab has no close request to hold back.
    onCloseRequested: async () => () => {},
  };
}

/** Rebuilds the in-memory backend (for tests; the URL flags are not re-read). */
export function resetMock(next: Partial<MockFlags> = {}): void {
  for (const session of [...sessions.values()]) closeSession(session);
  openCounts.clear();
  listeners.clear();
  pendingPrompts.clear();
  hostKeyPromptScheduled = false;
  authPromptScheduled = false;
  startupScheduled = false;
  flags = {
    empty: false,
    error: false,
    demo: false,
    edit: null,
    folder: null,
    identity: null,
    hostkey: false,
    hostkeyOtherTypes: false,
    hostkeyChanged: false,
    connect: null,
    sync: null,
    authprompt: null,
    theme: null,
    many: 0,
    long: false,
    v2: false,
    approval: false,
    review: null,
    importConfig: false,
    menu: false,
    palette: null,
    ...next,
  };
  state = seed(flags);
}
