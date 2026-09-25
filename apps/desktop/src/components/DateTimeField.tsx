/**
 * A date-and-time field that speaks the reader's calendar.
 *
 * The same problem as [`DateField`], with a clock attached: for a Persian
 * reader the native `datetime-local` control is replaced by a text field
 * reading `۱۴۰۵/۰۶/۰۳ ۰۹:۳۰`, and everywhere else the native one is kept.
 *
 * The value stays in the ISO shape the callers already exchange, so nothing
 * around it has to learn about calendars.
 */
import { useEffect, useState } from 'react';

import { useTranslation } from '../i18n';
import { figures } from '../services/digits';
import { formatJalaliInput, parseJalaliInput } from '../services/jalali';

/** `YYYY-MM-DDTHH:MM` in, `۱۴۰۵/۰۶/۰۳ ۰۹:۳۰` out. */
function toJalaliText(iso: string): string {
  if (!iso) return '';
  const [date, time = '00:00'] = iso.split('T');
  const parsed = new Date(`${date}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return iso;
  return figures(`${formatJalaliInput(parsed)} ${time}`);
}

function fromJalaliText(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed) return '';
  const [datePart, timePart = '00:00'] = trimmed.split(/\s+/);
  const day = parseJalaliInput(datePart ?? '');
  if (!day) return null;
  const clock = timePart.replace(/[۰-۹]/g, (d) => String('۰۱۲۳۴۵۶۷۸۹'.indexOf(d)));
  const match = clock.match(/^(\d{1,2}):(\d{2})$/);
  if (!match) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) return null;
  const pad = (v: number) => String(v).padStart(2, '0');
  return `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}T${pad(hours)}:${pad(minutes)}`;
}

export function DateTimeField({
  value,
  onChange,
  ariaLabel,
}: {
  /** `YYYY-MM-DDTHH:MM`, or empty. */
  value: string;
  onChange: (value: string) => void;
  ariaLabel?: string;
}) {
  const { locale } = useTranslation();
  const jalali = locale === 'fa';
  const [draft, setDraft] = useState<string | null>(null);
  useEffect(() => setDraft(null), [value, jalali]);

  if (!jalali) {
    return (
      <input
        type="datetime-local"
        aria-label={ariaLabel}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }

  return (
    <input
      type="text"
      inputMode="numeric"
      className="date-field wide"
      aria-label={ariaLabel}
      value={draft ?? toJalaliText(value)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft === null) return;
        const iso = fromJalaliText(draft);
        // An unreadable date snaps back rather than moving the session
        // somewhere nobody asked for.
        if (iso !== null) onChange(iso);
        setDraft(null);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur();
        if (event.key === 'Escape') setDraft(null);
      }}
    />
  );
}
