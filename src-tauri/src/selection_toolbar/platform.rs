#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use tokio::sync::mpsc::UnboundedSender;

use super::{PermissionState, RuntimeError, ScreenPoint, SelectionObservation};

/// Why the platform requested closing the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DismissReason {
    /// The user pressed Escape: always close.
    Escape,
    /// The foreground application changed / was hidden / minimized. While the
    /// result panel is open this must NOT close the toolbar — only an outside
    /// click, Escape or the close button may.
    AppChanged,
}

#[derive(Debug)]
pub enum PlatformEvent {
    Selection(SelectionObservation),
    Clear,
    Dismiss(DismissReason),
    GlobalPointerDown(ScreenPoint),
    Error(RuntimeError),
}

pub struct PlatformMonitorHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl PlatformMonitorHandle {
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self {
            stop: Some(Box::new(stop)),
        }
    }

    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

impl Drop for PlatformMonitorHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

#[derive(Debug)]
pub struct PlatformStartError {
    pub permission: PermissionState,
    pub error: RuntimeError,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{open_permission_settings, permission_state, request_permission, start_monitor};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{open_permission_settings, permission_state, request_permission, start_monitor};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{open_permission_settings, permission_state, request_permission, start_monitor};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn start_monitor(
    _sender: UnboundedSender<PlatformEvent>,
) -> Result<PlatformMonitorHandle, PlatformStartError> {
    Err(PlatformStartError {
        permission: PermissionState::Unknown,
        error: RuntimeError {
            code: "unsupported_platform".into(),
            message: "Selection monitoring is not supported on this platform".into(),
        },
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn open_permission_settings() -> Result<super::PermissionSettingsOutcome, String> {
    Err("Selection monitoring is not supported on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn permission_state() -> PermissionState {
    PermissionState::Unknown
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn request_permission() -> Result<PermissionState, String> {
    Err("Selection monitoring is not supported on this platform".into())
}
