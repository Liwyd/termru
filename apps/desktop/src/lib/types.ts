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
