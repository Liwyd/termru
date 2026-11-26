/**
 * A recording fake of the Tauri core API (`invoke` and `Channel`) for the
 * component tests.
 *
 * A test file installs it with `vi.mock` on the core module of the Tauri API,
 * answered by `import('../test-support/tauri-core-mock')`, and then reads
 * `calls` and sets `answers` / `failures`.
 */

export interface InvokeCall {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

/** Every command the app invoked, in order. */
export const calls: InvokeCall[] = [];

/** The answer of a command (a value, or a function of the arguments). */
export const answers = new Map<string, unknown>();

/** Commands that reject with this message. */
export const failures = new Map<string, string>();

/** The channels the app passed to commands, in order. */
export const channels: { cmd: string; channel: ChannelFake }[] = [];

export class ChannelFake<T = unknown> {
  onmessage: (update: T) => void = () => {};
}

export { ChannelFake as Channel };

/** Starts a test from a clean slate. */
export function resetTauriMock(): void {
  calls.length = 0;
  answers.clear();
  failures.clear();
  channels.length = 0;
}

/** The calls of one command. */
export function callsOf(cmd: string): InvokeCall[] {
  return calls.filter((call) => call.cmd === cmd);
}

export async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  calls.push({ cmd, args });
  const channel = args?.channel;
  if (channel instanceof ChannelFake) channels.push({ cmd, channel });
  const failure = failures.get(cmd);
  if (failure !== undefined) throw new Error(failure);
  if (answers.has(cmd)) {
    const answer = answers.get(cmd);
    return typeof answer === 'function' ? (answer as (a?: unknown) => unknown)(args) : answer;
  }
  switch (cmd) {
    case 'session_open':
      return 'session-1';
    case 'session_duplicate':
      return 'session-dup';
    case 'host_list':
    case 'identity_list':
    case 'recent_list':
    case 'group_list':
    case 'tag_list':
    case 'session_list':
    case 'sync_conflicts':
      return [];
    case 'secret_has':
      return false;
    default:
      return null;
  }
}
