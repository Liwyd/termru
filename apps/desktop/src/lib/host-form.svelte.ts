import type { HostEditDraft } from './stores/route.svelte';
import type { AuthMethod, Host } from './types';
import {
  DEFAULT_PORT,
  fieldOfBackendError,
  parseKeepalive,
  parsePort,
  validateHost,
  type FieldErrors,
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

  /** The fields the user has left; their errors may be shown. */
  touched = $state<Partial<Record<HostField, boolean>>>({});
  /** A save was tried: every error is shown. */
  submitted = $state(false);
  /** Refusals of the backend, by field, until the field is edited again. */
  serverErrors = $state<FieldErrors>({});
  /** What the form looked like when it was loaded or last saved. */
  #baseline = $state('');

  constructor(
    host: Host | null,
    options: { passwordStored?: boolean; draft?: HostEditDraft | null } = {},
  ) {
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
    });
  }

  /** Whether anything differs from the loaded (or last saved) host. */
  dirty = $derived(this.#snapshot() !== this.#baseline);

  /** Every rule the form breaks right now, shown or not. */
  errors = $derived<FieldErrors>(
    validateHost({
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
  );

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

  /** The auth method of the form as the host stores it. */
  get auth(): AuthMethod {
    if (this.authType === 'private_key') {
      return { type: 'private_key', identity_id: this.identityId };
    }
    return this.authType === 'agent' ? { type: 'agent' } : { type: 'password' };
  }

  /** The host to save (call when `errorCount` is 0). */
  toHost(): Host {
    return {
      ...this.#base,
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
      },
      authType: this.authType,
      identityId: this.identityId,
      groupId: this.groupId,
      tags: [...this.tags],
      portText: this.portText,
      keepaliveText: this.keepaliveText,
      savePassword: this.savePassword,
    };
  }
}
