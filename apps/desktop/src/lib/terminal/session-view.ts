/**
 * What the Sessions page says about a session: status words, the lines of
 * the pane footer and the status bar. Pure functions of the store's data, so
 * the wording is in one place and testable without a DOM.
 */

import type { TreeNode } from '../pane-tree';
import type { Host, Identity, SessionStatus } from '../types';
import type { Pane, Tab } from '../stores/sessions.svelte';

/** How a status looks: it picks the colour and the glyph. */
export type StatusTone = 'ready' | 'busy' | 'failed' | 'closed';

export function statusTone(status: SessionStatus): StatusTone {
  switch (status.state) {
    case 'ready':
      return 'ready';
    case 'connecting':
    case 'authenticating':
    case 'reconnecting':
      return 'busy';
    case 'failed':
      return 'failed';
    case 'closed':
      return 'closed';
  }
}

/** The one-word status of a pane (colour is never the only signal). */
export function statusLabel(pane: Pick<Pane, 'status' | 'restored' | 'host'>): string {
  if (!pane.host) return 'Empty';
  const status = pane.status;
  switch (status.state) {
    case 'ready':
      return 'Connected';
    case 'connecting':
      return 'Connecting';
    case 'authenticating':
      return 'Authenticating';
    case 'reconnecting':
      return 'Reconnecting';
    case 'failed':
      return 'Error';
    case 'closed':
      return pane.restored ? 'Not connected' : 'Disconnected';
  }
}

/** The pane carries a connected session that takes input. */
export function isInputReady(pane: Pick<Pane, 'status' | 'host' | 'dead'>): boolean {
  return pane.host !== null && pane.status.state === 'ready' && !pane.dead;
}

export const ENCODING_LABEL = 'UTF-8';
export const LINE_ENDING_LABEL = 'LF';

/**
 * The left half of the pane footer (frame 3): `INPUT READY · UTF-8 · LF`
 * while the pane takes input, the reason it does not otherwise.
 */
export function inputLine(
  pane: Pick<Pane, 'status' | 'host' | 'dead' | 'restored' | 'awaitingReconnect'>,
): string {
  if (!pane.host) return 'NO SESSION';
  if (isInputReady(pane)) return `INPUT READY · ${ENCODING_LABEL} · ${LINE_ENDING_LABEL}`;
  const status = pane.status;
  switch (status.state) {
    case 'connecting':
      return 'CONNECTING · INPUT OFF';
    case 'authenticating':
      return 'AUTHENTICATING · INPUT OFF';
    case 'reconnecting':
      return `RECONNECTING · ATTEMPT ${status.attempt}`;
    case 'failed':
      return 'CONNECTION FAILED · INPUT OFF';
    case 'closed':
      if (pane.awaitingReconnect) return 'CONNECTION LOST · RETRYING';
      return pane.restored ? 'NOT CONNECTED · INPUT OFF' : 'DISCONNECTED · INPUT OFF';
    case 'ready':
      return 'DISCONNECTED · INPUT OFF';
  }
}

/** `132 × 32`, or an empty string until the grid is measured. */
export function sizeLabel(cols: number, rows: number): string {
  return cols > 0 && rows > 0 ? `${cols} × ${rows}` : '';
}

/** The shell segment of the pane header: the remote's own title when it set one. */
export function shellLabel(pane: Pick<Pane, 'terminalTitle'>): string {
  return pane.terminalTitle ?? 'shell';
}

/** `ops@192.0.2.10:22` (an IPv6 address is bracketed so the port is clear). */
export function sshTarget(host: Pick<Host, 'username' | 'address' | 'port'>): string {
  const address = host.address.includes(':') ? `[${host.address}]` : host.address;
  return `${host.username}@${address}:${host.port}`;
}

/** The command that reaches the host from a shell: for the clipboard. */
export function sshCommand(host: Pick<Host, 'username' | 'address' | 'port'>): string {
  const port = host.port === 22 ? '' : ` -p ${host.port}`;
  return `ssh${port} ${host.username}@${host.address}`;
}

/** `just now`, `12 min ago`, `3 h ago`, `2 d ago`. */
export function relativeAge(since: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - since) / 1000));
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${Math.max(1, minutes)} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.round(hours / 24)} d ago`;
}

/** The label of how the host authenticates, for the status bar. */
function authLine(host: Host, identities: Identity[]): string | null {
  if (host.auth.type === 'agent') return 'SSH agent';
  if (host.auth.type === 'password') return null;
  const { identity_id: identityId } = host.auth;
  const identity = identities.find((entry) => entry.id === identityId && !entry.deleted);
  return identity ? `Identity: ${identity.name}` : null;
}

/**
 * The status bar line of the Sessions page (frames 2 and 3):
 *
 * - one pane: `atlas-prod · Connected 12 min ago · Identity: work-ed25519`
 * - split: `atlas-prod · 2 panes / 1 session · relay-edge connected in background`
 */
export function sessionsContext(
  tabs: readonly Tab[],
  active: Tab | null,
  now: number,
  identities: Identity[],
): string {
  if (!active) return 'No open sessions';
  const parts: string[] = [active.title];
  const panes = active.panes;
  if (panes.length > 1) {
    const live = panes.filter((pane) => pane.sessionId !== null).length;
    parts.push(`${panes.length} panes / ${live} session${live === 1 ? '' : 's'}`);
  } else {
    const pane = panes[0];
    if (pane) {
      if (pane.status.state === 'ready' && pane.connectedAt != null) {
        parts.push(`Connected ${relativeAge(pane.connectedAt, now)}`);
      } else {
        parts.push(statusLabel(pane));
      }
      const auth = pane.host ? authLine(pane.host, identities) : null;
      if (auth) parts.push(auth);
    }
  }
  const background = tabs.filter(
    (tab) => tab.id !== active.id && tab.panes.some((pane) => pane.status.state === 'ready'),
  );
  if (panes.length > 1 && background.length > 0) {
    const names = background.map((tab) => tab.title);
    const shown =
      names.length > 2 ? `${names.slice(0, 2).join(', ')} +${names.length - 2}` : names.join(', ');
    parts.push(`${shown} connected in background`);
  }
  return parts.join(' · ');
}

/** The directions of every split of a tree. */
function directions(node: TreeNode): Set<'columns' | 'rows'> {
  if (node.kind === 'leaf') return new Set();
  return new Set([node.direction, ...directions(node.first), ...directions(node.second)]);
}

/**
 * What the split is called in the hint (frame 3 calls two panes side by
 * side a vertical split: the divider is vertical).
 */
export function splitKind(root: TreeNode): string {
  const found = directions(root);
  if (found.size > 1) return 'Mixed split';
  return found.has('rows') ? 'Horizontal split' : 'Vertical split';
}

/** `⌘⌥ ← →` or `Ctrl Alt ← →`: how to move between panes, as bound. */
export function paneMoveHint(next: string, previous: string): string {
  const arrows = (label: string) => label.replace('ArrowRight', '→').replace('ArrowLeft', '←');
  const a = arrows(next);
  const b = arrows(previous);
  const prefix = a.slice(0, -1);
  const bothArrows = /[→←]$/.test(a) && /[→←]$/.test(b);
  if (bothArrows && b.slice(0, -1) === prefix) {
    const modifiers = prefix.replace(/\+/g, ' ').trim();
    return `${modifiers} ${b.slice(-1)} ${a.slice(-1)}`.trim();
  }
  return `${previous} / ${next}`;
}
