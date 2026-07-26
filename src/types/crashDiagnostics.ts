export interface PreviousCrashReport {
  id: string;
  crashed_at: string;
  app_version: string;
  bundle_id: string;
  signal: string | null;
  reason: string;
  summary: string;
  log_path: string;
  system_report_path: string | null;
}
