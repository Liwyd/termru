/**
 * The rules a host is refused for, mirrored from `termru-core/src/validate.rs`.
 *
 * The backend stays the authority (it validates every save, and a synced
 * record too); these checks exist so the editor can say what is wrong next
 * to the field, on blur, in the backend's own terms. A message the backend
 * sends back is mapped onto its field with `fieldOfBackendError`, so a rule
 * that drifted between the two still ends up next to the right input.
 */

import type { AuthKind, ForwardKind } from './types';

export const MAX_NAME_LEN = 128;
export const MAX_ADDRESS_LEN = 253;
export const MAX_USERNAME_LEN = 128;
export const MAX_DESCRIPTION_LEN = 2_000;
export const MAX_TERM_LEN = 64;
export const MAX_TAG_NAME_LEN = 64;
export const MAX_TAGS_PER_HOST = 32;
export const MAX_KEEPALIVE_SECONDS = 3_600;
export const MAX_JUMP_HOPS = 8;
export const MAX_FORWARDS = 32;
export const MAX_ENV_VARS = 32;
export const MAX_ENV_NAME_LEN = 64;
export const MAX_ENV_VALUE_LEN = 1_024;
export const MAX_STARTUP_COMMAND_LEN = 1_024;
export const DEFAULT_PORT = 22;
/** What `0` means in the keepalive field (`DEFAULT_KEEPALIVE_SECONDS` of the core). */
export const AUTOMATIC_KEEPALIVE_SECONDS = 30;

/** The fields of the host form that can carry an error. */
export type HostField =
  | 'name'
  | 'address'
  | 'port'
  | 'username'
  | 'identity'
  | 'term'
  | 'keepalive'
  | 'description'
  /** Data model v2 (ADR 0010): each is a whole section of the editor. */
  | 'jump'
  | 'forwards'
  | 'env'
  | 'startup'
  | 'authOrder';

export type FieldErrors = Partial<Record<HostField, string>>;

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/u;

/** The port, or null when the text is not a valid port (empty = the default). */
export function parsePort(text: string): number | null {
  const trimmed = text.trim();
  if (trimmed === '') return DEFAULT_PORT;
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= 1 && value <= 65535 ? value : null;
}

/** The keepalive in seconds, or null when invalid (empty = 0, automatic). */
export function parseKeepalive(text: string): number | null {
  const trimmed = text.trim();
  if (trimmed === '') return 0;
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value <= MAX_KEEPALIVE_SECONDS ? value : null;
}

export function nameError(value: string): string | null {
  if (value.trim().length === 0) return 'Name is required.';
  if ([...value].length > MAX_NAME_LEN) return `Name must be at most ${MAX_NAME_LEN} characters.`;
  if (CONTROL.test(value)) return 'Name must not contain control characters.';
  return null;
}

/** The address is checked the way it is saved: trimmed. */
export function addressError(value: string): string | null {
  const address = value.trim();
  if (address.length === 0) return 'Address is required.';
  if ([...address].length > MAX_ADDRESS_LEN) {
    return `Address must be at most ${MAX_ADDRESS_LEN} characters.`;
  }
  if (!/^[A-Za-z0-9._:-]+$/.test(address)) {
    return "Address can only contain letters, digits, '.', '-', '_' and ':'.";
  }
  if (address.startsWith('-') || address.endsWith('-')) {
    return "Address must not start or end with '-'.";
  }
  return null;
}

export function usernameError(value: string): string | null {
  const username = value.trim();
  if (username.length === 0) return 'Username is required.';
  if ([...username].length > MAX_USERNAME_LEN) {
    return `Username must be at most ${MAX_USERNAME_LEN} characters.`;
  }
  if (!/^[A-Za-z0-9._@-]+$/.test(username)) {
    return "Username can only contain letters, digits, '.', '-', '_' and '@'.";
  }
  return null;
}

export function portError(text: string): string | null {
  return parsePort(text) === null ? 'Port must be a whole number from 1 to 65535.' : null;
}

export function keepaliveError(text: string): string | null {
  return parseKeepalive(text) === null
    ? `Keepalive must be 0 (automatic) or a whole number of seconds up to ${MAX_KEEPALIVE_SECONDS}.`
    : null;
}

export function termError(value: string): string | null {
  const term = value.trim();
  if (term.length === 0) return 'Terminal type is required.';
  if ([...term].length > MAX_TERM_LEN) {
    return `Terminal type must be at most ${MAX_TERM_LEN} characters.`;
  }
  if (!/^[A-Za-z0-9._-]+$/.test(term)) {
    return "Terminal type can only contain letters, digits, '-', '_' and '.'.";
  }
  return null;
}

export function descriptionError(value: string): string | null {
  if ([...value].length > MAX_DESCRIPTION_LEN) {
    return `Comment must be at most ${MAX_DESCRIPTION_LEN} characters.`;
  }
  if (value.includes('\0')) return 'Comment must not contain NUL characters.';
  return null;
}

/** What the form holds, for `validateHost`. */
export interface HostFormValues {
  name: string;
  address: string;
  portText: string;
  username: string;
  authType: 'password' | 'private_key' | 'agent';
  identityId: string;
  term: string;
  keepaliveText: string;
  description: string;
}

/** Every field's error at once (a field without an error is absent). */
export function validateHost(values: HostFormValues): FieldErrors {
  const errors: FieldErrors = {};
  const found: [HostField, string | null][] = [
    ['name', nameError(values.name)],
    ['address', addressError(values.address)],
    ['port', portError(values.portText)],
    ['username', usernameError(values.username)],
    [
      'identity',
      values.authType === 'private_key' && values.identityId.length === 0
        ? 'Choose an identity for key authentication.'
        : null,
    ],
    ['term', termError(values.term)],
    ['keepalive', keepaliveError(values.keepaliveText)],
    ['description', descriptionError(values.description)],
  ];
  for (const [field, message] of found) {
    if (message) errors[field] = message;
  }
  return errors;
}

/**
 * The field a backend refusal is about, found from the wording of its
 * message (`validate.rs` starts every message with the field it checked).
 * `null` when the message is about something else (a failing disk).
 */
export function fieldOfBackendError(message: string): HostField | null {
  const text = message.trim().toLowerCase();
  const prefixes: [string, HostField][] = [
    ['name ', 'name'],
    ['address ', 'address'],
    ['port ', 'port'],
    ['username ', 'username'],
    ['term ', 'term'],
    ['keepalive ', 'keepalive'],
    ['description ', 'description'],
    // The v2 rules (`validate.rs`): the jump chain and the route to a host.
    ['a jump chain', 'jump'],
    ['a jump host', 'jump'],
    ['a host cannot be its own jump host', 'jump'],
    ['the jump chain of ', 'jump'],
    ['the route to ', 'jump'],
    // Forwards, and the fields of one.
    ['forward ', 'forwards'],
    ['a host may have at most 32 port forwards', 'forwards'],
    ['a local ', 'forwards'],
    ['a remote ', 'forwards'],
    ['a dynamic ', 'forwards'],
    ['bind address ', 'forwards'],
    ['target host ', 'forwards'],
    ['two forwards ', 'forwards'],
    // Environment, startup command, authentication order.
    ['environment variable', 'env'],
    ['a host may have at most 32 environment', 'env'],
    ['the value of ', 'env'],
    ['startup command ', 'startup'],
    ['the authentication order ', 'authOrder'],
  ];
  for (const [prefix, field] of prefixes) {
    if (text.startsWith(prefix)) return field;
  }
  return null;
}

/** `Automatic (30 s)` for 0, the plain number of seconds otherwise. */
export function keepaliveLabel(seconds: number | null): string {
  if (seconds === null) return '';
  return seconds === 0 ? `Automatic (${AUTOMATIC_KEEPALIVE_SECONDS} s)` : `Every ${seconds} s`;
}

function isIpv4(address: string): boolean {
  const parts = address.split('.');
  return parts.length === 4 && parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255);
}

const DOC_RANGES: [RegExp, string][] = [
  [/^192\.0\.2\./, 'documentation range'],
  [/^198\.51\.100\./, 'documentation range'],
  [/^203\.0\.113\./, 'documentation range'],
  [/^2001:db8:/i, 'documentation range'],
];

/**
 * The line under a valid address (frame 4): what kind of address it is,
 * "Valid address · documentation range" for the reserved example ranges.
 * `null` while the address is empty or invalid.
 */
export function describeAddress(value: string): string | null {
  const address = value.trim();
  if (addressError(address) !== null) return null;
  const range = DOC_RANGES.find(([pattern]) => pattern.test(address))?.[1];
  if (isIpv4(address) || address.includes(':')) {
    return range ? `Valid address · ${range}` : 'Valid address';
  }
  return 'Valid hostname';
}

/**
 * The sync server address, mirrored from `validate_server_url` of the
 * desktop backend: https, or plain http for this machine only (the access
 * token travels with every request). The backend stays the authority.
 */
export function serverUrlError(value: string): string | null {
  const text = value.trim().replace(/\/+$/, '');
  if (text.length === 0) return 'Server address is required.';
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return 'Enter a full address, such as https://sync.example.com.';
  }
  if (url.protocol === 'https:') return url.hostname ? null : 'The address has no host.';
  if (url.protocol === 'http:') {
    return ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)
      ? null
      : 'Use https. Plain http is only allowed for localhost.';
  }
  return `Unsupported scheme “${url.protocol.replace(':', '')}”. Use https.`;
}

/** A plausible email address; the server decides whether the account exists. */
export function emailError(value: string): string | null {
  const text = value.trim();
  if (text.length === 0) return 'Email is required.';
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(text))
    return 'Enter an email address like ops@example.com.';
  return null;
}

// ---------------------------------------------------------------------------
// Data model v2: jump hosts, port forwards, environment, startup command,
// authentication order. Mirrors `termru-core/src/validate.rs`; the backend
// stays the authority and its refusals are mapped onto the same sections.

/** A jump chain's structure, without a lookup (`validate_jump_chain`). */
export function jumpChainError(hostId: string, chain: readonly string[]): string | null {
  if (chain.length > MAX_JUMP_HOPS) {
    return `A jump chain may have at most ${MAX_JUMP_HOPS} hops.`;
  }
  for (const [index, hop] of chain.entries()) {
    if (hop === hostId) return 'A host cannot be its own jump host.';
    if (chain.indexOf(hop) !== index) return 'A jump chain lists the same host twice.';
  }
  return null;
}

/** IPv6 literal with an optional `%zone`, no brackets (`check_ipv6`). */
function ipv6Error(label: string, value: string): string | null {
  const [address, zone, ...more] = value.split('%');
  if (more.length > 0 || !isIpv6(address)) return `${label} is not a valid IPv6 address.`;
  if (
    zone !== undefined &&
    !(zone.length > 0 && zone.length <= 32 && /^[A-Za-z0-9._-]+$/.test(zone))
  ) {
    return `${label} has an invalid IPv6 zone id (letters, digits, '-', '_' and '.' only).`;
  }
  return null;
}

/** Whether `value` is an IPv6 address (the URL parser is the reference grammar). */
export function isIpv6(value: string): boolean {
  if (!value.includes(':') || !/^[0-9A-Fa-f:.]+$/.test(value)) return false;
  try {
    return new URL(`http://[${value}]/`).hostname.startsWith('[');
  } catch {
    return false;
  }
}

/** A dotted IPv4 address, parts without leading zeros (as Rust parses it). */
export function isIpv4Literal(value: string): boolean {
  const parts = value.split('.');
  return (
    parts.length === 4 &&
    parts.every((part) => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255)
  );
}

/** A host name or address a forward names (a bind address, a target). */
function forwardHostError(label: string, raw: string): string | null {
  const value = raw.trim();
  if (value.length === 0) return `${label} is required.`;
  if ([...value].length > MAX_ADDRESS_LEN + 2) {
    return `${label} must be at most ${MAX_ADDRESS_LEN} characters.`;
  }
  if (value.startsWith('[')) {
    if (!value.endsWith(']')) return `${label} has an opening '[' without a closing ']'.`;
    return ipv6Error(label, value.slice(1, -1));
  }
  if (value.includes('%') || value.includes(':')) return ipv6Error(label, value);
  if (!/^[A-Za-z0-9._-]+$/.test(value)) {
    return `${label} can only contain letters, digits, '.', '-' and '_'.`;
  }
  if (value.startsWith('-') || value.endsWith('-')) {
    return `${label} must not start or end with '-'.`;
  }
  return null;
}

/** An IP literal or `localhost`: what a listener on this machine can bind. */
function isBindableLiteral(raw: string): boolean {
  const value = stripBrackets(raw.trim());
  if (value.toLowerCase() === 'localhost') return true;
  const address = value.split('%')[0];
  return isIpv4Literal(address) || isIpv6(address);
}

function stripBrackets(value: string): string {
  return value.startsWith('[') && value.endsWith(']') ? value.slice(1, -1) : value;
}

/**
 * Whether a listener bound to `address` is reachable from this machine
 * (or, for a remote forward, from the server) only: no address, `localhost`
 * or a loopback IP. Anything else counts as exposed. Mirrors
 * `Forward::binds_loopback` of the core.
 */
export function isLoopbackBind(address: string | null | undefined): boolean {
  const trimmed = (address ?? '').trim();
  if (trimmed.length === 0) return true;
  const value = stripBrackets(trimmed).split('%')[0];
  if (value.toLowerCase() === 'localhost') return true;
  if (isIpv4Literal(value)) return Number(value.split('.')[0]) === 127;
  if (!isIpv6(value)) return false;
  try {
    const host = new URL(`http://[${value}]/`).hostname;
    // `::1` in every spelling; `::ffff:127.x.y.z` comes out as `::ffff:7fxx:yyyy`.
    return host === '[::1]' || /^\[::ffff:7f[0-9a-f]{2}:[0-9a-f]{1,4}\]$/.test(host);
  } catch {
    return false;
  }
}

/** One port forward as the editor holds it (everything typed is text). */
export interface ForwardRowValues {
  kind: ForwardKind;
  bindAddress: string;
  bindPortText: string;
  targetHost: string;
  targetPortText: string;
  enabled: boolean;
}

export interface ForwardRowErrors {
  bindAddress?: string;
  bindPort?: string;
  targetHost?: string;
  targetPort?: string;
}

/** A port that must be given (a forward has no default port). */
export function forwardPortError(label: string, text: string): string | null {
  const trimmed = text.trim();
  if (trimmed.length === 0) return `${label} is required.`;
  if (!/^\d+$/.test(trimmed) || Number(trimmed) < 1 || Number(trimmed) > 65535) {
    return `${label} must be a whole number from 1 to 65535.`;
  }
  return null;
}

/** The errors of one forward row (`validate_forward`). */
export function forwardRowErrors(row: ForwardRowValues): ForwardRowErrors {
  const errors: ForwardRowErrors = {};
  const bind = row.bindAddress.trim();
  if (bind.length > 0) {
    const problem = forwardHostError('Bind address', bind);
    if (problem) errors.bindAddress = problem;
    else if (row.kind !== 'remote' && !isBindableLiteral(bind)) {
      errors.bindAddress = 'A listener on this machine needs an IP address (or localhost).';
    }
  }
  const port = forwardPortError('Bind port', row.bindPortText);
  if (port) errors.bindPort = port;
  if (row.kind !== 'dynamic') {
    const host = forwardHostError('Target host', row.targetHost);
    if (host) errors.targetHost = host;
    const targetPort = forwardPortError('Target port', row.targetPortText);
    if (targetPort) errors.targetPort = targetPort;
  }
  return errors;
}

/** Where a forward listens, for the duplicate check (`validate_forwards`). */
function listenerKey(row: ForwardRowValues): string {
  const remote = row.kind === 'remote';
  let address = stripBrackets(row.bindAddress.trim()).toLowerCase();
  if (address.length === 0) address = remote ? 'localhost' : '127.0.0.1';
  else if (!remote && address === 'localhost') address = '127.0.0.1';
  return `${remote ? 'remote' : 'here'}|${address}|${Number(row.bindPortText.trim())}`;
}

export interface ForwardsValidation {
  /** The errors of each row, in the order of the rows. */
  rows: ForwardRowErrors[];
  /** An error about the list as a whole (too many forwards). */
  list: string | null;
  /** Any row or the list has an error. */
  invalid: boolean;
}

/**
 * Every rule about the forwards of a host: each row, their number, and that
 * no two claim the same listening address and port (the later row says so).
 */
export function validateForwards(rows: readonly ForwardRowValues[]): ForwardsValidation {
  const errors = rows.map((row) => forwardRowErrors(row));
  const claimed = new Map<string, number>();
  rows.forEach((row, index) => {
    if (errors[index].bindPort || errors[index].bindAddress) return;
    const key = listenerKey(row);
    if (claimed.has(key)) {
      const address =
        stripBrackets(row.bindAddress.trim()) ||
        (row.kind === 'remote' ? 'localhost' : '127.0.0.1');
      errors[index].bindPort =
        `Another forward already listens on ${address}:${row.bindPortText.trim()}.`;
    } else {
      claimed.set(key, index);
    }
  });
  const list =
    rows.length > MAX_FORWARDS ? `A host may have at most ${MAX_FORWARDS} port forwards.` : null;
  return {
    rows: errors,
    list,
    invalid: list !== null || errors.some((entry) => Object.keys(entry).length > 0),
  };
}

export interface EnvRowValues {
  name: string;
  value: string;
}

export interface EnvRowErrors {
  name?: string;
  value?: string;
}

export function envNameError(name: string): string | null {
  if (name.length === 0) return 'Name is required.';
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
    return "Use letters, digits and '_', not starting with a digit.";
  }
  if (name.length > MAX_ENV_NAME_LEN) return `Name must be at most ${MAX_ENV_NAME_LEN} characters.`;
  return null;
}

export function envValueError(value: string): string | null {
  if ([...value].length > MAX_ENV_VALUE_LEN) {
    return `Value must be at most ${MAX_ENV_VALUE_LEN} characters.`;
  }
  if (CONTROL.test(value)) return 'Value must not contain control characters.';
  return null;
}

export interface EnvValidation {
  rows: EnvRowErrors[];
  list: string | null;
  invalid: boolean;
}

/** The environment rules (`validate_env`): names, values, count, no repeats. */
export function validateEnv(rows: readonly EnvRowValues[]): EnvValidation {
  const errors: EnvRowErrors[] = rows.map((row) => {
    const entry: EnvRowErrors = {};
    const name = envNameError(row.name);
    if (name) entry.name = name;
    const value = envValueError(row.value);
    if (value) entry.value = value;
    return entry;
  });
  const seen = new Set<string>();
  rows.forEach((row, index) => {
    if (errors[index].name) return;
    if (seen.has(row.name)) errors[index].name = `${row.name} is listed twice.`;
    seen.add(row.name);
  });
  const list =
    rows.length > MAX_ENV_VARS
      ? `A host may have at most ${MAX_ENV_VARS} environment variables.`
      : null;
  return {
    rows: errors,
    list,
    invalid: list !== null || errors.some((entry) => Object.keys(entry).length > 0),
  };
}

/** The startup command; an empty one means "none" and is fine. */
export function startupCommandError(value: string): string | null {
  if (value.trim().length === 0) return null;
  if ([...value].length > MAX_STARTUP_COMMAND_LEN) {
    return `The startup command must be at most ${MAX_STARTUP_COMMAND_LEN} characters.`;
  }
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u0008\u000a-\u001f\u007f-\u009f]/u.test(value)) {
    return 'The startup command must be a single line without control characters.';
  }
  return null;
}

/** `null` is "Automatic" and fine; a custom order names each method once. */
export function authOrderError(order: readonly AuthKind[] | null): string | null {
  if (order === null) return null;
  if (order.length === 0) return 'Name at least one method, or choose Automatic.';
  const repeated = order.find((kind, index) => order.indexOf(kind) !== index);
  return repeated ? 'A method can be listed only once.' : null;
}
