import { useState } from 'react';

import { ErrorState, Loading } from '../components/States';
import { useTranslation, type MessageKey } from '../i18n';
import { api } from '../services/api';
import { formatBytes } from '../services/format';
import {
  queryKeys,
  useAppMutation,
  useExclusions,
  useManagedStatus,
  useSettings,
} from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import type { ExclusionAction, ExclusionRule, ExclusionTarget, UrlPolicy } from '../types';

const TARGETS: ExclusionTarget[] = ['DOMAIN', 'URL', 'APP', 'PROCESS', 'TITLE'];
const ACTIONS: ExclusionAction[] = ['IGNORE', 'DURATION_ONLY', 'REDACT'];

export function PrivacyPage() {
  const { t } = useTranslation();
  const settings = useSettings();
  const exclusions = useExclusions();
  const managed = useManagedStatus();
  const lockedBy = managed.data?.organization ?? 'your organization';
  const isLocked = (key: string) => managed.data?.lockedSettings.includes(key) ?? false;
  const showToast = useAppStore((state) => state.showToast);
  const [pattern, setPattern] = useState('');
  const [target, setTarget] = useState<ExclusionTarget>('DOMAIN');
  const [action, setAction] = useState<ExclusionAction>('IGNORE');
  const [customRetention, setCustomRetention] = useState('');

  const invalidate = [queryKeys.settings, queryKeys.exclusions, queryKeys.today, ['reports']];

  const updateSetting = useAppMutation(
    (payload: { key: string; value: unknown }) => api.updateSetting(payload.key, payload.value),
    invalidate,
  );
  const saveExclusion = useAppMutation(
    (rule: ExclusionRule) => api.saveExclusion(rule),
    invalidate,
    'toast.exclusionSaved',
  );
  const deleteExclusion = useAppMutation(
    (id: string) => api.deleteExclusion(id),
    invalidate,
    'toast.exclusionRemoved',
  );
  const [pending, setPending] = useState<{
    fromMs: number;
    toMs: number;
    label: string;
    sessions: number;
    segments: number;
  } | null>(null);

  const deleteRange = useAppMutation(
    (payload: { fromMs: number; toMs: number; includeSessions: boolean }) =>
      api.deleteRange(payload.fromMs, payload.toMs, payload.includeSessions),
    invalidate,
    'toast.dataDeleted',
  );

  /**
   * Ask before touching clock records: deleting activity and deleting the
   * session it belongs to are different decisions (spec §111).
   */
  const requestDeletion = async (fromMs: number, toMs: number, label: string) => {
    const preview = await api.previewDeletion(fromMs, toMs);
    if (preview.sessions === 0) {
      deleteRange.mutate({ fromMs, toMs, includeSessions: false });
      showToast('success', t('privacy.deletedCount', { count: preview.segments }));
      return;
    }
    setPending({ fromMs, toMs, label, ...preview });
  };
  const deleteEverything = useAppMutation(
    (includeSessions: boolean) => api.deleteAllActivity(includeSessions),
    invalidate,
  );

  if (settings.isLoading) return <Loading />;
  if (settings.error) {
    return <ErrorState error={settings.error} onRetry={() => void settings.refetch()} />;
  }
  if (!settings.data) return null;

  const current = settings.data.settings;

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('privacy.title')}</h1>
          <p>
            {managed.data?.enrolled
              ? t('privacy.introManaged', { organization: lockedBy })
              : t('privacy.introPersonal')}
          </p>
        </div>
      </div>

      <section className="card" style={{ marginBottom: 16 }}>
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('privacy.whatIsStored')}</h2>
        <table>
          <tbody>
            <tr>
              <td>{t('privacy.dataLocation')}</td>
              <td className="mono path" title={settings.data.dataDirectory}>
                {settings.data.dataDirectory}
              </td>
            </tr>
            <tr>
              <td>{t('privacy.databaseSize')}</td>
              <td>{formatBytes(settings.data.databaseSizeBytes)}</td>
            </tr>
            <tr>
              <td>{t('privacy.trackingScope')}</td>
              <td>
                <select
                  value={current.trackingScope}
                  disabled={isLocked('tracking_scope')}
                  onChange={(event) =>
                    updateSetting.mutate({ key: 'tracking_scope', value: event.target.value })
                  }
                >
                  <option value="WORK_SESSIONS_ONLY">{t('privacy.onlyClockedIn')}</option>
                  <option value="ALWAYS">{t('privacy.always')}</option>
                </select>
                {isLocked('tracking_scope') ? <PolicyLock by={lockedBy} /> : null}
              </td>
            </tr>
            <tr>
              <td>{t('privacy.urlStorage')}</td>
              <td>
                <select
                  value={current.urlPolicy}
                  disabled={isLocked('url_policy')}
                  onChange={(event) => {
                    const value = event.target.value as UrlPolicy;
                    if (value === 'FULL_URL') {
                      showToast(
                        'info',
                        'Full URLs can contain tokens and identifiers. They stay on this computer, but consider domain + path instead.',
                      );
                    }
                    updateSetting.mutate({ key: 'url_policy', value });
                  }}
                >
                  <option value="DOMAIN_ONLY">{t('privacy.domainOnly')}</option>
                  <option value="PATH_WITHOUT_QUERY">{t('privacy.domainPath')}</option>
                  <option value="FULL_URL">{t('privacy.fullUrl')}</option>
                </select>
                {isLocked('url_policy') ? <PolicyLock by={lockedBy} /> : null}
              </td>
            </tr>
            <tr>
              <td>{t('privacy.incognito')}</td>
              <td>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={current.trackIncognito}
                    disabled={isLocked('track_incognito')}
                    onChange={(event) =>
                      updateSetting.mutate({
                        key: 'track_incognito',
                        value: event.target.checked,
                      })
                    }
                  />
                  {t('privacy.incognitoLabel')}
                </label>
              </td>
            </tr>
            <tr>
              <td>{t('privacy.detailedInteractions')}</td>
              <td>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={current.detailedInteractions}
                    disabled={isLocked('detailed_interactions')}
                    onChange={(event) =>
                      updateSetting.mutate({
                        key: 'detailed_interactions',
                        value: event.target.checked,
                      })
                    }
                  />
                  {t('privacy.interactionsLabel')}
                </label>
              </td>
            </tr>
            <tr>
              <td>{t('privacy.excludedActivity')}</td>
              <td>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={current.recordExcludedDuration}
                    onChange={(event) =>
                      updateSetting.mutate({
                        key: 'record_excluded_duration',
                        value: event.target.checked,
                      })
                    }
                  />
                  {t('privacy.excludedDurationLabel')}
                </label>
              </td>
            </tr>
            <tr>
              <td>{t('privacy.retention')}</td>
              <td>
                <select
                  value={String(current.retentionDays)}
                  disabled={isLocked('retention_days')}
                  onChange={(event) =>
                    updateSetting.mutate({
                      key: 'retention_days',
                      value: Number(event.target.value),
                    })
                  }
                >
                  <option value="0">{t('privacy.keepForever')}</option>
                  <option value="30">{t('privacy.days', { count: 30 })}</option>
                  <option value="90">{t('privacy.days', { count: 90 })}</option>
                  <option value="180">{t('privacy.days', { count: 180 })}</option>
                  <option value="365">{t('privacy.days', { count: 365 })}</option>
                  {![0, 30, 90, 180, 365].includes(current.retentionDays) ? (
                    <option value={String(current.retentionDays)}>
                      {t('privacy.daysCustom', { count: current.retentionDays })}
                    </option>
                  ) : null}
                </select>
                <div className="row" style={{ marginTop: 6 }}>
                  <input
                    type="number"
                    min={1}
                    style={{ width: 90 }}
                    placeholder={t('privacy.customPlaceholder')}
                    value={customRetention}
                    onChange={(event) => setCustomRetention(event.target.value)}
                    aria-label={t('privacy.customRetention')}
                  />
                  <span className="muted small">{t('privacy.daysUnit')}</span>
                  <button
                    onClick={() => {
                      const days = Number(customRetention);
                      if (Number.isFinite(days) && days >= 1) {
                        updateSetting.mutate({ key: 'retention_days', value: Math.floor(days) });
                        setCustomRetention('');
                      } else {
                        showToast('error', t('privacy.retentionAtLeastDay'));
                      }
                    }}
                  >
                    {t('privacy.set')}
                  </button>
                </div>
                <label className="checkbox" style={{ marginTop: 6 }}>
                  <input
                    type="checkbox"
                    checked={current.retentionDeleteSessions}
                    onChange={(event) =>
                      updateSetting.mutate({
                        key: 'retention_delete_sessions',
                        value: event.target.checked,
                      })
                    }
                  />
                  {t('privacy.retentionSessionsLabel')}
                </label>
              </td>
            </tr>
          </tbody>
        </table>
      </section>

      <section className="card" style={{ marginBottom: 16 }}>
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('privacy.excludedTitle')}</h2>
        <p className="muted small">
          {t('privacy.exclusionHelp', {
            star: '*',
            one: 'bank.example.com/*',
            two: '1Password*',
          })}
        </p>
        <table>
          <thead>
            <tr>
              <th>{t('privacy.target')}</th>
              <th>{t('organize.pattern')}</th>
              <th>{t('privacy.action')}</th>
              <th>{t('privacy.enabled')}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {exclusions.data?.map((rule) => (
              <tr key={rule.id}>
                <td>{t(`target.${rule.target}` as MessageKey)}</td>
                <td className="mono">{rule.pattern}</td>
                <td>{t(`action.${rule.action}` as MessageKey)}</td>
                <td>
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={(event) =>
                      saveExclusion.mutate({ ...rule, enabled: event.target.checked })
                    }
                  />
                </td>
                <td className="numeric">
                  <button className="ghost" onClick={() => deleteExclusion.mutate(rule.id)}>
                    {t('common.delete')}
                  </button>
                </td>
              </tr>
            ))}
            {exclusions.data?.length === 0 ? (
              <tr>
                <td colSpan={5} className="muted small">
                  {t('privacy.nothingExcluded')}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>

        <div className="row" style={{ marginTop: 10 }}>
          <select value={target} onChange={(e) => setTarget(e.target.value as ExclusionTarget)}>
            {TARGETS.map((item) => (
              <option key={item} value={item}>
                {t(`target.${item}` as MessageKey)}
              </option>
            ))}
          </select>
          <input
            placeholder={t('privacy.patternPlaceholder')}
            value={pattern}
            onChange={(event) => setPattern(event.target.value)}
          />
          <select value={action} onChange={(e) => setAction(e.target.value as ExclusionAction)}>
            {ACTIONS.map((item) => (
              <option key={item} value={item}>
                {t(`action.${item}` as MessageKey)}
              </option>
            ))}
          </select>
          <button
            onClick={() => {
              if (!pattern.trim()) return;
              saveExclusion.mutate({
                id: '',
                enabled: true,
                target,
                pattern: pattern.trim(),
                action,
                createdAtMs: 0,
                updatedAtMs: 0,
              });
              setPattern('');
            }}
          >
            {t('privacy.addExclusion')}
          </button>
        </div>
      </section>

      <section className="card">
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('privacy.deleteData')}</h2>
        <p className="muted small">{t('privacy.deleteWarning')}</p>
        <div className="row">
          <button
            onClick={() => void requestDeletion(Date.now() - 5 * 60000, Date.now(), t('privacy.rangeLast5'))}
          >
            {t('privacy.deleteLast5')}
          </button>
          <button
            onClick={() =>
              void requestDeletion(Date.now() - 15 * 60000, Date.now(), t('privacy.rangeLast15'))
            }
          >
            {t('privacy.deleteLast15')}
          </button>
          <button
            onClick={() => {
              const start = new Date();
              start.setHours(0, 0, 0, 0);
              void requestDeletion(start.getTime(), Date.now(), t('privacy.rangeToday'));
            }}
          >
            {t('privacy.deleteToday')}
          </button>
          <ConfirmButton
            label={t('privacy.deleteAll')}
            confirmLabel={t('privacy.deleteAllConfirm')}
            onConfirm={() => deleteEverything.mutate(false)}
          />
          <ConfirmButton
            label={t('privacy.deleteEverything')}
            confirmLabel={t('privacy.deleteEverythingConfirm')}
            onConfirm={() => deleteEverything.mutate(true)}
          />
        </div>
      </section>

      {pending ? (
        <div className="modal-backdrop" role="dialog" aria-modal="true">
          <div className="modal" style={{ width: 460 }}>
            <h2 style={{ marginTop: 0, fontSize: 17 }}>{t('privacy.deletePrompt', { what: pending.label })}</h2>
            <p>
              {t('privacy.rangeContains', {
                segments: pending.segments,
                sessions: pending.sessions,
              })}
            </p>
            <p className="muted small">
              {t('privacy.deletingSessions')}
            </p>
            <div className="row" style={{ justifyContent: 'flex-end' }}>
              <button onClick={() => setPending(null)}>{t('common.cancel')}</button>
              <button
                onClick={() => {
                  deleteRange.mutate({
                    fromMs: pending.fromMs,
                    toMs: pending.toMs,
                    includeSessions: false,
                  });
                  setPending(null);
                }}
              >
                {t('privacy.deleteActivityOnly')}
              </button>
              <button
                className="danger"
                onClick={() => {
                  deleteRange.mutate({
                    fromMs: pending.fromMs,
                    toMs: pending.toMs,
                    includeSessions: true,
                  });
                  setPending(null);
                }}
              >
                {t('privacy.deleteActivityAndSessions')}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function PolicyLock({ by }: { by: string }) {
  const { t } = useTranslation();

  return (
    <span className="lock-note small">
      <svg
        width="11"
        height="11"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.4"
        aria-hidden
      >
        <rect x="4" y="10.5" width="16" height="10" rx="2" />
        <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
      </svg>
      {t('privacy.setBy', { by })}
    </span>
  );
}

function ConfirmButton({
  label,
  confirmLabel,
  onConfirm,
}: {
  label: string;
  confirmLabel: string;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const [armed, setArmed] = useState(false);
  if (!armed) {
    return (
      <button className="danger" onClick={() => setArmed(true)}>
        {label}
      </button>
    );
  }
  return (
    <span className="row">
      <span className="small">{confirmLabel}</span>
      <button
        className="danger"
        onClick={() => {
          onConfirm();
          setArmed(false);
        }}
      >
        {t('privacy.yesDelete')}
      </button>
      <button className="ghost" onClick={() => setArmed(false)}>
        {t('common.cancel')}
      </button>
    </span>
  );
}
