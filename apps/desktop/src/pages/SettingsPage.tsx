import { useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { disable as disableAutostart, enable as enableAutostart } from '@tauri-apps/plugin-autostart';

import { ErrorState, Loading } from '../components/States';
import { LOCALES, useTranslation } from '../i18n';
import { api } from '../services/api';
import { formatBytes, formatRelative } from '../services/format';
import {
  queryKeys,
  useAppMutation,
  useDiagnostics,
  useManagedStatus,
  useSettings,
} from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import type { Language, Theme } from '../types';

const AFK_CHOICES = [60, 120, 180, 300, 600];

/** Each language names itself, so it is findable whatever the interface is set to. */
const LANGUAGE_NAMES: Record<string, string> = {
  en: 'English',
  es: 'Español',
  ca: 'Català',
  fa: 'فارسی',
};

export function SettingsPage() {
  const settings = useSettings();
  const { t } = useTranslation();
  const diagnostics = useDiagnostics();
  const managed = useManagedStatus();
  const lockedBy = managed.data?.organization ?? 'your organization';
  const isLocked = (key: string) => managed.data?.lockedSettings.includes(key) ?? false;
  const showToast = useAppStore((state) => state.showToast);
  const [customAfk, setCustomAfk] = useState('');

  const invalidate = [queryKeys.settings, queryKeys.status, queryKeys.diagnostics];
  const updateSetting = useAppMutation(
    (payload: { key: string; value: unknown }) => api.updateSetting(payload.key, payload.value),
    invalidate,
  );
  const backup = useAppMutation(
    (destination: string | null) => api.backupDatabase(destination),
    invalidate,
  );
  const restore = useAppMutation((source: string) => api.restoreDatabase(source), invalidate);

  if (settings.isLoading) return <Loading />;
  if (settings.error) {
    return <ErrorState error={settings.error} onRetry={() => void settings.refetch()} />;
  }
  if (!settings.data) return null;
  const current = settings.data.settings;

  const setLaunchAtStartup = async (enabled: boolean) => {
    try {
      if (enabled) await enableAutostart();
      else await disableAutostart();
      updateSetting.mutate({ key: 'launch_at_startup', value: enabled });
    } catch {
      showToast('error', t('settings.autostartRefused'));
    }
  };

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('settings.title')}</h1>
          <p>
            {t('settings.versionLine', {
              version: settings.data.appVersion,
              schema: settings.data.schemaVersion,
            })}
          </p>
        </div>
      </div>

      <div className="grid-2">
        <section className="card">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('settings.general')}</h2>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={current.launchAtStartup}
              disabled={isLocked('launch_at_startup')}
              onChange={(event) => void setLaunchAtStartup(event.target.checked)}
            />
            {t('settings.startOnLogin')}
          </label>
          {isLocked('launch_at_startup') ? <LockNote by={lockedBy} /> : null}
          <label className="checkbox">
            <input
              type="checkbox"
              checked={current.closeToTray}
              onChange={(event) =>
                updateSetting.mutate({ key: 'close_to_tray', value: event.target.checked })
              }
            />
            {t('settings.closeToTray')}
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={current.startMinimized}
              onChange={(event) =>
                updateSetting.mutate({ key: 'start_minimized', value: event.target.checked })
              }
            />
            {t('settings.startMinimized')}
          </label>
          <div className="field">
            <label>{t('settings.timer')}</label>
            <select
              value={current.showMiniTimer ? 'bar' : 'tray'}
              disabled={isLocked('show_mini_timer')}
              onChange={(event) =>
                updateSetting.mutate({
                  key: 'show_mini_timer',
                  value: event.target.value === 'bar',
                })
              }
            >
              <option value="bar">{t('settings.timerBar')}</option>
              <option value="tray">{t('settings.timerTray')}</option>
            </select>
            <span className="muted small">
              {t('settings.timerHint')}
            </span>
            {isLocked('show_mini_timer') ? <LockNote by={lockedBy} /> : null}
          </div>
          <label className="checkbox">
            <input
              type="checkbox"
              disabled={isLocked('pause_tracking_during_break')}
              checked={current.pauseTrackingDuringBreak}
              onChange={(event) =>
                updateSetting.mutate({
                  key: 'pause_tracking_during_break',
                  value: event.target.checked,
                })
              }
            />
            {t('settings.pauseDuringBreaks')}
          </label>

          <div className="field">
            <label>{t('settings.idleThreshold')}</label>
            <div className="row">
              <select
                disabled={isLocked('afk_threshold_seconds')}
                value={String(current.afkThresholdSeconds)}
                onChange={(event) =>
                  updateSetting.mutate({
                    key: 'afk_threshold_seconds',
                    value: Number(event.target.value),
                  })
                }
              >
                {AFK_CHOICES.map((seconds) => (
                  <option key={seconds} value={seconds}>
                    {seconds < 60
                      ? t('unit.seconds', { count: seconds })
                      : t('unit.minutes', { count: seconds / 60 })}
                  </option>
                ))}
                {!AFK_CHOICES.includes(current.afkThresholdSeconds) ? (
                  <option value={current.afkThresholdSeconds}>
                    {current.afkThresholdSeconds} sec (custom)
                  </option>
                ) : null}
              </select>
              <input
                style={{ width: 110 }}
                placeholder={t('settings.idlePlaceholder')}
                aria-label={t('settings.idleAria')}
                value={customAfk}
                onChange={(event) => setCustomAfk(event.target.value)}
              />
              <button
                onClick={() => {
                  const seconds = Number(customAfk);
                  if (Number.isFinite(seconds) && seconds >= 10) {
                    updateSetting.mutate({ key: 'afk_threshold_seconds', value: seconds });
                    setCustomAfk('');
                  } else {
                    showToast('error', t('settings.idleAtLeast'));
                  }
                }}
              >
                {t('common.set')}
              </button>
            </div>
          </div>

          <div className="field">
            <label>{t('settings.autoClockOut')}</label>
            <select
              value={String(current.autoClockOutIdleMinutes)}
              disabled={isLocked('auto_clock_out_idle_minutes')}
              onChange={(event) =>
                updateSetting.mutate({
                  key: 'auto_clock_out_idle_minutes',
                  value: Number(event.target.value),
                })
              }
            >
              <option value="0">{t('settings.autoNever')}</option>
              <option value="10">{t('settings.autoAfter', { count: 10 })}</option>
              <option value="20">{t('settings.autoAfter', { count: 20 })}</option>
              <option value="30">{t('settings.autoAfter', { count: 30 })}</option>
              <option value="60">{t('settings.autoAfterHour')}</option>
            </select>
            <span className="muted small">
              {t('settings.autoHint')}
            </span>
            {isLocked('auto_clock_out_idle_minutes') ? <LockNote by={lockedBy} /> : null}
          </div>

          <label className="checkbox">
            <input
              type="checkbox"
              checked={current.splitSessionsAtMidnight}
              disabled={isLocked('split_sessions_at_midnight')}
              onChange={(event) =>
                updateSetting.mutate({
                  key: 'split_sessions_at_midnight',
                  value: event.target.checked,
                })
              }
            />
            {t('settings.splitMidnight')}
          </label>
          <span className="muted small" style={{ display: 'block', marginBottom: 10 }}>
            {t('settings.splitHint')}
          </span>

          <div className="field">
            <label>{t('settings.appearance')}</label>
            <select
              value={current.theme}
              onChange={(event) =>
                updateSetting.mutate({ key: 'theme', value: event.target.value as Theme })
              }
            >
              <option value="system">{t('theme.system')}</option>
              <option value="light">{t('theme.light')}</option>
              <option value="dark">{t('theme.dark')}</option>
            </select>
          </div>

          <div className="field">
            <label>{t('settings.language')}</label>
            <select
              value={current.language}
              onChange={(event) =>
                updateSetting.mutate({ key: 'language', value: event.target.value as Language })
              }
            >
              <option value="system">{t('language.system')}</option>
              {/* Each language names itself, the way a language picker should:
                  someone looking for Català should not have to know the word
                  the current interface uses for it. */}
              {LOCALES.map((locale) => (
                <option key={locale} value={locale}>
                  {LANGUAGE_NAMES[locale]}
                </option>
              ))}
            </select>
            <span className="muted small">{t('settings.languageHint')}</span>
          </div>
        </section>

        <section className="card">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('settings.data')}</h2>
          <table>
            <tbody>
              <tr>
                <td>{t('settings.database')}</td>
                <td className="mono path" title={settings.data.databasePath}>
                  {settings.data.databasePath}
                </td>
              </tr>
              <tr>
                <td>{t('settings.size')}</td>
                <td>{formatBytes(settings.data.databaseSizeBytes)}</td>
              </tr>
            </tbody>
          </table>
          <div className="row" style={{ marginTop: 10 }}>
            <button
              onClick={async () => {
                const destination = await save({
                  defaultPath: `localtrack-backup-${new Date().toISOString().slice(0, 10)}.db`,
                  filters: [{ name: 'SQLite database', extensions: ['db'] }],
                });
                const path = await backup.mutateAsync(destination ?? null);
                showToast('success', t('settings.backupWritten', { path }));
              }}
            >
              {t('settings.backup')}
            </button>
            <button
              onClick={async () => {
                const source = await open({
                  multiple: false,
                  filters: [{ name: 'SQLite database', extensions: ['db'] }],
                });
                if (!source || Array.isArray(source)) return;
                const safety = await restore.mutateAsync(source);
                showToast(
                  'success',
                  `Database restored. Your previous database was kept at ${safety}.`,
                );
              }}
            >
              {t('settings.restore')}
            </button>
          </div>
          <p className="muted small">
            {t('settings.backupHint')}
          </p>
        </section>
      </div>

      <section className="card" style={{ marginTop: 16 }}>
        <div className="row between">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('settings.diagnostics')}</h2>
          <button
            onClick={async () => {
              const text = await api.getDiagnosticsText();
              await navigator.clipboard.writeText(text);
              showToast('success', t('settings.diagnosticsCopied'));
            }}
          >
            {t('settings.copyDiagnostics')}
          </button>
        </div>
        {diagnostics.data ? (
          <table>
            <tbody>
              <tr>
                <td>{t('settings.platform')}</td>
                <td>
                  {diagnostics.data.platform} · adapter {diagnostics.data.desktop.adapter}
                </td>
              </tr>
              <tr>
                <td>{t('settings.desktopTracking')}</td>
                <td>
                  {diagnostics.data.desktop.activeWindow ? (
                    <span className="badge ok">{t('status.available')}</span>
                  ) : (
                    <span className="badge warn">{t('status.limited')}</span>
                  )}{' '}
                  <span className="muted small">{diagnostics.data.desktop.detail ?? ''}</span>
                </td>
              </tr>
              <tr>
                <td>{t('settings.idleDetection')}</td>
                <td>
                  {diagnostics.data.desktop.afk ? (
                    <span className="badge ok">{t('status.available')}</span>
                  ) : (
                    <span className="badge warn">{t('status.unavailable')}</span>
                  )}
                </td>
              </tr>
              {diagnostics.data.collectors.map((collector) => (
                <tr key={collector.name}>
                  <td>Collector: {collector.name}</td>
                  <td>
                    {!collector.available ? (
                      <span className="badge warn">{t('status.unavailable')}</span>
                    ) : collector.healthy ? (
                      <span className="badge ok">{t('status.healthy')}</span>
                    ) : (
                      <span className="badge err">{t('status.notConnected')}</span>
                    )}{' '}
                    <span className="muted small">
                      {collector.detail ?? collector.error ?? ''} · last event{' '}
                      {formatRelative(collector.lastEventMs)}
                    </span>
                  </td>
                </tr>
              ))}
              <tr>
                <td>{t('settings.database')}</td>
                <td>
                  {diagnostics.data.databaseIntegrityOk ? (
                    <span className="badge ok">{t('status.healthy')}</span>
                  ) : (
                    <span className="badge err">{t('status.integrityFailed')}</span>
                  )}{' '}
                  <span className="muted small">
                    {formatBytes(diagnostics.data.database.sizeBytes)} ·{' '}
                    {diagnostics.data.database.segmentCount} activities ·{' '}
                    {diagnostics.data.database.journalMode.toUpperCase()}
                  </span>
                </td>
              </tr>
              <tr>
                <td>{t('settings.nativeProtocol')}</td>
                <td>Version {diagnostics.data.protocolVersion}</td>
              </tr>
              <tr>
                <td>{t('settings.versions')}</td>
                <td className="small">
                  {t('settings.versionsLine', {
                    version: diagnostics.data.appVersion,
                    schema: diagnostics.data.schemaVersion,
                    host: diagnostics.data.nativeHostVersion ?? t('settings.hostNotConnected'),
                  })}
                  {diagnostics.data.chromeExtensionOrigin ? (
                    <>
                      <br />
                      <span className="muted">
                        Extension {diagnostics.data.chromeExtensionOrigin} · last connected{' '}
                        {formatRelative(diagnostics.data.chromeConnectedAtMs)}
                      </span>
                    </>
                  ) : null}
                </td>
              </tr>
            </tbody>
          </table>
        ) : (
          <Loading />
        )}
      </section>

      <section className="card" style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('settings.chromeExtension')}</h2>
        <ol className="small">
          <li>
            {t('settings.chromeStep1')} <span className="mono">pnpm build:extension</span>
          </li>
          <li>
            Open <span className="mono">chrome://extensions</span>, enable developer mode and load{' '}
            <span className="mono">apps/chrome-extension/dist</span>
          </li>
          <li>
            {t('settings.chromeStep3')}{' '}
            <span className="mono">localtrack-native-host install &lt;extension-id&gt;</span>
          </li>
        </ol>
        <p className="muted small">
          {t('settings.chromeHint')}
        </p>
      </section>
    </div>
  );
}

function LockNote({ by }: { by: string }) {
  const { t } = useTranslation();

  return (
    <span className="lock-note small">
      <LockIcon /> {t('settings.setBy', { by })}
    </span>
  );
}

function LockIcon() {
  return (
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
  );
}
