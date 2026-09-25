import { useState } from 'react';

import { EnrollmentDialog } from '../features/managed/EnrollmentDialog';
import { ErrorState, Loading } from '../components/States';
import { useTranslation } from '../i18n';
import { api } from '../services/api';
import { formatIsoDate, formatRelative } from '../services/format';
import {
  queryKeys,
  useAppMutation,
  useManagedStatus,
  useQueuedReports,
  useSentReports,
  useSettings,
} from '../hooks/useLocalTrack';
import type { SentReport } from '../types';

/**
 * What leaves this computer, and what has already left it.
 *
 * Employee mode is only defensible if the person being measured can read the
 * exact payloads, so this page shows them verbatim.
 */
export function SharedWithWorkPage() {
  const { t } = useTranslation();
  const managed = useManagedStatus();
  const settings = useSettings();
  const sent = useSentReports();
  const queued = useQueuedReports();
  const [enrolling, setEnrolling] = useState(false);
  const [open, setOpen] = useState<SentReport | null>(null);

  const sync = useAppMutation(
    api.syncNow,
    [queryKeys.managed, queryKeys.sentReports, queryKeys.queuedReports, ['sessions']],
    'toast.synchronized',
  );
  const updateSetting = useAppMutation(
    (payload: { key: string; value: unknown }) => api.updateSetting(payload.key, payload.value),
    [queryKeys.settings, queryKeys.managed],
  );
  const disconnect = useAppMutation(
    api.unenrollDevice,
    [queryKeys.managed, queryKeys.settings],
    'toast.deviceDisconnected',
  );

  if (managed.isLoading) return <Loading />;
  if (managed.error) {
    return <ErrorState error={managed.error} onRetry={() => void managed.refetch()} />;
  }
  const status = managed.data;
  if (!status) return null;

  if (!status.enrolled) {
    return (
      <div>
        <div className="page-header">
          <div>
            <h1>{t('shared.title')}</h1>
            <p>{t('shared.nothingShared')}</p>
          </div>
        </div>
        <section className="card">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('shared.notConnected')}</h2>
          <p className="muted small">
            {t('shared.personalMode')}
          </p>
          <button onClick={() => setEnrolling(true)}>{t('onboard.connectOrg')}</button>
        </section>
        {enrolling ? <EnrollmentDialog onClose={() => setEnrolling(false)} /> : null}
      </div>
    );
  }

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('shared.title')}</h1>
          <p>
            {t('shared.connectedTo', {
              organization: status.organization ?? t('managed.yourOrganization'),
            })}{' '}
            <span className="mono">{status.serverHost}</span>
          </p>
        </div>
        <div className="row">
          <button onClick={() => sync.mutate(undefined)} disabled={sync.isPending}>
            {sync.isPending ? t('shared.syncing') : t('shared.syncNow')}
          </button>
        </div>
      </div>

      {status.notice ? <div className="notice">{status.notice}</div> : null}
      {status.lastError ? (
        <div className="notice warn">
          <strong>{t('shared.lastAttemptFailed')}</strong> <span className="small">{status.lastError}</span>
        </div>
      ) : null}

      <div className="cards">
        <div className="card metric">
          <div className="label">{t('shared.summariesSent')}</div>
          <div className="value">{sent.data?.length ?? 0}</div>
          <div className="hint">Last {formatRelative(status.lastReportAtMs)}</div>
        </div>
        <div className="card metric">
          <div className="label">{t('shared.waitingToSend')}</div>
          <div className="value">{status.pendingReports}</div>
          <div className="hint">{t('shared.keptUntilAnswer')}</div>
        </div>
        <div className="card metric">
          <div className="label">{t('shared.settingsLocked')}</div>
          <div className="value">{status.lockedSettings.length}</div>
          <div className="hint">Policy revision {status.policyRevision ?? '—'}</div>
        </div>
      </div>

      <section className="card" style={{ marginBottom: 16 }}>
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('shared.whatIsShared')}</h2>
        <table>
          <tbody>
            <tr>
              <td>{t('shared.dailyTotals')}</td>
              <td className="muted small">
                {t('shared.dailyTotalsDetail')}
              </td>
              <td>
                <span className="badge ok">{t('shared.sent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.timePerApp')}</td>
              <td className="muted small">{t('shared.namesAndDurations')}</td>
              <td>
                <span className="badge ok">{t('shared.sent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.workingOn')}</td>
              <td className="muted small">
                {t('shared.workingOnDetail')}
              </td>
              <td>
                {settings.data?.settings.shareWorkspaceContext ? (
                  <span className="badge ok">{t('shared.sent')}</span>
                ) : (
                  <span className="badge">{t('shared.notSent')}</span>
                )}
              </td>
            </tr>
            <tr>
              <td>{t('shared.clockTimes')}</td>
              <td className="muted small">{t('shared.includingBreaks')}</td>
              <td>
                <span className="badge ok">{t('shared.sent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.whatYouType')}</td>
              <td className="muted small">
                {t('shared.timerDescription')}
              </td>
              <td>
                <span className="badge ok">{t('shared.sent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.addresses')}</td>
              <td className="muted small">{t('shared.stayHere')}</td>
              <td>
                <span className="badge">{t('shared.neverSent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.windowTitles')}</td>
              <td className="muted small">{t('shared.stayHere')}</td>
              <td>
                <span className="badge">{t('shared.neverSent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.notes')}</td>
              <td className="muted small">{t('shared.stayHere')}</td>
              <td>
                <span className="badge">{t('shared.neverSent')}</span>
              </td>
            </tr>
            <tr>
              <td>{t('shared.screenshots')}</td>
              <td className="muted small">{t('shared.neverRecordedAtAll')}</td>
              <td>
                <span className="badge">{t('shared.neverRecorded')}</span>
              </td>
            </tr>
          </tbody>
        </table>
        <label className="checkbox" style={{ marginTop: 12 }}>
          <input
            type="checkbox"
            checked={settings.data?.settings.shareWorkspaceContext ?? false}
            disabled={status.lockedSettings.includes('share_workspace_context')}
            onChange={(event) =>
              updateSetting.mutate({
                key: 'share_workspace_context',
                value: event.target.checked,
              })
            }
          />
          {t('shared.shareWorkspaces', {
            organization: status.organization ?? t('shared.myOrganization'),
          })}
        </label>
        {status.lockedSettings.includes('share_workspace_context') ? (
          <p className="muted small">
            {t('shared.lockedByOrg', {
              organization: status.organization ?? t('managed.yourOrganization'),
            })}
          </p>
        ) : (
          <p className="muted small">
            {t('shared.namesOnly')}
          </p>
        )}
      </section>

      <section className="card" style={{ marginBottom: 16 }}>
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('shared.alreadySent')}</h2>
        {sent.data && sent.data.length > 0 ? (
          <table>
            <thead>
              <tr>
                <th>{t('weekly.day')}</th>
                <th>{t('shared.contents')}</th>
                <th>{t('shared.sent')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {sent.data.map((report) => (
                <tr key={`${report.date}-${report.deliveredAtMs}`}>
                  <td>{formatIsoDate(report.date)}</td>
                  <td className="muted small">{report.description}</td>
                  <td className="muted small">{formatRelative(report.deliveredAtMs)}</td>
                  <td className="numeric">
                    <button className="ghost" onClick={() => setOpen(report)}>
                      {t('shared.readIt')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="muted small">{t('shared.nothingSentYet')}</p>
        )}
      </section>

      {queued.data && queued.data.length > 0 ? (
        <section className="card" style={{ marginBottom: 16 }}>
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('shared.waitingSection')}</h2>
          <table>
            <tbody>
              {queued.data.map((report) => (
                <tr key={report.date ?? report.description}>
                  <td>{formatIsoDate(report.date)}</td>
                  <td className="muted small">{report.description}</td>
                  <td className="numeric">
                    <button className="ghost" onClick={() => setOpen(report)}>
                      {t('shared.readIt')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}

      <section className="card">
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('shared.connection')}</h2>
        <table>
          <tbody>
            <tr>
              <td>{t('shared.organization')}</td>
              <td>{status.organization ?? '—'}</td>
            </tr>
            <tr>
              <td>{t('shared.server')}</td>
              <td className="mono">{status.serverHost}</td>
            </tr>
            <tr>
              <td>{t('shared.identifiedAs')}</td>
              <td>{status.employeeRef ?? 'this device'}</td>
            </tr>
            <tr>
              <td>{t('shared.policyChecked')}</td>
              <td>{formatRelative(status.lastPolicyAtMs)}</td>
            </tr>
          </tbody>
        </table>
        <div className="row" style={{ marginTop: 12 }}>
          {status.locked ? (
            <p className="muted small" style={{ margin: 0 }}>
              {t('shared.lockedConnection')}
            </p>
          ) : (
            <button className="danger" onClick={() => disconnect.mutate(undefined)}>
              {t('shared.disconnectFrom', {
                organization: status.organization ?? t('shared.theOrganization'),
              })}
            </button>
          )}
        </div>
      </section>

      {open ? (
        <div className="modal-backdrop" role="dialog" aria-modal="true">
          <div className="modal">
            <div className="row between">
              <h2 style={{ margin: 0, fontSize: 16 }}>{t('shared.sentOn', { date: formatIsoDate(open.date) })}</h2>
              <button className="ghost" onClick={() => setOpen(null)}>
                {t('common.close')}
              </button>
            </div>
            <p className="muted small">{t('shared.exactText')}</p>
            <pre className="mono payload">{prettyJson(open.payloadJson)}</pre>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function prettyJson(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}
