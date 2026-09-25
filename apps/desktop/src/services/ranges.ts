/** Date range presets (spec §82). All boundaries are local-time aware. */
import type { MessageKey } from '../i18n';

export type RangePresetId =
  | 'today'
  | 'yesterday'
  | 'thisWeek'
  | 'lastWeek'
  | 'thisMonth'
  | 'lastMonth'
  | 'last7'
  | 'last30'
  | 'custom';

export interface DateRange {
  fromMs: number;
  toMs: number;
  preset: RangePresetId;
}

function startOfDay(date: Date): number {
  const copy = new Date(date);
  copy.setHours(0, 0, 0, 0);
  return copy.getTime();
}

function addDays(ms: number, days: number): number {
  const date = new Date(ms);
  date.setDate(date.getDate() + days);
  return date.getTime();
}

function startOfWeek(date: Date): number {
  const copy = new Date(startOfDay(date));
  // Weeks start on Monday.
  const day = (copy.getDay() + 6) % 7;
  return addDays(copy.getTime(), -day);
}

function startOfMonth(date: Date): number {
  const copy = new Date(date);
  copy.setDate(1);
  return startOfDay(copy);
}

export function rangeFor(preset: RangePresetId, now = Date.now()): DateRange {
  const today = startOfDay(new Date(now));
  switch (preset) {
    case 'today':
      return { fromMs: today, toMs: addDays(today, 1), preset };
    case 'yesterday':
      return { fromMs: addDays(today, -1), toMs: today, preset };
    case 'thisWeek': {
      const from = startOfWeek(new Date(now));
      return { fromMs: from, toMs: addDays(from, 7), preset };
    }
    case 'lastWeek': {
      const from = addDays(startOfWeek(new Date(now)), -7);
      return { fromMs: from, toMs: addDays(from, 7), preset };
    }
    case 'thisMonth': {
      const from = startOfMonth(new Date(now));
      const to = new Date(from);
      to.setMonth(to.getMonth() + 1);
      return { fromMs: from, toMs: to.getTime(), preset };
    }
    case 'lastMonth': {
      const current = startOfMonth(new Date(now));
      const from = new Date(current);
      from.setMonth(from.getMonth() - 1);
      return { fromMs: from.getTime(), toMs: current, preset };
    }
    case 'last7':
      return { fromMs: addDays(today, -6), toMs: addDays(today, 1), preset };
    case 'last30':
      return { fromMs: addDays(today, -29), toMs: addDays(today, 1), preset };
    case 'custom':
    default:
      return { fromMs: today, toMs: addDays(today, 1), preset: 'custom' };
  }
}

/** The label is a key: a preset is named by the interface, not by this list. */
export const RANGE_PRESETS: { id: RangePresetId; label: MessageKey }[] = [
  { id: 'today', label: 'range.today' },
  { id: 'yesterday', label: 'range.yesterday' },
  { id: 'thisWeek', label: 'range.thisWeek' },
  { id: 'lastWeek', label: 'range.lastWeek' },
  { id: 'thisMonth', label: 'range.thisMonth' },
  { id: 'lastMonth', label: 'range.lastMonth' },
  { id: 'last7', label: 'range.last7' },
  { id: 'last30', label: 'range.last30' },
  { id: 'custom', label: 'range.custom' },
];
