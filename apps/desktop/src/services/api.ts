/**
 * The only bridge between the dashboard and Rust.
 *
 * Every function here maps to one narrow Tauri command; there is no way for the
 * UI to run arbitrary SQL or reach the filesystem (spec §120, §107).
 */

import { invoke } from '@tauri-apps/api/core';

import { translate } from '../i18n';

import type {
  ActivityFilter,
  ActivityPage,
  ActivitySegment,
  Category,
  ClassificationRule,
  ComparisonResult,
  CurrentStatus,
  DeletionPreview,
  Diagnostics,
  ExclusionRule,
  ExportRequest,
  ExportResult,
  FilterOptions,
  ManagedStatus,
  Project,
  RangeQuery,
  ReportBundle,
  ReportSet,
  RetentionReport,
  SessionDetail,
  Settings,
  SentReport,
  SessionSyncSummary,
  SettingsView,
  Summary,
  TimelineBlock,
  WeekSummary,
  TodayDashboard,
  UiError,
  WorkBreak,
  WorkSession,
} from '../types';

/** Commands reject with a JSON-encoded UiError; turn it back into an object. */
export function parseError(error: unknown): UiError {
  if (typeof error === 'string') {
    try {
      const parsed = JSON.parse(error) as UiError;
      if (parsed && typeof parsed.message === 'string') return parsed;
    } catch {
      return { code: 'ERROR', message: error };
    }
  }
  if (error instanceof Error) return { code: 'ERROR', message: error.message };
  return { code: 'ERROR', message: translate('states.somethingWrong') };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw parseError(error);
  }
}

export const api = {
  // status and clock
  getCurrentStatus: () => call<CurrentStatus>('get_current_status'),
  clockIn: (note?: string | null) => call<CurrentStatus>('clock_in', { note: note ?? null }),
  setSessionNote: (sessionId: string, note: string | null) =>
    call<void>('set_session_note', { sessionId, note }),
  addManualEntry: (startedAtMs: number, endedAtMs: number, note: string | null) =>
    call<WorkSession>('add_manual_entry', { startedAtMs, endedAtMs, note }),
  clockOut: () => call<CurrentStatus>('clock_out'),
  startBreak: () => call<CurrentStatus>('start_break'),
  endBreak: () => call<CurrentStatus>('end_break'),
  setTrackingPaused: (paused: boolean) => call<CurrentStatus>('set_tracking_paused', { paused }),

  // reports
  getToday: () => call<TodayDashboard>('get_today_summary'),
  getSummary: (query: RangeQuery) => call<Summary>('get_summary', { query }),
  getTimeline: (query: RangeQuery) => call<TimelineBlock[]>('get_timeline', { query }),
  getReports: (query: RangeQuery) => call<ReportBundle>('get_reports', { query }),
  getWeeklyReport: (query: RangeQuery) => call<WeekSummary[]>('get_weekly_report', { query }),
  getPageReport: (query: RangeQuery, domain: string) =>
    call<ReportSet>('get_page_report', { query, domain }),
  getComparison: (query: RangeQuery) => call<ComparisonResult>('get_comparison', { query }),
  getActivityPage: (filter: ActivityFilter, limit: number, offset: number, descending = false) =>
    call<ActivityPage>('get_activity_page', { filter, limit, offset, descending }),
  getFilterOptions: () => call<FilterOptions>('get_filter_options'),

  // sessions
  listSessions: (fromMs: number, toMs: number) =>
    call<SessionDetail[]>('list_sessions', { fromMs, toMs }),
  updateSession: (
    sessionId: string,
    startedAtMs: number,
    endedAtMs: number | null,
    note: string | null,
  ) => call<SessionDetail>('update_session', { sessionId, startedAtMs, endedAtMs, note }),
  createManualSession: (startedAtMs: number, endedAtMs: number, note: string | null) =>
    call<WorkSession>('create_manual_session', { startedAtMs, endedAtMs, note }),
  deleteSession: (sessionId: string) => call<void>('delete_session', { sessionId }),
  addBreak: (sessionId: string, startedAtMs: number, endedAtMs: number | null) =>
    call<WorkBreak>('add_break', { sessionId, startedAtMs, endedAtMs }),
  updateBreak: (
    breakId: string,
    startedAtMs: number,
    endedAtMs: number | null,
    note: string | null,
  ) => call<void>('update_break', { breakId, startedAtMs, endedAtMs, note }),
  deleteBreak: (breakId: string) => call<void>('delete_break', { breakId }),

  // activity
  classifySegment: (segmentId: string, categoryId: string | null, projectId: string | null) =>
    call<ActivitySegment>('classify_segment', { segmentId, categoryId, projectId }),
  annotateSegment: (segmentId: string, note: string | null) =>
    call<ActivitySegment>('annotate_segment', { segmentId, note }),
  splitSegment: (segmentId: string, atMs: number) =>
    call<[string, string]>('split_segment', { segmentId, atMs }),
  deleteSegment: (segmentId: string) => call<void>('delete_segment', { segmentId }),

  // organization
  listCategories: () => call<Category[]>('list_categories'),
  createCategory: (name: string) => call<Category>('create_category', { name }),
  renameCategory: (id: string, name: string) => call<void>('rename_category', { id, name }),
  deleteCategory: (id: string) => call<void>('delete_category', { id }),
  listProjects: (includeArchived = false) => call<Project[]>('list_projects', { includeArchived }),
  createProject: (name: string) => call<Project>('create_project', { name }),
  updateProject: (id: string, name: string, archived: boolean) =>
    call<void>('update_project', { id, name, archived }),
  deleteProject: (id: string) => call<void>('delete_project', { id }),

  // rules
  listRules: () => call<ClassificationRule[]>('list_rules'),
  saveRule: (rule: ClassificationRule, applyToExisting: boolean) =>
    call<number>('save_rule', { rule, applyToExisting }),
  deleteRule: (id: string, applyToExisting: boolean) =>
    call<number>('delete_rule', { id, applyToExisting }),
  reclassify: (fromMs: number | null, toMs: number | null) =>
    call<number>('reclassify', { fromMs, toMs }),

  // privacy
  listExclusions: () => call<ExclusionRule[]>('list_exclusions'),
  saveExclusion: (rule: ExclusionRule) => call<ExclusionRule>('save_exclusion', { rule }),
  deleteExclusion: (id: string) => call<void>('delete_exclusion', { id }),

  // settings
  getSettings: () => call<SettingsView>('get_settings'),
  updateSetting: (key: string, value: unknown) => call<Settings>('update_setting', { key, value }),

  // data
  previewDeletion: (fromMs: number, toMs: number) =>
    call<DeletionPreview>('preview_deletion', { fromMs, toMs }),
  deleteRange: (fromMs: number, toMs: number, includeSessions: boolean) =>
    call<RetentionReport>('delete_range', { fromMs, toMs, includeSessions }),
  deleteLastMinutes: (minutes: number, includeSessions: boolean) =>
    call<RetentionReport>('delete_last_minutes', { minutes, includeSessions }),
  deleteAllActivity: (includeSessions: boolean) =>
    call<RetentionReport>('delete_all_activity', { includeSessions }),
  runRetention: () => call<RetentionReport | null>('run_retention'),
  backupDatabase: (destination: string | null) => call<string>('backup_database', { destination }),
  restoreDatabase: (source: string) => call<string>('restore_database', { source }),

  // export
  exportReport: (request: ExportRequest) => call<ExportResult>('export_report', { request }),
  fullUrlsAvailable: (fromMs: number, toMs: number) =>
    call<boolean>('full_urls_available', { fromMs, toMs }),

  // employee mode
  getManagedStatus: () => call<ManagedStatus>('get_managed_status'),
  enrollDevice: (serverUrl: string, code: string, deviceName: string) =>
    call<ManagedStatus>('enroll_device', { serverUrl, code, deviceName }),
  unenrollDevice: () => call<void>('unenroll_device'),
  syncNow: () => call<SessionSyncSummary>('sync_now'),
  listSentReports: () => call<SentReport[]>('list_sent_reports'),
  listQueuedReports: () => call<SentReport[]>('list_queued_reports'),

  // diagnostics
  getDiagnostics: () => call<Diagnostics>('get_diagnostics'),
  getDiagnosticsText: () => call<string>('get_diagnostics_text'),
};
