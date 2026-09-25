/**
 * Long reports only list their busiest rows until asked for the rest, so the
 * tail of one-visit oddities does not cost paint work on every scroll.
 */

import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { ReportTable } from '../ReportTable';
import type { ReportSet } from '../../types';

function report(count: number): ReportSet {
  return {
    dimension: 'application',
    totalMs: count * 60_000,
    rows: Array.from({ length: count }, (_, index) => ({
      key: `app-${index}`,
      label: `App ${index}`,
      secondary: null,
      durationMs: (count - index) * 60_000,
      percentage: 1,
      visitCount: 1,
    })),
  };
}

function rows(markup: string): number {
  return markup.match(/<tr[ >]/g)?.length ?? 0;
}

describe('ReportTable', () => {
  it('lists everything when the report is short', () => {
    const markup = renderToStaticMarkup(<ReportTable report={report(9)} title="Applications" />);
    // Nine rows plus the header.
    expect(rows(markup)).toBe(10);
    expect(markup).not.toContain('Show all');
  });

  it('stops at the busiest rows and offers the rest', () => {
    const markup = renderToStaticMarkup(<ReportTable report={report(40)} title="Applications" />);

    expect(rows(markup)).toBe(16);
    expect(markup).toContain('App 14');
    expect(markup).not.toContain('App 15<');
    expect(markup).toContain('Show all 40');
  });

  it('says so when there is nothing to report', () => {
    const markup = renderToStaticMarkup(<ReportTable report={report(0)} title="Websites" />);
    expect(markup).toContain('No data for this period.');
  });
});
