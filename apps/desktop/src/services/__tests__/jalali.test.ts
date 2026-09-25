/**
 * The two directions of the Persian calendar have to agree exactly.
 *
 * Printing uses the platform's calendar data and reading uses our own
 * arithmetic; if they ever drift, a date field would show one day and save
 * another. So the arithmetic is checked against the platform across a long
 * span rather than at a handful of chosen dates.
 */

import { describe, expect, it } from 'vitest';

import { formatJalaliInput, fromJalali, parseJalaliInput, toJalali } from '../jalali';

describe('the Persian calendar', () => {
  it('round-trips every day across eight years', () => {
    // 2020-03-19 is 1398-12-29; the span covers leap years on both calendars.
    const start = new Date(2020, 2, 19);
    let mismatches = 0;
    let checked = 0;

    for (let offset = 0; offset < 8 * 365; offset += 1) {
      const day = new Date(start.getFullYear(), start.getMonth(), start.getDate() + offset);
      const jalali = toJalali(day);
      const back = fromJalali(jalali.year, jalali.month, jalali.day);
      checked += 1;
      if (!back || back.getTime() !== day.getTime()) mismatches += 1;
    }

    expect(checked).toBe(2920);
    expect(mismatches).toBe(0);
  });

  it('knows the dates a Persian reader would recognise', () => {
    // Nowruz 1405 is 21 March 2026.
    expect(formatJalaliInput(new Date(2026, 2, 21))).toBe('1405/01/01');
    // The day this was written.
    expect(formatJalaliInput(new Date(2026, 7, 25))).toBe('1405/06/03');
  });

  it('reads what it writes, in either script', () => {
    const written = new Date(2026, 7, 25);
    expect(parseJalaliInput(formatJalaliInput(written))?.getTime()).toBe(written.getTime());
    expect(parseJalaliInput('۱۴۰۵/۰۶/۰۳')?.getTime()).toBe(written.getTime());
    expect(parseJalaliInput('1405-6-3')?.getTime()).toBe(written.getTime());
  });

  it('refuses a day that does not exist', () => {
    // Esfand has 29 days outside a leap year, and the 7th month has 30.
    expect(fromJalali(1405, 8, 31)).toBeNull();
    expect(fromJalali(1405, 13, 1)).toBeNull();
    expect(parseJalaliInput('not a date')).toBeNull();
  });
});
