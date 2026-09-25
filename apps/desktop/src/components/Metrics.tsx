import { memo } from 'react';
import { useTranslation } from '../i18n';
import { formatDurationHm, formatDurationShort } from '../services/format';
import type { Summary } from '../types';

export function Metric({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="card metric">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
      {hint ? <div className="hint">{hint}</div> : null}
    </div>
  );
}

function Reading({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="readout-item">
      <div className="readout-label">{label}</div>
      <div className="readout-value">{value}</div>
      {hint ? <div className="readout-hint">{hint}</div> : null}
    </div>
  );
}

/**
 * The six duration metrics from spec §71. They always satisfy
 * work = active + idle + untracked, so untracked time never silently
 * becomes active time.
 *
 * They are read together, so they are drawn together: one divided strip,
 * the way a gauge cluster is, rather than six boxes competing for the eye.
 */
function SummaryCardsInner({ summary, framed = false }: { summary: Summary; framed?: boolean }) {
  const { t } = useTranslation();

  return (
    <div className={framed ? 'readout framed' : 'readout'}>
      <Reading
        label={t('metric.clocked')}
        value={formatDurationHm(summary.clockedMs)}
        hint={t('metric.clockedHint')}
      />
      <Reading
        label={t('metric.work')}
        value={formatDurationHm(summary.workMs)}
        hint={t('metric.workHint')}
      />
      <Reading
        label={t('metric.active')}
        value={formatDurationHm(summary.activeMs)}
        hint={t('metric.activeHint')}
      />
      <Reading
        label={t('metric.idle')}
        value={formatDurationHm(summary.idleMs)}
        hint={t('metric.idleHint')}
      />
      <Reading label={t('metric.break')} value={formatDurationHm(summary.breakMs)} />
      <Reading
        label={t('metric.untracked')}
        value={formatDurationHm(summary.untrackedMs)}
        hint={
          summary.untrackedAgentOffMs > 60_000
            ? t('metric.untrackedOffHint', {
                duration: formatDurationShort(summary.untrackedAgentOffMs),
              })
            : t('metric.untrackedHint')
        }
      />
    </div>
  );
}

/**
 * Drawing this is not free, and the live status updates around it constantly.
 * Memoising keeps the work tied to the data instead of the clock.
 */
export const SummaryCards = memo(SummaryCardsInner);
