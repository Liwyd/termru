import { invoke } from '@tauri-apps/api/core';
import type { Channel } from '@tauri-apps/api/core';
import type {
  ConflictView,
  Group,
  Host,
  HostKeyPromptRequest,
  Identity,
  IdentityBinding,
  Tag,
  Preferences,
  SessionGeometry,
  SessionInfo,
  SessionSecrets,
  SessionUpdate,
  SyncOutcome,
  SyncStatus,
  WorkspaceDoc,
} from './types';

/**
 * Typed wrappers around the Tauri command surface. Argument names are
 * camelCase; Tauri maps them to the Rust `snake_case` parameters.
 */
export const api = {
  hostList: () => invoke<Host[]>('host_list'),
  recentList: () => invoke<Host[]>('recent_list'),
  hostSave: (host: Host) => invoke<Host>('host_save', { host }),
  hostDelete: (id: string) => invoke<void>('host_delete', { id }),
  hostSetFavorite: (id: string, favorite: boolean) =>
    invoke<void>('host_set_favorite', { id, favorite }),

  groupList: () => invoke<Group[]>('group_list'),
  groupSave: (group: Group) => invoke<Group>('group_save', { group }),
  groupDelete: (id: string) => invoke<void>('group_delete', { id }),

  tagList: () => invoke<Tag[]>('tag_list'),
  tagSave: (tag: Tag) => invoke<Tag>('tag_save', { tag }),
  tagDelete: (id: string) => invoke<void>('tag_delete', { id }),

  identityList: () => invoke<Identity[]>('identity_list'),
  identitySave: (identity: Identity) => invoke<Identity>('identity_save', { identity }),
  identityDelete: (id: string) => invoke<void>('identity_delete', { id }),
  identityBind: (id: string, keyPath: string) => invoke<void>('identity_bind', { id, keyPath }),
  identityUnbind: (id: string) => invoke<void>('identity_unbind', { id }),
  identityBindings: () => invoke<IdentityBinding[]>('identity_bindings'),

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

  layoutSave: (doc: WorkspaceDoc) => invoke<void>('layout_save', { doc }),
  layoutLoad: () => invoke<WorkspaceDoc | null>('layout_load'),

  sessionAttach: (id: string, channel: Channel<SessionUpdate>) =>
    invoke<void>('session_attach', { id, channel }),
  windowOpenSession: (id: string, title: string) =>
    invoke<string>('window_open_session', { id, title }),
  windowClose: (label: string) => invoke<void>('window_close', { label }),

  secretSet: (hostId: string, kind: 'password' | 'passphrase', value: string) =>
    invoke<void>('secret_set', { hostId, kind, value }),
  secretDelete: (hostId: string, kind: 'password' | 'passphrase') =>
    invoke<void>('secret_delete', { hostId, kind }),
  secretHas: (hostId: string, kind: 'password' | 'passphrase') =>
    invoke<boolean>('secret_has', { hostId, kind }),

  syncStatus: () => invoke<SyncStatus>('sync_status'),
  syncSignIn: (serverUrl: string, email: string, password: string, deviceName: string) =>
    invoke<SyncStatus>('sync_sign_in', { serverUrl, email, password, deviceName }),
  syncSignOut: () => invoke<void>('sync_sign_out'),
  syncNow: () => invoke<SyncOutcome>('sync_now'),
  syncConflicts: () => invoke<ConflictView[]>('sync_conflicts'),
  syncResolveConflict: (recordId: string, keepLocal: boolean) =>
    invoke<void>('sync_resolve_conflict', { recordId, keepLocal }),
};

export type { HostKeyPromptRequest };
