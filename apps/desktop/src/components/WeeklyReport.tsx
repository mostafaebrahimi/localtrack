import { useState } from 'react';

import { useTranslation } from '../i18n';
import { formatDate, formatDurationHm, formatDurationShort } from '../services/format';
import type { ReportRow, WeekSummary } from '../types';

/**
 * Week-by-week totals, each expandable into where the week actually went.
 *
 * A day is often too small a unit to see a pattern and a month too large; the
 * week is what people plan and bill in.
 */
export function WeeklyReport({ weeks }: { weeks: WeekSummary[] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<string | null>(weeks.length > 0 ? weeks[weeks.length - 1]!.week : null);

  if (weeks.length === 0) {
    return (
      <section className="card">
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('weekly.byWeek')}</h2>
        <p className="muted small">{t('weekly.noWeeks')}</p>
      </section>
    );
  }

  const busiest = Math.max(...weeks.map((week) => week.summary.activeMs), 1);

  return (
    <section className="card">
      <div className="row between" style={{ marginBottom: 8 }}>
        <h2 style={{ fontSize: 15, margin: 0 }}>{t('weekly.byWeek')}</h2>
        <span className="muted small">{t('weekly.selectWeek')}</span>
      </div>

      <table className="week-table">
        <thead>
          <tr>
            <th>{t('weekly.week')}</th>
            <th>{t('weekly.dates')}</th>
            <th className="numeric">{t('metric.clocked')}</th>
            <th className="numeric">{t('metric.active')}</th>
            <th className="numeric">{t('metric.idle')}</th>
            <th className="numeric">{t('metric.break')}</th>
            <th style={{ width: '22%' }} />
          </tr>
        </thead>
        <tbody>
          {weeks.map((week) => (
            <tr
              key={week.week}
              onClick={() => setOpen(open === week.week ? null : week.week)}
              className={open === week.week ? 'selected' : ''}
              style={{ cursor: 'pointer' }}
            >
              <td>{week.week}</td>
              <td className="muted small">
                {formatDate(week.startsAtMs)} – {formatDate(week.endsAtMs - 1)}
              </td>
              <td className="numeric">{formatDurationHm(week.summary.clockedMs)}</td>
              <td className="numeric">{formatDurationHm(week.summary.activeMs)}</td>
              <td className="numeric">{formatDurationHm(week.summary.idleMs)}</td>
              <td className="numeric">{formatDurationHm(week.summary.breakMs)}</td>
              <td>
                <div
                  className="bar"
                  style={{ width: `${Math.max((week.summary.activeMs / busiest) * 100, 1)}%` }}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {weeks
        .filter((week) => week.week === open)
        .map((week) => (
          <div className="week-detail" key={week.week}>
            <div className="week-days">
              {week.days.map((day) => (
                <div className="week-day" key={day.date} title={`${day.date}: ${formatDurationHm(day.activeMs)} active`}>
                  <div
                    className="week-day-bar"
                    style={{
                      height: `${Math.max((day.activeMs / Math.max(...week.days.map((d) => d.activeMs), 1)) * 100, 2)}%`,
                    }}
                  />
                  <span className="muted small">{day.date.slice(8)}</span>
                </div>
              ))}
            </div>

            <div className="grid-3">
              <UsageList title={t('report.applications')} rows={week.applications.rows} />
              <UsageList title={t('report.websites')} rows={week.websites.rows} />
              <UsageList title={t('reports.addresses')} rows={week.pages.rows} showSecondary />
            </div>
          </div>
        ))}
    </section>
  );
}

function UsageList({
  title,
  rows,
  showSecondary = false,
}: {
  title: string;
  rows: ReportRow[];
  showSecondary?: boolean;
}) {
  const { t } = useTranslation();
  const top = rows.slice(0, 8);
  return (
    <div>
      <h3 className="usage-title">{title}</h3>
      {top.length === 0 ? (
        <p className="muted small">{t('weekly.nothingRecorded')}</p>
      ) : (
        <ul className="usage-list">
          {top.map((row) => (
            <li key={row.key} title={showSecondary ? (row.secondary ?? row.label) : row.label}>
              <span className="usage-name">{row.label}</span>
              <span className="usage-time muted">{formatDurationShort(row.durationMs)}</span>
            </li>
          ))}
        </ul>
      )}
      {rows.length > top.length ? (
        <p className="muted small">and {rows.length - top.length} more</p>
      ) : null}
    </div>
  );
}
