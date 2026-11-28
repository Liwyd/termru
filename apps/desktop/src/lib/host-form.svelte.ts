import { resolveRoute, type HostLookup } from './jump';
import type { HostEditDraft } from './stores/route.svelte';
import type { AuthKind, AuthMethod, EnvVar, Forward, ForwardKind, Host } from './types';
import {
  DEFAULT_PORT,
  MAX_ENV_VARS,
  MAX_FORWARDS,
  MAX_JUMP_HOPS,
  authOrderError,
  fieldOfBackendError,
  jumpChainError,
  parseKeepalive,
  parsePort,
  startupCommandError,
  validateEnv,
  validateForwards,
  validateHost,
  type EnvRowErrors,
  type EnvRowValues,
  type FieldErrors,
  type ForwardRowErrors,
  type ForwardRowValues,
  type HostField,
} from './validation';

export type AuthType = AuthMethod['type'];

/** A host that has not been saved yet. */
export function newHost(): Host {
  const now = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    name: '',
    address: '',
    port: DEFAULT_PORT,
    username: '',
    auth: { type: 'password' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

/** One port forward of the form; `key` keeps the row's identity while it is edited. */
export interface ForwardRow extends ForwardRowValues {
  key: number;
}

/** One environment variable of the form. */
export interface EnvRow extends EnvRowValues {
  key: number;
}

let lastRowKey = 0;
function nextRowKey(): number {
  lastRowKey += 1;
  return lastRowKey;
}

/** The text of a forward's fields, as the editor shows them. */
export function forwardRowOf(forward: Forward): ForwardRow {
  return {
    key: nextRowKey(),
    kind: forward.kind,
    bindAddress: forward.bind_address ?? '',
    bindPortText: String(forward.bind_port),
    targetHost: forward.target_host ?? '',
    targetPortText: forward.target_port ? String(forward.target_port) : '',
    enabled: forward.enabled !== false,
  };
}

/** The forward a row stands for (call when the row has no errors). */
function forwardOf(row: ForwardRowValues): Forward {
  const forward: Forward = {
    kind: row.kind,
    bind_port: Number(row.bindPortText.trim()),
    enabled: row.enabled,
  };
  if (row.bindAddress.trim()) forward.bind_address = row.bindAddress.trim();
  if (row.kind !== 'dynamic') {
    forward.target_host = row.targetHost.trim();
    forward.target_port = Number(row.targetPortText.trim());
  }
  return forward;
}

/** The method order "Automatic" stands for: the host's method, then the server's questions. */
export function automaticAuthOrder(authType: AuthMethod['type']): AuthKind[] {
  return [authType, 'keyboard_interactive'];
}

/** `0` is shown as an empty field with the "Automatic" placeholder. */
function keepaliveText(seconds: number | undefined): string {
  return seconds ? String(seconds) : '';
}

/**
 * The state of the host editor: what each field holds, which errors may be
 * shown yet, and whether anything differs from the host as it was loaded.
 *
 * It is a class of its own (not component state) so the rules — validation,
 * the unsaved-changes comparison, the payload that is saved — are tested
 * without a DOM. The editor page only renders it.
 */
export class HostForm {
  /** The host as loaded; the payload is built on top of it (ids, dates, sort order). */
  #base: Host;
  /** A host the user is adding, as opposed to one being edited. */
  readonly isNew: boolean;

  name = $state('');
  address = $state('');
  portText = $state(String(DEFAULT_PORT));
  username = $state('');
  authType = $state<AuthType>('password');
  identityId = $state('');
  /** The password typed in the form; saved to the credential store, never to the host record. */
  password = $state('');
  /** Whether this device keeps a password for the host (true = store it). */
  savePassword = $state(true);
  groupId = $state('');
  favorite = $state(false);
  tags = $state<string[]>([]);
  description = $state('');
  term = $state('xterm-256color');
  keepaliveText = $state('');

  /** Data model v2 (ADR 0010). */
  /** The ids of the jump hosts, first hop first. */
  jumpChain = $state<string[]>([]);
  forwardRows = $state<ForwardRow[]>([]);
  envRows = $state<EnvRow[]>([]);
  startupCommand = $state('');
  agentForward = $state(false);
  /** An explicit order of the authentication methods; null is "Automatic". */
  authOrder = $state<AuthKind[] | null>(null);
  /** The rows (by key) the user has left: their errors may be shown. */
  rowTouched = $state<Record<number, boolean>>({});

  /** The fields the user has left; their errors may be shown. */
  touched = $state<Partial<Record<HostField, boolean>>>({});
  /** A save was tried: every error is shown. */
  submitted = $state(false);
  /** Refusals of the backend, by field, until the field is edited again. */
  serverErrors = $state<FieldErrors>({});
  /** What the form looked like when it was loaded or last saved. */
  #baseline = $state('');

  /** Finds another host of the library (for the route through jump hosts). */
  #lookup: HostLookup | null;

  constructor(
    host: Host | null,
    options: {
      passwordStored?: boolean;
      draft?: HostEditDraft | null;
      /** The hosts of the library: lets the form resolve and check the route. */
      lookup?: HostLookup;
    } = {},
  ) {
    this.#lookup = options.lookup ?? null;
    this.isNew = host === null;
    this.#base = host ?? newHost();
    const stored = options.passwordStored === true;
    // The baseline is what the form says for the host as it is saved.
    this.#load(this.#base, stored, null);
    this.#baseline = this.#snapshot();
    // A form that was parked while another page was open comes back as it was
    // left, and is still compared with the host, so it still counts as changed.
    if (options.draft) this.#load(options.draft.host, stored, options.draft);
  }

  #load(source: Host, stored: boolean, draft: HostEditDraft | null): void {
    this.name = source.name;
    this.address = source.address;
    this.portText = draft?.portText ?? String(source.port);
    this.username = source.username;
    this.authType = draft?.authType ?? source.auth.type;
    this.identityId =
      draft?.identityId ?? (source.auth.type === 'private_key' ? source.auth.identity_id : '');
    this.password = '';
    // A host keeps its password on this device when one is stored; a new host
    // starts with the box ticked (the safe, convenient default of the frame).
    this.savePassword = draft?.savePassword ?? (this.isNew ? true : stored);
    this.groupId = draft?.groupId ?? source.group_id ?? '';
    this.favorite = source.favorite;
    this.tags = [...(draft?.tags ?? source.tags)];
    this.description = source.description ?? '';
    this.term = source.term;
    this.keepaliveText = draft?.keepaliveText ?? keepaliveText(source.keepalive_seconds);
    this.jumpChain = [...(draft?.jumpChain ?? source.jump_chain ?? [])];
    this.forwardRows = draft?.forwardRows
      ? draft.forwardRows.map((row) => ({ ...row }))
      : (source.forwards ?? []).map(forwardRowOf);
    this.envRows = draft?.envRows
      ? draft.envRows.map((row) => ({ ...row }))
      : (source.env ?? []).map((variable) => ({ key: nextRowKey(), ...variable }));
    this.startupCommand = source.startup_command ?? '';
    this.agentForward = source.agent_forward === true;
    this.authOrder = draft?.authOrder !== undefined ? draft.authOrder : (source.auth_order ?? null);
    this.rowTouched = {};
    // A route that already exists is looked at from the start: a hop that
    // is gone (deleted, or not synced yet) shows at once, not on the first blur.
    if (this.jumpChain.length > 0 && !this.touched.jump) {
      this.touched = { ...this.touched, jump: true };
    }
  }

  /** The values that make two forms "the same", normalised the way they are saved. */
  #snapshot(): string {
    const password = this.authType === 'password';
    return JSON.stringify({
      name: this.name.trim(),
      address: this.address.trim(),
      port: parsePort(this.portText) ?? this.portText,
      username: this.username.trim(),
      authType: this.authType,
      identityId: this.authType === 'private_key' ? this.identityId : '',
      password: password ? this.password : '',
      savePassword: password ? this.savePassword : true,
      groupId: this.groupId,
      favorite: this.favorite,
      tags: [...this.tags].sort(),
      description: this.description.trim(),
      term: this.term.trim(),
      keepalive: parseKeepalive(this.keepaliveText) ?? this.keepaliveText,
      jumpChain: this.jumpChain,
      forwards: this.forwardRows.map((row) => ({
        kind: row.kind,
        bind: row.bindAddress.trim(),
        port: row.bindPortText.trim(),
        target: row.kind === 'dynamic' ? '' : row.targetHost.trim(),
        targetPort: row.kind === 'dynamic' ? '' : row.targetPortText.trim(),
        enabled: row.enabled,
      })),
      env: this.envRows.map((row) => [row.name, row.value]),
      startup: this.startupCommand.trim(),
      agentForward: this.agentForward,
      authOrder: this.authOrder,
    });
  }

  /** Whether anything differs from the loaded (or last saved) host. */
  dirty = $derived(this.#snapshot() !== this.#baseline);

  /** Every rule the form breaks right now, shown or not. */
  errors = $derived<FieldErrors>({
    ...validateHost({
      name: this.name,
      address: this.address,
      portText: this.portText,
      username: this.username,
      authType: this.authType,
      identityId: this.identityId,
      term: this.term,
      keepaliveText: this.keepaliveText,
      description: this.description,
    }),
    ...this.#v2Errors(),
  });

  /** The route through the jump hosts as the form has it (null: no lookup to check it with). */
  route = $derived.by(() => {
    if (this.jumpChain.length === 0) return null;
    const name = this.name.trim() || 'this host';
    if (!this.#lookup) {
      const problem = jumpChainError(this.#base.id, this.jumpChain);
      return problem ? { ok: false as const, error: problem } : null;
    }
    return resolveRoute(this.#base.id, name, this.jumpChain, this.#lookup);
  });

  /** Every error of each forward row, by row. */
  forwardCheck = $derived(validateForwards(this.forwardRows));
  envCheck = $derived(validateEnv(this.envRows));

  #v2Errors(): FieldErrors {
    const found: FieldErrors = {};
    const route = this.route;
    if (route && !route.ok) found.jump = route.error;
    if (this.forwardCheck.invalid) {
      found.forwards = this.forwardCheck.list ?? 'Fix the port forwards marked below.';
    }
    if (this.envCheck.invalid) {
      found.env = this.envCheck.list ?? 'Fix the environment variables marked below.';
    }
    const startup = startupCommandError(this.startupCommand);
    if (startup) found.startup = startup;
    const order = authOrderError(this.authOrder);
    if (order) found.authOrder = order;
    return found;
  }

  /** The number of fields that break a rule. */
  errorCount = $derived(Object.keys(this.errors).length);

  /** The first field with an error, in the order the form shows them. */
  firstInvalid = $derived<HostField | null>(
    (
      [
        'name',
        'address',
        'port',
        'username',
        'identity',
        'term',
        'keepalive',
        'description',
        'jump',
        'forwards',
        'env',
        'startup',
        'authOrder',
      ] as const
    ).find((field) => this.errors[field] !== undefined) ?? null,
  );

  /** The error to show under a field: its own once left or saved, or the backend's. */
  errorFor(field: HostField): string | null {
    const server = this.serverErrors[field];
    if (server) return server;
    if (!this.touched[field] && !this.submitted) return null;
    return this.errors[field] ?? null;
  }

  /** The field was left: its error may be shown. */
  touch(field: HostField): void {
    if (!this.touched[field]) this.touched = { ...this.touched, [field]: true };
  }

  /** The field was edited: an old refusal of the backend no longer applies to it. */
  edited(field: HostField): void {
    if (this.serverErrors[field]) {
      const next = { ...this.serverErrors };
      delete next[field];
      this.serverErrors = next;
    }
  }

  /** Shows every error (a save was tried). */
  submit(): void {
    this.submitted = true;
    this.serverErrors = {};
  }

  /**
   * Puts a refusal of the backend next to its field. Returns false when the
   * message is not about a field (the caller shows it as a form message).
   */
  refuse(message: string): boolean {
    const field = fieldOfBackendError(message);
    if (!field) return false;
    this.serverErrors = { ...this.serverErrors, [field]: message };
    return true;
  }

  toggleTag(id: string): void {
    this.tags = this.tags.includes(id)
      ? this.tags.filter((entry) => entry !== id)
      : [...this.tags, id];
  }

  // --- Jump hosts -------------------------------------------------------

  addHop(id: string): void {
    if (this.jumpChain.includes(id)) return;
    this.jumpChain = [...this.jumpChain, id];
    this.edited('jump');
    this.touch('jump');
  }

  removeHop(id: string): void {
    this.jumpChain = this.jumpChain.filter((hop) => hop !== id);
    this.edited('jump');
  }

  /** Moves a hop one place earlier (-1) or later (1) on the route. */
  moveHop(id: string, direction: -1 | 1): void {
    const index = this.jumpChain.indexOf(id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= this.jumpChain.length) return;
    const next = [...this.jumpChain];
    [next[index], next[target]] = [next[target], next[index]];
    this.jumpChain = next;
    this.edited('jump');
  }

  /** Another hop fits on the route. */
  get canAddHop(): boolean {
    return this.jumpChain.length < MAX_JUMP_HOPS;
  }

  // --- Port forwards ------------------------------------------------------

  /** Adds an empty forward; returns its row key. */
  addForward(kind: ForwardKind = 'local'): number {
    const key = nextRowKey();
    this.forwardRows = [
      ...this.forwardRows,
      {
        key,
        kind,
        bindAddress: '',
        bindPortText: '',
        targetHost: '',
        targetPortText: '',
        enabled: true,
      },
    ];
    this.edited('forwards');
    return key;
  }

  removeForward(key: number): void {
    this.forwardRows = this.forwardRows.filter((row) => row.key !== key);
    this.edited('forwards');
  }

  /** Changes the kind of a forward; a dynamic one has no target, so what was typed there goes. */
  setForwardKind(row: ForwardRow, kind: ForwardKind): void {
    row.kind = kind;
    if (kind === 'dynamic') {
      row.targetHost = '';
      row.targetPortText = '';
    }
    this.edited('forwards');
  }

  get canAddForward(): boolean {
    return this.forwardRows.length < MAX_FORWARDS;
  }

  /** The error of one field of a forward row, once the row was left or a save was tried. */
  forwardError(index: number, field: keyof ForwardRowErrors): string | null {
    const row = this.forwardRows[index];
    if (!row || (!this.rowTouched[row.key] && !this.submitted)) return null;
    return this.forwardCheck.rows[index]?.[field] ?? null;
  }

  // --- Environment ----------------------------------------------------------

  addEnv(): number {
    const key = nextRowKey();
    this.envRows = [...this.envRows, { key, name: '', value: '' }];
    this.edited('env');
    return key;
  }

  removeEnv(key: number): void {
    this.envRows = this.envRows.filter((row) => row.key !== key);
    this.edited('env');
  }

  get canAddEnv(): boolean {
    return this.envRows.length < MAX_ENV_VARS;
  }

  envError(index: number, field: keyof EnvRowErrors): string | null {
    const row = this.envRows[index];
    if (!row || (!this.rowTouched[row.key] && !this.submitted)) return null;
    return this.envCheck.rows[index]?.[field] ?? null;
  }

  /** A row was left (or typed into): its errors may be shown. */
  touchRow(key: number): void {
    if (!this.rowTouched[key]) this.rowTouched = { ...this.rowTouched, [key]: true };
  }

  // --- Authentication order ---------------------------------------------------

  /** Switches between "Automatic" (null) and an explicit order, starting from the automatic one. */
  setCustomAuthOrder(custom: boolean): void {
    this.authOrder = custom ? automaticAuthOrder(this.authType) : null;
    this.edited('authOrder');
    this.touch('authOrder');
  }

  moveAuth(kind: AuthKind, direction: -1 | 1): void {
    const order = this.authOrder;
    if (!order) return;
    const index = order.indexOf(kind);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= order.length) return;
    const next = [...order];
    [next[index], next[target]] = [next[target], next[index]];
    this.authOrder = next;
    this.edited('authOrder');
  }

  removeAuth(kind: AuthKind): void {
    if (!this.authOrder) return;
    this.authOrder = this.authOrder.filter((entry) => entry !== kind);
    this.edited('authOrder');
    this.touch('authOrder');
  }

  addAuth(kind: AuthKind): void {
    if (!this.authOrder || this.authOrder.includes(kind)) return;
    this.authOrder = [...this.authOrder, kind];
    this.edited('authOrder');
  }

  /** The auth method of the form as the host stores it. */
  get auth(): AuthMethod {
    if (this.authType === 'private_key') {
      return { type: 'private_key', identity_id: this.identityId };
    }
    return this.authType === 'agent' ? { type: 'agent' } : { type: 'password' };
  }

  /**
   * The v2 fields of the host as the form has them. A field the host never
   * had and the form leaves empty stays absent, so a plain host is saved
   * exactly as it was before data model v2.
   */
  #v2Fields(): Partial<Host> {
    const base = this.#base;
    const fields: Partial<Host> = {};
    if (this.jumpChain.length > 0 || base.jump_chain !== undefined) {
      fields.jump_chain = [...this.jumpChain];
    }
    if (this.forwardRows.length > 0 || base.forwards !== undefined) {
      fields.forwards = this.forwardRows.map(forwardOf);
    }
    if (this.envRows.length > 0 || base.env !== undefined) {
      fields.env = this.envRows.map((row): EnvVar => ({ name: row.name, value: row.value }));
    }
    const command = this.startupCommand.trim();
    if (command.length > 0 || base.startup_command !== undefined) {
      fields.startup_command = command.length > 0 ? command : null;
    }
    if (this.agentForward || base.agent_forward !== undefined) {
      fields.agent_forward = this.agentForward;
    }
    if (this.authOrder !== null || base.auth_order !== undefined) {
      fields.auth_order = this.authOrder ? [...this.authOrder] : null;
    }
    return fields;
  }

  /** The host to save (call when `errorCount` is 0). */
  toHost(): Host {
    return {
      ...this.#base,
      ...this.#v2Fields(),
      name: this.name.trim(),
      address: this.address.trim(),
      port: parsePort(this.portText) ?? DEFAULT_PORT,
      username: this.username.trim(),
      auth: this.auth,
      group_id: this.groupId.length > 0 ? this.groupId : null,
      tags: [...this.tags],
      favorite: this.favorite,
      description: this.description.trim() ? this.description.trim() : null,
      term: this.term.trim(),
      keepalive_seconds: parseKeepalive(this.keepaliveText) ?? 0,
    };
  }

  /** The id of the host being edited (new hosts have theirs from the start). */
  get hostId(): string {
    return this.#base.id;
  }

  /**
   * The credential store said whether this device keeps the host's password
   * (or it was just forgotten). That is what "saved on this device" means for
   * the host, so it goes into the box and into the baseline: asking the store
   * is not an edit.
   */
  adoptStored(stored: boolean): void {
    const baseline = JSON.parse(this.#baseline) as Record<string, unknown>;
    if (baseline.authType === 'password') baseline.savePassword = stored;
    this.#baseline = JSON.stringify(baseline);
    this.savePassword = stored;
  }

  /** The host was saved: it is what the form now compares against. */
  markSaved(saved: Host): void {
    this.#base = saved;
    this.password = '';
    this.#baseline = this.#snapshot();
    this.submitted = false;
    this.serverErrors = {};
  }

  /** The state to park while another page is open (the password is not parked). */
  toDraft(): HostEditDraft {
    return {
      host: {
        ...this.#base,
        name: this.name,
        address: this.address,
        username: this.username,
        favorite: this.favorite,
        description: this.description,
        term: this.term,
        port: parsePort(this.portText) ?? DEFAULT_PORT,
        keepalive_seconds: parseKeepalive(this.keepaliveText) ?? 0,
        // What the text fields of the sections hold is parked as typed (below).
        startup_command: this.startupCommand,
        agent_forward: this.agentForward,
      },
      authType: this.authType,
      identityId: this.identityId,
      groupId: this.groupId,
      tags: [...this.tags],
      portText: this.portText,
      keepaliveText: this.keepaliveText,
      savePassword: this.savePassword,
      jumpChain: [...this.jumpChain],
      forwardRows: this.forwardRows.map((row) => ({ ...row })),
      envRows: this.envRows.map((row) => ({ ...row })),
      authOrder: this.authOrder ? [...this.authOrder] : null,
    };
  }
}
