import { useState } from 'react';

import { DateRangeBar } from '../components/DateRangeBar';
import { DateTimeField } from '../components/DateTimeField';
import { ErrorState, Loading } from '../components/States';
import { useTranslation } from '../i18n';
import { api } from '../services/api';
import {
  formatClock,
  formatDate,
  formatDateTimeInput,
  formatDurationHm,
  parseDateTimeInput,
} from '../services/format';
import { queryKeys, useAppMutation, useSessions } from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import type { SessionDetail } from '../types';

/** Sessions page with manual corrections (spec §114). */
export function SessionsPage() {
  const { t } = useTranslation();
  const range = useAppStore((state) => state.range);
  const sessions = useSessions(range.fromMs, range.toMs);
  const [editing, setEditing] = useState<SessionDetail | null>(null);
  const [adding, setAdding] = useState(false);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('sessions.title')}</h1>
          <p>{t('sessions.subtitle')}</p>
        </div>
        <div className="row">
          <DateRangeBar />
          <button onClick={() => setAdding(true)}>{t('sessions.addTime')}</button>
        </div>
      </div>

      {sessions.isLoading ? <Loading /> : null}
      {sessions.error ? (
        <ErrorState error={sessions.error} onRetry={() => void sessions.refetch()} />
      ) : null}

      {sessions.data ? (
        <section className="card">
          <table>
            <thead>
              <tr>
                <th>{t('sessions.date')}</th>
                <th>{t('sessions.clockIn')}</th>
                <th>{t('sessions.clockOut')}</th>
                <th className="numeric">{t('metric.clocked')}</th>
                <th className="numeric">{t('metric.break')}</th>
                <th className="numeric">{t('metric.work')}</th>
                <th className="numeric">{t('metric.active')}</th>
                <th className="numeric">{t('metric.idle')}</th>
                <th className="numeric">{t('metric.untracked')}</th>
                <th>{t('sessions.whatItWas')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {sessions.data.map((detail) => (
                <tr key={detail.session.id}>
                  <td>{formatDate(detail.session.startedAtMs)}</td>
                  <td>{formatClock(detail.session.startedAtMs)}</td>
                  <td>
                    {detail.session.endedAtMs ? (
                      formatClock(detail.session.endedAtMs)
                    ) : (
                      <span className="badge ok">{t('common.open')}</span>
                    )}
                  </td>
                  <td className="numeric">{formatDurationHm(detail.summary.clockedMs)}</td>
                  <td className="numeric">{formatDurationHm(detail.summary.breakMs)}</td>
                  <td className="numeric">{formatDurationHm(detail.summary.workMs)}</td>
                  <td className="numeric">{formatDurationHm(detail.summary.activeMs)}</td>
                  <td className="numeric">{formatDurationHm(detail.summary.idleMs)}</td>
                  <td className="numeric">{formatDurationHm(detail.summary.untrackedMs)}</td>
                  <td className="muted small">
                    {detail.session.note ?? ''}
                    {detail.session.editedManually ? ' (edited)' : ''}
                    {detail.session.createdManually ? ' (manual)' : ''}
                  </td>
                  <td>
                    <button className="ghost" onClick={() => setEditing(detail)}>
                      {t('common.edit')}
                    </button>
                  </td>
                </tr>
              ))}
              {sessions.data.length === 0 ? (
                <tr>
                  <td colSpan={11} className="muted">
                    {t('sessions.none')}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </section>
      ) : null}

      {editing ? <SessionEditor detail={editing} onClose={() => setEditing(null)} /> : null}
      {adding ? <ManualEntryDialog onClose={() => setAdding(false)} /> : null}
    </div>
  );
}

/**
 * Work done away from this computer: a meeting, a call, time on site.
 *
 * It is stored as a manually created session, so reports can always tell
 * observed time from time somebody typed in.
 */
function ManualEntryDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const now = Date.now();
  const [start, setStart] = useState(formatDateTimeInput(now - 3600000));
  const [end, setEnd] = useState(formatDateTimeInput(now));
  const [note, setNote] = useState('');
  const showToast = useAppStore((state) => state.showToast);

  const add = useAppMutation(
    (payload: { start: number; end: number; note: string | null }) =>
      api.addManualEntry(payload.start, payload.end, payload.note),
    [['sessions'], queryKeys.today, ['reports'], queryKeys.status],
    'toast.timeAdded',
  );

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal" style={{ width: 460 }}>
        <div className="row between">
          <h2 style={{ margin: 0, fontSize: 17 }}>{t('sessions.addTime')}</h2>
          <button className="ghost" onClick={onClose}>
            {t('common.close')}
          </button>
        </div>
        <p className="muted small">
          {t('sessions.addTimeHint')}
        </p>

        <div className="field">
          <label>{t('sessions.whatWasIt')}</label>
          <input
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder={t('sessions.awayPlaceholder')}
          />
        </div>
        <div className="row">
          <div className="field" style={{ flex: 1 }}>
            <label>{t('sessions.from')}</label>
            <DateTimeField value={start} onChange={setStart} />
          </div>
          <div className="field" style={{ flex: 1 }}>
            <label>{t('sessions.to')}</label>
            <DateTimeField value={end} onChange={setEnd} />
          </div>
        </div>

        <div className="row" style={{ justifyContent: 'flex-end', marginTop: 12 }}>
          <button onClick={onClose}>{t('common.cancel')}</button>
          <button
            className="primary"
            onClick={() => {
              const from = parseDateTimeInput(start);
              const to = parseDateTimeInput(end);
              if (from === null || to === null || to <= from) {
                showToast('error', t('sessions.needStartAndEnd'));
                return;
              }
              add.mutate({ start: from, end: to, note: note || null }, { onSuccess: onClose });
            }}
          >
            {t('sessions.addTime')}
          </button>
        </div>
      </div>
    </div>
  );
}

function SessionEditor({ detail, onClose }: { detail: SessionDetail; onClose: () => void }) {
  const { t } = useTranslation();
  const [start, setStart] = useState(formatDateTimeInput(detail.session.startedAtMs));
  const [end, setEnd] = useState(
    detail.session.endedAtMs ? formatDateTimeInput(detail.session.endedAtMs) : '',
  );
  const [note, setNote] = useState(detail.session.note ?? '');
  const [breakStart, setBreakStart] = useState('');
  const [breakEnd, setBreakEnd] = useState('');
  const showToast = useAppStore((state) => state.showToast);

  const invalidate = [
    queryKeys.sessions(detail.session.startedAtMs, detail.session.startedAtMs),
    ['sessions'],
    queryKeys.today,
    ['reports'],
  ];

  const save = useAppMutation(
    () =>
      api.updateSession(
        detail.session.id,
        parseDateTimeInput(start) ?? detail.session.startedAtMs,
        end ? parseDateTimeInput(end) : null,
        note || null,
      ),
    invalidate,
    'toast.sessionUpdated',
  );
  const addBreak = useAppMutation(
    () =>
      api.addBreak(
        detail.session.id,
        parseDateTimeInput(breakStart) ?? 0,
        breakEnd ? parseDateTimeInput(breakEnd) : null,
      ),
    invalidate,
    'toast.breakAdded',
  );
  const removeBreak = useAppMutation(
    (breakId: string) => api.deleteBreak(breakId),
    invalidate,
    'toast.breakRemoved',
  );
  const removeSession = useAppMutation(
    () => api.deleteSession(detail.session.id),
    invalidate,
    'toast.sessionDeleted',
  );

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal">
        <div className="row between">
          <h2 style={{ margin: 0, fontSize: 17 }}>{t('sessions.editSession')}</h2>
          <button className="ghost" onClick={onClose}>
            {t('common.close')}
          </button>
        </div>

        <div className="field">
          <label>{t('sessions.clockIn')}</label>
          <DateTimeField value={start} onChange={setStart} />
        </div>
        <div className="field">
          <label>{t('sessions.clockOutOpenHint')}</label>
          <DateTimeField value={end} onChange={setEnd} />
        </div>
        <div className="field">
          <label>{t('sessions.note')}</label>
          <input value={note} onChange={(e) => setNote(e.target.value)} />
        </div>

        <h3 style={{ fontSize: 14 }}>{t('sessions.breaks')}</h3>
        <table>
          <tbody>
            {detail.breaks.map((item) => (
              <tr key={item.id}>
                <td>{formatClock(item.startedAtMs)}</td>
                <td>{item.endedAtMs ? formatClock(item.endedAtMs) : 'open'}</td>
                <td className="numeric">
                  <button className="ghost" onClick={() => removeBreak.mutate(item.id)}>
                    {t('common.delete')}
                  </button>
                </td>
              </tr>
            ))}
            {detail.breaks.length === 0 ? (
              <tr>
                <td className="muted small">{t('sessions.noBreaks')}</td>
              </tr>
            ) : null}
          </tbody>
        </table>

        <div className="row" style={{ marginTop: 10 }}>
          <DateTimeField
            value={breakStart}
            onChange={setBreakStart}
            ariaLabel={t('sessions.breakStart')}
          />
          <DateTimeField
            value={breakEnd}
            onChange={setBreakEnd}
            ariaLabel={t('sessions.breakEnd')}
          />
          <button
            onClick={() => {
              if (!breakStart) {
                showToast('error', t('sessions.needBreakStart'));
                return;
              }
              addBreak.mutate(undefined);
            }}
          >
            {t('sessions.addBreak')}
          </button>
        </div>

        <div className="row between" style={{ marginTop: 20 }}>
          <button
            className="danger"
            onClick={() => {
              removeSession.mutate(undefined);
              onClose();
            }}
          >
            {t('sessions.deleteSession')}
          </button>
          <div className="row">
            <button onClick={onClose}>{t('common.cancel')}</button>
            <button
              className="primary"
              onClick={() => {
                save.mutate(undefined);
                onClose();
              }}
            >
              {t('sessions.saveChanges')}
            </button>
          </div>
        </div>
        <p className="muted small">
          {t('sessions.orderHint')}
        </p>
      </div>
    </div>
  );
}
