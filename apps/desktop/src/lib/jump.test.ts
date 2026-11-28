import { describe, expect, it } from 'vitest';
import { resolveRoute, type JumpNode } from './jump';

/** `resolve_jump_chain` of the core, as the editor and the connect dialog run it. */

function library(
  entries: Record<string, [string, string[]?]>,
): (id: string) => JumpNode | undefined {
  return (id) => {
    const entry = entries[id];
    return entry ? { name: entry[0], jump_chain: entry[1] ?? [] } : undefined;
  };
}

describe('the route to a host', () => {
  it('is empty for a direct connection', () => {
    expect(resolveRoute('app', 'app', [], library({}))).toEqual({ ok: true, route: [] });
  });

  it('lists the hops in order', () => {
    const lookup = library({ a: ['alpha'], b: ['bravo'] });
    expect(resolveRoute('app', 'app', ['a', 'b'], lookup)).toEqual({ ok: true, route: ['a', 'b'] });
  });

  it("follows the first hop's own route first, and only the first hop's", () => {
    // bastion sits behind gateway; app goes through bastion: gateway, bastion.
    const lookup = library({
      gateway: ['gateway'],
      bastion: ['bastion', ['gateway']],
      other: ['other', ['gateway']],
    });
    expect(resolveRoute('app', 'app', ['bastion'], lookup)).toEqual({
      ok: true,
      route: ['gateway', 'bastion'],
    });
    // The later hops of a chain are reached from the hop before: their own
    // chains are not used (an explicit route stays explicit).
    const second = resolveRoute('app', 'app', ['gateway', 'other'], lookup);
    expect(second).toEqual({ ok: true, route: ['gateway', 'other'] });
  });

  it('refuses a hop that does not exist, and says whose', () => {
    const result = resolveRoute('app', 'app', ['gone'], library({}));
    expect(result).toEqual({
      ok: false,
      error: 'A jump host of app does not exist (was it deleted?).',
    });
    const later = resolveRoute('app', 'app', ['a', 'gone'], library({ a: ['alpha'] }));
    expect(later.ok).toBe(false);
    const nested = resolveRoute('app', 'app', ['a'], library({ a: ['alpha', ['gone']] }));
    expect(nested).toEqual({
      ok: false,
      error: 'A jump host of alpha does not exist (was it deleted?).',
    });
  });

  it('refuses a cycle', () => {
    const lookup = library({ a: ['alpha', ['b']], b: ['bravo', ['a']] });
    const result = resolveRoute('app', 'app', ['a'], lookup);
    expect(result.ok).toBe(false);
    expect(result.ok === false && result.error).toMatch(/is a cycle/);
    // A hop that leads back to the host itself.
    const back = resolveRoute('app', 'app', ['a'], library({ a: ['alpha', ['app']] }));
    expect(back).toEqual({ ok: false, error: 'The jump chain of alpha is a cycle.' });
  });

  it('refuses the host itself, a repeated hop and more than 8 hops', () => {
    const lookup = library({ a: ['alpha'], b: ['bravo'] });
    expect(resolveRoute('app', 'app', ['app'], lookup)).toEqual({
      ok: false,
      error: 'A host cannot be its own jump host.',
    });
    expect(resolveRoute('app', 'app', ['a', 'b', 'a'], lookup).ok).toBe(false);
    // The first hop's own route and the chain pass the same host.
    const twice = resolveRoute(
      'app',
      'app',
      ['b', 'a'],
      library({ a: ['alpha'], b: ['bravo', ['a']] }),
    );
    expect(twice).toEqual({
      ok: false,
      error: 'The route to app passes the same jump host twice.',
    });
    // Nine hosts, each behind the next.
    const deep: Record<string, [string, string[]?]> = {};
    for (let index = 0; index < 10; index += 1) {
      deep[`h${index}`] = [`h${index}`, index < 9 ? [`h${index + 1}`] : []];
    }
    const result = resolveRoute('app', 'app', ['h0'], library(deep));
    expect(result.ok).toBe(false);
    expect(result.ok === false && result.error).toMatch(/more than 8 hops|at most 8/);
  });
});
