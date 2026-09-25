import { useState } from 'react';

import { formatClock, formatDurationShort } from '../services/format';
import { useActivityPage } from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import type { ActivityRow } from '../types';
import { ActivityDrawer } from './ActivityDrawer';
import { useTranslation } from '../i18n';
import { Pager } from './Pager';
import { EmptyState, ErrorState, Loading } from './States';

const PAGE_SIZE = 25;

/** The activity feed (spec §80) with cursor-free pagination (spec §121). */
export function ActivityFeed({
  fromMs,
  toMs,
  pageSize = PAGE_SIZE,
}: {
  fromMs: number;
  toMs: number;
  /** Today shows a shorter list: every row costs paint time on every scroll. */
  pageSize?: number;
}) {
  const { t } = useTranslation();
  const [offset, setOffset] = useState(0);
  const [selected, setSelected] = useState<ActivityRow | null>(null);
  const filter = useAppStore((state) => state.filter);

  const page = useActivityPage({ ...filter, fromMs, toMs }, pageSize, offset);

  if (page.isLoading) return <Loading />;
  if (page.error) return <ErrorState error={page.error} onRetry={() => void page.refetch()} />;
  if (!page.data || page.data.rows.length === 0) {
    return (
      <EmptyState
        title={t('states.noActivityTitle')}
        description={t('states.noActivityBody')}
      />
    );
  }

  const { rows, total } = page.data;

  const pager = (
    <Pager
      offset={offset}
      pageSize={pageSize}
      total={total}
      shown={rows.length}
      onChange={setOffset}
    />
  );

  return (
    <>
      <div className="card">
        {/* Controls at both ends: the list is long enough that a footer-only
            pager means scrolling past everything to change page. */}
        {total > pageSize ? pager : null}
        {rows.map((row) => (
          <div
            key={row.id}
            className="feed-item"
            role="button"
            tabIndex={0}
            onClick={() => setSelected(row)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') setSelected(row);
            }}
          >
            <div className="time">
              {formatClock(row.startedAtMs)}–{formatClock(row.endedAtMs)}
            </div>
            <div>
              <div className="title">{row.label}</div>
              <div className="subtitle">
                {row.pageTitle ?? row.windowTitle ?? ''}
                {row.projectName ? ` · ${row.projectName}` : ''}
                {row.categoryName ? ` · ${row.categoryName}` : ''}
              </div>
            </div>
            <div className="muted">{formatDurationShort(row.durationMs)}</div>
          </div>
        ))}
        {pager}
      </div>
      {selected ? <ActivityDrawer row={selected} onClose={() => setSelected(null)} /> : null}
    </>
  );
}
