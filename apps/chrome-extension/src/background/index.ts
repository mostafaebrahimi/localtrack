/**
 * LocalTrack background script.
 *
 * Runs as an MV3 service worker in Chrome and as an event page in Firefox; both
 * expose the same `chrome.*` surface, and both can be terminated at any moment,
 * so nothing important lives in a global variable.
 *
 * Responsibilities:
 *  - observe tab activation, navigation, title changes and window focus,
 *  - sanitize before anything leaves this file,
 *  - keep a long-lived native messaging connection with bounded reconnects,
 *  - heartbeat the active page roughly every 15 seconds,
 *  - survive service-worker termination using chrome.storage and chrome.alarms.
 *
 * It never performs a network request.
 */

import {
  envelope,
  isAck,
  MESSAGE_TYPES,
  sanitizeUrl,
  type ClockCommand,
  type HelloResponse,
  type HostResponse,
  type MessageType,
  type StatusResponse,
} from '@localtrack/protocol';

import { NativeConnection } from '../native/connection';
import {
  buildActivityPayload,
  buildStopPayload,
  type ActivityContext,
} from '../tracking/activity';
import { drainQueue, enqueue, readQueue } from '../utils/queue';
import { loadState, saveState, type OperationalState } from '../utils/state';

const HEARTBEAT_ALARM = 'localtrack:heartbeat';
const RECOVERY_ALARM = 'localtrack:recovery';
/** Chrome's minimum repeating alarm period is 30 seconds (spec §39). */
const RECOVERY_PERIOD_MINUTES = 0.5;
const HEARTBEAT_INTERVAL_MS = 15_000;

let lastPayloadKey: string | null = null;
let lastStatus: StatusResponse | null = null;
let heartbeatTimer: number | null = null;

const connection = new NativeConnection({
  onStatusChange: (status) => {
    void saveState({ nativeConnectionStatus: status });
    if (status === 'connected') {
      void flushQueue();
    }
  },
  onMessage: (response) => handleResponse(response),
  onDisconnect: (reason) => {
    console.info('[LocalTrack] native host disconnected', reason ?? '');
  },
});

function handleResponse(response: HostResponse): void {
  if (!isAck(response)) return;
  const payload = response.payload as Partial<HelloResponse & StatusResponse> | undefined;
  if (!payload) return;

  if (typeof payload.protocolVersion === 'number' && payload.settings) {
    // The host tells us the active privacy policy so sanitization matches it.
    void saveState({
      urlPolicy: payload.settings.urlPolicy,
      trackIncognito: payload.settings.trackIncognito,
      detailedInteractions: payload.settings.detailedInteractions,
    });
  }
  if (typeof payload.state === 'string') {
    lastStatus = payload as StatusResponse;
  }
}

async function context(state?: OperationalState): Promise<ActivityContext> {
  const current = state ?? (await loadState());
  return {
    urlPolicy: current.urlPolicy,
    trackIncognito: current.trackIncognito,
    windowFocused: current.windowFocused,
    now: Date.now(),
  };
}

/** Send an activity payload, queueing it when the host is unavailable. */
async function publish(payload: unknown, type: MessageType = MESSAGE_TYPES.activity): Promise<void> {
  const message = envelope(type, payload, crypto.randomUUID());
  if (connection.isConnected() && connection.sendEnvelope(message)) return;

  // Only sanitized payloads reach the queue (spec §102).
  const queue = await enqueue(message);
  await saveState({ droppedMessages: queue.dropped });
  connection.connect();
}

async function flushQueue(): Promise<void> {
  const messages = await drainQueue();
  for (const message of messages) {
    if (!connection.sendEnvelope(message)) {
      // Put the remainder back and try again after the next reconnect.
      await enqueue(message);
    }
  }
}

async function reportActiveTab(event: Parameters<typeof buildActivityPayload>[1]): Promise<void> {
  const state = await loadState();
  if (!state.trackingEnabled) return;

  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) return;

  const payload = buildActivityPayload(tab, event, await context(state));
  if (!payload) {
    // The active tab is not trackable (internal page or incognito).
    lastPayloadKey = null;
    await publish(buildStopPayload('blurred', Date.now()));
    return;
  }

  const key = `${payload.tabId}|${payload.url}|${payload.title}`;
  if (key === lastPayloadKey && event !== 'focused') {
    return;
  }
  lastPayloadKey = key;

  await saveState({
    lastActiveTabId: tab.id ?? null,
    lastKnownUrl: payload.url ?? null,
    lastKnownTitle: payload.title ?? null,
    lastKnownDomain: payload.url ? new URL(payload.url).hostname : null,
    lastActivityStartedAt: Date.now(),
  });
  await publish(payload);
  startHeartbeat();
}

function startHeartbeat(): void {
  if (heartbeatTimer !== null) return;
  heartbeatTimer = setInterval(() => {
    void sendHeartbeat();
  }, HEARTBEAT_INTERVAL_MS) as unknown as number;
}

async function sendHeartbeat(): Promise<void> {
  const state = await loadState();
  if (!state.trackingEnabled || !state.windowFocused || !state.lastKnownUrl) return;
  await publish({ capturedAt: Date.now() }, MESSAGE_TYPES.heartbeat);
  await saveState({ lastHeartbeatAt: Date.now() });
}

async function stopTracking(event: 'blurred' | 'closed'): Promise<void> {
  lastPayloadKey = null;
  if (heartbeatTimer !== null) {
    clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
  await publish(buildStopPayload(event, Date.now()));
}

// ------------------------------------------------------------------ events

chrome.runtime.onInstalled.addListener(() => {
  void saveState({ trackingEnabled: true });
  void chrome.alarms.create(RECOVERY_ALARM, { periodInMinutes: RECOVERY_PERIOD_MINUTES });
  connection.connect();
});

chrome.runtime.onStartup.addListener(() => {
  void chrome.alarms.create(RECOVERY_ALARM, { periodInMinutes: RECOVERY_PERIOD_MINUTES });
  connection.connect();
});

chrome.tabs.onActivated.addListener(() => {
  void reportActiveTab('activated');
void syncContentScript();
});

chrome.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (!tab.active) return;
  if (changeInfo.url) {
    void reportActiveTab('navigated');
  } else if (changeInfo.title) {
    void reportActiveTab('title_changed');
  } else if (changeInfo.status === 'complete') {
    void reportActiveTab('updated');
  }
});

chrome.webNavigation.onCommitted.addListener((details) => {
  // Only main-frame navigations are activity.
  if (details.frameId !== 0) return;
  void reportActiveTab('navigated');
});

chrome.webNavigation.onHistoryStateUpdated.addListener((details) => {
  // Single-page applications change the URL without a load.
  if (details.frameId !== 0) return;
  void reportActiveTab('navigated');
});

chrome.windows.onFocusChanged.addListener((windowId) => {
  const focused = windowId !== chrome.windows.WINDOW_ID_NONE;
  void saveState({ windowFocused: focused }).then(() => {
    if (focused) {
      void reportActiveTab('focused');
    } else {
      void stopTracking('blurred');
    }
  });
});

chrome.windows.onRemoved.addListener(() => {
  void chrome.windows.getAll().then((windows) => {
    if (windows.length === 0) void stopTracking('closed');
  });
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === RECOVERY_ALARM || alarm.name === HEARTBEAT_ALARM) {
    // Recover state that a terminated service worker may have lost (spec §39).
    if (!connection.isConnected()) connection.connect();
    void sendHeartbeat();
  }
});

// ---------------------------------------------------------- popup messaging

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const request = message as { type: string; command?: ClockCommand };

  if (request.type === 'localtrack:getStatus') {
    void (async () => {
      const state = await loadState();
      const queue = await readQueue();
      connection.send(MESSAGE_TYPES.statusRequest, {});
      sendResponse({
        connection: connection.getStatus(),
        state,
        status: lastStatus,
        queued: queue.messages.length,
        dropped: queue.dropped,
      });
    })();
    return true;
  }

  if (request.type === 'localtrack:clock' && request.command) {
    const delivered = connection.send(MESSAGE_TYPES.clockCommand, { command: request.command });
    if (!delivered) connection.connect();
    sendResponse({ delivered });
    return true;
  }

  if (request.type === 'localtrack:reconnect') {
    connection.connect();
    sendResponse({ connection: connection.getStatus() });
    return true;
  }

  if (request.type === 'localtrack:interaction') {
    // Detailed interactions are only forwarded while the feature is on.
    void (async () => {
      const state = await loadState();
      if (!state.detailedInteractions) {
        sendResponse({ ok: false });
        return;
      }
      const payload = (message as { payload?: Record<string, unknown> }).payload ?? {};
      const rawUrl = typeof payload.url === 'string' ? payload.url : undefined;
      await publish(
        {
          ...payload,
          // The raw page URL never leaves this worker unsanitized.
          url: rawUrl ? sanitizeUrl(rawUrl, state.urlPolicy) : undefined,
        },
        MESSAGE_TYPES.interaction,
      );
      sendResponse({ ok: true });
    })();
    return true;
  }

  if (request.type === 'localtrack:setTracking') {
    void saveState({ trackingEnabled: Boolean((message as { enabled?: boolean }).enabled) }).then(
      () => sendResponse({ ok: true }),
    );
    return true;
  }

  return false;
});

/**
 * Register or remove the interaction content script (spec §32).
 *
 * The script is only injected when the user enabled detailed tracking *and*
 * granted host permission; `<all_urls>` is never requested at install time.
 */
async function syncContentScript(): Promise<void> {
  const state = await loadState();
  const granted = await chrome.permissions.contains({ origins: ['http://*/*', 'https://*/*'] });
  const registered = await chrome.scripting.getRegisteredContentScripts({ ids: ['localtrack-interactions'] }).catch(() => []);

  if (state.detailedInteractions && granted) {
    if (registered.length === 0) {
      await chrome.scripting.registerContentScripts([
        {
          id: 'localtrack-interactions',
          js: ['content.js'],
          matches: ['http://*/*', 'https://*/*'],
          runAt: 'document_idle',
          allFrames: false,
        },
      ]);
    }
    return;
  }

  if (registered.length > 0) {
    await chrome.scripting.unregisterContentScripts({ ids: ['localtrack-interactions'] });
  }
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes['localtrack:state']) {
    void syncContentScript();
  }
});

// The service worker may start cold: restore the connection immediately.
connection.connect();
void chrome.alarms.create(RECOVERY_ALARM, { periodInMinutes: RECOVERY_PERIOD_MINUTES });
void reportActiveTab('activated');
void syncContentScript();
