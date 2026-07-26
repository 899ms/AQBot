use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

pub const LOG_FILE_ENV: &str = "AQBOT_LOG_FILE";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOG_FILE_COUNT: usize = 5;

#[derive(Clone)]
struct TeeLogWriter {
    file: Arc<Mutex<File>>,
}

struct TeeWriter {
    stderr: io::Stderr,
    file: Arc<Mutex<File>>,
}

impl<'writer> MakeWriter<'writer> for TeeLogWriter {
    type Writer = TeeWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        TeeWriter {
            stderr: io::stderr(),
            file: Arc::clone(&self.file),
        }
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stderr.write_all(buf)?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("AQBot log file lock poisoned"))?;
        file.write_all(buf)?;
        file.flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()?;
        self.file
            .lock()
            .map_err(|_| io::Error::other("AQBot log file lock poisoned"))?
            .flush()
    }
}

pub fn init() {
    let path = log_path_from_value(env::var_os(LOG_FILE_ENV)).unwrap_or_else(default_log_path);
    if let Err(error) = rotate_log_files(&path, MAX_LOG_BYTES, LOG_FILE_COUNT) {
        eprintln!(
            "failed to rotate AQBot diagnostic log '{}': {error}",
            path.display()
        );
    }

    match open_log_file(&path) {
        Ok(file) => {
            if let Err(error) = tracing_subscriber::fmt()
                .with_env_filter(env_filter())
                .with_writer(TeeLogWriter {
                    file: Arc::new(Mutex::new(file)),
                })
                .try_init()
            {
                eprintln!("failed to initialize AQBot diagnostic logging: {error}");
                return;
            }
            tracing::info!(
                log_file = %path.display(),
                "AQBot stderr and file diagnostic logging enabled"
            );
        }
        Err(error) => {
            eprintln!(
                "failed to open AQBot diagnostic log file '{}': {error}",
                path.display()
            );
            init_stderr();
            tracing::error!(
                log_file = %path.display(),
                %error,
                "AQBot diagnostic file logging is unavailable; stderr logging remains enabled"
            );
        }
    }
}

pub fn path() -> PathBuf {
    log_path_from_value(env::var_os(LOG_FILE_ENV)).unwrap_or_else(default_log_path)
}

fn default_log_path() -> PathBuf {
    crate::paths::aqbot_home().join("logs").join("aqbot.log")
}

fn init_stderr() {
    if let Err(error) = tracing_subscriber::fmt()
        .with_env_filter(env_filter())
        .try_init()
    {
        eprintln!("failed to initialize AQBot stderr logging: {error}");
    }
}

fn env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

fn log_path_from_value(value: Option<OsString>) -> Option<PathBuf> {
    let value = value?;
    if value.to_string_lossy().trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

fn open_log_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

fn rotate_log_files(path: &Path, max_bytes: u64, file_count: usize) -> io::Result<()> {
    if file_count < 2 || fs::metadata(path).map(|value| value.len()).unwrap_or(0) < max_bytes {
        return Ok(());
    }

    for index in (1..file_count).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_path(path, index - 1)
        };
        if !source.exists() {
            continue;
        }
        let destination = rotated_path(path, index);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(source, destination)?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

#[cfg(test)]
mod tests {
    use super::{log_path_from_value, open_log_file, rotate_log_files};
    use std::ffi::OsString;
    use std::io::Write;

    #[test]
    fn ignores_missing_or_blank_log_file_env() {
        assert_eq!(log_path_from_value(None), None);
        assert_eq!(log_path_from_value(Some(OsString::from("   "))), None);
    }

    #[test]
    fn keeps_non_blank_log_file_override() {
        assert_eq!(
            log_path_from_value(Some(OsString::from("/tmp/aqbot.log"))),
            Some(std::path::PathBuf::from("/tmp/aqbot.log"))
        );
    }

    #[test]
    fn opens_log_file_and_creates_parent_directories() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let log_path = temp_dir.path().join("nested").join("aqbot.log");
        let mut file = open_log_file(&log_path).expect("open log file");
        writeln!(file, "first").expect("write log line");
        assert!(log_path.exists());
    }

    #[test]
    fn rotates_to_a_bounded_number_of_files() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let log_path = temp_dir.path().join("aqbot.log");
        std::fs::write(&log_path, "first").expect("write current log");
        std::fs::write(format!("{}.1", log_path.display()), "older").expect("write old log");

        rotate_log_files(&log_path, 1, 3).expect("rotate logs");

        assert!(!log_path.exists());
        assert_eq!(
            std::fs::read_to_string(format!("{}.1", log_path.display())).expect("first archive"),
            "first"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{}.2", log_path.display())).expect("second archive"),
            "older"
        );
    }
}
