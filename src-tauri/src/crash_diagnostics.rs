use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviousCrashReport {
    pub id: String,
    pub crashed_at: String,
    pub app_version: String,
    pub bundle_id: String,
    pub signal: Option<String>,
    pub reason: String,
    pub summary: String,
    pub log_path: String,
    pub system_report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionMarker {
    pub session_id: String,
    pub pid: u32,
    pub started_at: String,
    pub app_version: String,
    pub bundle_id: String,
    pub log_path: String,
}

pub struct CrashDiagnosticsState {
    pending: Mutex<Option<PreviousCrashReport>>,
    pending_path: Option<PathBuf>,
    current_marker: Option<(PathBuf, SessionMarker)>,
    initialization_error: Option<String>,
}

impl CrashDiagnosticsState {
    pub fn initialize(
        aqbot_home: &Path,
        app_version: String,
        bundle_id: String,
        log_path: PathBuf,
    ) -> Self {
        match Self::initialize_at(aqbot_home, app_version, bundle_id, log_path, Utc::now()) {
            Ok(state) => state,
            Err(error) => {
                tracing::error!(%error, "Could not initialize AQBot crash diagnostics");
                Self {
                    pending: Mutex::new(None),
                    pending_path: None,
                    current_marker: None,
                    initialization_error: Some(error),
                }
            }
        }
    }

    pub fn previous(&self) -> Result<Option<PreviousCrashReport>, String> {
        self.ensure_available()?;
        self.pending
            .lock()
            .map_err(|_| "Crash diagnostics lock poisoned".to_string())
            .map(|report| report.clone())
    }

    pub fn acknowledge(&self, id: &str) -> Result<(), String> {
        self.ensure_available()?;
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Crash diagnostics lock poisoned".to_string())?;
        let Some(report) = pending.as_ref() else {
            return Err("No previous crash report is pending".to_string());
        };
        if report.id != id {
            return Err("Crash report identifier does not match the pending report".to_string());
        }
        if let Some(path) = &self.pending_path {
            remove_if_exists(path)?;
        }
        *pending = None;
        Ok(())
    }

    pub fn finish_clean(&self) -> Result<(), String> {
        self.ensure_available()?;
        let Some((path, current)) = &self.current_marker else {
            return Ok(());
        };
        let Some(on_disk) = read_json_optional::<SessionMarker>(path)? else {
            return Ok(());
        };
        if on_disk.session_id == current.session_id {
            remove_if_exists(path)?;
        }
        Ok(())
    }

    fn initialize_at(
        aqbot_home: &Path,
        app_version: String,
        bundle_id: String,
        log_path: PathBuf,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        let directory = aqbot_home.join("diagnostics");
        fs::create_dir_all(&directory).map_err(|error| {
            format!(
                "Could not create crash diagnostics directory '{}': {error}",
                directory.display()
            )
        })?;
        let current_path = directory.join(format!("current-session-{bundle_id}.json"));
        let pending_path = directory.join(format!("pending-crash-{bundle_id}.json"));
        let mut pending = load_or_quarantine::<PreviousCrashReport>(&pending_path)?;

        if let Some(previous_session) = load_or_quarantine::<SessionMarker>(&current_path)? {
            let report = report_for_session(&previous_session, now)?;
            atomic_write_json(&pending_path, &report)?;
            pending = Some(report);
        }

        let current = SessionMarker {
            session_id: Uuid::new_v4().to_string(),
            pid: std::process::id(),
            started_at: now.to_rfc3339(),
            app_version,
            bundle_id,
            log_path: log_path.to_string_lossy().into_owned(),
        };
        atomic_write_json(&current_path, &current)?;
        tracing::info!(
            session_id = %current.session_id,
            marker = %current_path.display(),
            "AQBot crash session marker created"
        );

        Ok(Self {
            pending: Mutex::new(pending),
            pending_path: Some(pending_path),
            current_marker: Some((current_path, current)),
            initialization_error: None,
        })
    }

    fn ensure_available(&self) -> Result<(), String> {
        match &self.initialization_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn report_for_session(
    session: &SessionMarker,
    detected_at: DateTime<Utc>,
) -> Result<PreviousCrashReport, String> {
    #[cfg(target_os = "macos")]
    let system_report =
        crate::macos_crash_report::find_matching(&macos_report_directory()?, session, detected_at)?;
    #[cfg(not(target_os = "macos"))]
    let system_report: Option<crate::macos_crash_report::MatchedCrashReport> = None;

    Ok(match system_report {
        Some(report) => PreviousCrashReport {
            id: session.session_id.clone(),
            crashed_at: report.crashed_at,
            app_version: session.app_version.clone(),
            bundle_id: session.bundle_id.clone(),
            signal: report.signal,
            reason: report.reason,
            summary: report.summary,
            log_path: session.log_path.clone(),
            system_report_path: Some(report.path.to_string_lossy().into_owned()),
        },
        None => PreviousCrashReport {
            id: session.session_id.clone(),
            crashed_at: detected_at.to_rfc3339(),
            app_version: session.app_version.clone(),
            bundle_id: session.bundle_id.clone(),
            signal: None,
            reason: "Previous AQBot session did not reach a clean shutdown".to_string(),
            summary: "No matching macOS DiagnosticReports entry was available.".to_string(),
            log_path: session.log_path.clone(),
            system_report_path: None,
        },
    })
}

#[cfg(target_os = "macos")]
fn macos_report_directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| "HOME is unavailable while resolving DiagnosticReports".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Logs")
        .join("DiagnosticReports"))
}

fn load_or_quarantine<T>(path: &Path) -> Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    match read_json_optional(path) {
        Ok(value) => Ok(value),
        Err(error) => {
            let quarantine = path.with_extension(format!("invalid-{}.json", Uuid::new_v4()));
            fs::rename(path, &quarantine).map_err(|rename_error| {
                format!(
                    "{error}; could not quarantine invalid file as '{}': {rename_error}",
                    quarantine.display()
                )
            })?;
            tracing::error!(
                %error,
                quarantine = %quarantine.display(),
                "Invalid crash diagnostics file quarantined"
            );
            Ok(None)
        }
    }
}

fn read_json_optional<T>(path: &Path) -> Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| format!("Invalid JSON in '{}': {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read '{}': {error}", path.display())),
    }
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if let Err(error) = result {
        return match fs::remove_file(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => Err(error),
            Err(cleanup) => Err(format!("{error}; temporary-file cleanup failed: {cleanup}")),
        };
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove '{}': {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::CrashDiagnosticsState;
    use chrono::{TimeZone, Utc};

    #[test]
    fn abnormal_session_becomes_pending_and_can_be_acknowledged() {
        let directory = tempfile::tempdir().expect("tempdir");
        let first_time = Utc.with_ymd_and_hms(2026, 7, 26, 1, 0, 0).unwrap();
        let first = CrashDiagnosticsState::initialize_at(
            directory.path(),
            "1.0.0".into(),
            "top.aqbot.test".into(),
            directory.path().join("aqbot.log"),
            first_time,
        )
        .expect("first session");
        drop(first);

        let second = CrashDiagnosticsState::initialize_at(
            directory.path(),
            "1.0.1".into(),
            "top.aqbot.test".into(),
            directory.path().join("aqbot.log"),
            first_time + chrono::Duration::minutes(1),
        )
        .expect("second session");
        let report = second
            .previous()
            .expect("previous report")
            .expect("pending report");
        assert_eq!(report.app_version, "1.0.0");
        assert!(report.system_report_path.is_none());

        second.acknowledge(&report.id).expect("acknowledge");
        assert!(second.previous().expect("previous report").is_none());
    }

    #[test]
    fn clean_session_does_not_create_a_crash_report() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = CrashDiagnosticsState::initialize_at(
            directory.path(),
            "1.0.0".into(),
            "top.aqbot.clean".into(),
            directory.path().join("aqbot.log"),
            Utc::now(),
        )
        .expect("session");
        state.finish_clean().expect("clean finish");

        let next = CrashDiagnosticsState::initialize_at(
            directory.path(),
            "1.0.1".into(),
            "top.aqbot.clean".into(),
            directory.path().join("aqbot.log"),
            Utc::now(),
        )
        .expect("next session");
        assert!(next.previous().expect("previous report").is_none());
    }
}
