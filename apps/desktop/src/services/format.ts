/**
 * Formatting helpers. All durations arrive as milliseconds (spec §13).
 *
 * Every helper prints its figures in the interface's own script: in Persian
 * that means ۰۱۲۳۴۵۶۷۸۹, everywhere else the Latin figures it always used.
 */
import { dateLocale, figures, hourCycle, unit } from './digits';

export function formatDurationHm(ms: number): string {
  const totalMinutes = Math.floor(Math.max(ms, 0) / 60000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return figures(
    `${String(hours).padStart(2, '0')}${unit('hour')} ${String(minutes).padStart(2, '0')}${unit('minute')}`,
  );
}

export function formatDurationShort(ms: number): string {
  const totalSeconds = Math.floor(Math.max(ms, 0) / 1000);
  if (totalSeconds < 60) return figures(`${totalSeconds}${unit('second')}`);
  const minutes = Math.floor(totalSeconds / 60);
  if (minutes < 60) return figures(`${minutes}${unit('minute')}`);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return figures(
    rest === 0
      ? `${hours}${unit('hour')}`
      : `${hours}${unit('hour')} ${rest}${unit('minute')}`,
  );
}

export function formatDurationHms(ms: number): string {
  const total = Math.floor(Math.max(ms, 0) / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return figures([hours, minutes, seconds].map((v) => String(v).padStart(2, '0')).join(':'));
}

export function formatClock(ms: number): string {
  return figures(
    new Date(ms).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      hour12: hourCycle(),
    }),
  );
}

export function formatClockSeconds(ms: number): string {
  return figures(
    new Date(ms).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: hourCycle(),
    }),
  );
}

export function formatDate(ms: number): string {
  return figures(
    new Date(ms).toLocaleDateString(dateLocale(), {
      weekday: 'short',
      day: '2-digit',
      month: 'short',
    }),
  );
}

/**
 * A date the backend wrote as `YYYY-MM-DD`, printed in the reader's calendar.
 *
 * The stored form never changes — it is the key a report is filed under — so
 * only the reading of it moves.
 */
export function formatIsoDate(iso: string | null | undefined): string {
  if (!iso) return '';
  const parsed = new Date(`${iso}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return iso;
  return figures(
    parsed.toLocaleDateString(dateLocale(), {
      day: '2-digit',
      month: 'short',
      year: 'numeric',
    }),
  );
}

/** A short day-and-month reading, for axes where the year is understood. */
export function formatShortDay(iso: string | null | undefined): string {
  if (!iso) return '';
  const parsed = new Date(`${iso}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return iso.slice(5);
  return figures(parsed.toLocaleDateString(dateLocale(), { day: '2-digit', month: 'short' }));
}

/**
 * The value for an `<input type="date">`.
 *
 * Always ISO and always Gregorian: this is the wire format the HTML control
 * itself is defined in, not something a reader sees.
 */
export function formatDateInput(ms: number): string {
  const date = new Date(ms);
  const pad = (v: number) => String(v).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function formatDateTimeInput(ms: number): string {
  const date = new Date(ms);
  const pad = (v: number) => String(v).padStart(2, '0');
  return `${formatDateInput(ms)}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function parseDateTimeInput(value: string): number | null {
  const parsed = new Date(value).getTime();
  return Number.isNaN(parsed) ? null : parsed;
}

export function formatPercent(value: number): string {
  return figures(`${value.toFixed(1)}%`);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return figures(`${bytes} B`);
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return figures(`${value.toFixed(1)} ${units[unit]}`);
}

export function formatRelative(ms: number | null | undefined, now = Date.now()): string {
  if (ms === null || ms === undefined) return 'never';
  const delta = Math.max(0, now - ms);
  if (delta < 5000) return 'just now';
  if (delta < 60000) return figures(`${Math.round(delta / 1000)} sec ago`);
  if (delta < 3600000) return figures(`${Math.round(delta / 60000)} min ago`);
  return figures(`${Math.round(delta / 3600000)} h ago`);
}

export function formatDelta(ms: number): string {
  const sign = ms >= 0 ? '+' : '−';
  return `${sign}${formatDurationHm(Math.abs(ms))}`;
}
