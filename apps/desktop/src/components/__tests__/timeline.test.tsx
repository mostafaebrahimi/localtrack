/**
 * The strip folds runs of very short blocks together. Getting that wrong either
 * loses time from the picture or puts hundreds of invisible elements back into
 * the page, so the arithmetic is worth pinning down.
 */

import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { TimelineList, TimelineStrip } from '../Timeline';
import type { TimelineBlock, TimelineBlockKind } from '../../types';

const HOUR = 3_600_000;
const DAY_START = new Date(2026, 7, 21, 7, 0, 0, 0).getTime();

function block(
  startMs: number,
  durationMs: number,
  blockKind: TimelineBlockKind = 'APPLICATION',
  label = 'Editor',
): TimelineBlock {
  return {
    startMs,
    endMs: startMs + durationMs,
    durationMs,
    blockKind,
    label,
    segmentId: null,
    source: null,
    kind: null,
    appName: null,
    processName: null,
    windowTitle: null,
    browser: null,
    domain: null,
    url: null,
    pageTitle: null,
    categoryId: null,
    projectId: null,
  };
}

function blocksIn(markup: string): number {
  return markup.match(/class="block kind-/g)?.length ?? 0;
}

describe('TimelineStrip', () => {
  const window8h = { fromMs: DAY_START, toMs: DAY_START + 8 * HOUR };

  it('draws blocks that are wide enough on their own', () => {
    const markup = renderToStaticMarkup(
      <TimelineStrip
        blocks={[block(DAY_START, HOUR), block(DAY_START + HOUR, 2 * HOUR)]}
        {...window8h}
      />,
    );
    expect(blocksIn(markup)).toBe(2);
  });

  it('folds a run of one-second blocks into a single piece', () => {
    const blocks = Array.from({ length: 40 }, (_, index) =>
      block(DAY_START + index * 1_000, 1_000),
    );
    const markup = renderToStaticMarkup(<TimelineStrip blocks={blocks} {...window8h} />);

    expect(blocksIn(markup)).toBe(1);
    expect(markup).toContain('40 short activities');
  });

  it('gives a folded run the colour of whatever held it longest', () => {
    const blocks = [
      block(DAY_START, 2_000, 'APPLICATION', 'Editor'),
      block(DAY_START + 2_000, 20_000, 'BROWSER_PAGE', 'example.com'),
      block(DAY_START + 22_000, 2_000, 'APPLICATION', 'Editor'),
    ];
    const markup = renderToStaticMarkup(<TimelineStrip blocks={blocks} {...window8h} />);

    expect(blocksIn(markup)).toBe(1);
    expect(markup).toContain('kind-BROWSER_PAGE');
    expect(markup).toContain('mostly example.com');
  });

  it('keeps a wide block between two folded runs separate', () => {
    const short = (offset: number) => block(DAY_START + offset, 1_000);
    const blocks = [
      short(0),
      short(1_000),
      block(DAY_START + 2_000, HOUR, 'APPLICATION', 'Editor'),
      short(HOUR + 2_000),
      short(HOUR + 3_000),
    ];
    const markup = renderToStaticMarkup(<TimelineStrip blocks={blocks} {...window8h} />);
    expect(blocksIn(markup)).toBe(3);
  });

  it('leaves out blocks outside the window', () => {
    const markup = renderToStaticMarkup(
      <TimelineStrip blocks={[block(DAY_START - 4 * HOUR, HOUR)]} {...window8h} />,
    );
    expect(blocksIn(markup)).toBe(0);
    expect(markup).toContain('No activity recorded');
  });
});

describe('TimelineList', () => {
  const many = Array.from({ length: 60 }, (_, index) =>
    block(DAY_START + index * 60_000, 60_000, 'APPLICATION', `App ${index}`),
  );

  function rows(markup: string): number {
    return markup.match(/class="feed-item"/g)?.length ?? 0;
  }

  it('shows one page of blocks with a pager at each end', () => {
    const markup = renderToStaticMarkup(<TimelineList blocks={many} />);

    expect(rows(markup)).toBe(25);
    expect(markup).toContain('1–25 of 60');
    expect(markup).toContain('Page 1 of 3');
    expect(markup.match(/class="pagination"/g)).toHaveLength(2);
    expect(markup).toContain('App 24');
    expect(markup).not.toContain('App 25<');
  });

  it('leaves a short list alone', () => {
    const markup = renderToStaticMarkup(<TimelineList blocks={many.slice(0, 10)} />);

    expect(rows(markup)).toBe(10);
    expect(markup).not.toContain('class="pagination"');
  });
});
