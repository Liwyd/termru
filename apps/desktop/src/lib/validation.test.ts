import { describe, expect, it } from 'vitest';
import {
  addressError,
  describeAddress,
  descriptionError,
  fieldOfBackendError,
  keepaliveError,
  keepaliveLabel,
  nameError,
  parseKeepalive,
  parsePort,
  portError,
  termError,
  usernameError,
  validateHost,
  type HostFormValues,
} from './validation';

const valid: HostFormValues = {
  name: 'web-1',
  address: 'web-1.example.com',
  portText: '22',
  username: 'deploy',
  authType: 'password',
  identityId: '',
  term: 'xterm-256color',
  keepaliveText: '',
  description: '',
};

describe('the host rules, as validate.rs states them', () => {
  it('accepts a valid host', () => {
    expect(validateHost(valid)).toEqual({});
  });

  it('names: required, at most 128 characters, no control characters', () => {
    expect(nameError('')).toBe('Name is required.');
    expect(nameError('   ')).toBe('Name is required.');
    expect(nameError('x'.repeat(128))).toBeNull();
    expect(nameError('x'.repeat(129))).toContain('at most 128');
    expect(nameError('bad\nname')).toContain('control characters');
    expect(nameError('a\u0000b')).toContain('control characters');
  });

  it('addresses: the characters the backend allows, and no leading or trailing dash', () => {
    for (const ok of ['example.com', '192.168.1.10', 'fe80::1', 'internal_host-1.corp']) {
      expect(addressError(ok), ok).toBeNull();
    }
    expect(addressError('')).toBe('Address is required.');
    expect(addressError('host name')).toContain("letters, digits, '.'");
    expect(addressError('host/name')).not.toBeNull();
    expect(addressError('host;rm -rf')).not.toBeNull();
    expect(addressError('-bad.com')).toContain("must not start or end with '-'");
    expect(addressError('bad-')).toContain("must not start or end with '-'");
    expect(addressError('a'.repeat(254))).toContain('at most 253');
    // The editor saves the trimmed address, so the rule applies to it.
    expect(addressError('  example.com  ')).toBeNull();
  });

  it('usernames: letters, digits and . - _ @', () => {
    expect(usernameError('deploy')).toBeNull();
    expect(usernameError('svc-account@corp')).toBeNull();
    expect(usernameError('')).toBe('Username is required.');
    expect(usernameError('user name')).toContain("letters, digits, '.'");
    expect(usernameError('user;id')).not.toBeNull();
  });

  it('ports: whole numbers from 1 to 65535, empty means 22', () => {
    expect(parsePort('')).toBe(22);
    expect(parsePort(' 2222 ')).toBe(2222);
    expect(parsePort('65535')).toBe(65535);
    for (const bad of ['0', '65536', '-1', '22.5', 'ssh', '1e3']) {
      expect(parsePort(bad), bad).toBeNull();
      expect(portError(bad), bad).toBe('Port must be a whole number from 1 to 65535.');
    }
    expect(portError('')).toBeNull();
  });

  it('keepalive: 0 (automatic) up to 3600 seconds, empty means 0', () => {
    expect(parseKeepalive('')).toBe(0);
    expect(parseKeepalive('0')).toBe(0);
    expect(parseKeepalive('3600')).toBe(3600);
    for (const bad of ['3601', '-5', '1.5', 'x']) {
      expect(parseKeepalive(bad), bad).toBeNull();
      expect(keepaliveError(bad), bad).toContain('0 (automatic)');
    }
  });

  it('shows 0 as Automatic (30 s)', () => {
    expect(keepaliveLabel(0)).toBe('Automatic (30 s)');
    expect(keepaliveLabel(45)).toBe('Every 45 s');
    expect(keepaliveLabel(null)).toBe('');
  });

  it('terminal type and comment', () => {
    expect(termError('xterm-256color')).toBeNull();
    expect(termError('')).toBe('Terminal type is required.');
    expect(termError('xterm;rm')).not.toBeNull();
    expect(descriptionError('x'.repeat(2000))).toBeNull();
    expect(descriptionError('x'.repeat(2001))).toContain('at most 2000');
    expect(descriptionError('a\0b')).toContain('NUL');
  });

  it('a key host needs an identity', () => {
    expect(validateHost({ ...valid, authType: 'private_key' }).identity).toBe(
      'Choose an identity for key authentication.',
    );
    expect(
      validateHost({ ...valid, authType: 'private_key', identityId: 'i1' }).identity,
    ).toBeUndefined();
    // Another method has no identity to choose.
    expect(validateHost({ ...valid, authType: 'agent' }).identity).toBeUndefined();
  });

  it('reports every broken field at once', () => {
    const errors = validateHost({
      ...valid,
      name: '',
      address: 'a b',
      portText: '0',
      username: '',
    });
    expect(Object.keys(errors).sort()).toEqual(['address', 'name', 'port', 'username']);
  });
});

describe('backend refusals', () => {
  it('maps the wording of validate.rs onto its field', () => {
    expect(fieldOfBackendError('name must not be empty')).toBe('name');
    expect(fieldOfBackendError('address contains invalid characters (allowed: letters)')).toBe(
      'address',
    );
    expect(fieldOfBackendError('port must be between 1 and 65535')).toBe('port');
    expect(fieldOfBackendError('username contains invalid characters')).toBe('username');
    expect(fieldOfBackendError('term must not be empty')).toBe('term');
    expect(
      fieldOfBackendError('keepalive interval must be 0 (automatic) or at most 3600 seconds'),
    ).toBe('keepalive');
    expect(fieldOfBackendError('description must be at most 2000 characters')).toBe('description');
  });

  it('leaves a message about something else unmapped', () => {
    expect(fieldOfBackendError('database is locked')).toBeNull();
    expect(fieldOfBackendError('could not read the host store')).toBeNull();
  });
});

describe('the line under a valid address', () => {
  it('names the documentation ranges, as frame 4 does', () => {
    expect(describeAddress('198.51.100.12')).toBe('Valid address · documentation range');
    expect(describeAddress('192.0.2.10')).toBe('Valid address · documentation range');
    expect(describeAddress('203.0.113.30')).toBe('Valid address · documentation range');
    expect(describeAddress('2001:db8::1')).toBe('Valid address · documentation range');
  });

  it('tells an address from a hostname, and says nothing for an invalid one', () => {
    expect(describeAddress('10.1.2.3')).toBe('Valid address');
    expect(describeAddress('fe80::1')).toBe('Valid address');
    expect(describeAddress('example.com')).toBe('Valid hostname');
    expect(describeAddress('')).toBeNull();
    expect(describeAddress('bad host')).toBeNull();
  });
});
