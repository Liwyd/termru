/** TypeScript mirrors of the Rust DTOs (serde `snake_case` field names). */

export type AuthMethod =
  { type: 'password' } | { type: 'private_key'; identity_id: string } | { type: 'agent' };

/** One authentication method of a host's explicit `auth_order`. */
export type AuthKind = 'password' | 'private_key' | 'agent' | 'keyboard_interactive';

/** `local` = -L, `remote` = -R, `dynamic` = -D (a SOCKS5 proxy). */
export type ForwardKind = 'local' | 'remote' | 'dynamic';

/**
 * A port forward started with the session. Without a `bind_address` the
 * listener is on loopback only; `target_host`/`target_port` are required for
 * `local` and `remote` and must be absent for `dynamic`. A disabled forward
 * is kept but not started.
 */
export interface Forward {
  kind: ForwardKind;
  /** `local`/`dynamic`: an IP address or `localhost`; `remote`: an address on the server. */
  bind_address?: string | null;
  bind_port: number;
  target_host?: string | null;
  target_port?: number | null;
  enabled: boolean;
}

/** An environment variable requested from the server (`SetEnv`). Synced in plain text. */
export interface EnvVar {
  name: string;
  value: string;
}

export interface Host {
  id: string;
  name: string;
  address: string;
  port: number;
  username: string;
  auth: AuthMethod;
  group_id: string | null;
  tags: string[];
  favorite: boolean;
  description: string | null;
  term: string;
  keepalive_seconds: number;
  /**
   * Data model v2. All of these are omitted by the backend when unused (a
   * host without them serializes as it did before) and default to "off"
   * when missing, so they are optional here.
   *
   * Hosts to pass through on the way, in order (ids of other hosts, at most
   * 8). The first hop's own `jump_chain` is followed before it.
   */
  jump_chain?: string[];
  forwards?: Forward[];
  env?: EnvVar[];
  /** Typed into the shell once it starts (one line, followed by Enter). */
  startup_command?: string | null;
  /**
   * Lets the server use the local SSH agent for as long as the session lives.
   * Off by default; whoever controls the server can sign as the user while it
   * is on.
   */
  agent_forward?: boolean;
  /** Explicit order of the authentication methods; absent = the default for `auth`. */
  auth_order?: AuthKind[] | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
  deleted: boolean;
}

/** A recently-used host with its last connection time (ms since epoch). */
export interface RecentHost {
  host: Host;
  last_used_at: number;
}

export interface Identity {
  id: string;
  name: string;
  comment: string | null;
  fingerprint: string | null;
  created_at: string;
  updated_at: string;
  deleted: boolean;
}

/** The public half of the key an identity is bound to (`identity_public_key`). */
export interface PublicKeyInfo {
  /** One OpenSSH line: `ssh-ed25519 AAAA… comment`. */
  public_key: string;
  /** `SHA256:…`, as `ssh-keygen -l` prints it. */
  fingerprint: string;
  /** `ssh-ed25519`, `ssh-rsa`, … */
  algorithm: string;
  /** A passphrase protects the private key. */
  encrypted: boolean;
  key_path: string;
}

/** What `identity_generate` made: the identity, where its key is and the result. */
export interface GeneratedIdentity {
  identity: Identity;
  key_path: string;
  public_key: string;
  fingerprint: string;
  /** The passphrase is now in the credential store. */
  passphrase_remembered: boolean;
  /** Why it was not, when the user asked for that. */
  passphrase_note: string | null;
}

/** A folder (the group a host can live in). */
export interface Group {
  id: string;
  name: string;
  parent_id: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
  deleted: boolean;
}

/** A label that can be put on a host. */
export interface Tag {
  id: string;
  name: string;
  color: string | null;
  created_at: string;
  updated_at: string;
  deleted: boolean;
}

/**
 * A device-local key binding: which key file an identity uses on this
 * machine. The path is never synced.
 */
export interface IdentityBinding {
  identityId: string;
  keyPath: string;
}

/** What the `app_info` command reports about the running build. */
export interface AppInfo {
  name: string;
  version: string;
}

export type ThemePreference = 'light' | 'dark' | 'system';
export type CursorStyle = 'block' | 'bar' | 'underline';
export type MotionPreference = 'system' | 'on' | 'off';

export interface TerminalPreferences {
  font_family: string;
  font_size: number;
  scrollback: number;
  cursor_blink: boolean;
  cursor_style: CursorStyle;
  copy_on_select: boolean;
  line_height: number;
  /** Ask before a paste of several lines reaches the shell. */
  confirm_multiline_paste: boolean;
}

export interface Preferences {
  theme: ThemePreference;
  terminal: TerminalPreferences;
  keybindings: Record<string, string>;
  reduced_motion: MotionPreference;
  focus_visibility: boolean;
  routine_notices: boolean;
}

export type ErrorCategory = 'auth' | 'host_key' | 'network' | 'timeout' | 'protocol' | 'local';

export type SessionStatus =
  | { state: 'connecting' }
  | { state: 'authenticating' }
  | { state: 'ready' }
  | { state: 'reconnecting'; attempt: number; delay_ms: number; last_error: string }
  | { state: 'closed'; reason: string }
  | { state: 'failed'; category: ErrorCategory; message: string };

/**
 * What became of one port forward of a session. `id` is the forward's index
 * in the host's `forwards` list. They are reported after the session is
 * `ready`, and again after a reconnect; every status other than `ready`
 * clears them.
 */
export type ForwardUpdate =
  | { state: 'listening'; id: number; kind: ForwardKind; address: string; port: number }
  | { state: 'failed'; id: number; kind: ForwardKind; reason: string };

export type SessionUpdate =
  | { kind: 'status'; status: SessionStatus }
  | { kind: 'data'; base64: string }
  | { kind: 'exit'; code: number | null }
  | { kind: 'forward'; forward: ForwardUpdate };

export interface SessionInfo {
  id: string;
  host_id: string;
  status: SessionStatus;
  /** The forwards that are up (or failed) right now; omitted when there are none. */
  forwards?: ForwardUpdate[];
}

/** Secrets for one connection. Never persisted; memory only. */
export interface SessionSecrets {
  password?: string;
  passphrase?: string;
}

/**
 * Secrets typed for the jump hosts of a route, by host id. A hop without an
 * entry uses what the credential store holds for it.
 */
export type HopSecrets = Record<string, SessionSecrets>;

/** One host of `~/.ssh/config`, as `ssh_config_preview` lists it. */
export interface SshConfigCandidate {
  /** The `Host` alias; also the name the imported host gets (and the key of a selection). */
  alias: string;
  host: string;
  /** The configured user, else this machine's; null when there is neither. */
  user: string | null;
  port: number;
  /** Key files with `~` expanded (as far as that was possible). */
  identity_files: string[];
  /** The route as written, first hop first (`bastion`, `ops@gw:2200`). */
  jump_chain: string[];
  warnings: string[];
  /** The library already has a host with this address, user and port. */
  existing_host_id: string | null;
  /** Why it cannot be imported, when it cannot. */
  problem: string | null;
}

export interface SshConfigPreview {
  /** False when there is no `~/.ssh/config` (not an error). */
  found: boolean;
  path: string;
  candidates: SshConfigCandidate[];
  /** About the file as a whole: skipped `Match` blocks, includes that failed. */
  warnings: string[];
}

export interface SshConfigImportReport {
  /** Every host created: the ones asked for and the jump hosts they needed. */
  imported: { id: string; name: string }[];
  identities_created: number;
  /** Asked for, not imported. */
  skipped: { alias: string; reason: string }[];
  /** Asked for, but the library already has the host. */
  already_present: string[];
  warnings: string[];
}

export interface SessionGeometry {
  cols: number;
  rows: number;
}

/** The host-key question (`HostKeyPromptRequest` of the SSH engine). */
export type HostKeyPromptRequest =
  | {
      state: 'unknown';
      host: string;
      port: number;
      key: { key_type: string; fingerprint: string };
      /**
       * Key types the host is already known under (a different type than
       * `key`'s); left out when there are none.
       */
      other_key_types?: string[];
    }
  | {
      state: 'changed';
      host: string;
      port: number;
      old: { key_type: string; fingerprint: string };
      new: { key_type: string; fingerprint: string };
    };

/** Payload of the `host-key-prompt` frontend event. */
export interface HostKeyPromptMessage {
  id: string;
  /** The session whose connection asks. */
  session_id: string;
  request: HostKeyPromptRequest;
}

/** One question of an authentication round. */
export interface AuthPromptField {
  text: string;
  /** What the user types may be shown (false: a secret, hidden). */
  echo: boolean;
}

/** The identity a private key belongs to. */
export interface KeyIdentity {
  id: string;
  name: string;
}

export type AuthPromptKind =
  | { type: 'keyboard_interactive' }
  | {
      type: 'passphrase';
      identity: KeyIdentity | null;
      /** An earlier answer did not unlock the key. */
      retry: boolean;
    };

export interface AuthPromptRequest {
  kind: AuthPromptKind;
  /** Title the server gave the round (often empty; server-controlled text). */
  name: string;
  /** Text the server shows above the questions (often empty; server-controlled). */
  instructions: string;
  prompts: AuthPromptField[];
}

/** Payload of the `auth-prompt` frontend event. */
export interface AuthPromptMessage {
  id: string;
  session_id: string;
  /** The host the session connects to. */
  host_id: string | null;
  request: AuthPromptRequest;
}

/** Layout persistence (mirrors `termru_core::workspace`). */
export interface PaneNode {
  leaf?: { pane: string; host_id: string | null };
  split?: {
    direction: 'columns' | 'rows';
    ratio: number;
    first: PaneNode;
    second: PaneNode;
  };
}

export interface TabLayout {
  id: string;
  title?: string | null;
  root: PaneNode;
  active_pane?: string | null;
}

export interface WindowLayout {
  id: string;
  tabs: TabLayout[];
  active_tab?: string | null;
  bounds?: { x: number; y: number; width: number; height: number } | null;
}

export interface WorkspaceDoc {
  version: number;
  windows: WindowLayout[];
  active_window?: string | null;
  /** The last page the user was on; absent in documents from before routing. */
  last_route?: string | null;
}

/** The kind of a synced record (mirrors `termru_core::sync::RecordKind`). */
export type RecordKind = 'host' | 'identity' | 'group' | 'tag' | 'preferences';

/** The sync settings (no secrets — the tokens live in the credential store). */
export interface SyncSettings {
  server_url: string;
  email: string;
  device_name: string;
  device_id: string;
}

/** The state of the sync feature. */
export interface SyncStatus {
  settings: SyncSettings;
  signed_in: boolean;
  pending: number;
  conflicts: number;
  cursor: number;
  last_sync_at: string | null;
  last_error: string | null;
}

/** The result of one sync cycle. */
export interface SyncOutcome {
  pushed: number;
  pulled: number;
  applied: number;
  conflicts: number;
  cursor: number;
}

/** A conflict between the local and the remote copy of a record. */
export interface ConflictView {
  record_id: string;
  kind: RecordKind;
  server_rev: number;
  observed_at: string;
  local: unknown | null;
  remote: unknown | null;
}
