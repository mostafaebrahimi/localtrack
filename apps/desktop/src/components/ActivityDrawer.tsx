import { useState } from 'react';

import { useTranslation } from '../i18n';
import { api } from '../services/api';
import {
  formatClockSeconds,
  formatDateTimeInput,
  formatDurationShort,
  parseDateTimeInput,
} from '../services/format';
import {
  queryKeys,
  useAppMutation,
  useCategories,
  useProjects,
} from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import type { ActivityRow } from '../types';

/** Activity details drawer (spec §74). */
export function ActivityDrawer({ row, onClose }: { row: ActivityRow; onClose: () => void }) {
  const { t } = useTranslation();
  const categories = useCategories();
  const projects = useProjects();
  const showToast = useAppStore((state) => state.showToast);
  const [splitAt, setSplitAt] = useState(() =>
    formatDateTimeInput(row.startedAtMs + Math.floor(row.durationMs / 2)),
  );
  const [showUrl, setShowUrl] = useState(false);
  const [note, setNote] = useState(() => {
    try {
      const parsed = row.metadataJson ? (JSON.parse(row.metadataJson) as { note?: string }) : null;
      return parsed?.note ?? '';
    } catch {
      return '';
    }
  });

  const invalidate = [queryKeys.today, ['activity'], ['timeline'], ['reports']];

  const classify = useAppMutation(
    (payload: { categoryId: string | null; projectId: string | null }) =>
      api.classifySegment(row.id, payload.categoryId, payload.projectId),
    invalidate,
    'toast.activityUpdated',
  );
  const split = useAppMutation(
    (atMs: number) => api.splitSegment(row.id, atMs),
    invalidate,
    'toast.activitySplit',
  );
  const annotate = useAppMutation(
    (value: string) => api.annotateSegment(row.id, value.trim() ? value.trim() : null),
    invalidate,
    'toast.noteSaved',
  );
  const remove = useAppMutation(
    () => api.deleteSegment(row.id),
    invalidate,
    'toast.activityDeleted',
  );

  return (
    <aside className="drawer" aria-label={t('drawer.title')}>
      <div className="row between">
        <h2 style={{ fontSize: 16, margin: 0 }}>{row.label}</h2>
        <button className="ghost" onClick={onClose}>
          {t('common.close')}
        </button>
      </div>

      <dl>
        <Detail label={t('drawer.start')} value={formatClockSeconds(row.startedAtMs)} />
        <Detail label={t('drawer.end')} value={formatClockSeconds(row.endedAtMs)} />
        <Detail label={t('drawer.duration')} value={formatDurationShort(row.durationMs)} />
        <Detail label={t('drawer.source')} value={row.source} />
        <Detail label={t('drawer.kind')} value={row.kind} />
        {row.appName ? <Detail label={t('filter.application')} value={row.appName} /> : null}
        {row.processName ? <Detail label={t('report.process')} value={row.processName} /> : null}
        {row.windowTitle ? <Detail label={t('field.window_title')} value={row.windowTitle} /> : null}
        {row.domain ? <Detail label={t('drawer.website')} value={row.domain} /> : null}
        {row.pageTitle ? <Detail label={t('report.page')} value={row.pageTitle} /> : null}
        <Detail
          label={t('drawer.classification')}
          value={row.classificationSource ?? 'DEFAULT'}
        />
      </dl>

      {row.url ? (
        <div className="field">
          <label>{t('drawer.url')}</label>
          {showUrl ? (
            <div className="mono">{row.url}</div>
          ) : (
            <button className="ghost" onClick={() => setShowUrl(true)}>
              Show stored URL
            </button>
          )}
        </div>
      ) : null}

      <div className="field">
        <label>{t('filter.category')}</label>
        <select
          value={row.categoryId ?? ''}
          onChange={(event) =>
            classify.mutate({
              categoryId: event.target.value || null,
              projectId: row.projectId,
            })
          }
        >
          <option value="">{t('drawer.uncategorized')}</option>
          {categories.data?.map((category) => (
            <option key={category.id} value={category.id}>
              {category.name}
            </option>
          ))}
        </select>
      </div>

      <div className="field">
        <label>{t('filter.project')}</label>
        <select
          value={row.projectId ?? ''}
          onChange={(event) =>
            classify.mutate({
              categoryId: row.categoryId,
              projectId: event.target.value || null,
            })
          }
        >
          <option value="">{t('drawer.unassigned')}</option>
          {projects.data
            ?.filter((project) => !project.archived)
            .map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
        </select>
      </div>

      <div className="field">
        <label>{t('sessions.note')}</label>
        <div className="row">
          <input
            value={note}
            placeholder={t('drawer.notePlaceholder')}
            onChange={(event) => setNote(event.target.value)}
            style={{ flex: 1 }}
          />
          <button onClick={() => annotate.mutate(note)}>{t('common.save')}</button>
        </div>
      </div>

      <div className="field">
        <label>{t('drawer.splitAt')}</label>
        <div className="row">
          <input
            type="datetime-local"
            value={splitAt}
            onChange={(event) => setSplitAt(event.target.value)}
          />
          <button
            onClick={() => {
              const at = parseDateTimeInput(splitAt);
              if (at === null || at <= row.startedAtMs || at >= row.endedAtMs) {
                showToast('error', t('drawer.needMomentInside'));
                return;
              }
              split.mutate(at);
            }}
          >
            {t('common.split')}
          </button>
        </div>
      </div>

      <div className="row" style={{ marginTop: 16 }}>
        <button
          className="danger"
          onClick={() => {
            remove.mutate(undefined);
            onClose();
          }}
        >
          {t('drawer.deleteActivity')}
        </button>
      </div>
      <p className="muted small">
        {t('drawer.manualKept')}
      </p>
    </aside>
  );
}

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="row between" style={{ borderBottom: '1px solid var(--border)', padding: '5px 0' }}>
      <dt className="muted small">{label}</dt>
      <dd style={{ margin: 0, textAlign: 'right', maxWidth: '70%' }}>{value}</dd>
    </div>
  );
}
