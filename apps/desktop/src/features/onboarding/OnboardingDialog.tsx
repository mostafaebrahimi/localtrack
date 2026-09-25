import { useState } from 'react';

import { useTranslation } from '../../i18n';
import { api } from '../../services/api';
import { queryKeys, useAppMutation } from '../../hooks/useLocalTrack';
import { EnrollmentDialog } from '../managed/EnrollmentDialog';
import type { UrlPolicy } from '../../types';

/** First-launch setup (spec §156). */
export function OnboardingDialog() {
  const { t } = useTranslation();
  const [step, setStep] = useState(0);
  const [enrolling, setEnrolling] = useState(false);
  const update = useAppMutation(
    (payload: { key: string; value: unknown }) => api.updateSetting(payload.key, payload.value),
    [queryKeys.settings, queryKeys.status],
  );
  const clockIn = useAppMutation(api.clockIn, [queryKeys.status, queryKeys.today]);

  const finish = async (alsoClockIn: boolean) => {
    await update.mutateAsync({ key: 'onboarding_completed', value: true });
    if (alsoClockIn) await clockIn.mutateAsync(undefined);
  };

  const steps = [
    {
      title: t('onboard.welcome'),
      body: (
        <>
          <p>{t('onboard.staysHere')}</p>
          <p className="muted">{t('onboard.noAccount')}</p>
        </>
      ),
      next: t('onboard.continue'),
    },
    {
      title: t('onboard.tracking'),
      body: (
        <ul>
          <li>{t('onboard.desktopApps')}</li>
          <li>{t('onboard.windowTitles')}</li>
          <li>{t('onboard.idleDetection')}</li>
        </ul>
      ),
      next: t('onboard.continue'),
    },
    {
      title: t('onboard.howUse'),
      body: (
        <>
          <p className="muted small">
            {t('onboard.howUseHint')}
          </p>
          <div className="choice">
            <strong>{t('onboard.justForMe')}</strong>
            <span className="muted small">
              {t('onboard.justForMeHint')}
            </span>
          </div>
          <div className="choice">
            <strong>{t('onboard.forWork')}</strong>
            <span className="muted small">
              {t('onboard.forWorkHint')}
            </span>
            <button style={{ marginTop: 8 }} onClick={() => setEnrolling(true)}>
              {t('onboard.connectOrg')}
            </button>
          </div>
        </>
      ),
      next: t('onboard.continue'),
    },
    {
      title: t('onboard.chrome'),
      body: (
        <>
          <p>{t('onboard.chromeBody')}</p>
          <p className="muted small">
            Load <span className="mono">apps/chrome-extension/dist</span> at{' '}
            <span className="mono">chrome://extensions</span>, then run{' '}
            <span className="mono">localtrack-native-host install &lt;extension-id&gt;</span>. You
            can do this later.
          </p>
        </>
      ),
      next: t('onboard.chromeLater'),
    },
    {
      title: t('privacy.title'),
      body: (
        <>
          <div className="field">
            <label>{t('privacy.urlStorage')}</label>
            <select
              defaultValue="PATH_WITHOUT_QUERY"
              onChange={(event) =>
                update.mutate({ key: 'url_policy', value: event.target.value as UrlPolicy })
              }
            >
              <option value="PATH_WITHOUT_QUERY">{t('privacy.domainPath')}</option>
              <option value="DOMAIN_ONLY">{t('privacy.domainOnly')}</option>
              <option value="FULL_URL">{t('onboard.fullUrlQuery')}</option>
            </select>
          </div>
          <p className="muted small">
            {t('onboard.privacyHint')}
          </p>
        </>
      ),
      next: t('onboard.continue'),
    },
    {
      title: t('onboard.ready'),
      body: <p>{t('onboard.readyBody')}</p>,
      next: t('clock.clockIn'),
    },
  ];

  const current = steps[step];
  if (!current) return null;
  const isLast = step === steps.length - 1;

  if (enrolling) {
    return <EnrollmentDialog onClose={() => setEnrolling(false)} />;
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal" style={{ width: 460 }}>
        <h2 style={{ marginTop: 0 }}>{current.title}</h2>
        {current.body}
        <div className="row between" style={{ marginTop: 20 }}>
          <button className="ghost" onClick={() => void finish(false)}>
            {t('onboard.skip')}
          </button>
          <div className="row">
            {step > 0 ? <button onClick={() => setStep(step - 1)}>{t('onboard.back')}</button> : null}
            <button
              className="primary"
              onClick={() => (isLast ? void finish(true) : setStep(step + 1))}
            >
              {current.next}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
