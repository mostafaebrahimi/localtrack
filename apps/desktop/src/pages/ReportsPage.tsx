import { useState } from 'react';

import { DailyBars } from '../components/Charts';
import { WeeklyReport } from '../components/WeeklyReport';
import { DateRangeBar } from '../components/DateRangeBar';
import { FilterBar } from '../components/FilterBar';
import { ReportTable } from '../components/ReportTable';
import { SummaryCards } from '../components/Metrics';
import { ErrorState, Loading } from '../components/States';
import { ExportDialog } from '../features/export/ExportDialog';
import {
  useComparison,
  usePageReport,
  useRangeQuery,
  useReports,
  useWeekly,
} from '../hooks/useLocalTrack';
import { formatDelta, formatDurationShort } from '../services/format';
import { useTranslation } from '../i18n';
import { useAppStore } from '../stores/useAppStore';

export function ReportsPage() {
  const { t } = useTranslation();
  const query = useRangeQuery();
  const reports = useReports(query);
  const weekly = useWeekly(query);
  const [compareOn, setCompareOn] = useState(false);
  const comparison = useComparison(query, compareOn);
  const selectedDomain = useAppStore((state) => state.selectedDomain);
  const selectDomain = useAppStore((state) => state.selectDomain);
  const pages = usePageReport(query, selectedDomain);
  const [exportOpen, setExportOpen] = useState(false);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('reports.title')}</h1>
          <p>{t('reports.subtitle')}</p>
        </div>
        <div className="row">
          <DateRangeBar />
          <button onClick={() => setCompareOn((value) => !value)}>
            {compareOn ? t('reports.hideComparison') : t('reports.compare')}
          </button>
          <button className="primary" onClick={() => setExportOpen(true)}>
            {t('common.export')}
          </button>
        </div>
      </div>

      <FilterBar />

      {reports.isLoading ? <Loading /> : null}
      {reports.error ? (
        <ErrorState error={reports.error} onRetry={() => void reports.refetch()} />
      ) : null}

      {reports.data ? (
        <>
          <SummaryCards summary={reports.data.summary} framed />

          {compareOn && comparison.data ? (
            <section className="card" style={{ marginBottom: 16 }}>
              <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('reports.versusPrevious')}</h2>
              <div className="row" style={{ gap: 24 }}>
                <Delta label={t('metric.clocked')} value={comparison.data.clockedDeltaMs} />
                <Delta label={t('metric.work')} value={comparison.data.workDeltaMs} />
                <Delta label={t('metric.active')} value={comparison.data.activeDeltaMs} />
                <Delta label={t('metric.idle')} value={comparison.data.idleDeltaMs} />
                <Delta label={t('metric.break')} value={comparison.data.breakDeltaMs} />
              </div>
            </section>
          ) : null}

          <div style={{ marginBottom: 16 }}>
            <WeeklyReport weeks={weekly.data ?? []} />
          </div>

          <section className="card" style={{ marginBottom: 16 }}>
            <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('reports.dailyBreakdown')}</h2>
            <DailyBars days={reports.data.daily} />
          </section>

          <div style={{ marginBottom: 16 }}>
            <ReportTable
              report={reports.data.workspaces}
              title={t('report.workingOn')}
              secondaryHeader={t('report.seenIn')}
              emptyLabel={t('report.nothingRecognisablePeriod')}
              topRows={20}
            />
          </div>

          <div className="grid-2" style={{ marginBottom: 16 }}>
            <ReportTable
              report={reports.data.applications}
              title={t('report.applications')}
              secondaryHeader={t('report.process')}
            />
            <ReportTable
              report={reports.data.websites}
              title={t('report.websites')}
              onSelect={(domain) => selectDomain(domain)}
            />
          </div>

          {selectedDomain ? (
            <section style={{ marginBottom: 16 }}>
              <div className="row between" style={{ marginBottom: 6 }}>
                <h2 style={{ fontSize: 15, margin: 0 }}>Pages on {selectedDomain}</h2>
                <button className="ghost" onClick={() => selectDomain(null)}>
                  {t('common.clear')}
                </button>
              </div>
              <ReportTable report={pages.data} title={selectedDomain} />
            </section>
          ) : (
            <div className="grid-2" style={{ marginBottom: 16 }}>
              <ReportTable
                report={reports.data.pages}
                title={t('reports.addresses')}
                secondaryHeader={t('report.page')}
              />
              <ReportTable report={reports.data.categories} title={t('report.categories')} />
            </div>
          )}

          <div className="grid-2">
            <ReportTable report={reports.data.projects} title={t('report.projects')} />
            <section className="card">
              <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('reports.focus')}</h2>
              <p className="muted small">
                Context switches and focus blocks are computed locally. LocalTrack deliberately
                does not score your productivity.
              </p>
              <table>
                <tbody>
                  <tr>
                    <td>{t('reports.contextSwitches')}</td>
                    <td className="numeric">{reports.data.summary.contextSwitches}</td>
                  </tr>
                  <tr>
                    <td>{t('reports.averageFocusBlock')}</td>
                    <td className="numeric">
                      {formatDurationShort(reports.data.summary.averageFocusMs)}
                    </td>
                  </tr>
                  <tr>
                    <td>{t('reports.medianFocusBlock')}</td>
                    <td className="numeric">
                      {formatDurationShort(reports.data.summary.medianFocusMs)}
                    </td>
                  </tr>
                  <tr>
                    <td>{t('reports.longestFocusBlock')}</td>
                    <td className="numeric">
                      {formatDurationShort(reports.data.summary.longestFocusMs)}
                    </td>
                  </tr>
                </tbody>
              </table>
            </section>
          </div>
        </>
      ) : null}

      {exportOpen ? (
        <ExportDialog fromMs={query.fromMs} toMs={query.toMs} onClose={() => setExportOpen(false)} />
      ) : null}
    </div>
  );
}

function Delta({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <div className="muted small">{label}</div>
      <div style={{ fontWeight: 600, color: value >= 0 ? 'var(--accent)' : 'var(--danger)' }}>
        {formatDelta(value)}
      </div>
    </div>
  );
}
