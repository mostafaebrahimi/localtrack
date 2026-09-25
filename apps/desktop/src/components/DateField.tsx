/**
 * A date field that speaks the reader's calendar.
 *
 * `<input type="date">` is defined by HTML in ISO Gregorian and drawn by the
 * platform; it cannot be told to show another calendar. So for a Persian
 * reader it is replaced by a text field showing the Jalali date, which reads
 * what it writes — in either script, with `/`, `-` or `.` between the parts.
 * Everywhere else the native control is kept, picker and all.
 */
import { useEffect, useState } from 'react';

import { useTranslation } from '../i18n';
import { figures } from '../services/digits';
import { formatDateInput } from '../services/format';
import { formatJalaliInput, parseJalaliInput } from '../services/jalali';

export function DateField({
  valueMs,
  onChange,
  ariaLabel,
}: {
  valueMs: number;
  /** Local midnight of the chosen day. */
  onChange: (ms: number) => void;
  ariaLabel: string;
}) {
  const { locale } = useTranslation();
  const jalali = locale === 'fa';

  // What is being typed, so a half-written date is not fought over.
  const [draft, setDraft] = useState<string | null>(null);
  useEffect(() => setDraft(null), [valueMs, jalali]);

  if (!jalali) {
    return (
      <input
        type="date"
        aria-label={ariaLabel}
        value={formatDateInput(valueMs)}
        onChange={(event) => {
          const parsed = new Date(`${event.target.value}T00:00:00`);
          if (!Number.isNaN(parsed.getTime())) onChange(parsed.getTime());
        }}
      />
    );
  }

  const shown = draft ?? figures(formatJalaliInput(new Date(valueMs)));

  return (
    <input
      type="text"
      inputMode="numeric"
      className="date-field"
      aria-label={ariaLabel}
      value={shown}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft === null) return;
        const parsed = parseJalaliInput(draft);
        // An unreadable date leaves the range alone and snaps back to what it
        // was, rather than quietly moving it somewhere unintended.
        if (parsed) onChange(parsed.getTime());
        setDraft(null);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur();
        if (event.key === 'Escape') setDraft(null);
      }}
    />
  );
}
