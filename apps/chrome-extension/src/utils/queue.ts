/**
 * Bounded outbox for observations that could not be delivered (spec §101, §102).
 *
 * The queue holds at most 500 messages or 5 MB, whichever comes first, and only
 * ever holds *already sanitized* payloads. When it overflows, the oldest entries
 * are discarded and the user is told that some activity could not be saved.
 */

import type { Envelope } from '@localtrack/protocol';

export const MAX_QUEUE_MESSAGES = 500;
export const MAX_QUEUE_BYTES = 5 * 1024 * 1024;

const KEY = 'localtrack:queue';

export interface QueueState {
  messages: Envelope<unknown>[];
  dropped: number;
}

export function trimQueue(messages: Envelope<unknown>[]): { kept: Envelope<unknown>[]; dropped: number } {
  let kept = messages;
  let dropped = 0;

  if (kept.length > MAX_QUEUE_MESSAGES) {
    dropped += kept.length - MAX_QUEUE_MESSAGES;
    kept = kept.slice(kept.length - MAX_QUEUE_MESSAGES);
  }

  // Drop from the front until the serialized size fits.
  while (kept.length > 0 && JSON.stringify(kept).length > MAX_QUEUE_BYTES) {
    kept = kept.slice(1);
    dropped += 1;
  }

  return { kept, dropped };
}

export async function readQueue(): Promise<QueueState> {
  const stored = await chrome.storage.local.get(KEY);
  const value = stored[KEY] as QueueState | undefined;
  return value ?? { messages: [], dropped: 0 };
}

export async function enqueue(message: Envelope<unknown>): Promise<QueueState> {
  const current = await readQueue();
  const { kept, dropped } = trimQueue([...current.messages, message]);
  const next: QueueState = { messages: kept, dropped: current.dropped + dropped };
  await chrome.storage.local.set({ [KEY]: next });
  return next;
}

/** Take everything out of the queue, preserving order. */
export async function drainQueue(): Promise<Envelope<unknown>[]> {
  const current = await readQueue();
  await chrome.storage.local.set({ [KEY]: { messages: [], dropped: current.dropped } });
  return current.messages;
}

export async function clearDropped(): Promise<void> {
  const current = await readQueue();
  await chrome.storage.local.set({ [KEY]: { ...current, dropped: 0 } });
}
