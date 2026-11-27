import { describe, expect, it } from 'vitest';
import { HostForm } from './host-form.svelte';
import type { Host } from './types';

function host(overrides: Partial<Host> = {}): Host {
  return {
    id: 'h1',
    name: 'staging-api',
    address: '198.51.100.12',
    port: 22,
    username: 'dev',
    auth: { type: 'password' },
    group_id: 'g1',
    tags: ['t1', 't2'],
    favorite: true,
    description: 'Pre-release API',
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 4,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-02-02T00:00:00Z',
    deleted: false,
    ...overrides,
  };
}

describe('a form for an existing host', () => {
  it('starts clean, with the host in its fields', () => {
    const form = new HostForm(host(), { passwordStored: true });
    expect(form.isNew).toBe(false);
    expect(form.dirty).toBe(false);
    expect(form.name).toBe('staging-api');
    expect(form.portText).toBe('22');
    // 0 is an empty field: the placeholder says Automatic (30 s).
    expect(form.keepaliveText).toBe('');
    expect(form.savePassword).toBe(true);
    expect(form.errorCount).toBe(0);
  });

  it('becomes dirty on an edit and clean again when the edit is undone', () => {
    const form = new HostForm(host());
    form.name = 'staging-api-2';
    expect(form.dirty).toBe(true);
    form.name = 'staging-api';
    expect(form.dirty).toBe(false);
    // Whitespace around a value is not a change: it is saved trimmed.
    form.address = '  198.51.100.12 ';
    expect(form.dirty).toBe(false);
    // An empty port is the default 22, so it is not a change either.
    form.portText = '';
    expect(form.dirty).toBe(false);
    form.portText = '2222';
    expect(form.dirty).toBe(true);
  });

  it('counts a typed password, a tag, the favorite and the folder as changes', () => {
    const form = new HostForm(host());
    form.password = 'x';
    expect(form.dirty).toBe(true);
    form.password = '';
    form.toggleTag('t3');
    expect(form.dirty).toBe(true);
    form.toggleTag('t3');
    expect(form.dirty).toBe(false);
    form.favorite = false;
    expect(form.dirty).toBe(true);
    form.favorite = true;
    form.groupId = '';
    expect(form.dirty).toBe(true);
  });

  it("does not count the other method's leftovers: a password typed for a key host is ignored", () => {
    const form = new HostForm(host({ auth: { type: 'agent' } }));
    form.password = 'left over';
    expect(form.dirty).toBe(false);
  });

  it('builds the host to save on top of the loaded one, trimmed', () => {
    const form = new HostForm(host());
    form.name = '  prod-1  ';
    form.address = ' prod.example.com ';
    form.portText = '2200';
    form.keepaliveText = '45';
    form.description = '   ';
    const saved = form.toHost();
    expect(saved).toMatchObject({
      id: 'h1',
      name: 'prod-1',
      address: 'prod.example.com',
      port: 2200,
      keepalive_seconds: 45,
      description: null,
      sort_order: 4,
      created_at: '2026-01-01T00:00:00Z',
      group_id: 'g1',
      tags: ['t1', 't2'],
      favorite: true,
    });
  });

  it('turns a cleared port and keepalive into 22 and 0, never null', () => {
    const form = new HostForm(host());
    form.portText = '';
    form.keepaliveText = '';
    const saved = form.toHost();
    expect(saved.port).toBe(22);
    expect(saved.keepalive_seconds).toBe(0);
  });

  it('builds the auth method from the choice', () => {
    const form = new HostForm(host());
    expect(form.toHost().auth).toEqual({ type: 'password' });
    form.authType = 'private_key';
    form.identityId = 'i1';
    expect(form.toHost().auth).toEqual({ type: 'private_key', identity_id: 'i1' });
    form.authType = 'agent';
    expect(form.toHost().auth).toEqual({ type: 'agent' });
  });
});

describe('a form for a new host', () => {
  it('starts clean and empty, asks nothing yet, and keeps the password box ticked', () => {
    const form = new HostForm(null);
    expect(form.isNew).toBe(true);
    expect(form.dirty).toBe(false);
    expect(form.savePassword).toBe(true);
    expect(form.name).toBe('');
    expect(form.hostId).toMatch(/[0-9a-f-]{36}/);
    // Empty required fields are errors, but none is shown before it is left.
    expect(form.errorCount).toBe(3);
    expect(form.errorFor('name')).toBeNull();
  });
});

describe('inline validation', () => {
  it('shows an error once its field was left, not while the user is still typing', () => {
    const form = new HostForm(null);
    form.name = '';
    expect(form.errorFor('name')).toBeNull();
    form.touch('name');
    expect(form.errorFor('name')).toBe('Name is required.');
    form.name = 'web';
    expect(form.errorFor('name')).toBeNull();
    expect(form.errorFor('address')).toBeNull();
  });

  it('shows every error after a save was tried, and finds the first invalid field', () => {
    const form = new HostForm(null);
    form.submit();
    expect(form.errorFor('name')).toBe('Name is required.');
    expect(form.errorFor('address')).toBe('Address is required.');
    expect(form.errorFor('username')).toBe('Username is required.');
    expect(form.firstInvalid).toBe('name');
    form.name = 'web';
    expect(form.firstInvalid).toBe('address');
  });

  it('asks for an identity only for a key host', () => {
    const form = new HostForm(host());
    form.authType = 'private_key';
    expect(form.errors.identity).toBe('Choose an identity for key authentication.');
    form.identityId = 'i1';
    expect(form.errors.identity).toBeUndefined();
  });
});

describe('refusals of the backend', () => {
  it('puts a refusal next to its field until the field is edited', () => {
    const form = new HostForm(host());
    expect(form.refuse('address contains invalid characters (allowed: letters, digits)')).toBe(
      true,
    );
    expect(form.errorFor('address')).toContain('invalid characters');
    form.edited('address');
    expect(form.errorFor('address')).toBeNull();
  });

  it('leaves a refusal that is not about a field to the caller', () => {
    const form = new HostForm(host());
    expect(form.refuse('database is locked')).toBe(false);
    expect(form.serverErrors).toEqual({});
  });

  it('forgets old refusals when a save is tried again', () => {
    const form = new HostForm(host());
    form.refuse('port must be between 1 and 65535');
    form.submit();
    expect(form.serverErrors).toEqual({});
  });
});

describe('saved, parked and restored', () => {
  it('is clean again once the host was saved, and compares against what was saved', () => {
    const form = new HostForm(host());
    form.name = 'renamed';
    form.password = 'secret';
    expect(form.dirty).toBe(true);
    form.markSaved({ ...host(), name: 'renamed' });
    expect(form.dirty).toBe(false);
    expect(form.password).toBe('');
    form.name = 'renamed again';
    expect(form.dirty).toBe(true);
  });

  it('parks the typed values without the password, and comes back still dirty', () => {
    const original = host();
    const form = new HostForm(original);
    form.name = 'typed so far';
    form.portText = '2200';
    form.authType = 'private_key';
    form.identityId = 'i1';
    form.keepaliveText = '90';
    form.password = 'never parked';
    const draft = form.toDraft();
    expect(JSON.stringify(draft)).not.toContain('never parked');
    expect(draft.host.name).toBe('typed so far');
    expect(draft.host.port).toBe(2200);
    expect(draft.authType).toBe('private_key');
    expect(draft.identityId).toBe('i1');

    const back = new HostForm(original, { draft });
    expect(back.name).toBe('typed so far');
    expect(back.portText).toBe('2200');
    expect(back.authType).toBe('private_key');
    expect(back.identityId).toBe('i1');
    expect(back.keepaliveText).toBe('90');
    // The baseline is the saved host: the restored form still has unsaved changes.
    expect(back.dirty).toBe(true);
  });

  it('restores a draft of a new host as a new host, still dirty', () => {
    const form = new HostForm(null);
    form.name = 'new one';
    const back = new HostForm(null, { draft: form.toDraft() });
    expect(back.isNew).toBe(true);
    expect(back.name).toBe('new one');
    expect(back.dirty).toBe(true);
  });

  it('takes the answer of the credential store as the baseline, not as an edit', () => {
    const form = new HostForm(host());
    expect(form.savePassword).toBe(false);
    form.adoptStored(true);
    expect(form.savePassword).toBe(true);
    expect(form.dirty).toBe(false);
    // Unticking it afterwards is an edit.
    form.savePassword = false;
    expect(form.dirty).toBe(true);
    // Forgetting the password makes "not stored" the baseline.
    form.adoptStored(false);
    expect(form.dirty).toBe(false);
  });
});
