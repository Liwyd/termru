import { describe, expect, it } from 'vitest';
import { decodeBase64, encodeUtf8 } from './codec';

describe('decodeBase64', () => {
  it('decodes ascii', () => {
    expect(Array.from(decodeBase64('aGVsbG8='))).toEqual([104, 101, 108, 108, 111]);
  });

  it('preserves arbitrary bytes', () => {
    const bytes = decodeBase64('AAEC/v8=');
    expect(Array.from(bytes)).toEqual([0, 1, 2, 254, 255]);
  });

  it('decodes an empty payload', () => {
    expect(decodeBase64('').length).toBe(0);
  });
});

describe('encodeUtf8', () => {
  it('encodes ascii', () => {
    expect(encodeUtf8('ok')).toEqual([111, 107]);
  });

  it('encodes multi-byte characters', () => {
    expect(encodeUtf8('é')).toEqual([195, 169]);
  });
});
