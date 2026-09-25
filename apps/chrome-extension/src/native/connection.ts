/**
 * Long-lived native messaging connection (spec §37, §40).
 *
 * `chrome.runtime.connectNative` keeps one host process alive for the whole
 * session; heartbeats reuse it instead of spawning a process per message.
 * Reconnection uses a bounded backoff of 1, 2, 5, 10 and 30 seconds.
 */

import {
  envelope,
  isError,
  MESSAGE_TYPES,
  NATIVE_HOST_NAME,
  type Envelope,
  type HostResponse,
  type MessageType,
} from '@localtrack/protocol';

export const BACKOFF_STEPS_MS = [1000, 2000, 5000, 10000, 30000];

export function backoffDelay(attempt: number): number {
  const index = Math.min(Math.max(attempt, 0), BACKOFF_STEPS_MS.length - 1);
  return BACKOFF_STEPS_MS[index] as number;
}

export type ConnectionStatus = 'connected' | 'connecting' | 'disconnected';

export interface ConnectionCallbacks {
  onStatusChange?: (status: ConnectionStatus) => void;
  onMessage?: (response: HostResponse) => void;
  onDisconnect?: (reason?: string) => void;
}

let messageCounter = 0;

/** Message ids only need to be unique within a session. */
export function nextMessageId(): string {
  messageCounter += 1;
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.round(Math.random() * 1e9)}`;
  return `${random}-${messageCounter}`;
}

export class NativeConnection {
  private port: chrome.runtime.Port | null = null;
  private status: ConnectionStatus = 'disconnected';
  private attempt = 0;
  private reconnectTimer: number | null = null;

  constructor(private readonly callbacks: ConnectionCallbacks = {}) {}

  getStatus(): ConnectionStatus {
    return this.status;
  }

  isConnected(): boolean {
    return this.status === 'connected' && this.port !== null;
  }

  connect(): void {
    if (this.port) return;
    this.setStatus('connecting');
    try {
      const port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
      this.port = port;
      port.onMessage.addListener((message) => {
        const response = message as HostResponse;
        if (isError(response)) {
          console.warn('[LocalTrack] native host rejected a message', response.error.code);
        }
        this.callbacks.onMessage?.(response);
      });
      port.onDisconnect.addListener(() => {
        const reason = chrome.runtime.lastError?.message;
        this.port = null;
        this.setStatus('disconnected');
        this.callbacks.onDisconnect?.(reason);
        this.scheduleReconnect();
      });
      this.attempt = 0;
      this.setStatus('connected');
      this.send(MESSAGE_TYPES.hello, {});
    } catch (error) {
      this.port = null;
      this.setStatus('disconnected');
      this.callbacks.onDisconnect?.(String(error));
      this.scheduleReconnect();
    }
  }

  disconnect(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.port?.disconnect();
    this.port = null;
    this.setStatus('disconnected');
  }

  /** Send a message; returns false when the host is not available. */
  send(type: MessageType, payload: unknown): boolean {
    if (!this.port) return false;
    try {
      this.port.postMessage(envelope(type, payload, nextMessageId()));
      return true;
    } catch (error) {
      console.warn('[LocalTrack] could not post to the native host', error);
      this.port = null;
      this.setStatus('disconnected');
      this.scheduleReconnect();
      return false;
    }
  }

  sendEnvelope(message: Envelope<unknown>): boolean {
    if (!this.port) return false;
    try {
      this.port.postMessage(message);
      return true;
    } catch {
      this.port = null;
      this.setStatus('disconnected');
      this.scheduleReconnect();
      return false;
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer !== null) return;
    const delay = backoffDelay(this.attempt);
    this.attempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay) as unknown as number;
  }

  private setStatus(status: ConnectionStatus): void {
    if (this.status === status) return;
    this.status = status;
    this.callbacks.onStatusChange?.(status);
  }
}
