/**
 * The charts are drawn by hand, so their geometry is worth pinning down: a
 * wrong arc length or bar height is a silently wrong report.
 */

import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { CategoryDonut, DailyBars } from '../Charts';
import type { DaySummary, ReportRow } from '../../types';

function row(key: string, durationMs: number): ReportRow {
  return { key, label: key, secondary: null, durationMs, percentage: 0, visitCount: 1 };
}

function day(date: string, activeMs: number, idleMs = 0): DaySummary {
  return {
    date,
    fromMs: 0,
    toMs: 0,
    clockedMs: activeMs + idleMs,
    breakMs: 0,
    workMs: activeMs,
    activeMs,
    idleMs,
    untrackedMs: 0,
    untrackedAgentOffMs: 0,
    contextSwitches: 0,
    averageFocusMs: 0,
    longestFocusMs: 0,
    medianFocusMs: 0,
    sessionCount: 1,
    firstClockInMs: null,
    lastClockOutMs: null,
  };
}

describe('CategoryDonut', () => {
  it('gives each slice an arc in proportion to its share', () => {
    const markup = renderToStaticMarkup(
      <CategoryDonut rows={[row('Work', 3 * 3_600_000), row('Email', 3_600_000)]} />,
    );

    const circumference = 2 * Math.PI * 42;
    const dashes = [...markup.matchAll(/stroke-dasharray="([\d.]+) ([\d.]+)"/g)].map((match) =>
      Number(match[1]),
    );

    expect(dashes).toHaveLength(2);
    expect(dashes[0]! / circumference).toBeCloseTo(0.75, 5);
    expect(dashes[1]! / circumference).toBeCloseTo(0.25, 5);
    // The second slice starts where the first one ended.
    expect(markup).toContain(`stroke-dashoffset="${-dashes[0]!}"`);
  });

  it('says so rather than drawing an empty ring', () => {
    expect(renderToStaticMarkup(<CategoryDonut rows={[]} />)).toContain('No data');
    expect(renderToStaticMarkup(<CategoryDonut rows={[row('Idle', 0)]} />)).toContain('No data');
  });
});

describe('DailyBars', () => {
  it('scales every day against the longest one', () => {
    const markup = renderToStaticMarkup(
      <DailyBars days={[day('2026-08-20', 4 * 3_600_000), day('2026-08-21', 2 * 3_600_000)]} />,
    );

    const heights = [...markup.matchAll(/height:([\d.]+)%/g)].map((match) => Number(match[1]));
    expect(heights).toEqual([100, 50]);
    // Hours, because the longest day is well past the two-hour mark.
    expect(markup).toContain('4h');
    // The column is labelled by reading the date in the viewer's own calendar,
    // not by slicing the stored string — so the expectation is derived the
    // same way rather than pinned to the locale this happens to run under.
    const label = new Date('2026-08-20T00:00:00').toLocaleDateString(undefined, {
      day: '2-digit',
      month: 'short',
    });
    expect(markup).toContain(label);
  });

  it('drops bands with no time in them', () => {
    const markup = renderToStaticMarkup(<DailyBars days={[day('2026-08-21', 60_000)]} />);
    expect(markup.match(/height:/g)).toHaveLength(1);
    // Minutes, for a day that never reached two hours.
    expect(markup).toContain('1m');
  });
});
