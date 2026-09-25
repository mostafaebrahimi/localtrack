import { Link } from 'react-router-dom';

import { ActivityFeed } from '../components/ActivityFeed';
import { CategoryDonut } from '../components/Charts';
import { ReportTable } from '../components/ReportTable';
import { StatusCard, StatusNotices } from '../components/StatusCard';
import { SummaryCards } from '../components/Metrics';
import { TimelineStrip } from '../components/Timeline';
import { EmptyState, ErrorState, Loading } from '../components/States';
import { useTranslation } from '../i18n';
import { useStatusSelector, useToday } from '../hooks/useLocalTrack';
import { formatDurationShort } from '../services/format';

export function TodayPage() {
  const today = useToday();
  const { t } = useTranslation();
  // Only this one flag, so a status update every few seconds does not redraw
  // the timeline and the report tables.
  const chromeConnected = useStatusSelector((status) => status.chromeConnected);

  const dayStart = new Date();
  dayStart.setHours(0, 0, 0, 0);
  const fromMs = dayStart.getTime();
  const toMs = fromMs + 86400000;

  // A whole flat day squeezes a morning of work into a few pixels. Frame the
  // strip on the recorded activity instead, rounded out to whole hours.
  const blocks = today.data?.timeline ?? [];
  // Untracked stretches (the machine was off, or nothing was clocked in) would
  // otherwise squeeze the actual work into a few pixels.
  const recorded = blocks.filter((block) => block.blockKind !== 'UNTRACKED');
  const framing = recorded.length > 0 ? recorded : blocks;
  const firstBlock = framing.length > 0 ? Math.min(...framing.map((b) => b.startMs)) : Date.now();
  const lastBlock = framing.length > 0 ? Math.max(...framing.map((b) => b.endMs)) : Date.now();
  const floorHour = (ms: number) => new Date(new Date(ms).setMinutes(0, 0, 0)).getTime();
  const stripFrom = Math.max(fromMs, floorHour(firstBlock));
  const stripTo = Math.min(toMs, floorHour(Math.max(lastBlock, Date.now())) + 3_600_000);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('today.title')}</h1>
          <p>{t('today.subtitle')}</p>
        </div>
      </div>

      <StatusNotices />

      {chromeConnected.data === false ? (
        <div className="notice">
          <strong>{t('today.chromeNoticeTitle')}</strong>
          <p className="small muted">
            {t('today.chromeNoticeBody', { link: '\u0000' })
              .split('\u0000')
              .flatMap((part, index) =>
                index === 0
                  ? [part]
                  : [
                      <Link key="link" to="/settings">
                        {t('today.chromeNoticeLink')}
                      </Link>,
                      part,
                    ],
              )}
          </p>
        </div>
      ) : null}

      {today.isLoading ? <Loading /> : null}
      {today.error ? <ErrorState error={today.error} onRetry={() => void today.refetch()} /> : null}

      {today.data ? (
        <>
          {/* The clock, the day drawn to scale, and the six readings behind
              it: one instrument, read top to bottom. */}
          <section className="day-panel">
            <StatusCard />
            <div className="day-band">
              <TimelineStrip blocks={today.data.timeline} fromMs={stripFrom} toMs={stripTo} />
              <div className="row between band-footnote">
                <span className="muted small">
                  {t('band.contextSwitches', { count: today.data.summary.contextSwitches })}
                  {' · '}
                  {t('band.averageFocus', {
                    duration: formatDurationShort(today.data.summary.averageFocusMs),
                  })}
                  {' · '}
                  {t('band.longestFocus', {
                    duration: formatDurationShort(today.data.summary.longestFocusMs),
                  })}
                </span>
              </div>
            </div>
            <SummaryCards summary={today.data.summary} />
          </section>

          {/* The headline answer to "what did I do today", so it gets the
              full width: workspace names are long and worth reading. */}
          <div style={{ marginBottom: 16 }}>
            <ReportTable
              report={today.data.workspaces}
              title={t('report.workingOn')}
              secondaryHeader={t('report.seenIn')}
              emptyLabel={t('report.nothingRecognisable')}
            />
          </div>

          <div className="grid-2" style={{ marginBottom: 16 }}>
            <ReportTable report={today.data.applications} title={t('report.applications')} />
            <ReportTable report={today.data.websites} title={t('report.websites')} />
          </div>

          <div className="grid-2" style={{ marginBottom: 16 }}>
            <section className="card">
              <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('report.categories')}</h2>
              <CategoryDonut rows={today.data.categories.rows} />
            </section>
            <ReportTable report={today.data.projects} title={t('report.projects')} />
          </div>

          <h2 style={{ fontSize: 15 }}>{t('report.activity')}</h2>
          {today.data.summary.clockedMs === 0 && today.data.timeline.length === 0 ? (
            <EmptyState
              title={t('states.noActivityTitle')}
              description={t('states.noActivityBody')}
            />
          ) : (
            <ActivityFeed fromMs={fromMs} toMs={toMs} pageSize={10} />
          )}
        </>
      ) : null}
    </div>
  );
}
