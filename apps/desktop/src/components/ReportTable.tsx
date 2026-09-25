import { memo, useState } from 'react';

import { useTranslation } from '../i18n';
import { figures } from '../services/digits';
import { formatDurationShort, formatPercent } from '../services/format';
import type { ReportSet } from '../types';

/**
 * How many rows a report shows before it has to be asked for the rest.
 *
 * The rows are sorted by time, so the tail of a long report is a list of
 * one-visit oddities — not worth the paint work on every scroll until someone
 * actually wants it.
 */
const TOP_ROWS = 15;

/**
 * The one row whose text the aggregation writes rather than the machine:
 * time no title could place. It is a label, not data, so it is translated
 * here rather than left in the language the core happens to be written in.
 */
function useRowLabel() {
  const { t } = useTranslation();
  return (row: { key: string; label: string }) =>
    row.key.startsWith('UNKNOWN:') ? t('report.notIdentified') : row.label;
}

function ReportTableInner({
  report,
  title,
  emptyLabel,
  onSelect,
  secondaryHeader,
  topRows = TOP_ROWS,
}: {
  report: ReportSet | undefined;
  title: string;
  emptyLabel?: string;
  onSelect?: (key: string) => void;
  secondaryHeader?: string;
  topRows?: number;
}) {
  const { t } = useTranslation();
  const label = useRowLabel();
  const [showAll, setShowAll] = useState(false);
  const rows = report?.rows ?? [];
  const visible = showAll ? rows : rows.slice(0, topRows);

  return (
    <section className="card">
      <div className="row between" style={{ marginBottom: 8 }}>
        <h2 style={{ fontSize: 15, margin: 0 }}>{title}</h2>
        <span className="muted small">
          {report ? formatDurationShort(report.totalMs) : ''}
        </span>
      </div>
      {!report || report.rows.length === 0 ? (
        <div className="muted small">{emptyLabel ?? t('table.noData')}</div>
      ) : (
        <table className="report-table">
          <thead>
            <tr>
              <th>{t('table.name')}</th>
              {secondaryHeader ? <th className="secondary-col">{secondaryHeader}</th> : null}
              <th className="numeric time-col">{t('table.time')}</th>
              <th className="numeric share-col">{t('table.share')}</th>
              <th className="numeric visits-col">{t('table.visits')}</th>
              <th className="bar-col" />
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => (
              <tr
                key={row.key}
                onClick={() => onSelect?.(row.key)}
                style={{ cursor: onSelect ? 'pointer' : 'default' }}
              >
                <td className="ellipsis" title={label(row)}>
                  {label(row)}
                </td>
                {secondaryHeader ? (
                  <td className="muted small ellipsis secondary-col" title={row.secondary ?? ''}>
                    {row.secondary ?? ''}
                  </td>
                ) : null}
                <td className="numeric">{formatDurationShort(row.durationMs)}</td>
                <td className="numeric">{formatPercent(row.percentage)}</td>
                <td className="numeric">{figures(row.visitCount)}</td>
                <td>
                  <div className="bar" style={{ width: `${Math.max(row.percentage, 1)}%` }} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {rows.length > topRows ? (
        <button className="ghost small" onClick={() => setShowAll(!showAll)}>
          {showAll ? t('table.showTop', { count: topRows }) : t('table.showAll', { count: rows.length })}
        </button>
      ) : null}
    </section>
  );
}

/**
 * Drawing this is not free, and the live status updates around it constantly.
 * Memoising keeps the work tied to the data instead of the clock.
 */
export const ReportTable = memo(ReportTableInner);
