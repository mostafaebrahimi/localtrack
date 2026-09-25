/**
 * Types mirroring the Rust command surface. They are hand-written rather than
 * generated so the boundary stays small and reviewable (spec §120).
 */

export type ClockState = 'CLOCKED_OUT' | 'CLOCKED_IN' | 'ON_BREAK';
export type TrackingDecision = 'RECORDING' | 'PAUSED' | 'NOT_CLOCKED_IN' | 'ON_BREAK';
export type ActivitySource = 'DESKTOP' | 'CHROME' | 'SYSTEM' | 'MANUAL';
export type ActivityKind =
  | 'WINDOW'
  | 'BROWSER_PAGE'
  | 'IDLE'
  | 'LOCKED'
  | 'INTERACTION'
  | 'MANUAL'
  | 'INPUT';
export type ClassificationSource = 'MANUAL' | 'RULE' | 'DEFAULT';
export type UrlPolicy = 'DOMAIN_ONLY' | 'PATH_WITHOUT_QUERY' | 'FULL_URL';
export type TrackingScope = 'WORK_SESSIONS_ONLY' | 'ALWAYS';
export type Theme = 'system' | 'light' | 'dark';
/**
 * Interface languages. 'system' follows the desktop's own locale.
 * Persian ('fa') is read right to left; the interface mirrors for it.
 */
export type Language = 'system' | 'en' | 'es' | 'ca' | 'fa';

export interface WorkSession {
  id: string;
  startedAtMs: number;
  endedAtMs: number | null;
  startTimezoneOffsetMin: number;
  endTimezoneOffsetMin: number | null;
  note: string | null;
  createdManually: boolean;
  editedManually: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface WorkBreak {
  id: string;
  workSessionId: string;
  startedAtMs: number;
  endedAtMs: number | null;
  note: string | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface ActivitySegment {
  id: string;
  source: ActivitySource;
  kind: ActivityKind;
  startedAtMs: number;
  endedAtMs: number;
  timezoneOffsetMin: number | null;
  appName: string | null;
  processName: string | null;
  windowTitle: string | null;
  browser: string | null;
  domain: string | null;
  url: string | null;
  pageTitle: string | null;
  interactionType: string | null;
  categoryId: string | null;
  projectId: string | null;
  classificationSource: ClassificationSource | null;
  isAfk: boolean;
  metadataJson: string | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface Summary {
  fromMs: number;
  toMs: number;
  clockedMs: number;
  breakMs: number;
  workMs: number;
  activeMs: number;
  idleMs: number;
  untrackedMs: number;
  untrackedAgentOffMs: number;
  contextSwitches: number;
  averageFocusMs: number;
  longestFocusMs: number;
  medianFocusMs: number;
  sessionCount: number;
}

export interface DaySummary extends Summary {
  date: string;
  firstClockInMs: number | null;
  lastClockOutMs: number | null;
}

export type TimelineBlockKind =
  | 'LOCKED'
  | 'IDLE'
  | 'BROWSER_PAGE'
  | 'APPLICATION'
  | 'INPUT'
  | 'BREAK'
  | 'UNTRACKED';

export interface TimelineBlock {
  startMs: number;
  endMs: number;
  durationMs: number;
  blockKind: TimelineBlockKind;
  label: string;
  segmentId: string | null;
  source: ActivitySource | null;
  kind: ActivityKind | null;
  appName: string | null;
  processName: string | null;
  windowTitle: string | null;
  browser: string | null;
  domain: string | null;
  url: string | null;
  pageTitle: string | null;
  categoryId: string | null;
  projectId: string | null;
}

export interface ReportRow {
  key: string;
  label: string;
  secondary: string | null;
  durationMs: number;
  percentage: number;
  visitCount: number;
}

export interface ReportSet {
  dimension: string;
  totalMs: number;
  rows: ReportRow[];
}

export interface WeekSummary {
  week: string;
  startsAtMs: number;
  endsAtMs: number;
  summary: Summary;
  days: DaySummary[];
  applications: ReportSet;
  websites: ReportSet;
  pages: ReportSet;
}

export interface ReportBundle {
  summary: Summary;
  daily: DaySummary[];
  applications: ReportSet;
  websites: ReportSet;
  pages: ReportSet;
  /** What the window titles said was being worked on. */
  workspaces: ReportSet;
  categories: ReportSet;
  projects: ReportSet;
}

export interface TodayDashboard {
  summary: Summary;
  timeline: TimelineBlock[];
  applications: ReportSet;
  websites: ReportSet;
  /** What the window titles said was being worked on. */
  workspaces: ReportSet;
  categories: ReportSet;
  projects: ReportSet;
}

export interface ComparisonResult {
  current: ReportBundle;
  previous: ReportBundle;
  clockedDeltaMs: number;
  workDeltaMs: number;
  activeDeltaMs: number;
  idleDeltaMs: number;
  breakDeltaMs: number;
}

export interface CollectorStatus {
  name: string;
  healthy: boolean;
  available: boolean;
  lastEventMs: number | null;
  error: string | null;
  detail: string | null;
}

export interface CurrentActivity {
  label: string;
  appName: string | null;
  domain: string | null;
  title: string | null;
  sinceMs: number;
  categoryName: string | null;
  projectName: string | null;
}

export interface CurrentStatus {
  state: ClockState;
  tracking: TrackingDecision;
  session: WorkSession | null;
  openBreak: WorkBreak | null;
  sessionDurationMs: number;
  breakDurationMs: number;
  currentActivity: CurrentActivity | null;
  today: Summary;
  collectors: CollectorStatus[];
  chromeConnected: boolean;
  staleSessionWarning: boolean;
  appVersion: string;
  schemaVersion: number;
}

export interface ActivityFilter {
  fromMs?: number | null;
  toMs?: number | null;
  sources?: string[];
  kinds?: string[];
  appNames?: string[];
  processNames?: string[];
  browsers?: string[];
  domains?: string[];
  categoryIds?: string[];
  projectIds?: string[];
  isAfk?: boolean | null;
  minDurationMs?: number | null;
  maxDurationMs?: number | null;
  search?: string | null;
  uncategorizedOnly?: boolean;
  unassignedOnly?: boolean;
}

export interface RangeQuery {
  fromMs: number;
  toMs: number;
  filter: ActivityFilter;
}

export interface ActivityRow extends ActivitySegment {
  label: string;
  durationMs: number;
  categoryName: string | null;
  projectName: string | null;
}

export interface ActivityPage {
  rows: ActivityRow[];
  total: number;
  limit: number;
  offset: number;
}

export interface SessionDetail {
  session: WorkSession;
  breaks: WorkBreak[];
  summary: Summary;
}

export interface Category {
  id: string;
  name: string;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface Project {
  id: string;
  name: string;
  archived: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export type RuleField =
  | 'app_name'
  | 'process_name'
  | 'window_title'
  | 'browser'
  | 'domain'
  | 'url'
  | 'page_title'
  | 'workspace';

export type RuleOperator = 'EXACT' | 'CONTAINS' | 'STARTS_WITH' | 'ENDS_WITH' | 'GLOB' | 'REGEX';

export interface ClassificationRule {
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
  targetField: RuleField;
  operator: RuleOperator;
  pattern: string;
  categoryId: string | null;
  projectId: string | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export type ExclusionTarget = 'DOMAIN' | 'URL' | 'APP' | 'PROCESS' | 'TITLE';
export type ExclusionAction = 'IGNORE' | 'DURATION_ONLY' | 'REDACT';

export interface ExclusionRule {
  id: string;
  enabled: boolean;
  target: ExclusionTarget;
  pattern: string;
  action: ExclusionAction;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface Settings {
  trackingScope: TrackingScope;
  pauseTrackingDuringBreak: boolean;
  trackingPaused: boolean;
  afkThresholdSeconds: number;
  autoClockOutIdleMinutes: number;
  splitSessionsAtMidnight: boolean;
  launchAtStartup: boolean;
  closeToTray: boolean;
  startMinimized: boolean;
  showMiniTimer: boolean;
  /** Where the timer bar was last parked; null until it has been moved. */
  miniTimerX: number | null;
  miniTimerY: number | null;
  /** Send detected workspaces with the daily report (employee mode). */
  shareWorkspaceContext: boolean;
  theme: Theme;
  language: Language;
  onboardingCompleted: boolean;
  urlPolicy: UrlPolicy;
  trackIncognito: boolean;
  detailedInteractions: boolean;
  recordExcludedDuration: boolean;
  retentionDays: number;
  retentionDeleteSessions: boolean;
  lastRetentionRunMs: number;
  mergeGapSeconds: number;
  heartbeatIntervalSeconds: number;
  heartbeatToleranceSeconds: number;
  pollIntervalSeconds: number;
  titleStabilitySeconds: number;
  contextSwitchNoiseSeconds: number;
  focusInterruptionToleranceSeconds: number;
}

export interface SettingsView {
  settings: Settings;
  dataDirectory: string;
  databasePath: string;
  databaseSizeBytes: number;
  schemaVersion: number;
  appVersion: string;
}

export interface FilterOptions {
  applications: string[];
  processes: string[];
  browsers: string[];
  domains: string[];
}

export interface DeletionPreview {
  segments: number;
  sessions: number;
}

export interface RetentionReport {
  segmentsDeleted: number;
  sessionsDeleted: number;
  cutoffMs: number;
}

export interface DesktopSupport {
  adapter: string;
  activeWindow: boolean;
  windowTitle: boolean;
  afk: boolean;
  lock: boolean;
  detail: string | null;
}

export interface PipelineStats {
  observationsReceived: number;
  observationsDroppedPrivacy: number;
  observationsDroppedState: number;
  segmentsWritten: number;
  lastDesktopEventMs: number | null;
  lastBrowserEventMs: number | null;
  lastWriteErrorMs: number | null;
}

export interface DatabaseHealth {
  path: string;
  sizeBytes: number;
  schemaVersion: number;
  segmentCount: number;
  sessionCount: number;
  journalMode: string;
  oldestSegmentMs: number | null;
  newestSegmentMs: number | null;
}

export interface Diagnostics {
  appVersion: string;
  schemaVersion: number;
  protocolVersion: number;
  nativeHostVersion: string | null;
  chromeExtensionOrigin: string | null;
  chromeConnectedAtMs: number | null;
  platform: string;
  desktop: DesktopSupport;
  collectors: CollectorStatus[];
  tracking: TrackingDecision;
  database: DatabaseHealth;
  databaseIntegrityOk: boolean;
  dataDirectory: string;
  stats: PipelineStats;
  generatedAtMs: number;
}

export type ExportFormat = 'xlsx' | 'csv';
export type ExportUrlPrivacy = 'DOMAIN_ONLY' | 'SANITIZED_URL' | 'FULL_URL';

export interface ExportOptions {
  includeSummary: boolean;
  includeDailySummary: boolean;
  includeSessions: boolean;
  includeApplications: boolean;
  includeWebsites: boolean;
  includePages: boolean;
  includeCategories: boolean;
  includeProjects: boolean;
  includeActivityDetail: boolean;
  includeInteractions: boolean;
  urlPrivacy: ExportUrlPrivacy;
}

export interface ExportRequest {
  fromMs: number;
  toMs: number;
  format: ExportFormat;
  destination: string;
  options: ExportOptions;
  filter: ActivityFilter;
}

export interface ExportResult {
  files: string[];
  rowCounts: { sessions: number; activities: number; interactions: number };
}

export type AppMode = 'PERSONAL' | 'EMPLOYEE';

export interface ManagedStatus {
  mode: AppMode;
  enrolled: boolean;
  organization: string | null;
  serverHost: string | null;
  employeeRef: string | null;
  locked: boolean;
  lockedSettings: string[];
  policyRevision: number | null;
  notice: string | null;
  lastReportAtMs: number | null;
  lastHeartbeatAtMs: number | null;
  lastPolicyAtMs: number | null;
  lastError: string | null;
  pendingReports: number;
}

export interface SentReport {
  date: string | null;
  description: string;
  deliveredAtMs: number | null;
  attempts: number;
  lastError: string | null;
  payloadJson: string;
}

export interface SessionSyncSummary {
  pushed: number;
  inserted: number;
  updated: number;
  deleted: number;
  keptLocal: number;
  skipped: number;
  closedDuplicates: number;
}

export interface UiError {
  code: string;
  message: string;
}
