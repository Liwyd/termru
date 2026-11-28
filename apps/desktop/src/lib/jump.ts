/**
 * The route a connection takes through jump hosts, resolved the way
 * `resolve_jump_chain` of the core does it (ADR 0010):
 *
 * - the first hop is reached from this machine and **its own chain is
 *   followed first** (`bastion` behind `gateway`: `gateway → bastion → app`);
 * - the later hops of a chain are reached from the hop before them, so their
 *   own chains are not used;
 * - a hop that does not exist, a cycle, a route that passes a host twice and
 *   a route longer than 8 hops are refused.
 *
 * The backend resolves it again when it builds the connection and stays the
 * authority; this copy lets the editor show the route and its problems next
 * to the picker, and the connect dialog know which hops to ask passwords for.
 */

import { MAX_JUMP_HOPS, jumpChainError } from './validation';

/** What the route walk needs to know about a host. */
export interface JumpNode {
  name: string;
  jump_chain?: readonly string[];
}

export type HostLookup = (id: string) => JumpNode | undefined;

export type RouteResult = { ok: true; route: string[] } | { ok: false; error: string };

/**
 * The hops from this machine to the host, in order (the host itself is not
 * in the list). `chain` is the host's own `jump_chain` as it is being edited,
 * which may differ from what the store holds.
 */
export function resolveRoute(
  hostId: string,
  hostName: string,
  chain: readonly string[],
  lookup: HostLookup,
): RouteResult {
  const structural = jumpChainError(hostId, chain);
  if (structural) return { ok: false, error: structural };
  const path: string[] = [];
  const visiting = [hostId];
  const walked = expand(hostName, chain, lookup, visiting, path, MAX_JUMP_HOPS);
  if (walked) return { ok: false, error: walked };
  if (path.length > MAX_JUMP_HOPS) {
    return {
      ok: false,
      error: `The route to ${hostName} has ${path.length} hops; at most ${MAX_JUMP_HOPS} are allowed.`,
    };
  }
  for (const [index, hop] of path.entries()) {
    if (hop === hostId) {
      return { ok: false, error: `The route to ${hostName} leads back to ${hostName}.` };
    }
    if (path.indexOf(hop) !== index) {
      return { ok: false, error: `The route to ${hostName} passes the same jump host twice.` };
    }
  }
  return { ok: true, route: path };
}

/** Walks the first hop's own chain, then the hops; returns an error message. */
function expand(
  owner: string,
  chain: readonly string[],
  lookup: HostLookup,
  visiting: string[],
  path: string[],
  budget: number,
): string | null {
  const [first, ...rest] = chain;
  if (first === undefined) return null;
  if (visiting.includes(first)) return `The jump chain of ${owner} is a cycle.`;
  if (budget === 0) return `The route to ${owner} has more than ${MAX_JUMP_HOPS} hops.`;
  const node = lookup(first);
  if (!node) return `A jump host of ${owner} does not exist (was it deleted?).`;
  visiting.push(first);
  const inner = expand(node.name, node.jump_chain ?? [], lookup, visiting, path, budget - 1);
  if (inner) return inner;
  path.push(first);
  for (const hop of rest) {
    if (!lookup(hop)) return `A jump host of ${owner} does not exist (was it deleted?).`;
    path.push(hop);
  }
  return null;
}
