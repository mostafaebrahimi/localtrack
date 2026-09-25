/** TanStack Query hooks. Queries are always bounded by the selected period. */

import { useEffect } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';

import { api } from '../services/api';
import { useTranslation, type MessageKey } from '../i18n';
import { useAppStore } from '../stores/useAppStore';
import type {
  ActivityFilter,
  CurrentStatus,
  RangeQuery,
  UiError,
} from '../types';

export const queryKeys = {
  status: ['status'] as const,
  today: ['today'] as const,
  timeline: (query: RangeQuery) => ['timeline', query] as const,
  reports: (query: RangeQuery) => ['reports', query] as const,
  comparison: (query: RangeQuery) => ['comparison', query] as const,
  weekly: (query: RangeQuery) => ['weekly', query] as const,
  pages: (query: RangeQuery, domain: string) => ['pages', query, domain] as const,
  activity: (filter: ActivityFilter, limit: number, offset: number) =>
    ['activity', filter, limit, offset] as const,
  sessions: (fromMs: number, toMs: number) => ['sessions', fromMs, toMs] as const,
  categories: ['categories'] as const,
  projects: ['projects'] as const,
  rules: ['rules'] as const,
  exclusions: ['exclusions'] as const,
  settings: ['settings'] as const,
  diagnostics: ['diagnostics'] as const,
  managed: ['managed'] as const,
  sentReports: ['sentReports'] as const,
  queuedReports: ['queuedReports'] as const,
  filterOptions: ['filterOptions'] as const,
};

/** Live status, refreshed by a Rust event and polled as a safety net. */
export function useStatus() {
  useStatusEvents();

  return useQuery({
    queryKey: queryKeys.status,
    queryFn: api.getCurrentStatus,
    // The backend pushes every change and heartbeats besides; polling is only
    // the safety net for a missed event.
    refetchInterval: 15000,
  });
}

/**
 * One field of the live status.
 *
 * Status arrives while the user is looking at reports that cost real work to
 * draw. Selecting a single value means the page only re-renders when that value
 * actually changes, instead of on every update.
 */
export function useStatusSelector<T>(select: (status: CurrentStatus) => T) {
  useStatusEvents();

  return useQuery({
    queryKey: queryKeys.status,
    queryFn: api.getCurrentStatus,
    refetchInterval: 15000,
    select,
  });
}

/** Keeps the status cache fed from the Rust side. */
function useStatusEvents() {
  const client = useQueryClient();

  useEffect(() => {
    const unlisten = listen<CurrentStatus>('localtrack://status', (event) => {
      client.setQueryData(queryKeys.status, event.payload);
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, [client]);
}

export function useRangeQuery(): RangeQuery {
  const range = useAppStore((state) => state.range);
  const filter = useAppStore((state) => state.filter);
  return { fromMs: range.fromMs, toMs: range.toMs, filter };
}

export function useToday() {
  // Today's totals are a day's worth of numbers; refreshing them twice a minute
  // is plenty, and each refresh repaints the whole page.
  return useQuery({ queryKey: queryKeys.today, queryFn: api.getToday, refetchInterval: 30000 });
}

export function useTimeline(query: RangeQuery) {
  return useQuery({
    queryKey: queryKeys.timeline(query),
    queryFn: () => api.getTimeline(query),
  });
}

export function useReports(query: RangeQuery) {
  return useQuery({
    queryKey: queryKeys.reports(query),
    queryFn: () => api.getReports(query),
  });
}

/** Week-by-week totals with the applications and addresses behind them. */
export function useWeekly(query: RangeQuery) {
  return useQuery({
    queryKey: queryKeys.weekly(query),
    queryFn: () => api.getWeeklyReport(query),
  });
}

export function useComparison(query: RangeQuery, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.comparison(query),
    queryFn: () => api.getComparison(query),
    enabled,
  });
}

export function usePageReport(query: RangeQuery, domain: string | null) {
  return useQuery({
    queryKey: queryKeys.pages(query, domain ?? ''),
    queryFn: () => api.getPageReport(query, domain as string),
    enabled: Boolean(domain),
  });
}

export function useActivityPage(filter: ActivityFilter, limit: number, offset: number) {
  return useQuery({
    queryKey: queryKeys.activity(filter, limit, offset),
    queryFn: () => api.getActivityPage(filter, limit, offset),
  });
}

export function useSessions(fromMs: number, toMs: number) {
  return useQuery({
    queryKey: queryKeys.sessions(fromMs, toMs),
    queryFn: () => api.listSessions(fromMs, toMs),
  });
}

export function useCategories() {
  return useQuery({ queryKey: queryKeys.categories, queryFn: api.listCategories });
}

export function useProjects(includeArchived = true) {
  return useQuery({
    queryKey: queryKeys.projects,
    queryFn: () => api.listProjects(includeArchived),
  });
}

export function useRules() {
  return useQuery({ queryKey: queryKeys.rules, queryFn: api.listRules });
}

export function useExclusions() {
  return useQuery({ queryKey: queryKeys.exclusions, queryFn: api.listExclusions });
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: api.getSettings });
}

export function useDiagnostics() {
  return useQuery({
    queryKey: queryKeys.diagnostics,
    queryFn: api.getDiagnostics,
    refetchInterval: 10000,
  });
}

/** Employee-mode status; also tells the interface which pages to show. */
export function useManagedStatus() {
  return useQuery({
    queryKey: queryKeys.managed,
    queryFn: api.getManagedStatus,
    refetchInterval: 30000,
  });
}

export function useSentReports() {
  return useQuery({ queryKey: queryKeys.sentReports, queryFn: api.listSentReports });
}

export function useQueuedReports() {
  return useQuery({ queryKey: queryKeys.queuedReports, queryFn: api.listQueuedReports });
}

export function useFilterOptions() {
  return useQuery({ queryKey: queryKeys.filterOptions, queryFn: api.getFilterOptions });
}

/** A mutation that refreshes the affected queries and surfaces errors. */
/**
 * `successMessage` is a message key, not a message: the toast is shown to the
 * person, so it is written in their language rather than in the one this call
 * site happens to be typed in.
 */
export function useAppMutation<TArgs, TResult>(
  fn: (args: TArgs) => Promise<TResult>,
  invalidate: readonly (readonly unknown[])[] = [],
  successMessage?: MessageKey,
) {
  const client = useQueryClient();
  const showToast = useAppStore((state) => state.showToast);
  const { t } = useTranslation();

  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      for (const key of invalidate) {
        void client.invalidateQueries({ queryKey: key });
      }
      if (successMessage) showToast('success', t(successMessage));
    },
    onError: (error: UiError) => {
      showToast('error', error.message ?? t('states.somethingWrong'));
    },
  });
}
