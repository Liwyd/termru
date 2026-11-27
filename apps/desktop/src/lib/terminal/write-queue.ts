/**
 * Keeps what the user types in order on its way to a session.
 *
 * `invoke` calls that are in flight at the same time may reach the backend in
 * any order, so a fast typist (or a paste followed by Enter) could get their
 * input shuffled. Each session therefore has one queue with at most one
 * `session_write` in flight; whatever is typed meanwhile is joined into the
 * next single write. The backend rejects a write with the "input queue is
 * full" error when the remote stopped reading; that chunk is retried after a
 * short pause instead of being lost, and nothing is sent past it meanwhile.
 */

import { api } from '../ipc';
import { encodeUtf8 } from './codec';

/** The most bytes one `session_write` carries; a bigger paste is split. */
export const MAX_WRITE_BYTES = 64 * 1024;
/** The first pause before a rejected chunk is sent again. */
export const BACKLOG_RETRY_START_MS = 20;
/** The pause doubles up to this. */
export const BACKLOG_RETRY_MAX_MS = 250;

/** The part of the backend's `InputBacklog` message that identifies it. */
const BACKLOG_MESSAGE = 'the input queue is full';

function isBacklog(err: unknown): boolean {
  let message = '';
  if (typeof err === 'string') {
    message = err;
  } else if (err && typeof err === 'object' && 'message' in err) {
    const value = (err as { message: unknown }).message;
    if (typeof value === 'string') message = value;
  }
  return message.includes(BACKLOG_MESSAGE);
}

class WriteQueue {
  readonly #sessionId: string;
  readonly #release: (queue: WriteQueue) => void;
  /** Encoded input that was not sent yet, oldest first. */
  #pieces: number[][] = [];
  /** How much of the first piece is already part of a sent chunk. */
  #offset = 0;
  #running = false;
  #closed = false;
  /** Ends the pause before a retry early (the queue was closed). */
  #wake: (() => void) | null = null;

  constructor(sessionId: string, release: (queue: WriteQueue) => void) {
    this.#sessionId = sessionId;
    this.#release = release;
  }

  push(bytes: number[]): void {
    if (this.#closed || bytes.length === 0) return;
    this.#pieces.push(bytes);
    // A running pump picks the new input up when its write returns.
    if (!this.#running) void this.#pump();
  }

  /** Drops what was not sent yet and stops after the write in flight. */
  close(): void {
    this.#closed = true;
    this.#pieces = [];
    this.#offset = 0;
    this.#wake?.();
    this.#release(this);
  }

  /** The next chunk: up to `MAX_WRITE_BYTES`, joined from the pieces. */
  #take(): number[] | null {
    if (this.#pieces.length === 0) return null;
    const chunk: number[] = [];
    while (this.#pieces.length > 0 && chunk.length < MAX_WRITE_BYTES) {
      const piece = this.#pieces[0];
      const end = Math.min(piece.length, this.#offset + (MAX_WRITE_BYTES - chunk.length));
      for (let index = this.#offset; index < end; index += 1) chunk.push(piece[index]);
      if (end === piece.length) {
        this.#pieces.shift();
        this.#offset = 0;
      } else {
        this.#offset = end;
      }
    }
    return chunk;
  }

  #pause(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, ms);
      this.#wake = () => {
        clearTimeout(timer);
        resolve();
      };
    });
  }

  async #pump(): Promise<void> {
    this.#running = true;
    try {
      let pending: number[] | null = null;
      let backoff = BACKLOG_RETRY_START_MS;
      while (!this.#closed) {
        // A rejected chunk goes out again as it was: later input never
        // overtakes it.
        pending ??= this.#take();
        if (!pending) return;
        try {
          await api.sessionWrite(this.#sessionId, pending);
          pending = null;
          backoff = BACKLOG_RETRY_START_MS;
        } catch (err) {
          if (!isBacklog(err)) {
            // The session is gone (or not ready): the rest cannot be
            // delivered; the status stream tells the user what happened.
            this.close();
            return;
          }
          await this.#pause(backoff);
          this.#wake = null;
          backoff = Math.min(backoff * 2, BACKLOG_RETRY_MAX_MS);
        }
      }
    } finally {
      this.#running = false;
    }
  }
}

/** One queue per session that has input in flight or waiting. */
const queues = new Map<string, WriteQueue>();

/** Queues `data` for the session; it is delivered in typing order. */
export function writeToSession(sessionId: string, data: string): void {
  let queue = queues.get(sessionId);
  if (!queue) {
    queue = new WriteQueue(sessionId, (done) => {
      if (queues.get(sessionId) === done) queues.delete(sessionId);
    });
    queues.set(sessionId, queue);
  }
  queue.push(encodeUtf8(data));
}

/**
 * Forgets the input that waits for a session. Called when its pane closes or
 * moves on to another session: late keystrokes must not reach a session the
 * user no longer sees.
 */
export function clearWriteQueue(sessionId: string): void {
  queues.get(sessionId)?.close();
}
