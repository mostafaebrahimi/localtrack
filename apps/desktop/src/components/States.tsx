import type { ReactNode } from 'react';

import { useTranslation } from '../i18n';

export function Loading({ label }: { label?: string }) {
  const { t } = useTranslation();
  return <div className="empty">{label ?? t('states.loading')}</div>;
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const { t } = useTranslation();
  const message =
    typeof error === 'object' && error !== null && 'message' in error
      ? String((error as { message: unknown }).message)
      : t('states.somethingWrong');
  return (
    <div className="notice danger">
      <strong>{message}</strong>
      {onRetry ? (
        <div style={{ marginTop: 8 }}>
          <button onClick={onRetry}>{t('states.retry')}</button>
        </div>
      ) : null}
    </div>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <h2>{title}</h2>
      {description ? <p>{description}</p> : null}
      {action ? <div style={{ marginTop: 12 }}>{action}</div> : null}
    </div>
  );
}
