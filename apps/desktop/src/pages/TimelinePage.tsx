import { useState } from 'react';

import { DateRangeBar } from '../components/DateRangeBar';
import { FilterBar } from '../components/FilterBar';
import { ErrorState, Loading } from '../components/States';
import { TimelineList, TimelineStrip } from '../components/Timeline';
import { useTranslation, type MessageKey } from '../i18n';
import { useRangeQuery, useTimeline } from '../hooks/useLocalTrack';
import { formatClock } from '../services/format';
import type { TimelineBlock } from '../types';

const ZOOMS = [
  { id: 'day', label: 'timeline.day', hours: 24 },
  { id: '6h', label: 'timeline.sixHours', hours: 6 },
  { id: '3h', label: 'timeline.threeHours', hours: 3 },
  { id: '1h', label: 'timeline.oneHour', hours: 1 },
] as const satisfies readonly { id: string; label: MessageKey; hours: number }[];

export function TimelinePage() {
  const { t } = useTranslation();
  const query = useRangeQuery();
  const timeline = useTimeline(query);
  const [zoom, setZoom] = useState<(typeof ZOOMS)[number]['id']>('day');
  const [offsetHours, setOffsetHours] = useState(0);
  const [selected, setSelected] = useState<TimelineBlock | null>(null);

  const zoomHours = ZOOMS.find((option) => option.id === zoom)?.hours ?? 24;
  const windowMs = zoomHours * 3600000;
  const fromMs = query.fromMs + offsetHours * 3600000;
  const toMs = Math.min(fromMs + windowMs, query.toMs);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('timeline.title')}</h1>
          <p>{t('timeline.subtitle')}</p>
        </div>
        <DateRangeBar />
      </div>

      <FilterBar />

      <section className="card" style={{ marginBottom: 16 }}>
        <div className="row between" style={{ marginBottom: 10 }}>
          <div className="row">
            {ZOOMS.map((option) => (
              <button
                key={option.id}
                className={zoom === option.id ? 'primary' : ''}
                onClick={() => {
                  setZoom(option.id);
                  setOffsetHours(0);
                }}
              >
                {t(option.label)}
              </button>
            ))}
          </div>
          <div className="row">
            <button onClick={() => setOffsetHours((value) => Math.max(0, value - zoomHours))}>
              {t('timeline.earlier')}
            </button>
            <span className="muted small">
              {formatClock(fromMs)} – {formatClock(toMs)}
            </span>
            <button
              onClick={() =>
                setOffsetHours((value) =>
                  query.fromMs + (value + zoomHours) * 3600000 < query.toMs
                    ? value + zoomHours
                    : value,
                )
              }
            >
              {t('timeline.later')}
            </button>
          </div>
        </div>

        {timeline.isLoading ? <Loading /> : null}
        {timeline.error ? (
          <ErrorState error={timeline.error} onRetry={() => void timeline.refetch()} />
        ) : null}
        {timeline.data ? (
          <TimelineStrip
            blocks={timeline.data}
            fromMs={fromMs}
            toMs={toMs}
            onSelect={setSelected}
          />
        ) : null}
      </section>

      <section className="card">
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('timeline.blocks')}</h2>
        {timeline.data ? (
          <TimelineList
            // Moving the window or changing the zoom starts the list again.
            key={`${fromMs}-${toMs}`}
            blocks={timeline.data.filter((block) => block.endMs > fromMs && block.startMs < toMs)}
            onSelect={setSelected}
          />
        ) : null}
      </section>

      {selected ? (
        <aside className="drawer">
          <div className="row between">
            <h2 style={{ fontSize: 16, margin: 0 }}>{selected.label}</h2>
            <button className="ghost" onClick={() => setSelected(null)}>
              {t('common.close')}
            </button>
          </div>
          <p className="muted small">
            {formatClock(selected.startMs)}–{formatClock(selected.endMs)} ·{' '}
            {t(`block.${selected.blockKind.toLowerCase()}` as MessageKey)}
          </p>
          {selected.windowTitle ? <p>{selected.windowTitle}</p> : null}
          {selected.pageTitle ? <p>{selected.pageTitle}</p> : null}
          {selected.url ? <p className="mono">{selected.url}</p> : null}
          <p className="muted small">
            {t('drawer.changeElsewhere')}
          </p>
        </aside>
      ) : null}
    </div>
  );
}
