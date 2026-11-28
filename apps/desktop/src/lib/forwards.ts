/** How a port forward is written, and which of a host's fields need approval. */

import { isLoopbackBind } from './validation';
import type { ExecFields, Forward, ForwardKind, ForwardUpdate, Host } from './types';

/** `L`, `R`, `D`: the letters of `ssh -L`, `-R`, `-D`. */
export const KIND_LETTER: Record<ForwardKind, string> = {
  local: 'L',
  remote: 'R',
  dynamic: 'D',
};

export const KIND_LABEL: Record<ForwardKind, string> = {
  local: 'Local (-L)',
  remote: 'Remote (-R)',
  dynamic: 'Dynamic (-D)',
};

/** What a forward without a bind address listens on. */
export function defaultBind(kind: ForwardKind): string {
  return kind === 'remote' ? 'localhost' : '127.0.0.1';
}

/** `host:port`, with an IPv6 address in brackets. */
export function hostPort(host: string, port: number | string): string {
  const bare = host.startsWith('[') ? host : host.includes(':') ? `[${host}]` : host;
  return `${bare}:${port}`;
}

/** Where a forward listens, as `address:port`. */
export function bindText(forward: Pick<Forward, 'kind' | 'bind_address' | 'bind_port'>): string {
  const address = forward.bind_address?.trim() || defaultBind(forward.kind);
  return hostPort(address, forward.bind_port);
}

/**
 * One line for a forward: `L 127.0.0.1:8080 → db:5432`, or
 * `D 127.0.0.1:1080 · SOCKS` for a dynamic one (it has no target).
 */
export function forwardLine(forward: Forward): string {
  const head = `${KIND_LETTER[forward.kind]} ${bindText(forward)}`;
  if (forward.kind === 'dynamic') return `${head} · SOCKS`;
  const target = hostPort(forward.target_host ?? '?', forward.target_port ?? '?');
  return `${head} → ${target}`;
}

/**
 * The `ssh` option a forward stands for: `-L 127.0.0.1:8080:db:5432`,
 * `-R 9000:localhost:3000`, `-D 1080`. Null while the row is not complete.
 */
export function sshForwardFlag(row: {
  kind: ForwardKind;
  bindAddress: string;
  bindPortText: string;
  targetHost: string;
  targetPortText: string;
}): string | null {
  const port = row.bindPortText.trim();
  if (!/^\d+$/.test(port)) return null;
  const bind = row.bindAddress.trim();
  const listen = bind ? `${hostPort(bind, port)}` : port;
  if (row.kind === 'dynamic') return `-D ${listen}`;
  const host = row.targetHost.trim();
  const targetPort = row.targetPortText.trim();
  if (!host || !/^\d+$/.test(targetPort)) return null;
  return `-${row.kind === 'local' ? 'L' : 'R'} ${listen}:${hostPort(host, targetPort)}`;
}

/** The enabled forwards of a host whose listener is not loopback only. */
export function exposedForwards(forwards: readonly Forward[] | undefined): Forward[] {
  return (forwards ?? []).filter(
    (forward) => forward.enabled !== false && !isLoopbackBind(forward.bind_address),
  );
}

/** The fields of a host that need this device's approval (`Host::exec_fields`). */
export function execFieldsOf(host: Host): ExecFields {
  return {
    startup_command: host.startup_command ?? null,
    agent_forward: host.agent_forward === true,
    env: host.env ?? [],
    forwards: exposedForwards(host.forwards),
  };
}

/** The host asks for something that needs approval. */
export function hasExecFields(host: Host): boolean {
  const fields = execFieldsOf(host);
  return (
    fields.startup_command !== null ||
    fields.agent_forward ||
    fields.env.length > 0 ||
    fields.forwards.length > 0
  );
}

/** The state of one forward for the toolbar. */
export type ForwardState =
  | { state: 'listening'; address: string; port: number }
  | { state: 'failed'; reason: string }
  /** The session is not up yet, or the forward has not reported. */
  | { state: 'pending' }
  /** Not started: it is exposed and this device has not approved it. */
  | { state: 'blocked' };

export interface ForwardRow {
  /** Index in the host's list (the id the backend reports under). */
  id: number;
  forward: Forward;
  line: string;
  status: ForwardState;
}

/**
 * The enabled forwards of a host with what became of each. `updates` are what
 * the session reported, `execPending` says the session runs without the
 * approval-gated fields.
 */
export function forwardRows(
  host: Host | null,
  updates: readonly ForwardUpdate[] | undefined,
  execPending: boolean,
): ForwardRow[] {
  if (!host) return [];
  const byId = new Map((updates ?? []).map((update) => [update.id, update]));
  const rows: ForwardRow[] = [];
  (host.forwards ?? []).forEach((forward, id) => {
    if (forward.enabled === false) return;
    const update = byId.get(id);
    let status: ForwardState = { state: 'pending' };
    if (execPending && !isLoopbackBind(forward.bind_address)) status = { state: 'blocked' };
    else if (update?.state === 'listening') {
      status = { state: 'listening', address: update.address, port: update.port };
    } else if (update?.state === 'failed') status = { state: 'failed', reason: update.reason };
    rows.push({ id, forward, line: forwardLine(forward), status });
  });
  return rows;
}

/** What the forwards of a session amount to: the glyph, the colour and the words. */
export type ForwardsTone = 'bad' | 'warn' | 'ok' | 'wait';

export interface ForwardsSummary {
  tone: ForwardsTone;
  /** `2 forwards`, `1 of 2 failed`, `1 needs approval`. */
  label: string;
}

/** The state of the whole set of forwards of a session (the toolbar chip). */
export function forwardsSummary(rows: readonly ForwardRow[]): ForwardsSummary {
  const failed = rows.filter((row) => row.status.state === 'failed').length;
  const blocked = rows.filter((row) => row.status.state === 'blocked').length;
  const up = rows.filter((row) => row.status.state === 'listening').length;
  if (failed > 0) return { tone: 'bad', label: `${failed} of ${rows.length} failed` };
  if (blocked > 0) {
    return { tone: 'warn', label: `${blocked} need${blocked === 1 ? 's' : ''} approval` };
  }
  return {
    tone: up === rows.length ? 'ok' : 'wait',
    label: `${rows.length} ${rows.length === 1 ? 'forward' : 'forwards'}`,
  };
}

/**
 * The forwards in the pane footer, in one line: the first forward as it is
 * written (`L 127.0.0.1:5432 → db:5432`) and how many more there are
 * (`+2`). Empty when the host has none.
 */
export function forwardFootLine(rows: readonly ForwardRow[]): string {
  if (rows.length === 0) return '';
  const rest = rows.length - 1;
  return rest > 0 ? `${rows[0].line} +${rest}` : rows[0].line;
}
