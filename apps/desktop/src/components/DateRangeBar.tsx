import { DateField } from './DateField';
import { useTranslation } from '../i18n';
import { RANGE_PRESETS, type RangePresetId } from '../services/ranges';
import { useAppStore } from '../stores/useAppStore';

export function DateRangeBar() {
  const { t } = useTranslation();
  const range = useAppStore((state) => state.range);
  const setPreset = useAppStore((state) => state.setPreset);
  const setCustomRange = useAppStore((state) => state.setCustomRange);

  const onFrom = (from: number) => {
    setCustomRange(from, Math.max(range.toMs, from + 86400000));
  };
  const onTo = (day: number) => {
    // The end is exclusive, so include the whole selected day.
    const to = day + 86400000;
    setCustomRange(Math.min(range.fromMs, to - 86400000), to);
  };

  return (
    <div className="row">
      <select
        value={range.preset}
        onChange={(event) => setPreset(event.target.value as RangePresetId)}
        aria-label={t('range.preset')}
      >
        {RANGE_PRESETS.map((preset) => (
          <option key={preset.id} value={preset.id}>
            {t(preset.label)}
          </option>
        ))}
      </select>
      <DateField valueMs={range.fromMs} onChange={onFrom} ariaLabel={t('range.fromDate')} />
      <span className="muted">{t('common.to')}</span>
      <DateField valueMs={range.toMs - 1} onChange={onTo} ariaLabel={t('range.toDate')} />
    </div>
  );
}
