import { invoke } from '@tauri-apps/api/core';
import type { Channel } from '@tauri-apps/api/core';
import type {
  Host,
  HostKeyPromptRequest,
  Identity,
  Preferences,
  SessionGeometry,
  SessionInfo,
  SessionSecrets,
  SessionUpdate,
} from './types';

/**
 * Typed wrappers around the Tauri command surface. Argument names are
 * camelCase; Tauri maps them to the Rust `snake_case` parameters.
 */
export const api = {
  hostList: () => invoke<Host[]>('host_list'),
  hostSave: (host: Host) => invoke<Host>('host_save', { host }),
  hostDelete: (id: string) => invoke<void>('host_delete', { id }),
  hostSetFavorite: (id: string, favorite: boolean) =>
    invoke<void>('host_set_favorite', { id, favorite }),

  identityList: () => invoke<Identity[]>('identity_list'),
  identityBind: (id: string, keyPath: string) => invoke<void>('identity_bind', { id, keyPath }),
  identityUnbind: (id: string) => invoke<void>('identity_unbind', { id }),

  prefsGet: () => invoke<Preferences>('prefs_get'),
  prefsSet: (prefs: Preferences) => invoke<void>('prefs_set', { prefs }),

  sessionOpen: (
    hostId: string,
    options: { cols: number; rows: number; secrets: SessionSecrets },
    channel: Channel<SessionUpdate>,
  ) => invoke<string>('session_open', { hostId, options, channel }),
  sessionRetry: (id: string, geometry: SessionGeometry, channel: Channel<SessionUpdate>) =>
    invoke<void>('session_retry', { id, geometry, channel }),
  sessionDuplicate: (id: string, geometry: SessionGeometry, channel: Channel<SessionUpdate>) =>
    invoke<string>('session_duplicate', { id, geometry, channel }),
  sessionClose: (id: string) => invoke<void>('session_close', { id }),
  sessionWrite: (id: string, data: number[]) => invoke<void>('session_write', { id, data }),
  sessionResize: (id: string, cols: number, rows: number) =>
    invoke<void>('session_resize', { id, cols, rows }),
  sessionList: () => invoke<SessionInfo[]>('session_list'),

  hostKeyPromptResolve: (id: string, trust: boolean) =>
    invoke<boolean>('host_key_prompt_resolve', { id, trust }),
};

export type { HostKeyPromptRequest };
