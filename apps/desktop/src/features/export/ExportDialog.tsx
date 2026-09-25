import { useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';

import { useTranslation } from '../../i18n';
import { api } from '../../services/api';
import { useAppMutation } from '../../hooks/useLocalTrack';
import { useAppStore } from '../../stores/useAppStore';
import { formatDateInput } from '../../services/format';
import type { ExportFormat, ExportOptions, ExportUrlPrivacy } from '../../types';

const DEFAULT_OPTIONS: ExportOptions = {
  includeSummary: true,
  includeDailySummary: true,
  includeSessions: true,
  includeApplications: true,
  includeWebsites: true,
  includePages: true,
  includeCategories: true,
  includeProjects: true,
  includeActivityDetail: true,
  includeInteractions: false,
  urlPrivacy: 'SANITIZED_URL',
};

/** Export dialog (spec §89). Everything is written locally. */
export function ExportDialog({
  fromMs,
  toMs,
  onClose,
}: {
  fromMs: number;
  toMs: number;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<ExportFormat>('xlsx');
  const [options, setOptions] = useState<ExportOptions>(DEFAULT_OPTIONS);
  const [fullUrlsExist, setFullUrlsExist] = useState(false);
  const filter = useAppStore((state) => state.filter);
  const showToast = useAppStore((state) => state.showToast);

  useEffect(() => {
    let active = true;
    void api
      .fullUrlsAvailable(fromMs, toMs)
      .then((value) => {
        if (active) setFullUrlsExist(value);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [fromMs, toMs]);

  const runExport = useAppMutation(
    (destination: string) =>
      api.exportReport({ fromMs, toMs, format, destination, options, filter }),
    [],
  );

  const toggle = (key: keyof ExportOptions) => (checked: boolean) =>
    setOptions((current) => ({ ...current, [key]: checked }));

  const pickDestination = async () => {
    const suggested = `localtrack-${formatDateInput(fromMs)}`;
    const destination =
      format === 'xlsx'
        ? await save({
            defaultPath: `${suggested}.xlsx`,
            filters: [{ name: t('export.excelWorkbook'), extensions: ['xlsx'] }],
          })
        : await open({ directory: true, title: t('export.chooseFolder') });

    if (!destination || Array.isArray(destination)) return;
    const result = await runExport.mutateAsync(destination);
    showToast(
      'success',
      `Exported ${result.rowCounts.activities} activities to ${result.files.length} file(s).`,
    );
    onClose();
  };

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal">
        <div className="row between">
          <h2 style={{ margin: 0, fontSize: 17 }}>{t('export.title')}</h2>
          <button className="ghost" onClick={onClose}>
            {t('common.close')}
          </button>
        </div>

        <p className="muted small">
          {formatDateInput(fromMs)} to {formatDateInput(toMs - 1)} · saved on this computer only.
        </p>

        <div className="field">
          <label>{t('export.format')}</label>
          <div className="row">
            <button className={format === 'xlsx' ? 'primary' : ''} onClick={() => setFormat('xlsx')}>
              XLSX
            </button>
            <button className={format === 'csv' ? 'primary' : ''} onClick={() => setFormat('csv')}>
              CSV
            </button>
          </div>
        </div>

        <div className="field">
          <label>{t('export.include')}</label>
          {(
            [
              ['includeSummary', t('export.summary')],
              ['includeDailySummary', t('export.dailySummary')],
              ['includeSessions', t('export.sessions')],
              ['includeApplications', t('export.applications')],
              ['includeWebsites', t('export.websites')],
              ['includePages', t('export.pages')],
              ['includeCategories', t('export.categories')],
              ['includeProjects', t('export.projects')],
              ['includeActivityDetail', t('export.activityDetail')],
              ['includeInteractions', t('export.interactions')],
            ] as [keyof ExportOptions, string][]
          ).map(([key, label]) => (
            <label className="checkbox" key={key}>
              <input
                type="checkbox"
                checked={Boolean(options[key])}
                onChange={(event) => toggle(key)(event.target.checked)}
              />
              {label}
            </label>
          ))}
        </div>

        <div className="field">
          <label>{t('export.urlPrivacy')}</label>
          <select
            value={options.urlPrivacy}
            onChange={(event) =>
              setOptions((current) => ({
                ...current,
                urlPrivacy: event.target.value as ExportUrlPrivacy,
              }))
            }
          >
            <option value="DOMAIN_ONLY">{t('export.domainOnly')}</option>
            <option value="SANITIZED_URL">{t('export.sanitizedUrl')}</option>
            {/* Only offered when full URLs were actually stored (spec §89). */}
            {fullUrlsExist ? <option value="FULL_URL">{t('export.fullUrl')}</option> : null}
          </select>
        </div>

        <div className="row" style={{ justifyContent: 'flex-end' }}>
          <button onClick={onClose}>{t('common.cancel')}</button>
          <button className="primary" onClick={() => void pickDestination()}>
            {t('export.chooseAndExport')}
          </button>
        </div>
      </div>
    </div>
  );
}
