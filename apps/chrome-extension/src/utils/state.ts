/**
 * Small operational state persisted in `chrome.storage.local` (spec §38).
 *
 * MV3 service workers are terminated when idle, so nothing important may live
 * only in a global variable. Historical activity is never stored here: SQLite
 * remains authoritative.
 */

import type { UrlPolicy } from '@localtrack/protocol';

export interface OperationalState {
  trackingEnabled: boolean;
  nativeConnectionStatus: 'connected' | 'disconnected' | 'connecting';
  lastActiveTabId: number | null;
  lastKnownUrl: string | null;
  lastKnownTitle: string | null;
  lastKnownDomain: string | null;
  lastHeartbeatAt: number | null;
  lastActivityStartedAt: number | null;
  windowFocused: boolean;
  droppedMessages: number;
  urlPolicy: UrlPolicy;
  trackIncognito: boolean;
  detailedInteractions: boolean;
}

export const DEFAULT_STATE: OperationalState = {
  trackingEnabled: true,
  nativeConnectionStatus: 'disconnected',
  lastActiveTabId: null,
  lastKnownUrl: null,
  lastKnownTitle: null,
  lastKnownDomain: null,
  lastHeartbeatAt: null,
  lastActivityStartedAt: null,
  windowFocused: true,
  droppedMessages: 0,
  urlPolicy: 'PATH_WITHOUT_QUERY',
  trackIncognito: false,
  detailedInteractions: false,
};

const KEY = 'localtrack:state';

export async function loadState(): Promise<OperationalState> {
  const stored = await chrome.storage.local.get(KEY);
  const value = stored[KEY] as Partial<OperationalState> | undefined;
  return { ...DEFAULT_STATE, ...(value ?? {}) };
}

export async function saveState(patch: Partial<OperationalState>): Promise<OperationalState> {
  const current = await loadState();
  const next = { ...current, ...patch };
  await chrome.storage.local.set({ [KEY]: next });
  return next;
}

export async function resetState(): Promise<void> {
  await chrome.storage.local.remove(KEY);
}
