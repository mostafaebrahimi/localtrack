import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';

import { useTranslation, type MessageKey } from '../i18n';
import { api } from '../services/api';
import { formatClock, formatDurationHm } from '../services/format';
import { queryKeys, useAppMutation, useStatus } from '../hooks/useLocalTrack';
import type { CurrentStatus } from '../types';

/** How often the session length on the card is refreshed. */
const TICK_MS = 20_000;

/**
 * The session length, counted in the page.
 *
 * It shows hours and minutes rather than seconds, and moves every twenty
 * seconds. A per-second clock forced the whole dashboard to repaint once a
 * second — around 8% of a CPU core, all day. The running seconds live in the
 * timer bar and the tray badge, which are small enough to repaint cheaply.
 */
function Elapsed({ sinceMs }: { sinceMs: number }) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(id);
  }, []);

  return (
    <span className="clock display-time">{formatDurationHm(Math.max(0, now - sinceMs))}</span>
  );
}

function stateLabel(status: CurrentStatus): { key: MessageKey; dot: string } {
  switch (status.state) {
    case 'CLOCKED_IN':
      return { key: 'clock.clockedIn', dot: 'on' };
    case 'ON_BREAK':
      return { key: 'clock.onBreak', dot: 'break' };
    default:
      return { key: 'clock.clockedOut', dot: 'off' };
  }
}

/**
 * The warnings that belong to the clock but not to the instrument: they are
 * about something being wrong, so they sit above the panel where they cannot
 * be mistaken for a reading.
 */
export function StatusNotices() {
  const status = useStatus();
  const { t } = useTranslation();
  const client = useQueryClient();
  const refresh = () => {
    void client.invalidateQueries();
  };
  const clockOut = useAppMutation(api.clockOut, [queryKeys.status, queryKeys.today], 'toast.clockedOut');

  if (!status.data) return null;
  const current = status.data;

  return (
    <>
      {current.staleSessionWarning ? (
        <div className="notice warn">
          <strong>{t('notice.staleTitle')}</strong>
          <p className="small muted">
            {t('notice.staleBody', {
              time: formatClock(current.session?.startedAtMs ?? Date.now()),
            })}
          </p>
          <div className="row" style={{ marginTop: 8 }}>
            <button onClick={refresh}>{t('notice.continueSession')}</button>
            <button onClick={() => clockOut.mutate(undefined)}>{t('notice.clockOutNow')}</button>
          </div>
        </div>
      ) : null}

      {current.tracking === 'PAUSED' ? (
        <div className="notice warn">
          <strong>{t('notice.pausedTitle')}</strong> {t('notice.pausedBody')}
        </div>
      ) : null}
    </>
  );
}

/**
 * The head of the day panel: what the clock is doing, for how long, and the
 * controls that change it.
 */
export function StatusCard() {
  const status = useStatus();
  const { t } = useTranslation();

  const clockIn = useAppMutation(
    (note: string | null) => api.clockIn(note),
    [queryKeys.status, queryKeys.today],
    'toast.clockedIn',
  );
  const clockOut = useAppMutation(api.clockOut, [queryKeys.status, queryKeys.today], 'toast.clockedOut');
  const startBreak = useAppMutation(api.startBreak, [queryKeys.status], 'toast.breakStarted');
  const endBreak = useAppMutation(api.endBreak, [queryKeys.status], 'toast.backToWork');
  const pause = useAppMutation(
    (paused: boolean) => api.setTrackingPaused(paused),
    [queryKeys.status, queryKeys.settings],
  );
  const saveNote = useAppMutation(
    (payload: { sessionId: string; note: string | null }) =>
      api.setSessionNote(payload.sessionId, payload.note),
    [queryKeys.status, ['sessions']],
    'toast.saved',
  );

  const [task, setTask] = useState('');
  const sessionId = status.data?.session?.id ?? null;
  const sessionNote = status.data?.session?.note ?? '';

  // Follow the session's description, but never overwrite what is being typed.
  useEffect(() => {
    setTask(sessionNote);
  }, [sessionId, sessionNote]);

  if (!status.data) return null;
  const current = status.data;
  const { key: stateKey, dot } = stateLabel(current);
  const paused = current.tracking === 'PAUSED';

  return (
    <div className="day-head">
      <div>
        <div className="state-line">
          <span className={`dot ${dot}`} />
          {t(stateKey)}
          {current.session ? (
            <span className="muted">
              {' · '}
              {t('clock.since', { time: formatClock(current.session.startedAtMs) })}
            </span>
          ) : null}
        </div>

        {current.state !== 'CLOCKED_OUT' && current.session ? (
          <Elapsed sinceMs={current.session.startedAtMs} />
        ) : (
          <div className="muted small" style={{ marginTop: 6 }}>
            {t('clock.invite')}
          </div>
        )}

        {current.currentActivity ? (
          <div className="current-activity">
            <div>
              <strong>{current.currentActivity.label}</strong>
            </div>
            <div className="muted small">
              {current.currentActivity.title ?? ''}
              {current.currentActivity.projectName
                ? ` · ${current.currentActivity.projectName}`
                : ''}
            </div>
          </div>
        ) : null}
      </div>

      <div className="row task-row">
        <input
          className="task-input"
          placeholder={t('clock.taskPlaceholder')}
          value={task}
          onChange={(event) => setTask(event.target.value)}
          onBlur={() => {
            if (sessionId && task !== sessionNote) {
              saveNote.mutate({ sessionId, note: task || null });
            }
          }}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return;
            if (current.state === 'CLOCKED_OUT') {
              clockIn.mutate(task || null);
            } else if (sessionId) {
              saveNote.mutate({ sessionId, note: task || null });
            }
          }}
        />
        {current.state === 'CLOCKED_OUT' ? (
          <button className="primary" onClick={() => clockIn.mutate(task || null)}>
            {t('clock.clockIn')}
          </button>
        ) : (
          <>
            {current.state === 'ON_BREAK' ? (
              <button className="primary" onClick={() => endBreak.mutate(undefined)}>
                {t('clock.resumeWork')}
              </button>
            ) : (
              <button onClick={() => startBreak.mutate(undefined)}>{t('clock.takeBreak')}</button>
            )}
            <button onClick={() => clockOut.mutate(undefined)}>{t('clock.clockOut')}</button>
          </>
        )}
        <button className="ghost" onClick={() => pause.mutate(!paused)}>
          {paused ? t('clock.resumeTracking') : t('clock.pauseTracking')}
        </button>
      </div>
    </div>
  );
}
