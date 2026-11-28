import { describe, expect, it } from 'vitest';
import {
  bindText,
  execFieldsOf,
  exposedForwards,
  forwardLine,
  forwardRows,
  hasExecFields,
  hostPort,
  sshForwardFlag,
} from './forwards';
import type { Forward, Host } from './types';

function forward(over: Partial<Forward> = {}): Forward {
  return {
    kind: 'local',
    bind_port: 8080,
    target_host: 'db',
    target_port: 5432,
    enabled: true,
    ...over,
  };
}

function host(over: Partial<Host> = {}): Host {
  return {
    id: 'h1',
    name: 'atlas',
    address: 'atlas.test',
    port: 22,
    username: 'ops',
    auth: { type: 'agent' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
    ...over,
  };
}

describe('how a forward is written', () => {
  it('shows where it listens and where it leads', () => {
    expect(forwardLine(forward({ bind_address: '127.0.0.1' }))).toBe('L 127.0.0.1:8080 → db:5432');
    expect(forwardLine(forward())).toBe('L 127.0.0.1:8080 → db:5432');
    expect(
      forwardLine(forward({ kind: 'remote', bind_port: 9000, target_host: 'localhost' })),
    ).toBe('R localhost:9000 → localhost:5432');
    expect(forwardLine({ kind: 'dynamic', bind_port: 1080, enabled: true })).toBe(
      'D 127.0.0.1:1080 · SOCKS',
    );
    expect(forwardLine(forward({ bind_address: '::1' }))).toBe('L [::1]:8080 → db:5432');
    expect(bindText(forward({ bind_address: '[::1]' }))).toBe('[::1]:8080');
    expect(hostPort('fe80::1', 22)).toBe('[fe80::1]:22');
  });

  it('is the option ssh takes', () => {
    const row = {
      kind: 'local' as const,
      bindAddress: '',
      bindPortText: '8080',
      targetHost: 'db',
      targetPortText: '5432',
    };
    expect(sshForwardFlag(row)).toBe('-L 8080:db:5432');
    expect(sshForwardFlag({ ...row, bindAddress: '0.0.0.0' })).toBe('-L 0.0.0.0:8080:db:5432');
    expect(sshForwardFlag({ ...row, kind: 'remote' })).toBe('-R 8080:db:5432');
    expect(sshForwardFlag({ ...row, kind: 'dynamic' })).toBe('-D 8080');
    expect(sshForwardFlag({ ...row, bindAddress: '127.0.0.1', kind: 'dynamic' })).toBe(
      '-D 127.0.0.1:8080',
    );
    // Not complete yet: nothing to show.
    expect(sshForwardFlag({ ...row, bindPortText: '' })).toBeNull();
    expect(sshForwardFlag({ ...row, targetHost: '' })).toBeNull();
    expect(sshForwardFlag({ ...row, targetPortText: 'x' })).toBeNull();
  });
});

describe('what needs approval on this device', () => {
  it('is the startup command, agent forwarding, the environment and exposed forwards', () => {
    expect(hasExecFields(host())).toBe(false);
    // A forward on loopback (or a disabled one) is not exec-like.
    expect(
      hasExecFields(
        host({
          forwards: [forward(), forward({ bind_address: '0.0.0.0', enabled: false })],
        }),
      ),
    ).toBe(false);
    expect(hasExecFields(host({ startup_command: 'tmux' }))).toBe(true);
    expect(hasExecFields(host({ agent_forward: true }))).toBe(true);
    expect(hasExecFields(host({ env: [{ name: 'A', value: 'b' }] }))).toBe(true);
    expect(hasExecFields(host({ forwards: [forward({ bind_address: '0.0.0.0' })] }))).toBe(true);
  });

  it('lists exactly what is withheld', () => {
    const exposed = forward({ bind_address: '10.0.0.5', bind_port: 9000 });
    const fields = execFieldsOf(
      host({
        startup_command: 'tmux',
        agent_forward: true,
        env: [{ name: 'A', value: 'b' }],
        forwards: [forward(), exposed],
      }),
    );
    expect(fields).toEqual({
      startup_command: 'tmux',
      agent_forward: true,
      env: [{ name: 'A', value: 'b' }],
      forwards: [exposed],
    });
    expect(exposedForwards(undefined)).toEqual([]);
  });
});

describe('the forwards of a session', () => {
  const atlas = host({
    forwards: [
      forward({ bind_address: '127.0.0.1' }),
      forward({ kind: 'dynamic', bind_port: 1080, target_host: undefined, target_port: undefined }),
      forward({ bind_address: '0.0.0.0', bind_port: 9090, enabled: false }),
      forward({ bind_address: '0.0.0.0', bind_port: 9091 }),
    ],
  });

  it('lists the enabled ones under the index the backend reports them by', () => {
    const rows = forwardRows(
      atlas,
      [
        { state: 'listening', id: 0, kind: 'local', address: '127.0.0.1', port: 8080 },
        { state: 'failed', id: 1, kind: 'dynamic', reason: 'address in use' },
      ],
      false,
    );
    expect(rows.map((row) => row.id)).toEqual([0, 1, 3]);
    expect(rows[0].status).toEqual({ state: 'listening', address: '127.0.0.1', port: 8080 });
    expect(rows[1].status).toEqual({ state: 'failed', reason: 'address in use' });
    // Not reported yet: it is starting.
    expect(rows[2].status).toEqual({ state: 'pending' });
    expect(rows[1].line).toBe('D 127.0.0.1:1080 · SOCKS');
  });

  it('marks an exposed forward as not approved when the session runs without the fields', () => {
    const rows = forwardRows(atlas, [], true);
    expect(rows.map((row) => row.status.state)).toEqual(['pending', 'pending', 'blocked']);
  });

  it('has nothing for a pane without a host or a host without forwards', () => {
    expect(forwardRows(null, [], false)).toEqual([]);
    expect(forwardRows(host(), undefined, false)).toEqual([]);
  });
});
