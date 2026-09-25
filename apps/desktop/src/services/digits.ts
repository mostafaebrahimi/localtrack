/**
 * Persian digits.
 *
 * When the interface is in Persian every figure it prints is Persian too —
 * durations, clock times, shares, counts, sizes. The alternative, Latin
 * figures inside Persian sentences, is what a machine translation looks like.
 *
 * The active script is module state rather than a parameter because the
 * formatters below are called from everywhere, including outside React: one
 * assignment when the language changes beats threading a locale through every
 * call site. It is set by the i18n provider and read nowhere else.
 */
const PERSIAN = ['۰', '۱', '۲', '۳', '۴', '۵', '۶', '۷', '۸', '۹'];

let persianFigures = false;

export function setPersianFigures(on: boolean): void {
  persianFigures = on;
}

/** Latin figures in, the current script's figures out. */
export function figures(value: string | number): string {
  const text = String(value);
  if (!persianFigures) return text;
  return text.replace(/[0-9]/g, (digit) => PERSIAN[Number(digit)]!);
}

/**
 * Duration units, and whether the clock is a 24-hour one.
 *
 * "۰۲h ۰۶m" is only half translated: Persian figures with English units. And
 * Iran keeps a 24-hour clock, so an AM/PM suffix is not something to
 * translate — it is something to drop.
 */
export interface DurationUnits {
  hour: string;
  minute: string;
  second: string;
}

let units: DurationUnits = { hour: 'h', minute: 'm', second: 's' };
let twentyFourHour = false;

export function setDurationUnits(next: DurationUnits): void {
  units = next;
}

export function setTwentyFourHour(on: boolean): void {
  twentyFourHour = on;
}

export function unit(which: keyof DurationUnits): string {
  return units[which];
}

/**
 * The calendar to print dates in.
 *
 * Iran keeps the Persian (Jalali) calendar, and a date shown to someone
 * reading Persian should be the date they would write down: 25 August 2026 is
 * ۳ شهریور ۱۴۰۵. `Intl` knows the conversion, so this is a locale tag rather
 * than arithmetic of our own — and `undefined` keeps the system's calendar for
 * everyone else.
 */
let dateTag: string | undefined;

export function setDateLocale(tag: string | undefined): void {
  dateTag = tag;
}

export function dateLocale(): string | undefined {
  return dateTag;
}

/** `undefined` leaves the choice to the system, as it always did. */
export function hourCycle(): boolean | undefined {
  return twentyFourHour ? false : undefined;
}

/**
 * A date formatted for the current script.
 *
 * `toLocaleTimeString` already knows how to write Persian figures, but asking
 * it for them changes the whole shape of the string — the Persian calendar,
 * Persian AM/PM, Persian separators. The application prints one clock format
 * everywhere, so only the figures are swapped.
 */
export function localeDigits(text: string): string {
  return figures(text);
}
