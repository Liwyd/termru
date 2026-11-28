/**
 * Text that does not come from the user of this device (a startup command, an
 * environment value, a name) shown in a place where the user decides on it:
 * control characters and the characters that reorder or hide text (right to
 * left overrides, zero-width characters) are written out as `‹U+202E›`
 * instead of being drawn, so the line cannot look like something it is not.
 */

/** Control characters (except a plain tab) and bidirectional or invisible ones. */
const HIDDEN = new RegExp(
  '[\\u0000-\\u0008\\u000a-\\u001f\\u007f-\\u009f\\u200b-\\u200f\\u2028-\\u202e\\u2060-\\u2069\\ufeff]',
  'gu',
);

export function revealed(text: string): string {
  return text.replace(HIDDEN, (char) => {
    const code = (char.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, '0');
    return `‹U+${code}›`;
  });
}
