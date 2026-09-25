import { useEffect } from 'react';

import { useTranslation } from '../i18n';
import { useAppStore } from '../stores/useAppStore';

export function Toast() {
  const { t } = useTranslation();
  const toast = useAppStore((state) => state.toast);
  const dismiss = useAppStore((state) => state.dismissToast);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(dismiss, toast.kind === 'error' ? 8000 : 3500);
    return () => window.clearTimeout(timer);
  }, [toast, dismiss]);

  if (!toast) return null;

  return (
    <div className={`toast ${toast.kind}`} role="status">
      <span>{toast.message}</span>
      <button className="ghost" onClick={dismiss}>
        {t('common.dismiss')}
      </button>
    </div>
  );
}
