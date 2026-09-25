import { describe, expect, it } from 'vitest';

import {
  formatBytes,
  formatDelta,
  formatDurationHm,
  formatDurationHms,
  formatDurationShort,
  formatRelative,
} from '../format';
import { rangeFor } from '../ranges';

describe('duration formatting', () => {
  it('matches the dashboard style from the spec', () => {
    expect(formatDurationHm(8 * 3600000 + 34 * 60000)).toBe('08h 34m');
    expect(formatDurationHms(3 * 3600000 + 28 * 60000 + 14000)).toBe('03:28:14');
    expect(formatDurationShort(45000)).toBe('45s');
    expect(formatDurationShort(14 * 60000)).toBe('14m');
    expect(formatDurationShort(3600000 + 17 * 60000)).toBe('1h 17m');
  });

  it('never renders negative time', () => {
    expect(formatDurationHm(-1000)).toBe('00h 00m');
    expect(formatDurationHms(-1000)).toBe('00:00:00');
  });

  it('formats deltas with a sign', () => {
    expect(formatDelta(3600000)).toBe('+01h 00m');
    expect(formatDelta(-3600000)).toBe('−01h 00m');
  });
});

describe('formatBytes', () => {
  it('scales units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(44_339_200)).toBe('42.3 MB');
  });
});

describe('formatRelative', () => {
  it('describes recency', () => {
    const now = 1_000_000;
    expect(formatRelative(now - 1000, now)).toBe('just now');
    expect(formatRelative(now - 10_000, now)).toBe('10 sec ago');
    expect(formatRelative(now - 180_000, now)).toBe('3 min ago');
    expect(formatRelative(null, now)).toBe('never');
  });
});

describe('date range presets', () => {
  const now = new Date('2026-08-20T15:30:00').getTime();

  it('covers exactly one day for today', () => {
    const range = rangeFor('today', now);
    expect(range.toMs - range.fromMs).toBe(86400000);
    expect(range.fromMs).toBeLessThanOrEqual(now);
    expect(range.toMs).toBeGreaterThan(now);
  });

  it('places yesterday immediately before today', () => {
    const today = rangeFor('today', now);
    const yesterday = rangeFor('yesterday', now);
    expect(yesterday.toMs).toBe(today.fromMs);
  });

  it('builds seven and thirty day windows', () => {
    expect(rangeFor('last7', now).toMs - rangeFor('last7', now).fromMs).toBe(7 * 86400000);
    expect(rangeFor('last30', now).toMs - rangeFor('last30', now).fromMs).toBe(30 * 86400000);
  });

  it('starts weeks on Monday', () => {
    const week = rangeFor('thisWeek', now);
    expect(new Date(week.fromMs).getDay()).toBe(1);
    expect(week.toMs - week.fromMs).toBe(7 * 86400000);
  });

  it('last week ends where this week starts', () => {
    expect(rangeFor('lastWeek', now).toMs).toBe(rangeFor('thisWeek', now).fromMs);
  });
});
