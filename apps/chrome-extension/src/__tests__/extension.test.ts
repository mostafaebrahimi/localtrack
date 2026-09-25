import { describe, expect, it } from 'vitest';

import { backoffDelay, BACKOFF_STEPS_MS } from '../native/connection';
import { buildActivityPayload, buildStopPayload, isSamePage } from '../tracking/activity';
import { MAX_QUEUE_MESSAGES, trimQueue } from '../utils/queue';
import { DEFAULT_STATE } from '../utils/state';

const context = {
  urlPolicy: 'PATH_WITHOUT_QUERY' as const,
  trackIncognito: false,
  windowFocused: true,
  now: 1_787_253_458_123,
};

describe('activity payloads', () => {
  it('sanitizes the URL before it can leave the extension', () => {
    const payload = buildActivityPayload(
      {
        id: 152,
        windowId: 41,
        url: 'https://github.com/company/project/pull/23?token=secret#files',
        title: '   Fix authentication  ·  Pull Request #23 ',
      },
      'activated',
      context,
    );
    expect(payload?.url).toBe('https://github.com/company/project/pull/23');
    expect(payload?.url).not.toContain('token');
    expect(payload?.title).toBe('Fix authentication · Pull Request #23');
    expect(payload?.focused).toBe(true);
  });

  it('never reports incognito tabs unless the user opted in', () => {
    const tab = { id: 1, url: 'https://example.com/private', incognito: true };
    expect(buildActivityPayload(tab, 'activated', context)).toBeNull();
    expect(
      buildActivityPayload(tab, 'activated', { ...context, trackIncognito: true })?.url,
    ).toBe('https://example.com/private');
  });

  it('ignores internal pages and tabs without a URL', () => {
    expect(buildActivityPayload({ id: 1, url: 'chrome://extensions' }, 'activated', context)).toBeNull();
    expect(buildActivityPayload({ id: 1 }, 'activated', context)).toBeNull();
  });

  it('marks blur and close as not focused', () => {
    const stop = buildStopPayload('blurred', context.now);
    expect(stop.focused).toBe(false);
    expect(stop.url).toBeUndefined();
  });

  it('detects unchanged pages so no message is sent', () => {
    const tab = { id: 1, url: 'https://example.com/a', title: 'A' };
    const first = buildActivityPayload(tab, 'activated', context);
    const second = buildActivityPayload(tab, 'updated', context);
    expect(isSamePage(first, second)).toBe(true);
    expect(isSamePage(first, null)).toBe(false);
  });

  it('honours the domain-only policy', () => {
    const payload = buildActivityPayload(
      { id: 1, url: 'https://example.com/orders/183?a=b' },
      'activated',
      { ...context, urlPolicy: 'DOMAIN_ONLY' },
    );
    expect(payload?.url).toBe('https://example.com');
  });
});

describe('outbox queue', () => {
  const message = (index: number) =>
    ({
      version: 1 as const,
      messageId: `m${index}`,
      type: 'browser.activity' as const,
      sentAt: index,
      payload: {},
    });

  it('keeps at most 500 messages and drops the oldest', () => {
    const messages = Array.from({ length: 520 }, (_, index) => message(index));
    const { kept, dropped } = trimQueue(messages);
    expect(kept).toHaveLength(MAX_QUEUE_MESSAGES);
    expect(dropped).toBe(20);
    expect(kept[0]?.messageId).toBe('m20');
  });

  it('keeps small queues untouched', () => {
    const { kept, dropped } = trimQueue([message(1), message(2)]);
    expect(kept).toHaveLength(2);
    expect(dropped).toBe(0);
  });

  it('drops oversized payloads to respect the 5 MB budget', () => {
    const big = {
      ...message(1),
      payload: { title: 'x'.repeat(200_000) },
    };
    const messages = Array.from({ length: 40 }, () => big);
    const { kept, dropped } = trimQueue(messages);
    expect(JSON.stringify(kept).length).toBeLessThanOrEqual(5 * 1024 * 1024);
    expect(dropped).toBeGreaterThan(0);
  });
});

describe('reconnect backoff', () => {
  it('follows 1s, 2s, 5s, 10s, 30s and stops there', () => {
    expect(BACKOFF_STEPS_MS).toEqual([1000, 2000, 5000, 10000, 30000]);
    expect(backoffDelay(0)).toBe(1000);
    expect(backoffDelay(4)).toBe(30000);
    expect(backoffDelay(99)).toBe(30000);
  });
});

describe('default operational state', () => {
  it('is privacy-safe out of the box', () => {
    expect(DEFAULT_STATE.trackIncognito).toBe(false);
    expect(DEFAULT_STATE.detailedInteractions).toBe(false);
    expect(DEFAULT_STATE.urlPolicy).toBe('PATH_WITHOUT_QUERY');
  });
});
