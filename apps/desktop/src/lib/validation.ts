/**
 * The rules a host is refused for, mirrored from `termru-core/src/validate.rs`.
 *
 * The backend stays the authority (it validates every save, and a synced
 * record too); these checks exist so the editor can say what is wrong next
 * to the field, on blur, in the backend's own terms. A message the backend
 * sends back is mapped onto its field with `fieldOfBackendError`, so a rule
 * that drifted between the two still ends up next to the right input.
 */

export const MAX_NAME_LEN = 128;
export const MAX_ADDRESS_LEN = 253;
export const MAX_USERNAME_LEN = 128;
export const MAX_DESCRIPTION_LEN = 2_000;
export const MAX_TERM_LEN = 64;
export const MAX_TAG_NAME_LEN = 64;
export const MAX_TAGS_PER_HOST = 32;
export const MAX_KEEPALIVE_SECONDS = 3_600;
export const DEFAULT_PORT = 22;
/** What `0` means in the keepalive field (`DEFAULT_KEEPALIVE_SECONDS` of the core). */
export const AUTOMATIC_KEEPALIVE_SECONDS = 30;

/** The fields of the host form that can carry an error. */
export type HostField =
  'name' | 'address' | 'port' | 'username' | 'identity' | 'term' | 'keepalive' | 'description';

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
