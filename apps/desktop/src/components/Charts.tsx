/**
 * The two charts the reports need, drawn by hand.
 *
 * A charting library was tried first and cost 12% of a CPU core continuously —
 * its responsive container re-measures in a loop — plus 390 KB of JavaScript
 * held in memory all day. A tracker that is supposed to go unnoticed cannot
 * afford either, and these two shapes are a few lines of SVG.
 */

import { memo } from 'react';

import { useTranslation, type MessageKey } from '../i18n';
import { formatShortDay, formatDurationShort } from '../services/format';
import type { DaySummary, ReportRow } from '../types';

const PALETTE = [
  '#2f6f4f',
  '#3c6ea5',
  '#8a6116',
  '#7a4f8a',
  '#a5342c',
  '#4a5a6a',
  '#2b7c78',
  '#9aa4ae',
];

const SLICES = 8;

function DonutInner({ rows }: { rows: ReportRow[] }) {
  const { t } = useTranslation();
  const data = rows.slice(0, SLICES).filter((row) => row.durationMs > 0);
  const total = data.reduce((sum, row) => sum + row.durationMs, 0);
  if (total === 0) return <div className="muted small">{t('table.noData')}</div>;

  // One circle per slice, offset around the ring by what came before it.
  const radius = 42;
  const circumference = 2 * Math.PI * radius;
  let travelled = 0;

  return (
    <div className="donut">
      <svg viewBox="0 0 120 120" role="img" aria-label={t('chart.shareByCategory')}>
        {data.map((row, index) => {
          const share = row.durationMs / total;
          const dash = share * circumference;
          const offset = travelled;
          travelled += dash;
          return (
            <circle
              key={row.key}
              cx="60"
              cy="60"
              r={radius}
              fill="none"
              stroke={PALETTE[index % PALETTE.length]}
              strokeWidth="16"
              strokeDasharray={`${dash} ${circumference - dash}`}
              // -90° puts the first slice at twelve o'clock.
              strokeDashoffset={-offset}
              transform="rotate(-90 60 60)"
            >
              <title>{`${row.label} · ${formatDurationShort(row.durationMs)}`}</title>
            </circle>
          );
        })}
        <text x="60" y="57" className="donut-total">
          {formatDurationShort(total)}
        </text>
        <text x="60" y="72" className="donut-caption">
          {t('chart.tracked')}
        </text>
      </svg>
      <ul className="donut-legend">
        {data.map((row, index) => (
          <li key={row.key}>
            <span className="swatch" style={{ background: PALETTE[index % PALETTE.length] }} />
            <span className="donut-label">{row.label}</span>
            <span className="muted">{formatDurationShort(row.durationMs)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

const BANDS = [
  { key: 'activeMs', label: 'chart.active' as MessageKey, colour: '#0e7a57' },
  { key: 'idleMs', label: 'chart.idle' as MessageKey, colour: '#98a4ad' },
  { key: 'breakMs', label: 'chart.break' as MessageKey, colour: '#b07c3c' },
  { key: 'untrackedMs', label: 'chart.untracked' as MessageKey, colour: '#ccd3d9' },
] as const;

function DailyBarsInner({ days }: { days: DaySummary[] }) {
  const { t } = useTranslation();
  if (days.length === 0) return <div className="muted small">{t('table.noData')}</div>;

  const totals = days.map((day) => day.activeMs + day.idleMs + day.breakMs + day.untrackedMs);
  const longest = Math.max(1, ...totals);
  // Past a couple of hours a minutes axis stops being readable.
  const inHours = longest > 2 * 3_600_000;
  const unit = inHours ? 'h' : 'm';
  const axisTop = inHours
    ? Math.ceil(longest / 3_600_000)
    : Math.ceil(longest / 60_000 / 15) * 15;
  const marks = [axisTop, axisTop / 2, 0];

  return (
    <div className="bars">
      <div className="bars-plot">
        <div className="bars-axis">
          {marks.map((mark) => (
            <span key={mark}>
              {Math.round(mark * 10) / 10}
              {unit}
            </span>
          ))}
        </div>
        <div className="bars-columns">
          {days.map((day, index) => {
            const total = totals[index] ?? 0;
            return (
              <div className="bars-column" key={day.date}>
                <div className="bars-stack">
                  {BANDS.map((band) => {
                    const value = day[band.key];
                    if (value <= 0) return null;
                    return (
                      <div
                        key={band.key}
                        style={{
                          height: `${(value / (longest || 1)) * 100}%`,
                          background: band.colour,
                        }}
                        title={`${formatShortDay(day.date)} · ${t(band.label)} ${formatDurationShort(value)}`}
                      />
                    );
                  })}
                </div>
                <span className="bars-label" title={`${formatShortDay(day.date)} · ${formatDurationShort(total)}`}>
                  {formatShortDay(day.date)}
                </span>
              </div>
            );
          })}
        </div>
      </div>
      <div className="legend">
        {BANDS.map((band) => (
          <span key={band.key} style={{ color: band.colour }}>
            {t(band.label)}
          </span>
        ))}
      </div>
    </div>
  );
}

/** Both charts redraw only when their data changes. */
export const CategoryDonut = memo(DonutInner);
export const DailyBars = memo(DailyBarsInner);
