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

describe('the sections of data model v2', () => {
  const bastion = { name: 'bastion', jump_chain: [] as string[] };
  const lookup = (id: string) => (id === 'b1' ? bastion : undefined);

  function rich(): Host {
    return host({
      jump_chain: ['b1'],
      forwards: [
        {
          kind: 'local',
          bind_address: '127.0.0.1',
          bind_port: 5433,
          target_host: 'db',
          target_port: 5432,
          enabled: true,
        },
        { kind: 'dynamic', bind_port: 1080, enabled: false },
      ],
      env: [{ name: 'LANG', value: 'C.UTF-8' }],
      startup_command: 'tmux new -A -s main',
      agent_forward: true,
      auth_order: ['agent', 'password'],
    });
  }

  it('starts clean with the host as it is and saves it back unchanged', () => {
    const base = rich();
    const form = new HostForm(base, { lookup });
    expect(form.dirty).toBe(false);
    expect(form.errorCount).toBe(0);
    expect(form.jumpChain).toEqual(['b1']);
    expect(form.forwardRows.map((row) => [row.kind, row.bindPortText, row.enabled])).toEqual([
      ['local', '5433', true],
      ['dynamic', '1080', false],
    ]);
    expect(form.envRows.map((row) => [row.name, row.value])).toEqual([['LANG', 'C.UTF-8']]);
    expect(form.startupCommand).toBe('tmux new -A -s main');
    expect(form.agentForward).toBe(true);
    expect(form.authOrder).toEqual(['agent', 'password']);
    expect(form.toHost()).toEqual(base);
  });

  it('leaves a plain host exactly as it was: none of the new keys appear', () => {
    const form = new HostForm(host(), { lookup });
    const saved = form.toHost();
    for (const key of [
      'jump_chain',
      'forwards',
      'env',
      'startup_command',
      'agent_forward',
      'auth_order',
    ]) {
      expect(saved, key).not.toHaveProperty(key);
    }
    expect(saved).toEqual(host());
    // An untouched form is clean, and adding then removing something is clean again.
    const key = form.addForward();
    expect(form.dirty).toBe(true);
    form.removeForward(key);
    expect(form.dirty).toBe(false);
  });

  it('writes what was emptied as empty, not as the old value', () => {
    const form = new HostForm(rich(), { lookup });
    form.jumpChain = [];
    form.forwardRows = [];
    form.envRows = [];
    form.startupCommand = '  ';
    form.agentForward = false;
    form.authOrder = null;
    const saved = form.toHost();
    expect(saved.jump_chain).toEqual([]);
    expect(saved.forwards).toEqual([]);
    expect(saved.env).toEqual([]);
    expect(saved.startup_command).toBeNull();
    expect(saved.agent_forward).toBe(false);
    expect(saved.auth_order).toBeNull();
  });

  it('counts every section as a change', () => {
    const form = new HostForm(rich(), { lookup });
    const dirty = (change: () => void, undo: () => void) => {
      change();
      expect(form.dirty).toBe(true);
      undo();
      expect(form.dirty).toBe(false);
    };
    dirty(
      () => (form.startupCommand = 'htop'),
      () => (form.startupCommand = 'tmux new -A -s main'),
    );
    dirty(
      () => (form.agentForward = false),
      () => (form.agentForward = true),
    );
    dirty(
      () => (form.envRows[0].value = 'C'),
      () => (form.envRows[0].value = 'C.UTF-8'),
    );
    dirty(
      () => (form.forwardRows[0].bindPortText = '5434'),
      () => (form.forwardRows[0].bindPortText = '5433'),
    );
    dirty(
      () => form.moveAuth('agent', 1),
      () => form.moveAuth('agent', -1),
    );
    dirty(
      () => form.removeHop('b1'),
      () => form.addHop('b1'),
    );
  });

  describe('jump hosts', () => {
    it('adds a hop once, reorders, removes and stays within 8', () => {
      const form = new HostForm(host({ id: 'me' }), { lookup: () => ({ name: 'x' }) });
      form.addHop('a');
      form.addHop('b');
      form.addHop('a');
      expect(form.jumpChain).toEqual(['a', 'b']);
      form.moveHop('b', -1);
      expect(form.jumpChain).toEqual(['b', 'a']);
      // The ends do not move past the list.
      form.moveHop('b', -1);
      form.moveHop('a', 1);
      expect(form.jumpChain).toEqual(['b', 'a']);
      form.removeHop('b');
      expect(form.jumpChain).toEqual(['a']);
      for (const id of ['c', 'd', 'e', 'f', 'g', 'h', 'i']) form.addHop(id);
      expect(form.jumpChain).toHaveLength(8);
      expect(form.canAddHop).toBe(false);
    });

    it('resolves the route and says what is wrong with it', () => {
      const form = new HostForm(host({ id: 'me' }), {
        lookup: (id) =>
          id === 'bastion'
            ? { name: 'bastion', jump_chain: ['gateway'] }
            : id === 'gateway'
              ? { name: 'gateway' }
              : undefined,
      });
      expect(form.route).toBeNull();
      form.addHop('bastion');
      expect(form.route).toEqual({ ok: true, route: ['gateway', 'bastion'] });
      expect(form.errors.jump).toBeUndefined();
      form.addHop('deleted');
      expect(form.route).toMatchObject({ ok: false });
      expect(form.errors.jump).toMatch(/does not exist/);
      expect(form.firstInvalid).toBe('jump');
      // Adding a hop is an interaction with the section: its error shows at once.
      expect(form.errorFor('jump')).toMatch(/does not exist/);
    });

    it('catches a cycle that goes back through the stored hosts', () => {
      const form = new HostForm(host({ id: 'me', name: 'app' }), {
        lookup: (id) => (id === 'a' ? { name: 'alpha', jump_chain: ['me'] } : undefined),
      });
      form.addHop('a');
      expect(form.errors.jump).toMatch(/cycle/);
    });
  });

  describe('forwards', () => {
    it('adds a row, edits it and builds the forward from the text', () => {
      const form = new HostForm(host());
      const key = form.addForward('local');
      const row = form.forwardRows.find((entry) => entry.key === key);
      expect(row).toBeDefined();
      if (!row) return;
      // A new row is empty: it cannot be saved yet, and says so on the save.
      expect(form.errors.forwards).toBeDefined();
      expect(form.forwardError(0, 'bindPort')).toBeNull();
      form.submit();
      expect(form.forwardError(0, 'bindPort')).toBe('Bind port is required.');
      row.bindPortText = '8080';
      row.targetHost = ' db ';
      row.targetPortText = '5432';
      expect(form.errors.forwards).toBeUndefined();
      expect(form.toHost().forwards).toEqual([
        { kind: 'local', bind_port: 8080, target_host: 'db', target_port: 5432, enabled: true },
      ]);
      row.bindAddress = ' 0.0.0.0 ';
      expect(form.toHost().forwards?.[0].bind_address).toBe('0.0.0.0');
    });

    it('drops the target when a forward becomes dynamic', () => {
      const form = new HostForm(host());
      form.addForward('local');
      const row = form.forwardRows[0];
      row.bindPortText = '1080';
      row.targetHost = 'db';
      row.targetPortText = '5432';
      form.setForwardKind(row, 'dynamic');
      expect(row.targetHost).toBe('');
      expect(form.errors.forwards).toBeUndefined();
      expect(form.toHost().forwards).toEqual([{ kind: 'dynamic', bind_port: 1080, enabled: true }]);
    });

    it('shows a row error once the row was left, and not before', () => {
      const form = new HostForm(host());
      const key = form.addForward('local');
      expect(form.forwardError(0, 'bindPort')).toBeNull();
      form.touchRow(key);
      expect(form.forwardError(0, 'bindPort')).toBe('Bind port is required.');
      expect(form.forwardError(5, 'bindPort')).toBeNull();
    });

    it('refuses two forwards on one listener and more than 32', () => {
      const form = new HostForm(host());
      for (const port of ['8080', '8080']) {
        form.addForward('dynamic');
        form.forwardRows[form.forwardRows.length - 1].bindPortText = port;
      }
      form.submit();
      expect(form.errors.forwards).toBeDefined();
      expect(form.forwardError(1, 'bindPort')).toMatch(/already listens/);
      const many = new HostForm(host());
      for (let index = 0; index < 32; index += 1) many.addForward();
      expect(many.canAddForward).toBe(false);
    });
  });

  describe('environment', () => {
    it('adds and removes rows, and checks the names', () => {
      const form = new HostForm(host());
      const key = form.addEnv();
      form.submit();
      expect(form.envError(0, 'name')).toBe('Name is required.');
      form.envRows[0].name = '1BAD';
      expect(form.envError(0, 'name')).toMatch(/not starting with a digit/);
      form.envRows[0].name = 'GOOD';
      expect(form.errors.env).toBeUndefined();
      form.addEnv();
      form.envRows[1].name = 'GOOD';
      expect(form.envError(1, 'name')).toBe('GOOD is listed twice.');
      form.removeEnv(key);
      expect(form.errors.env).toBeUndefined();
      expect(form.toHost().env).toEqual([{ name: 'GOOD', value: '' }]);
    });
  });

  describe('startup command and agent forwarding', () => {
    it('trims the command, and refuses a second line', () => {
      const form = new HostForm(host());
      form.startupCommand = '  uptime  ';
      expect(form.toHost().startup_command).toBe('uptime');
      form.startupCommand = 'a\u0007b';
      expect(form.errors.startup).toMatch(/single line/);
      expect(form.firstInvalid).toBe('startup');
      form.startupCommand = '';
      expect(form.errors.startup).toBeUndefined();
    });
  });

  describe('authentication order', () => {
    it('starts from the automatic order of the method and edits it', () => {
      const form = new HostForm(host({ auth: { type: 'agent' } }));
      expect(form.authOrder).toBeNull();
      form.setCustomAuthOrder(true);
      expect(form.authOrder).toEqual(['agent', 'keyboard_interactive']);
      form.addAuth('password');
      form.addAuth('password');
      expect(form.authOrder).toEqual(['agent', 'keyboard_interactive', 'password']);
      form.moveAuth('password', -1);
      expect(form.authOrder).toEqual(['agent', 'password', 'keyboard_interactive']);
      form.removeAuth('agent');
      form.removeAuth('password');
      form.removeAuth('keyboard_interactive');
      expect(form.authOrder).toEqual([]);
      expect(form.errors.authOrder).toBe('Name at least one method, or choose Automatic.');
      form.setCustomAuthOrder(false);
      expect(form.authOrder).toBeNull();
      expect(form.errors.authOrder).toBeUndefined();
    });
  });

  it('puts what the backend refuses next to its section until it is edited', () => {
    const form = new HostForm(host(), { lookup });
    expect(form.refuse('two forwards listen on 127.0.0.1:8080')).toBe(true);
    expect(form.errorFor('forwards')).toBe('two forwards listen on 127.0.0.1:8080');
    form.addForward();
    expect(form.serverErrors.forwards).toBeUndefined();
    expect(form.refuse('a jump host of web does not exist (was it deleted?)')).toBe(true);
    expect(form.errorFor('jump')).toMatch(/does not exist/);
    expect(form.refuse('environment variable A is listed twice')).toBe(true);
    expect(form.errorFor('env')).toMatch(/listed twice/);
    expect(form.refuse('startup command must be at most 1024 characters')).toBe(true);
    expect(form.errorFor('startup')).toMatch(/1024/);
  });

  it('parks the sections as typed and gets them back', () => {
    const form = new HostForm(rich(), { lookup });
    form.addForward('remote');
    form.forwardRows[2].bindPortText = '90';
    form.addEnv();
    form.envRows[1].name = 'HALF';
    form.startupCommand = 'half typed';
    form.setCustomAuthOrder(true);
    const draft = form.toDraft();
    const back = new HostForm(rich(), { lookup, draft });
    expect(back.dirty).toBe(true);
    expect(back.forwardRows.map((row) => row.bindPortText)).toEqual(['5433', '1080', '90']);
    expect(back.envRows.map((row) => row.name)).toEqual(['LANG', 'HALF']);
    expect(back.startupCommand).toBe('half typed');
    expect(back.authOrder).toEqual(['password', 'keyboard_interactive']);
    expect(back.jumpChain).toEqual(['b1']);
  });
});
