import { create } from 'zustand';

import type { ActivityFilter } from '../types';
import { rangeFor, type DateRange, type RangePresetId } from '../services/ranges';

interface AppStoreState {
  range: DateRange;
  filter: ActivityFilter;
  selectedSegmentId: string | null;
  selectedDomain: string | null;
  toast: { kind: 'info' | 'error' | 'success'; message: string } | null;

  setPreset: (preset: RangePresetId) => void;
  setCustomRange: (fromMs: number, toMs: number) => void;
  setFilter: (filter: ActivityFilter) => void;
  patchFilter: (patch: Partial<ActivityFilter>) => void;
  clearFilter: () => void;
  selectSegment: (id: string | null) => void;
  selectDomain: (domain: string | null) => void;
  showToast: (kind: 'info' | 'error' | 'success', message: string) => void;
  dismissToast: () => void;
}

/**
 * A range can be deep-linked: `#/reports?range=last30`.
 *
 * The tray, notifications and the timer window all open specific views, and
 * this keeps "which period" part of the address rather than hidden state.
 */
function initialRange(): DateRange {
  const presets: RangePresetId[] = [
    'today',
    'yesterday',
    'thisWeek',
    'lastWeek',
    'thisMonth',
    'lastMonth',
    'last7',
    'last30',
  ];
  const query = window.location.hash.split('?')[1] ?? '';
  const requested = new URLSearchParams(query).get('range');
  const preset = presets.find((candidate) => candidate === requested);
  return rangeFor(preset ?? 'today');
}

export const useAppStore = create<AppStoreState>((set) => ({
  range: initialRange(),
  filter: {},
  selectedSegmentId: null,
  selectedDomain: null,
  toast: null,

  setPreset: (preset) => set({ range: rangeFor(preset) }),
  setCustomRange: (fromMs, toMs) => set({ range: { fromMs, toMs, preset: 'custom' } }),
  setFilter: (filter) => set({ filter }),
  patchFilter: (patch) => set((state) => ({ filter: { ...state.filter, ...patch } })),
  clearFilter: () => set({ filter: {} }),
  selectSegment: (id) => set({ selectedSegmentId: id }),
  selectDomain: (domain) => set({ selectedDomain: domain }),
  showToast: (kind, message) => set({ toast: { kind, message } }),
  dismissToast: () => set({ toast: null }),
}));
