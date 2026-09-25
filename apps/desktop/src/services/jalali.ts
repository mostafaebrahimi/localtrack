/**
 * The Persian (Jalali) calendar.
 *
 * `Intl` can print a Jalali date but not read one back, and a date field has
 * to do both: show ۱۴۰۵/۰۶/۰۳ and understand it when someone types over it.
 * Printing therefore goes through `Intl` — it is the platform's own calendar
 * data, and correct by construction — while reading uses the standard
 * arithmetic below, which the tests check against `Intl` day by day so the two
 * directions can never drift apart.
 */

export interface JalaliDate {
  year: number;
  month: number;
  day: number;
}

/** Latin figures, so the parts can be read as numbers whatever the locale. */
const PARTS = new Intl.DateTimeFormat('en-u-ca-persian', {
  year: 'numeric',
  month: 'numeric',
  day: 'numeric',
  timeZone: 'UTC',
});

/** Gregorian to Jalali, via the platform's own calendar data. */
export function toJalali(date: Date): JalaliDate {
  // Read at UTC noon: the conversion is about the calendar day, and a local
  // midnight can land on either side of it depending on the offset.
  const noon = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate(), 12);
  const parts = PARTS.formatToParts(new Date(noon));
  const value = (type: string) => Number(parts.find((part) => part.type === type)?.value ?? '0');
  return { year: value('year'), month: value('month'), day: value('day') };
}

/**
 * Day of the Jalali year, which is exact: months 1–6 have 31 days and 7–11
 * have 30, so only Esfand's length varies and it never affects this count.
 */
function dayOfYear(month: number, day: number): number {
  return month <= 6 ? (month - 1) * 31 + day : 186 + (month - 7) * 30 + day;
}

/** 1 Farvardin 1405, the anchor the search starts from. */
const ANCHOR = { gregorian: [2026, 2, 21] as const, year: 1405 };

/**
 * Jalali to Gregorian.
 *
 * Rather than carry epoch constants of its own — the part of this conversion
 * that is easiest to get subtly wrong — this walks to the answer using the
 * platform's calendar as the judge: estimate a day, ask what Jalali date it
 * is, step by the difference, repeat. The estimate is close enough that it
 * converges in two or three steps.
 *
 * Returns null for a date that does not exist — the 31st of a 30-day month,
 * the 30th of Esfand outside a leap year — rather than silently rolling it
 * forward into the next month.
 */
export function fromJalali(year: number, month: number, day: number): Date | null {
  if (!Number.isInteger(year) || !Number.isInteger(month) || !Number.isInteger(day)) return null;
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;

  // The estimate lands within a day or two: month lengths are exact and only
  // the year length is approximated.
  const estimate = new Date(ANCHOR.gregorian[0], ANCHOR.gregorian[1], ANCHOR.gregorian[2]);
  estimate.setDate(
    estimate.getDate() + Math.round((year - ANCHOR.year) * 365.2422) + dayOfYear(month, day) - 1,
  );

  // Then settle it by asking the platform. Stepping by the difference can
  // oscillate across a year boundary, where the approximation and the real
  // year length disagree; walking out from the estimate cannot.
  for (let offset = 0; offset <= 30; offset += 1) {
    for (const direction of offset === 0 ? [0] : [-1, 1]) {
      const candidate = new Date(estimate);
      candidate.setDate(candidate.getDate() + offset * direction);
      const here = toJalali(candidate);
      if (here.year === year && here.month === month && here.day === day) {
        // Rebuilt from its own parts: stepping across a daylight-saving
        // change lands an hour into the day, and this is a calendar date,
        // so it should start where that day actually starts.
        return new Date(candidate.getFullYear(), candidate.getMonth(), candidate.getDate());
      }
    }
  }
  // No such day: the 31st of a 30-day month, or the 30th of Esfand outside a
  // leap year.
  return null;
}

/** `1405/06/03`, in Latin figures; the caller localises them. */
export function formatJalaliInput(date: Date): string {
  const { year, month, day } = toJalali(date);
  return `${year}/${String(month).padStart(2, '0')}/${String(day).padStart(2, '0')}`;
}

/** Reads `1405/6/3`, `۱۴۰۵/۰۶/۰۳`, or the same with - or . between. */
export function parseJalaliInput(text: string): Date | null {
  const latin = text.replace(/[۰-۹]/g, (d) => String('۰۱۲۳۴۵۶۷۸۹'.indexOf(d)));
  const match = latin.trim().match(/^(\d{3,4})[/\-.](\d{1,2})[/\-.](\d{1,2})$/);
  if (!match) return null;
  return fromJalali(Number(match[1]), Number(match[2]), Number(match[3]));
}
