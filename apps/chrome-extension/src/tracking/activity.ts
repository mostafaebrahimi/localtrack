/**
 * Turns Chrome tab and window events into sanitized activity payloads.
 *
 * Sanitization happens here — before anything is sent or queued — so no raw URL
 * with a query string ever leaves this module (spec §68, §102).
 */

import {
  clamp,
  domainOf,
  LIMITS,
  sanitizeUrl,
  type BrowserActivityPayload,
  type BrowserEvent,
  type UrlPolicy,
} from '@localtrack/protocol';

export interface TabLike {
  id?: number;
  windowId?: number;
  url?: string;
  pendingUrl?: string;
  title?: string;
  incognito?: boolean;
  audible?: boolean;
  active?: boolean;
}

export interface ActivityContext {
  urlPolicy: UrlPolicy;
  trackIncognito: boolean;
  windowFocused: boolean;
  now: number;
}

/**
 * Build a payload for a tab, or `null` when it must not be tracked
 * (incognito without opt-in, internal pages, no URL).
 */
export function buildActivityPayload(
  tab: TabLike,
  event: BrowserEvent,
  context: ActivityContext,
): BrowserActivityPayload | null {
  if (tab.incognito && !context.trackIncognito) return null;

  const rawUrl = tab.url ?? tab.pendingUrl;
  if (!rawUrl) return null;

  const domain = domainOf(rawUrl);
  const url = sanitizeUrl(rawUrl, context.urlPolicy);
  // chrome://, file:// and about: pages are not activity.
  if (!domain || !url) return null;

  return {
    event,
    capturedAt: context.now,
    browser: 'chrome',
    windowId: tab.windowId,
    tabId: tab.id,
    url,
    title: clamp(tab.title, LIMITS.pageTitle),
    incognito: Boolean(tab.incognito),
    audible: Boolean(tab.audible),
    focused: context.windowFocused,
  };
}

/** A payload that tells the host browsing stopped (blur, close, lock). */
export function buildStopPayload(event: BrowserEvent, now: number): BrowserActivityPayload {
  return {
    event,
    capturedAt: now,
    browser: 'chrome',
    incognito: false,
    audible: false,
    focused: false,
  };
}

/** True when two payloads describe the same page, so no message is needed. */
export function isSamePage(
  a: BrowserActivityPayload | null,
  b: BrowserActivityPayload | null,
): boolean {
  if (!a || !b) return false;
  return a.url === b.url && a.title === b.title && a.tabId === b.tabId;
}
