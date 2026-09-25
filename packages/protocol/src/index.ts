/**
 * Shared native messaging protocol definitions (spec §34–§36, §106, §150).
 *
 * This package is the single source of truth for the message shapes exchanged
 * between the Chrome extension and the Rust native host. It contains no
 * network code of any kind.
 */

export const PROTOCOL_VERSION = 1 as const;
export const NATIVE_HOST_NAME = 'com.localtrack.native' as const;

/** Maximum metadata lengths, mirroring the Rust limits (spec §106). */
export const LIMITS = {
  appName: 255,
  processName: 255,
  windowTitle: 2048,
  domain: 255,
  url: 8192,
  pageTitle: 2048,
  interactionLabel: 120,
  messageBytes: 1024 * 1024,
} as const;

export const MESSAGE_TYPES = {
  hello: 'hello',
  activity: 'browser.activity',
  interaction: 'browser.interaction',
  heartbeat: 'browser.heartbeat',
  clockCommand: 'clock.command',
  statusRequest: 'status.request',
} as const;

export type MessageType = (typeof MESSAGE_TYPES)[keyof typeof MESSAGE_TYPES];

export type BrowserEvent =
  | 'activated'
  | 'updated'
  | 'navigated'
  | 'title_changed'
  | 'focused'
  | 'blurred'
  | 'closed'
  | 'heartbeat';

export type InteractionType =
  | 'navigation'
  | 'button_click'
  | 'link_click'
  | 'form_submit'
  | 'scroll_activity';

export type UrlPolicy = 'DOMAIN_ONLY' | 'PATH_WITHOUT_QUERY' | 'FULL_URL';

export type ClockState = 'CLOCKED_OUT' | 'CLOCKED_IN' | 'ON_BREAK';

export type ClockCommand = 'clock_in' | 'clock_out' | 'start_break' | 'end_break';

export interface Envelope<T = unknown> {
  version: typeof PROTOCOL_VERSION;
  messageId: string;
  type: MessageType;
  sentAt: number;
  payload: T;
}

export interface BrowserActivityPayload {
  event: BrowserEvent;
  capturedAt: number;
  browser: 'chrome';
  windowId?: number;
  tabId?: number;
  /** Already sanitized according to the active URL policy. */
  url?: string;
  title?: string;
  incognito: boolean;
  audible: boolean;
  focused: boolean;
}

export interface BrowserInteractionPayload {
  interaction: InteractionType;
  capturedAt: number;
  /** Element kind only — never a value. */
  element?: string;
  /** Accessible label, at most 120 characters. */
  label?: string;
  url?: string;
}

export interface HeartbeatPayload {
  capturedAt: number;
}

export interface ClockCommandPayload {
  command: ClockCommand;
}

export interface AckMessage<T = unknown> {
  version: number;
  messageId: string;
  type: 'ack';
  receivedAt: number;
  success: true;
  payload?: T;
}

export interface ErrorMessage {
  version: number;
  messageId: string;
  type: 'error';
  error: { code: string; message: string };
}

export type HostResponse<T = unknown> = AckMessage<T> | ErrorMessage;

export interface HelloResponse {
  protocolVersion: number;
  hostVersion: string;
  origin?: string | null;
  settings?: HostSettings;
}

/** The subset of settings the extension needs to behave correctly. */
export interface HostSettings {
  urlPolicy: UrlPolicy;
  trackIncognito: boolean;
  detailedInteractions: boolean;
  trackingPaused: boolean;
}

export interface StatusResponse {
  state: ClockState;
  sessionStartedAtMs: number | null;
  sessionDurationMs: number;
  onBreakSinceMs: number | null;
  trackingDecision: string;
  todayActiveMs: number;
  hostVersion: string;
}

export function isAck<T>(response: HostResponse<T>): response is AckMessage<T> {
  return response.type === 'ack';
}

export function isError(response: HostResponse): response is ErrorMessage {
  return response.type === 'error';
}

/** Build a protocol envelope. */
export function envelope<T>(type: MessageType, payload: T, messageId: string): Envelope<T> {
  return {
    version: PROTOCOL_VERSION,
    messageId,
    type,
    sentAt: Date.now(),
    payload,
  };
}

/** Collapse whitespace and clamp a string to a maximum length. */
export function clamp(value: string | undefined, max: number): string | undefined {
  if (value === undefined) return undefined;
  const normalized = value.replace(/\s+/g, ' ').trim();
  if (normalized.length === 0) return undefined;
  return normalized.length > max ? normalized.slice(0, max) : normalized;
}

/**
 * The registrable host of a URL, lower-cased and without credentials.
 * Returns undefined for anything that is not an http(s) page.
 */
export function domainOf(raw: string): string | undefined {
  try {
    const parsed = new URL(raw);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined;
    const host = parsed.hostname.replace(/^www\./i, '').toLowerCase();
    return host.length > 0 ? clamp(host, LIMITS.domain) : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Sanitize a URL according to policy (spec §44).
 *
 * Credentials, query strings and fragments are removed unless the user
 * explicitly chose FULL_URL, and credentials are always removed. The extension
 * applies this *before* anything is queued, so nothing sensitive is ever held
 * in browser storage (spec §102).
 */
export function sanitizeUrl(raw: string, policy: UrlPolicy): string | undefined {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return undefined;
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined;

  parsed.username = '';
  parsed.password = '';

  if (policy === 'DOMAIN_ONLY') {
    return `${parsed.protocol}//${parsed.host}`;
  }
  if (policy === 'PATH_WITHOUT_QUERY') {
    parsed.search = '';
    parsed.hash = '';
    const value = parsed.toString();
    return parsed.pathname === '/' ? value.replace(/\/$/, '') : value;
  }
  return clamp(parsed.toString(), LIMITS.url);
}
