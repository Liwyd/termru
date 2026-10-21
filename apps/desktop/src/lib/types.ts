/** TypeScript mirrors of the Rust DTOs (serde `snake_case` field names). */

export type AuthMethod =
  { type: 'password' } | { type: 'private_key'; identity_id: string } | { type: 'agent' };

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
  sort_order: number;
  created_at: string;
  updated_at: string;
  deleted: boolean;
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

export type ThemePreference = 'light' | 'dark' | 'system';
export type CursorStyle = 'block' | 'bar' | 'underline';

export interface TerminalPreferences {
  font_family: string;
  font_size: number;
  scrollback: number;
  cursor_blink: boolean;
  cursor_style: CursorStyle;
  copy_on_select: boolean;
}

export interface Preferences {
  theme: ThemePreference;
  terminal: TerminalPreferences;
  keybindings: Record<string, string>;
}

export type ErrorCategory = 'auth' | 'host_key' | 'network' | 'timeout' | 'protocol' | 'local';

export type SessionStatus =
  | { state: 'connecting' }
  | { state: 'authenticating' }
  | { state: 'ready' }
  | { state: 'reconnecting'; attempt: number; delay_ms: number; last_error: string }
  | { state: 'closed'; reason: string }
  | { state: 'failed'; category: ErrorCategory; message: string };

export type SessionUpdate =
  | { kind: 'status'; status: SessionStatus }
  | { kind: 'data'; base64: string }
  | { kind: 'exit'; code: number | null };

export interface SessionInfo {
  id: string;
  host_id: string;
  status: SessionStatus;
}

/** Secrets for one connection. Never persisted; memory only. */
export interface SessionSecrets {
  password?: string;
  passphrase?: string;
}

export interface SessionGeometry {
  cols: number;
  rows: number;
}

/** Payload of the `host-key-prompt` frontend event. */
export type HostKeyPromptRequest =
  | { state: 'unknown'; host: string; port: number; key: { key_type: string; fingerprint: string } }
  | {
      state: 'changed';
      host: string;
      port: number;
      old: { key_type: string; fingerprint: string };
      new: { key_type: string; fingerprint: string };
    };

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
