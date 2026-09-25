import { useState } from 'react';

import { useTranslation } from '../../i18n';
import { api } from '../../services/api';
import { queryKeys, useAppMutation } from '../../hooks/useLocalTrack';

/**
 * Joining an organization.
 *
 * Deliberately explicit about what changes: enrolling turns on daily reporting
 * and hands some settings to an administrator, and the person doing it should
 * know that before they type a code.
 */
export function EnrollmentDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [serverUrl, setServerUrl] = useState('https://');
  const [code, setCode] = useState('');
  const [deviceName, setDeviceName] = useState(defaultDeviceName());

  const enroll = useAppMutation(
    () => api.enrollDevice(serverUrl, code, deviceName),
    [queryKeys.managed, queryKeys.settings, queryKeys.status, queryKeys.categories, queryKeys.rules],
    'toast.deviceConnected',
  );

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal" style={{ width: 520 }}>
        <h2 style={{ marginTop: 0, fontSize: 17 }}>{t('enroll.title')}</h2>

        <div className="notice">
          <strong>{t('enroll.whatChanges')}</strong>
          <ul className="small" style={{ margin: '8px 0 0', paddingLeft: 18 }}>
            <li>{t('enroll.point1')}</li>
            <li>{t('enroll.point2')}</li>
            <li>{t('enroll.point3')}</li>
            <li>{t('enroll.point4')}</li>
            <li>{t('enroll.point5')}</li>
          </ul>
        </div>

        <div className="field">
          <label>{t('enroll.serverAddress')}</label>
          <input
            value={serverUrl}
            onChange={(event) => setServerUrl(event.target.value)}
            placeholder={t('enroll.serverPlaceholder')}
            spellCheck={false}
          />
        </div>
        <div className="field">
          <label>{t('enroll.code')}</label>
          <input
            value={code}
            onChange={(event) => setCode(event.target.value)}
            placeholder={t('enroll.codePlaceholder')}
          />
        </div>
        <div className="field">
          <label>{t('enroll.thisDevice')}</label>
          <input value={deviceName} onChange={(event) => setDeviceName(event.target.value)} />
        </div>

        <div className="row" style={{ justifyContent: 'flex-end', marginTop: 16 }}>
          <button onClick={onClose}>{t('common.cancel')}</button>
          <button
            className="primary"
            disabled={enroll.isPending || code.trim().length === 0}
            onClick={() => {
              enroll.mutate(undefined, { onSuccess: onClose });
            }}
          >
            {enroll.isPending ? t('enroll.connecting') : t('enroll.connect')}
          </button>
        </div>
      </div>
    </div>
  );
}

function defaultDeviceName(): string {
  const platform = navigator.userAgent.includes('Windows')
    ? 'Windows'
    : navigator.userAgent.includes('Mac')
      ? 'Mac'
      : 'Linux';
  return `${platform} device`;
}
