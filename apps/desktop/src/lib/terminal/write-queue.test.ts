/**
 * The ordered write queue: what the user types reaches the session in the
 * order it was typed, one `session_write` at a time, and survives a backend
 * that is briefly not accepting input.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../ipc';
import { encodeUtf8 } from './codec';
import {
  BACKLOG_RETRY_MAX_MS,
  BACKLOG_RETRY_START_MS,
  MAX_WRITE_BYTES,
  clearWriteQueue,
  writeToSession,
} from './write-queue';

vi.mock('../ipc', () => ({ api: { sessionWrite: vi.fn() } }));

/** What the backend answers when the remote stopped reading input. */
const BACKLOG = { message: 'the remote host is not accepting input (the input queue is full)' };

/** One `session_write` that was invoked and is not answered yet. */
interface Call {
  id: string;
  data: number[];
  resolve: () => void;
  reject: (reason: unknown) => void;
}

/** Every invoked write, in order, answered by the test. */
let calls: Call[] = [];
let inFlight = 0;
let maxInFlight = 0;

function text(call: Call): string {
  return new TextDecoder().decode(new Uint8Array(call.data));
}

/** Lets the queue react to answers that were just given. */
async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
}

/** Answers the oldest unanswered write with success. */
async function accept(call: Call | undefined): Promise<void> {
  call?.resolve();
  await settle();
}

beforeEach(() => {
  vi.useFakeTimers();
  calls = [];
  inFlight = 0;
  maxInFlight = 0;
  vi.mocked(api.sessionWrite).mockImplementation(
    (id, data) =>
      new Promise<void>((resolve, reject) => {
        inFlight += 1;
        maxInFlight = Math.max(maxInFlight, inFlight);
        calls.push({
          id,
          data,
          resolve: () => {
            inFlight -= 1;
            resolve();
          },
          reject: (reason) => {
            inFlight -= 1;
            reject(reason);
          },
        });
      }),
  );
});

afterEach(() => {
  // Queues are per session id: the tests use distinct ids, but a queue that
  // is still waiting must not keep a timer alive into the next test.
  for (const id of ['s1', 's2']) clearWriteQueue(id);
  vi.useRealTimers();
  vi.mocked(api.sessionWrite).mockReset();
});

describe('writeToSession', () => {
  it('sends the first keystroke at once, as UTF-8 bytes', () => {
    writeToSession('s1', 'é');
    expect(calls).toHaveLength(1);
    expect(calls[0].id).toBe('s1');
    expect(calls[0].data).toEqual(encodeUtf8('é'));
  });

  it('never has two writes of a session in flight and joins what is typed meanwhile', async () => {
    writeToSession('s1', 'a');
    writeToSession('s1', 'b');
    writeToSession('s1', 'c');
    writeToSession('s1', 'd');
    await settle();
    // Only the first one is out; the others wait for it.
    expect(calls.map(text)).toEqual(['a']);

    await accept(calls[0]);
    // One write for everything typed meanwhile.
    expect(calls.map(text)).toEqual(['a', 'bcd']);

    writeToSession('s1', 'e');
    await settle();
    expect(calls).toHaveLength(2);
    await accept(calls[1]);
    expect(calls.map(text)).toEqual(['a', 'bcd', 'e']);
    expect(maxInFlight).toBe(1);
  });

  it('keeps the typing order however the backend is delayed', async () => {
    const typed = 'the quick brown fox jumps over the lazy dog 0123456789 ünïcödé';
    for (const char of typed) writeToSession('s1', char);

    // The backend answers each write with a different delay; nothing may
    // overtake: the next write only starts after the answer.
    let delivered = '';
    let answered = 0;
    while (calls.length > answered) {
      const call = calls[answered];
      delivered += text(call);
      answered += 1;
      const delay = (answered * 7) % 13;
      setTimeout(() => call.resolve(), delay);
      await settle(13);
    }
    expect(delivered).toBe(typed);
    expect(maxInFlight).toBe(1);
  });

  it('keeps sessions apart', async () => {
    writeToSession('s1', 'a');
    writeToSession('s2', 'b');
    writeToSession('s1', 'c');
    await settle();
    // Each session has its own write in flight.
    expect(calls.map((call) => [call.id, text(call)])).toEqual([
      ['s1', 'a'],
      ['s2', 'b'],
    ]);
    await accept(calls[1]);
    await accept(calls[0]);
    expect(calls.map((call) => [call.id, text(call)])).toEqual([
      ['s1', 'a'],
      ['s2', 'b'],
      ['s1', 'c'],
    ]);
  });
});

describe('chunking', () => {
  it('splits a large paste into ordered writes of at most 64 KiB', async () => {
    // Multi-byte characters, so chunk borders fall inside characters.
    const paste = 'aé€😀'.repeat(Math.ceil((MAX_WRITE_BYTES * 2.5) / 10));
    const bytes = encodeUtf8(paste);
    expect(bytes.length).toBeGreaterThan(MAX_WRITE_BYTES * 2);

    writeToSession('s1', paste);
    // Typed right after the paste (the Enter that follows it).
    writeToSession('s1', '\r');

    const sent: number[] = [];
    let answered = 0;
    while (calls.length > answered) {
      const call = calls[answered];
      expect(call.data.length).toBeLessThanOrEqual(MAX_WRITE_BYTES);
      sent.push(...call.data);
      answered += 1;
      await accept(call);
    }
    expect(calls.length).toBeGreaterThanOrEqual(3);
    // Every byte, once, in order; the Enter last.
    expect(sent).toEqual([...bytes, 13]);
    expect(maxInFlight).toBe(1);
  });

  it('fills a write up to the limit from several small ones', async () => {
    writeToSession('s1', 'x');
    const filler = 'y'.repeat(MAX_WRITE_BYTES - 10);
    writeToSession('s1', filler);
    writeToSession('s1', 'z'.repeat(40));
    await accept(calls[0]);
    expect(calls[1].data).toHaveLength(MAX_WRITE_BYTES);
    await accept(calls[1]);
    expect(calls[2].data).toHaveLength(30);
    expect(text(calls[2])).toBe('z'.repeat(30));
  });
});

describe('a backend that is not accepting input', () => {
  it('sends the same chunk again after a growing pause, before anything typed later', async () => {
    writeToSession('s1', 'ab');
    writeToSession('s1', 'cd');
    await settle();
    expect(calls.map(text)).toEqual(['ab']);

    // Rejected: nothing is lost, nothing goes out during the pause.
    calls[0].reject(BACKLOG);
    await settle(BACKLOG_RETRY_START_MS - 1);
    expect(calls).toHaveLength(1);
    await settle(1);
    expect(calls.map(text)).toEqual(['ab', 'ab']);

    // Rejected again: the pause doubles.
    calls[1].reject(BACKLOG);
    await settle(BACKLOG_RETRY_START_MS * 2 - 1);
    expect(calls).toHaveLength(2);
    await settle(1);
    expect(calls.map(text)).toEqual(['ab', 'ab', 'ab']);

    // Accepted at last: the input typed meanwhile follows, in order.
    await accept(calls[2]);
    expect(calls.map(text)).toEqual(['ab', 'ab', 'ab', 'cd']);
    // The pause starts over after a success.
    calls[3].reject(BACKLOG);
    await settle(BACKLOG_RETRY_START_MS);
    expect(calls.map(text)).toEqual(['ab', 'ab', 'ab', 'cd', 'cd']);
    expect(maxInFlight).toBe(1);
  });

  it('caps the pause', async () => {
    writeToSession('s1', 'x');
    const pauses: number[] = [];
    let answered = 0;
    for (let attempt = 0; attempt < 8; attempt += 1) {
      const call = calls[answered];
      answered += 1;
      call.reject(BACKLOG);
      // The next attempt comes after exactly the pause, not a bit before.
      let waited = 0;
      while (calls.length === answered) {
        await settle(1);
        waited += 1;
      }
      pauses.push(waited);
    }
    expect(pauses).toEqual([20, 40, 80, 160, 250, 250, 250, 250]);
    expect(pauses[0]).toBe(BACKLOG_RETRY_START_MS);
    expect(Math.max(...pauses)).toBe(BACKLOG_RETRY_MAX_MS);
  });

  it('recognises the error however the backend delivers it', async () => {
    writeToSession('s1', 'a');
    calls[0].reject('the remote host is not accepting input (the input queue is full)');
    await settle(BACKLOG_RETRY_START_MS);
    // A plain string is a backlog too: sent again.
    expect(calls.map(text)).toEqual(['a', 'a']);
  });
});

describe('a session that is gone', () => {
  it('drops the input that waits when a write fails for another reason', async () => {
    writeToSession('s1', 'a');
    writeToSession('s1', 'b');
    await settle();
    calls[0].reject({ message: 'no session with id s1' });
    await settle(1000);
    // `b` is not sent to a session that does not exist, and nothing retries.
    expect(calls.map(text)).toEqual(['a']);
  });

  it('starts over with a fresh queue on the next keystroke', async () => {
    writeToSession('s1', 'a');
    calls[0].reject({ message: 'session is not ready yet' });
    await settle();

    writeToSession('s1', 'b');
    await settle();
    expect(calls.map(text)).toEqual(['a', 'b']);
    await accept(calls[1]);
    writeToSession('s1', 'c');
    expect(calls.map(text)).toEqual(['a', 'b', 'c']);
  });

  it('treats a non-backlog failure that is not an object as final too', async () => {
    writeToSession('s1', 'a');
    writeToSession('s1', 'b');
    calls[0].reject('boom');
    await settle(1000);
    expect(calls.map(text)).toEqual(['a']);
  });
});

describe('clearWriteQueue', () => {
  it('drops the input that waits, also while a write is in flight', async () => {
    writeToSession('s1', 'a');
    writeToSession('s1', 'b');
    clearWriteQueue('s1');
    await accept(calls[0]);
    expect(calls.map(text)).toEqual(['a']);
  });

  it('stops the retries of a rejected chunk', async () => {
    writeToSession('s1', 'a');
    calls[0].reject(BACKLOG);
    await settle();
    clearWriteQueue('s1');
    await settle(10_000);
    expect(calls.map(text)).toEqual(['a']);
  });

  it('leaves other sessions and later input alone', async () => {
    writeToSession('s1', 'a');
    writeToSession('s2', 'b');
    clearWriteQueue('s1');
    await accept(calls[0]);
    writeToSession('s1', 'c');
    writeToSession('s2', 'd');
    await settle();
    expect(calls.map((call) => [call.id, text(call)])).toEqual([
      ['s1', 'a'],
      ['s2', 'b'],
      ['s1', 'c'],
    ]);
    await accept(calls[1]);
    expect(calls.map((call) => [call.id, text(call)]).slice(-1)).toEqual([['s2', 'd']]);
  });

  it('is harmless for a session without a queue', () => {
    expect(() => clearWriteQueue('nobody')).not.toThrow();
  });
});
