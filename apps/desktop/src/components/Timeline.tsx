import { memo, useMemo, useState } from 'react';

import { useTranslation, type Translation } from '../i18n';

type Translate = Translation['t'];
import { formatClock, formatDurationShort } from '../services/format';
import type { TimelineBlock, TimelineBlockKind } from '../types';
import { Pager } from './Pager';

const HOUR_MS = 3_600_000;

/**
 * Hour marks across the visible window, thinned out so labels never collide.
 */
function hourTicks(fromMs: number, toMs: number): { at: number; label: string }[] {
  const span = Math.max(1, toMs - fromMs);
  const hours = span / HOUR_MS;
  const step = hours <= 8 ? 1 : hours <= 16 ? 2 : 4;

  const first = new Date(fromMs);
  first.setMinutes(0, 0, 0);
  if (first.getTime() < fromMs) first.setHours(first.getHours() + 1);

  const ticks: { at: number; label: string }[] = [];
  for (let at = first.getTime(); at < toMs; at += HOUR_MS) {
    if (new Date(at).getHours() % step !== 0) continue;
    ticks.push({ at, label: formatClock(at) });
  }
  return ticks;
}


/** A block, or a run of blocks too short to draw separately. */
interface Piece {
  key: string;
  kind: TimelineBlockKind;
  block: TimelineBlock;
  merged: number;
  left: number;
  width: number;
}

/**
 * Anything narrower than this is thinner than a pixel on a normal window.
 */
const MIN_WIDTH_PERCENT = 0.12;

/**
 * Lay the blocks out, folding runs of very short ones together.
 *
 * A busy day produces hundreds of blocks, most of them a few seconds long and
 * all of them landing on the same pixel column. Drawing each one costs paint
 * time on every scroll for something nobody can see or click; a folded run
 * keeps the colour of whatever dominated it and says so in its tooltip.
 */
function merge(
  blocks: TimelineBlock[],
  fromMs: number,
  toMs: number,
  span: number,
): Piece[] {
  const visible = blocks.filter((block) => block.endMs > fromMs && block.startMs < toMs);
  const pieces: Piece[] = [];
  let run: TimelineBlock[] = [];

  const flush = () => {
    if (run.length === 0) return;
    const startMs = Math.max(run[0]!.startMs, fromMs);
    const endMs = Math.min(run[run.length - 1]!.endMs, toMs);
    // The run wears the colour of whichever kind held it longest.
    const byKind = new Map<TimelineBlockKind, number>();
    let dominant = run[0]!;
    for (const block of run) {
      const held = (byKind.get(block.blockKind) ?? 0) + block.durationMs;
      byKind.set(block.blockKind, held);
      if (held >= (byKind.get(dominant.blockKind) ?? 0)) dominant = block;
    }
    pieces.push({
      key: `${startMs}-run`,
      kind: dominant.blockKind,
      block: dominant,
      merged: run.length,
      left: ((startMs - fromMs) / span) * 100,
      width: ((endMs - startMs) / span) * 100,
    });
    run = [];
  };

  for (const block of visible) {
    const left = ((Math.max(block.startMs, fromMs) - fromMs) / span) * 100;
    const width =
      ((Math.min(block.endMs, toMs) - Math.max(block.startMs, fromMs)) / span) * 100;

    if (width < MIN_WIDTH_PERCENT) {
      run.push(block);
      continue;
    }
    flush();
    pieces.push({
      key: `${block.startMs}-${block.label}`,
      kind: block.blockKind,
      block,
      merged: 1,
      left,
      width,
    });
  }
  flush();
  return pieces;
}

/**
 * The primary timeline (spec §73, §117): one row where blocks never overlap,
 * because the overlay was already resolved in Rust.
 */
function TimelineStripInner({
  blocks,
  fromMs,
  toMs,
  onSelect,
}: {
  blocks: TimelineBlock[];
  fromMs: number;
  toMs: number;
  onSelect?: (block: TimelineBlock) => void;
}) {
  const { t } = useTranslation();
  const span = Math.max(1, toMs - fromMs);
  const pieces = useMemo(() => merge(blocks, fromMs, toMs, span), [blocks, fromMs, toMs, span]);

  if (pieces.length === 0) {
    return <div className="muted small">{t('states.noActivityPeriod')}</div>;
  }

  const ticks = hourTicks(fromMs, toMs);
  // Where the moment is on the band, when the moment is on the band at all:
  // a past day is a finished measurement and gets no needle.
  const now = Date.now();
  const nowLeft = now > fromMs && now < toMs ? ((now - fromMs) / span) * 100 : null;

  return (
    <div>
      <div className="timeline" style={{ position: 'relative' }}>
        {ticks.map((tick) => (
          <span
            key={tick.at}
            className="timeline-tick"
            style={{ left: `${((tick.at - fromMs) / span) * 100}%` }}
          />
        ))}
        {pieces.map((piece) => (
          <button
            key={piece.key}
            type="button"
            className={`block kind-${piece.kind}`}
            style={{
              position: 'absolute',
              left: `${piece.left}%`,
              width: `${Math.max(piece.width, 0.2)}%`,
            }}
            title={describe(piece, t)}
            onClick={() => onSelect?.(piece.block)}
          />
        ))}
        {nowLeft !== null ? (
          <span className="now-needle" style={{ left: `${nowLeft}%` }} aria-hidden="true" />
        ) : null}
      </div>
      <div className="timeline-axis small muted">
        {/* The range ends are only labelled when no hour tick is already there. */}
        {ticks.length === 0 || (ticks[0]!.at - fromMs) / span > 0.14 ? (
          <span style={{ left: 0 }}>{formatClock(fromMs)}</span>
        ) : null}
        {ticks.map((tick) => (
          <span key={tick.at} style={{ left: `${((tick.at - fromMs) / span) * 100}%` }}>
            {tick.label}
          </span>
        ))}
        {ticks.length === 0 || (toMs - ticks[ticks.length - 1]!.at) / span > 0.14 ? (
          <span style={{ right: 0 }}>{formatClock(toMs)}</span>
        ) : null}
      </div>
      <div className="legend">
        <span style={{ color: 'var(--app)' }}>{t('legend.application')}</span>
        <span style={{ color: 'var(--browser)' }}>{t('legend.website')}</span>
        <span style={{ color: 'var(--idle)' }}>{t('legend.idle')}</span>
        <span style={{ color: 'var(--app)', opacity: 0.7 }}>{t('legend.activeNoDetail')}</span>
        <span style={{ color: 'var(--break)' }}>{t('legend.break')}</span>
        <span style={{ color: 'var(--untracked)' }}>{t('legend.untracked')}</span>
      </div>
    </div>
  );
}

/**
 * The label for a block whose text the aggregation wrote rather than the
 * machine: idle, locked, break, untracked. An application or a page carries
 * its own name and is left exactly as recorded.
 */
function blockLabel(block: TimelineBlock, t: Translate): string {
  switch (block.blockKind) {
    case 'UNTRACKED':
      return t('block.untracked');
    case 'IDLE':
      return t('block.idle');
    case 'LOCKED':
      return t('block.locked');
    case 'BREAK':
      return t('block.break');
    case 'INPUT':
      return t('block.active');
    default:
      return block.label;
  }
}

/** What a block, or a folded run of them, says when pointed at. */
function describe(piece: Piece, t: Translate): string {
  const { block } = piece;
  const when = `${formatClock(block.startMs)}–${formatClock(block.endMs)}`;
  if (piece.merged > 1) {
    return `${when} · ${t('timeline.shortActivities', {
      count: piece.merged,
      label: blockLabel(block, t),
    })}`;
  }
  return `${when} · ${blockLabel(block, t)} · ${formatDurationShort(block.durationMs)}`;
}

/** How many blocks the list shows at a time. */
const PAGE_SIZE = 25;

/**
 * The chronological block list (spec §73), a page at a time.
 *
 * A day can hold hundreds of blocks. Showing them all made one very long page
 * that had to be scrolled past, and every row cost paint work on the way; a
 * page of twenty-five is a screenful and keeps the page a sensible length.
 */
export function TimelineList({
  blocks,
  onSelect,
  pageSize = PAGE_SIZE,
}: {
  blocks: TimelineBlock[];
  onSelect?: (block: TimelineBlock) => void;
  pageSize?: number;
}) {
  const { t } = useTranslation();
  const [offset, setOffset] = useState(0);

  if (blocks.length === 0) {
    return <div className="muted small">{t('weekly.nothingRecorded')}</div>;
  }

  // Changing the range or the zoom can leave the offset past the end.
  const start = offset < blocks.length ? offset : 0;
  const page = blocks.slice(start, start + pageSize);
  const pager = (
    <Pager
      offset={start}
      pageSize={pageSize}
      total={blocks.length}
      shown={page.length}
      onChange={setOffset}
    />
  );

  return (
    <div>
      {blocks.length > pageSize ? pager : null}
      {page.map((block) => (
        <div
          className="feed-item"
          key={`${block.startMs}-${block.label}`}
          onClick={() => onSelect?.(block)}
          role="button"
          tabIndex={0}
          onKeyDown={(event) => {
            if (event.key === 'Enter') onSelect?.(block);
          }}
        >
          <div className="time">
            {formatClock(block.startMs)}–{formatClock(block.endMs)}
          </div>
          <div>
            <div className="title">{blockLabel(block, t)}</div>
            <div className="subtitle">
              {block.pageTitle ?? block.windowTitle ?? block.url ?? ''}
            </div>
          </div>
          <div className="muted">{formatDurationShort(block.durationMs)}</div>
        </div>
      ))}
      {blocks.length > pageSize ? pager : null}
    </div>
  );
}

/**
 * Drawing this is not free, and the live status updates around it constantly.
 * Memoising keeps the work tied to the data instead of the clock.
 */
export const TimelineStrip = memo(TimelineStripInner);
