use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::crash_diagnostics::SessionMarker;

#[derive(Debug, Clone)]
pub struct MatchedCrashReport {
    pub crashed_at: String,
    pub signal: Option<String>,
    pub reason: String,
    pub summary: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ReportMetadata {
    timestamp: String,
    #[serde(rename = "bundleID")]
    bundle_id: String,
}

pub fn find_matching(
    directory: &Path,
    session: &SessionMarker,
    detected_at: DateTime<Utc>,
) -> Result<Option<MatchedCrashReport>, String> {
    if !directory.exists() {
        return Ok(None);
    }

    let started_at = DateTime::parse_from_rfc3339(&session.started_at)
        .map_err(|error| {
            format!(
                "Invalid session start time '{}': {error}",
                session.started_at
            )
        })?
        .with_timezone(&Utc);
    let mut best: Option<(DateTime<Utc>, MatchedCrashReport)> = None;

    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("AQBot-") || !name.ends_with(".ips") {
            continue;
        }

        let Some((capture_time, report)) = parse_candidate(&path, session)? else {
            continue;
        };
        if capture_time < started_at - Duration::seconds(5)
            || capture_time > detected_at + Duration::seconds(5)
        {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_time, _)| capture_time > *best_time)
        {
            best = Some((capture_time, report));
        }
    }

    Ok(best.map(|(_, report)| report))
}

fn parse_candidate(
    path: &Path,
    session: &SessionMarker,
) -> Result<Option<(DateTime<Utc>, MatchedCrashReport)>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
    let Some((metadata_json, report_json)) = contents.split_once('\n') else {
        return Err(format!(
            "Crash report '{}' is missing its JSON body",
            path.display()
        ));
    };
    let metadata: ReportMetadata = serde_json::from_str(metadata_json)
        .map_err(|error| format!("Invalid metadata in '{}': {error}", path.display()))?;
    if metadata.bundle_id != session.bundle_id {
        return Ok(None);
    }

    let report: Value = serde_json::from_str(report_json)
        .map_err(|error| format!("Invalid report body in '{}': {error}", path.display()))?;
    if report.get("pid").and_then(Value::as_u64) != Some(u64::from(session.pid)) {
        return Ok(None);
    }

    let capture_text = report
        .get("captureTime")
        .and_then(Value::as_str)
        .unwrap_or(&metadata.timestamp);
    let capture_time = parse_macos_time(capture_text).ok_or_else(|| {
        format!(
            "Crash report '{}' has an unsupported timestamp: {capture_text}",
            path.display()
        )
    })?;
    let signal = report
        .pointer("/exception/signal")
        .and_then(Value::as_str)
        .map(str::to_string);
    let reason = report
        .pointer("/termination/indicator")
        .or_else(|| report.pointer("/exception/type"))
        .and_then(Value::as_str)
        .unwrap_or("Process terminated unexpectedly")
        .to_string();
    let summary = report_summary(&report);

    Ok(Some((
        capture_time,
        MatchedCrashReport {
            crashed_at: capture_time.to_rfc3339(),
            signal,
            reason,
            summary,
            path: path.to_path_buf(),
        },
    )))
}

fn parse_macos_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f %z")
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn report_summary(report: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(assertions) = report.get("asi").and_then(Value::as_object) {
        for messages in assertions.values() {
            if let Some(messages) = messages.as_array() {
                lines.extend(
                    messages
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
            }
        }
    }

    let faulting_thread = report
        .get("faultingThread")
        .and_then(Value::as_u64)
        .and_then(|index| report.get("threads")?.get(index as usize));
    if let Some(frames) = faulting_thread
        .and_then(|thread| thread.get("frames"))
        .and_then(Value::as_array)
    {
        lines.extend(
            frames
                .iter()
                .filter_map(|frame| frame.get("symbol").and_then(Value::as_str))
                .take(12)
                .map(str::to_string),
        );
    }

    if lines.is_empty() {
        "No assertion or symbolicated faulting-thread frames were available.".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::find_matching;
    use crate::crash_diagnostics::SessionMarker;
    use chrono::{TimeZone, Utc};

    fn session() -> SessionMarker {
        SessionMarker {
            session_id: "session-1".into(),
            pid: 42,
            started_at: "2026-07-26T01:40:00Z".into(),
            app_version: "1.2.3".into(),
            bundle_id: "top.aqbot.desktop.dev".into(),
            log_path: "/tmp/aqbot.log".into(),
        }
    }

    #[test]
    fn matches_two_part_ips_and_extracts_fault_details() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("AQBot-2026-07-26-094416.ips");
        let metadata =
            r#"{"timestamp":"2026-07-26 09:44:16.00 +0800","bundleID":"top.aqbot.desktop.dev"}"#;
        let body = r#"{
          "pid": 42,
          "captureTime": "2026-07-26 09:44:11.0698 +0800",
          "exception": {"type":"EXC_BREAKPOINT","signal":"SIGTRAP"},
          "termination": {"indicator":"Trace/BPT trap: 5"},
          "faultingThread": 0,
          "asi": {"libsystem_c.dylib":["Must only be used from the main thread"]},
          "threads": [{"frames":[{"symbol":"-[NSPanel setFloatingPanel:]"},{"symbol":"ensure_panel"}]}]
        }"#;
        std::fs::write(&path, format!("{metadata}\n{body}")).expect("write report");

        let detected_at = Utc
            .with_ymd_and_hms(2026, 7, 26, 2, 0, 0)
            .single()
            .expect("time");
        let report = find_matching(directory.path(), &session(), detected_at)
            .expect("find report")
            .expect("matching report");

        assert_eq!(report.signal.as_deref(), Some("SIGTRAP"));
        assert_eq!(report.reason, "Trace/BPT trap: 5");
        assert!(report
            .summary
            .contains("Must only be used from the main thread"));
        assert!(report.summary.contains("setFloatingPanel"));
        assert_eq!(report.path, path);
    }

    #[test]
    fn rejects_wrong_pid_bundle_and_time_range() {
        let directory = tempfile::tempdir().expect("tempdir");
        let metadata = r#"{"timestamp":"2026-07-25 09:44:16.00 +0800","bundleID":"other.bundle"}"#;
        let body = r#"{"pid":99,"captureTime":"2026-07-25 09:44:11.00 +0800"}"#;
        std::fs::write(
            directory.path().join("AQBot-unrelated.ips"),
            format!("{metadata}\n{body}"),
        )
        .expect("write report");
        let detected_at = Utc
            .with_ymd_and_hms(2026, 7, 26, 2, 0, 0)
            .single()
            .expect("time");

        assert!(find_matching(directory.path(), &session(), detected_at)
            .expect("find report")
            .is_none());
    }
}
