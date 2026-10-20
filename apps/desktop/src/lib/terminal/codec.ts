/** Binary helpers for the terminal bridge (base64 in, UTF-8 bytes out). */

/** Decodes a base64 payload (as sent by the Rust side) into raw bytes. */
export function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/** Encodes terminal input as UTF-8 bytes (Tauri transports `Vec<u8>`). */
export function encodeUtf8(value: string): number[] {
  return Array.from(new TextEncoder().encode(value));
}
