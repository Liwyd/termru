import { describe, expect, it } from 'vitest';
import {
  authOrderError,
  envNameError,
  envValueError,
  fieldOfBackendError,
  forwardPortError,
  forwardRowErrors,
  isIpv4Literal,
  isIpv6,
  isLoopbackBind,
  jumpChainError,
  startupCommandError,
  validateEnv,
  validateForwards,
  type ForwardRowValues,
} from './validation';

/** The checks mirror `termru-core/src/validate.rs` (data model v2, ADR 0010). */

function row(over: Partial<ForwardRowValues> = {}): ForwardRowValues {
  return {
    kind: 'local',
    bindAddress: '',
    bindPortText: '8080',
    targetHost: 'db.internal',
    targetPortText: '5432',
    enabled: true,
    ...over,
  };
}

describe('the jump chain', () => {
  it('allows up to 8 hops, each host once, and never the host itself', () => {
    expect(jumpChainError('me', [])).toBeNull();
    expect(jumpChainError('me', ['a', 'b', 'c'])).toBeNull();
    const eight = Array.from({ length: 8 }, (_, index) => `h${index}`);
    expect(jumpChainError('me', eight)).toBeNull();
    expect(jumpChainError('me', [...eight, 'h8'])).toBe('A jump chain may have at most 8 hops.');
    expect(jumpChainError('me', ['a', 'me'])).toBe('A host cannot be its own jump host.');
    expect(jumpChainError('me', ['a', 'b', 'a'])).toBe('A jump chain lists the same host twice.');
  });
});

describe('addresses a forward names', () => {
  it('knows IPv6 literals, with a zone id, and IPv4 without leading zeros', () => {
    expect(isIpv6('::1')).toBe(true);
    expect(isIpv6('fe80::1')).toBe(true);
    expect(isIpv6('2001:db8::8a2e:370:7334')).toBe(true);
    expect(isIpv6('::ffff:127.0.0.1')).toBe(true);
    expect(isIpv6('1.2.3.4')).toBe(false);
    expect(isIpv6('not:an:address')).toBe(false);
    expect(isIpv6('12345::1')).toBe(false);
    expect(isIpv4Literal('127.0.0.1')).toBe(true);
    expect(isIpv4Literal('256.0.0.1')).toBe(false);
    expect(isIpv4Literal('01.2.3.4')).toBe(false);
    expect(isIpv4Literal('1.2.3')).toBe(false);
  });

  it('tells loopback from an exposed listener, as Forward::binds_loopback does', () => {
    for (const loopback of [
      undefined,
      null,
      '',
      '  ',
      'localhost',
      'LOCALHOST',
      '127.0.0.1',
      '127.9.9.9',
      '::1',
      '[::1]',
      '0:0:0:0:0:0:0:1',
      '::1%lo',
      '::ffff:127.0.0.1',
    ]) {
      expect(isLoopbackBind(loopback), String(loopback)).toBe(true);
    }
    for (const exposed of [
      '0.0.0.0',
      '::',
      '192.168.1.5',
      '10.0.0.1',
      'fe80::1%eth0',
      'gateway.example.com',
      '*',
      '128.0.0.1',
    ]) {
      expect(isLoopbackBind(exposed), exposed).toBe(false);
    }
  });
});

describe('one port forward', () => {
  it('needs a bind port, and a target host and port unless it is dynamic', () => {
    expect(forwardRowErrors(row())).toEqual({});
    expect(forwardRowErrors(row({ bindPortText: '' })).bindPort).toBe('Bind port is required.');
    expect(forwardRowErrors(row({ bindPortText: '0' })).bindPort).toBe(
      'Bind port must be a whole number from 1 to 65535.',
    );
    expect(forwardRowErrors(row({ bindPortText: '70000' })).bindPort).toMatch(/65535/);
    expect(forwardRowErrors(row({ bindPortText: '80a' })).bindPort).toMatch(/whole number/);
    expect(forwardRowErrors(row({ targetHost: '' })).targetHost).toBe('Target host is required.');
    expect(forwardRowErrors(row({ targetPortText: '' })).targetPort).toBe(
      'Target port is required.',
    );
    // A dynamic forward has no target: whatever is in those fields is not checked.
    expect(forwardRowErrors(row({ kind: 'dynamic', targetHost: '', targetPortText: '' }))).toEqual(
      {},
    );
  });

  it('binds an IP address or localhost on this machine, but any name on the server', () => {
    expect(forwardRowErrors(row({ bindAddress: '0.0.0.0' }))).toEqual({});
    expect(forwardRowErrors(row({ bindAddress: '[::1]' }))).toEqual({});
    expect(forwardRowErrors(row({ bindAddress: 'localhost' }))).toEqual({});
    expect(forwardRowErrors(row({ bindAddress: 'lan.example.com' })).bindAddress).toBe(
      'A listener on this machine needs an IP address (or localhost).',
    );
    expect(forwardRowErrors(row({ kind: 'dynamic', bindAddress: 'lan' })).bindAddress).toMatch(
      /IP address/,
    );
    // `remote`: the address names a place on the server.
    expect(forwardRowErrors(row({ kind: 'remote', bindAddress: 'lan.example.com' }))).toEqual({});
    expect(forwardRowErrors(row({ bindAddress: 'bad host' })).bindAddress).toMatch(
      /letters, digits/,
    );
    expect(forwardRowErrors(row({ bindAddress: '[::1' })).bindAddress).toMatch(/closing/);
    expect(forwardRowErrors(row({ bindAddress: 'fe80::zz' })).bindAddress).toMatch(/IPv6/);
    expect(forwardRowErrors(row({ bindAddress: 'fe80::1%' })).bindAddress).toMatch(/zone/);
    expect(forwardRowErrors(row({ bindAddress: 'fe80::1%eth0' }))).toEqual({});
    expect(forwardRowErrors(row({ targetHost: '-db' })).targetHost).toMatch(/start or end/);
  });

  it('has no default port', () => {
    expect(forwardPortError('Port', ' 22 ')).toBeNull();
    expect(forwardPortError('Port', '')).toBe('Port is required.');
  });
});

describe('the forwards of a host', () => {
  it('reports the errors of each row, in order', () => {
    const result = validateForwards([row(), row({ bindPortText: '', bindAddress: '127.0.0.2' })]);
    expect(result.invalid).toBe(true);
    expect(result.rows[0]).toEqual({});
    expect(result.rows[1].bindPort).toBe('Bind port is required.');
    expect(validateForwards([row()]).invalid).toBe(false);
    expect(validateForwards([]).invalid).toBe(false);
  });

  it('refuses two forwards on one listener, the later one says so', () => {
    const twice = validateForwards([row(), row({ targetHost: 'other' })]);
    expect(twice.rows[0]).toEqual({});
    expect(twice.rows[1].bindPort).toBe('Another forward already listens on 127.0.0.1:8080.');
    // localhost is loopback: the same listener as 127.0.0.1 on this machine.
    expect(validateForwards([row(), row({ bindAddress: 'localhost' })]).invalid).toBe(true);
    // A remote forward listens on the server: another port space.
    expect(validateForwards([row(), row({ kind: 'remote' })]).invalid).toBe(false);
    // Another address, or another port, is another listener.
    expect(validateForwards([row(), row({ bindAddress: '0.0.0.0' })]).invalid).toBe(false);
    expect(validateForwards([row(), row({ bindPortText: '8081' })]).invalid).toBe(false);
  });

  it('allows at most 32', () => {
    const many = Array.from({ length: 33 }, (_, index) =>
      row({ bindPortText: String(2000 + index) }),
    );
    const result = validateForwards(many);
    expect(result.list).toBe('A host may have at most 32 port forwards.');
    expect(result.invalid).toBe(true);
    expect(validateForwards(many.slice(0, 32)).invalid).toBe(false);
  });
});

describe('environment variables', () => {
  it('names follow the POSIX rule and stay short', () => {
    expect(envNameError('LANG')).toBeNull();
    expect(envNameError('_x1')).toBeNull();
    expect(envNameError('')).toBe('Name is required.');
    expect(envNameError('1X')).toMatch(/not starting with a digit/);
    expect(envNameError('A-B')).toMatch(/letters, digits/);
    expect(envNameError('A'.repeat(65))).toMatch(/at most 64/);
    expect(envNameError('A'.repeat(64))).toBeNull();
  });

  it('values are short plain text', () => {
    expect(envValueError('')).toBeNull();
    expect(envValueError('en_US.UTF-8')).toBeNull();
    expect(envValueError('x'.repeat(1025))).toMatch(/at most 1024/);
    expect(envValueError('line\nbreak')).toMatch(/control characters/);
    expect(envValueError('tab\there')).toMatch(/control characters/);
  });

  it('refuses a name twice (the later row says so) and more than 32', () => {
    const rows = [
      { name: 'A', value: '1' },
      { name: 'B', value: '2' },
      { name: 'A', value: '3' },
    ];
    const result = validateEnv(rows);
    expect(result.rows[0]).toEqual({});
    expect(result.rows[2].name).toBe('A is listed twice.');
    expect(result.invalid).toBe(true);
    const many = Array.from({ length: 33 }, (_, index) => ({ name: `V${index}`, value: '' }));
    expect(validateEnv(many).list).toBe('A host may have at most 32 environment variables.');
    expect(validateEnv(many.slice(0, 32)).invalid).toBe(false);
    expect(validateEnv([]).invalid).toBe(false);
  });
});

describe('the startup command', () => {
  it('is one short line, or empty (none)', () => {
    expect(startupCommandError('')).toBeNull();
    expect(startupCommandError('   ')).toBeNull();
    expect(startupCommandError('tmux new -A -s main')).toBeNull();
    expect(startupCommandError('echo\thi')).toBeNull();
    expect(startupCommandError('a\nb')).toMatch(/single line/);
    expect(startupCommandError('a\u001bb')).toMatch(/single line/);
    expect(startupCommandError('x'.repeat(1025))).toMatch(/at most 1024/);
    expect(startupCommandError('x'.repeat(1024))).toBeNull();
  });
});

describe('the authentication order', () => {
  it('is Automatic, or a non-empty list that names each method once', () => {
    expect(authOrderError(null)).toBeNull();
    expect(authOrderError(['agent', 'password'])).toBeNull();
    expect(authOrderError([])).toBe('Name at least one method, or choose Automatic.');
    expect(authOrderError(['agent', 'agent'])).toBe('A method can be listed only once.');
  });
});

describe('what the backend refuses lands in the section it is about', () => {
  it.each([
    ['a jump chain may have at most 8 hops', 'jump'],
    ['a host cannot be its own jump host', 'jump'],
    ['a jump chain lists the same host twice', 'jump'],
    ['a jump host of web does not exist (was it deleted?)', 'jump'],
    ['the jump chain of web is a cycle', 'jump'],
    ['the route to web has 9 hops; at most 8 are allowed', 'jump'],
    ['the route to web leads back to web', 'jump'],
    ['the route to web passes the same jump host twice', 'jump'],
    ['forward bind port must be between 1 and 65535', 'forwards'],
    ['a local forward needs a target host', 'forwards'],
    ['a remote forward needs a target port between 1 and 65535', 'forwards'],
    ['a dynamic (SOCKS) forward has no target: the client of the proxy names it', 'forwards'],
    ['a local listener needs an IP address (or localhost) as its bind address', 'forwards'],
    ['bind address is not a valid IPv6 address', 'forwards'],
    ['target host contains invalid characters (allowed: letters)', 'forwards'],
    ['a host may have at most 32 port forwards', 'forwards'],
    ['two forwards listen on 127.0.0.1:8080', 'forwards'],
    ['environment variable name "1X" is invalid (use letters, digits and \'_\')', 'env'],
    ['environment variable A is listed twice', 'env'],
    ['environment variable names must be at most 64 characters', 'env'],
    ['a host may have at most 32 environment variables', 'env'],
    ['the value of A must be at most 1024 characters', 'env'],
    ['startup command must be a single line without control characters', 'startup'],
    ['startup command must be at most 1024 characters', 'startup'],
    ['the authentication order must name at least one method (leave it unset)', 'authOrder'],
    ['the authentication order names password twice', 'authOrder'],
  ])('%s', (message, field) => {
    expect(fieldOfBackendError(message)).toBe(field);
  });

  it('still maps the older fields, and leaves other failures alone', () => {
    expect(fieldOfBackendError('name must not be empty')).toBe('name');
    expect(fieldOfBackendError('a host may have at most 32 tags')).toBeNull();
    expect(fieldOfBackendError('could not read the host store')).toBeNull();
  });
});
