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
 *   &hostkey         an unknown-host-key prompt appears shortly after start
 *                    (`&hostkey=other`: the host is known under other key types)
 *   &authprompt      a keyboard-interactive question appears shortly after
 *                    start (`&authprompt=passphrase`: a key passphrase)
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
import type {
  AuthPromptMessage,
  ConflictView,
  Group,
  Host,
  HostKeyPromptMessage,
  HostKeyPromptRequest,
  Identity,
  IdentityBinding,
  Preferences,
  RecentHost,
  SessionInfo,
  SessionStatus,
  SessionUpdate,
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
  hostkey: boolean;
  /** The host-key prompt says the host is known under other key types. */
  hostkeyOtherTypes: boolean;
  /** An authentication question appears on start (`kbd` or `passphrase`). */
  authprompt: 'kbd' | 'passphrase' | null;
  theme: ThemePreference | null;
}

function readFlags(): MockFlags {
  const params = new URLSearchParams(typeof location === 'undefined' ? '' : location.search);
  const theme = params.get('theme');
  return {
    empty: params.has('empty'),
    error: params.has('error'),
    demo: params.has('demo'),
    edit: params.has('edit') ? params.get('edit') || 'staging-api' : null,
    hostkey: params.has('hostkey'),
    hostkeyOtherTypes: params.get('hostkey') === 'other',
    authprompt: params.has('authprompt')
      ? params.get('authprompt') === 'passphrase'
        ? 'passphrase'
        : 'kbd'
      : null,
    theme: theme === 'dark' || theme === 'light' || theme === 'system' ? theme : null,
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
    theme: 'system',
    terminal: {
      font_family: 'IBM Plex Mono, monospace',
      font_size: 14,
      scrollback: 10_000,
      cursor_blink: true,
      cursor_style: 'block',
      copy_on_select: false,
      line_height: 1.65,
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
// State

interface MockState {
  hosts: Host[];
  groups: Group[];
  tags: Tag[];
  identities: Identity[];
  /** identity id -> key file path (device-local). */
  bindings: Map<string, string>;
  /** host id -> last connection time (ms). */
  recents: Map<string, number>;
  /** `${host id}:${kind}` of the secrets "in the credential store". */
  secrets: Set<string>;
  prefs: Preferences;
  sync: SyncStatus;
  conflicts: ConflictView[];
  layout: WorkspaceDoc | null;
}

function seed(flags: MockFlags): MockState {
  const prefs = defaultPreferences();
  if (flags.theme) prefs.theme = flags.theme;
  const hosts = flags.empty ? [] : fixtureHosts();
  const recents = new Map<string, number>();
  if (!flags.empty) {
    recents.set(ID.staging, todayAt(9, 18));
    recents.set(ID.build, yesterday());
    recents.set(ID.archive, eighteenthOfOctober());
  }
  return {
    hosts,
    groups: fixtureGroups(),
    tags: [],
    identities: fixtureIdentities(),
    bindings: new Map([
      [ID.work, '/home/demo/.ssh/work_ed25519'],
      [ID.buildKey, '/home/demo/.ssh/build_ed25519'],
    ]),
    recents,
    // The password of staging-api is "stored on this device"; archive asks.
    secrets: new Set(flags.empty ? [] : [`${ID.staging}:password`]),
    prefs,
    sync: signedOutSync(),
    conflicts: [],
    layout: flags.demo ? demoLayout() : null,
  };
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
      const request: HostKeyPromptRequest = {
        state: 'unknown',
        host: '192.0.2.10',
        port: 22,
        key: {
          key_type: 'ssh-ed25519',
          fingerprint: 'SHA256:Vq8m2nT0cYb5LhZr7WdEa1KsXo4GuPj9RfIt3NxMe6A',
        },
        ...(flags.hostkeyOtherTypes ? { other_key_types: ['ssh-rsa', 'ecdsa-sha2-nistp256'] } : {}),
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
  send(session, { kind: 'status', status });
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
  });
  transcript(session, host).forEach((chunk, index) => {
    later(session, 420 + index * 35, () => sendData(session, chunk));
  });
}

function setStatusLater(session: MockSession, ms: number, status: SessionStatus): void {
  later(session, ms, () => setStatus(session, status));
}

function openSession(hostId: string, view: ChannelLike): MockSession {
  const host = hostOf(hostId);
  const variants = variantsFor(host);
  const count = openCounts.get(host.id) ?? 0;
  openCounts.set(host.id, count + 1);
  const session = createSession(host, variants[count % variants.length]);
  session.views.push(view);
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
  if (startupScheduled || !(flags.demo || flags.edit !== null)) return;
  startupScheduled = true;
  // After the app has applied the layout it just received.
  setTimeout(() => {
    void (async () => {
      if (flags.demo) await connectRestoredPanes();
      if (flags.edit !== null) await openEditor(flags.edit);
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
    state.hosts = upsert(state.hosts, host);
    return host;
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
    state.groups = upsert(state.groups, group);
    return group;
  },
  group_delete: (args) => {
    const id = arg<string>(args, 'id');
    state.groups = state.groups.filter((group) => group.id !== id);
    state.hosts = state.hosts.map((host) =>
      host.group_id === id ? { ...host, group_id: null } : host,
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
    const options = arg<{ cols: number; rows: number }>(args, 'options');
    validateGeometry(options.cols, options.rows);
    return openSession(arg<string>(args, 'hostId'), arg<ChannelLike>(args, 'channel')).id;
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
    return openSession(source.hostId, arg<ChannelLike>(args, 'channel')).id;
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
    })),
  session_attach: (args) => {
    const session = sessionOf(arg<string>(args, 'id'));
    const view = arg<ChannelLike>(args, 'channel');
    // The replay buffer first, then the status, then the live stream.
    if (session.buffer.length > 0)
      view.onmessage({ kind: 'data', base64: toBase64(session.buffer) });
    view.onmessage({ kind: 'status', status: session.status });
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

/** The mock of the window: the title bar controls do nothing. */
export async function currentWindow(): Promise<WindowHandle> {
  return {
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: async () => {},
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
    hostkey: false,
    hostkeyOtherTypes: false,
    authprompt: null,
    theme: null,
    ...next,
  };
  state = seed(flags);
}
