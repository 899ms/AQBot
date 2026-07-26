use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use axuielement::async_api::AXNotificationStream;
use axuielement::ax_attribute::{
    AX_BOUNDS_FOR_RANGE_PARAMETERIZED_ATTRIBUTE, AX_FOCUSED_UI_ELEMENT_ATTRIBUTE,
    AX_FOCUSED_WINDOW_ATTRIBUTE, AX_PARENT_ATTRIBUTE, AX_SELECTED_TEXT_ATTRIBUTE,
    AX_SELECTED_TEXT_RANGE_ATTRIBUTE, AX_TITLE_ATTRIBUTE,
};
use axuielement::ax_notification::{
    AX_FOCUSED_UI_ELEMENT_CHANGED_NOTIFICATION, AX_FOCUSED_WINDOW_CHANGED_NOTIFICATION,
    AX_SELECTED_TEXT_CHANGED_NOTIFICATION, AX_WINDOW_MINIATURIZED_NOTIFICATION,
};
use axuielement::{
    is_process_trusted, is_process_trusted_with_prompt, AXObserverEvent, AXRange,
    AXTextMarkerRange, AXUIElement, AXValue, SystemWideElement,
};
use block2::RcBlock;
use core_foundation::{base::TCFType, runloop::CFRunLoop};
use core_foundation_sys::mach_port::CFMachPortRef;
use core_foundation_sys::runloop::{kCFRunLoopCommonModes, CFRunLoopRef, CFRunLoopStop};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use core_graphics::geometry::CGPoint;
use objc2::{
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidDeactivateApplicationNotification,
    NSWorkspaceDidHideApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSString};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
};

use super::{DismissReason, PlatformEvent, PlatformMonitorHandle, PlatformStartError};
use crate::selection_toolbar::{
    PermissionSettingsOutcome, PermissionState, RuntimeError, ScreenPoint, ScreenRect,
    SelectionAnchorKind, SelectionObservation,
};

const MAX_SELECTION_ANCESTORS: usize = 16;
/// Chromium/WebKit publish the AX selection asynchronously after mouse-up — often
/// ~50ms, but heavy pages can take several hundred ms. Probe repeatedly with
/// backoff (cumulative 80/230/630ms) until a selection is readable; applications
/// with AX notifications still take their faster event-driven path, and the
/// controller drops re-announcements of the selection that is already live.
const SELECTION_PROBE_DELAYS_MS: [u64; 3] = [80, 150, 400];
const AX_SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE: &str = "AXSelectedTextMarkerRange";
const AX_STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str = "AXStringForTextMarkerRange";
const AX_TEXT_MARKER_RANGE_FOR_UNORDERED_TEXT_MARKERS_PARAMETERIZED_ATTRIBUTE: &str =
    "AXTextMarkerRangeForUnorderedTextMarkers";
const AX_NEXT_TEXT_MARKER_FOR_TEXT_MARKER_PARAMETERIZED_ATTRIBUTE: &str =
    "AXNextTextMarkerForTextMarker";
const AX_BOUNDS_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str = "AXBoundsForTextMarkerRange";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

#[derive(Debug, Clone)]
struct WorkspaceApplication {
    pid: i32,
    source_app: String,
    /// Regular activation policy (Dock app). Accessory/prohibited processes are
    /// transient overlays — the screenshot UI, Spotlight, launchers — whose
    /// activation and clicks must not dismiss the toolbar or steal the binding.
    is_regular: bool,
}

#[derive(Debug, Clone, Copy)]
struct LogicalPoint {
    x: f64,
    y: f64,
}

#[derive(Debug)]
enum MacSignal {
    ApplicationActivated(WorkspaceApplication),
    ApplicationDismissed(i32),
    SelectionProbeRequested(LogicalPoint),
    SelectionProbeReady { point: LogicalPoint, attempt: usize },
}

#[derive(Debug, Default)]
struct MonitorLifecycle {
    active_pid: Option<i32>,
    generation: u64,
}

impl MonitorLifecycle {
    fn activate(&mut self, pid: i32, own_pid: i32) -> Option<u64> {
        if pid == own_pid {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.active_pid = Some(pid);
        Some(self.generation)
    }

    fn dismiss(&mut self, pid: i32) -> bool {
        if self.active_pid != Some(pid) {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.active_pid = None;
        true
    }

    fn refresh(&mut self, pid: i32) -> Option<u64> {
        if self.active_pid != Some(pid) {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        Some(self.generation)
    }

    #[cfg(test)]
    fn accepts(&self, pid: i32, generation: u64) -> bool {
        self.active_pid == Some(pid) && self.generation == generation
    }
}

pub fn start_monitor(
    sender: UnboundedSender<PlatformEvent>,
) -> Result<PlatformMonitorHandle, PlatformStartError> {
    if !is_process_trusted() {
        return Err(PlatformStartError {
            permission: PermissionState::Denied,
            error: RuntimeError {
                code: "macos_accessibility_permission_required".into(),
                message: "macOS Accessibility permission is required".into(),
            },
        });
    }

    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (stop_tx, stop_rx) = oneshot::channel();
    let (mac_sender, mac_receiver) = tokio::sync::mpsc::unbounded_channel();
    // True while a non-regular (overlay) app — screenshot UI, Spotlight — is
    // frontmost; global mouse/Esc events then belong to the overlay.
    let overlay_active = Arc::new(AtomicBool::new(false));
    let ax_sender = sender.clone();
    let ax_mac_sender = mac_sender.clone();
    let ax_overlay = Arc::clone(&overlay_active);
    let ax_thread = thread::Builder::new()
        .name("aqbot-selection-ax".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let result = runtime
                .map_err(|error| error.to_string())
                .and_then(|runtime| {
                    runtime.block_on(run_monitor(
                        ax_sender,
                        ax_mac_sender,
                        mac_receiver,
                        stop_rx,
                        ready_tx,
                        ax_overlay,
                    ));
                    Ok(())
                });
            if let Err(error) = result {
                tracing::error!(%error, "macOS selection monitor stopped");
            }
        })
        .map_err(|error| PlatformStartError {
            permission: PermissionState::Granted,
            error: RuntimeError {
                code: "macos_monitor_thread_failed".into(),
                message: error.to_string(),
            },
        })?;

    if let Err(error) = ready_rx
        .recv()
        .map_err(|error| PlatformStartError {
            permission: PermissionState::Granted,
            error: RuntimeError {
                code: "macos_monitor_start_failed".into(),
                message: error.to_string(),
            },
        })
        .and_then(|result| result)
    {
        let _ = stop_tx.send(());
        let _ = ax_thread.join();
        return Err(error);
    }

    let (global_stop, global_thread) =
        match start_global_dismiss_listener(sender, mac_sender, overlay_active) {
            Ok(listener) => listener,
        Err(error) => {
            let _ = stop_tx.send(());
            let _ = ax_thread.join();
            return Err(error);
        }
    };

    Ok(PlatformMonitorHandle::new(move || {
        let _ = stop_tx.send(());
        global_stop();
        let _ = ax_thread.join();
        let _ = global_thread.join();
    }))
}

pub fn permission_state() -> PermissionState {
    if is_process_trusted() {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

fn start_global_dismiss_listener(
    sender: UnboundedSender<PlatformEvent>,
    mac_sender: UnboundedSender<MacSignal>,
    overlay_active: Arc<AtomicBool>,
) -> Result<(impl FnOnce() + Send + 'static, thread::JoinHandle<()>), PlatformStartError> {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("aqbot-selection-global-events".into())
        .spawn(move || {
            let event_sender = sender;
            let event_tap_ref = Arc::new(AtomicUsize::new(0));
            let callback_event_tap_ref = Arc::clone(&event_tap_ref);
            let event_tap = match CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![
                    CGEventType::KeyDown,
                    CGEventType::LeftMouseDown,
                    CGEventType::LeftMouseUp,
                    CGEventType::RightMouseDown,
                    CGEventType::OtherMouseDown,
                ],
                move |_, event_type, event| {
                    if let Some(reason) = event_tap_disable_reason(event_type) {
                        let tap_ref = callback_event_tap_ref.load(Ordering::Acquire);
                        if tap_ref == 0 {
                            tracing::error!(
                                reason,
                                "macOS global event tap was disabled before initialization"
                            );
                        } else {
                            // SAFETY: The pointer belongs to the live CGEventTap retained by this
                            // event thread and is only used while its run loop callback is active.
                            unsafe {
                                CGEventTapEnable(tap_ref as CFMachPortRef, true);
                            }
                            tracing::warn!(
                                reason,
                                "macOS global event tap was disabled and re-enabled"
                            );
                        }
                        return CallbackResult::Keep;
                    }
                    // While a screenshot/launcher overlay is frontmost, its
                    // clicks and Esc belong to the overlay — not to us.
                    if overlay_active.load(Ordering::Relaxed) {
                        return CallbackResult::Keep;
                    }
                    if matches!(event_type, CGEventType::KeyDown)
                        && event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) == 53
                    {
                        let _ = event_sender.send(PlatformEvent::Dismiss(DismissReason::Escape));
                    } else if matches!(event_type, CGEventType::LeftMouseUp) {
                        let location = event.location();
                        let _ = mac_sender.send(MacSignal::SelectionProbeRequested(LogicalPoint {
                            x: location.x,
                            y: location.y,
                        }));
                    } else if matches!(
                        event_type,
                        CGEventType::LeftMouseDown
                            | CGEventType::RightMouseDown
                            | CGEventType::OtherMouseDown
                    ) {
                        let location = event.location();
                        let _ = event_sender.send(PlatformEvent::GlobalPointerDown(
                            screen_point_from_cg(location),
                        ));
                    }
                    CallbackResult::Keep
                },
            ) {
                Ok(event_tap) => event_tap,
                Err(()) => {
                    let _ = ready_sender.send(Err(
                        "Could not create the macOS read-only global event tap".to_string(),
                    ));
                    return;
                }
            };
            event_tap_ref.store(
                event_tap.mach_port().as_concrete_TypeRef() as usize,
                Ordering::Release,
            );
            let source = match event_tap.mach_port().create_runloop_source(0) {
                Ok(source) => source,
                Err(()) => {
                    let _ = ready_sender.send(Err(
                        "Could not create the macOS global event run loop source".to_string(),
                    ));
                    return;
                }
            };
            let run_loop = CFRunLoop::get_current();
            run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
            event_tap.enable();
            let run_loop_ref = run_loop.as_concrete_TypeRef() as usize;
            let _ = ready_sender.send(Ok(run_loop_ref));
            CFRunLoop::run_current();
        })
        .map_err(|error| start_error("macos_global_event_thread_failed", &error.to_string()))?;
    let run_loop_ref = ready_receiver
        .recv()
        .map_err(|error| start_error("macos_global_event_start_failed", &error.to_string()))?
        .map_err(|message| start_error("macos_global_event_unavailable", &message))?;
    let stop = move || unsafe {
        CFRunLoopStop(run_loop_ref as CFRunLoopRef);
    };
    Ok((stop, thread))
}

fn event_tap_disable_reason(event_type: CGEventType) -> Option<&'static str> {
    match event_type {
        CGEventType::TapDisabledByTimeout => Some("timeout"),
        CGEventType::TapDisabledByUserInput => Some("user_input"),
        _ => None,
    }
}

type WorkspaceObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

struct WorkspaceObserver {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<WorkspaceObserverToken>,
}

#[derive(Debug, Clone, Copy)]
enum WorkspaceEventKind {
    Activated,
    Deactivated,
    Dismissed,
}

impl WorkspaceObserver {
    fn new(
        sender: UnboundedSender<MacSignal>,
        overlay_active: Arc<AtomicBool>,
    ) -> (Self, Option<WorkspaceApplication>) {
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let tokens = vec![
            add_workspace_observer(
                &center,
                unsafe { NSWorkspaceDidActivateApplicationNotification },
                WorkspaceEventKind::Activated,
                sender.clone(),
                Some(Arc::clone(&overlay_active)),
            ),
            add_workspace_observer(
                &center,
                unsafe { NSWorkspaceDidDeactivateApplicationNotification },
                WorkspaceEventKind::Deactivated,
                sender.clone(),
                None,
            ),
            add_workspace_observer(
                &center,
                unsafe { NSWorkspaceDidHideApplicationNotification },
                WorkspaceEventKind::Dismissed,
                sender.clone(),
                None,
            ),
            add_workspace_observer(
                &center,
                unsafe { NSWorkspaceDidTerminateApplicationNotification },
                WorkspaceEventKind::Dismissed,
                sender,
                None,
            ),
        ];
        let initial = workspace
            .frontmostApplication()
            .as_deref()
            .and_then(workspace_application);
        if let Some(initial) = initial.as_ref() {
            overlay_active.store(!initial.is_regular, Ordering::Relaxed);
        }
        (Self { center, tokens }, initial)
    }
}

impl Drop for WorkspaceObserver {
    fn drop(&mut self) {
        for token in &self.tokens {
            // SAFETY: Every token was returned by this notification center and remains valid.
            unsafe {
                let protocol: &ProtocolObject<dyn NSObjectProtocol> = token;
                let observer: &AnyObject = protocol.as_ref();
                self.center.removeObserver(observer);
            }
        }
    }
}

fn add_workspace_observer(
    center: &NSNotificationCenter,
    name: &NSString,
    kind: WorkspaceEventKind,
    sender: UnboundedSender<MacSignal>,
    overlay_active: Option<Arc<AtomicBool>>,
) -> WorkspaceObserverToken {
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        let notification = unsafe { notification.as_ref() };
        let Some(application) = workspace_application_from_notification(notification) else {
            return;
        };
        if let Some(overlay) = overlay_active.as_ref() {
            overlay.store(!application.is_regular, Ordering::Relaxed);
        }
        if let Some(signal) = workspace_signal(kind, application) {
            let _ = sender.send(signal);
        }
    });
    // SAFETY: The block is sendable, notification names are static, and the returned token is
    // retained until it is explicitly removed by WorkspaceObserver::drop.
    unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
}

fn workspace_signal(
    kind: WorkspaceEventKind,
    application: WorkspaceApplication,
) -> Option<MacSignal> {
    match kind {
        // Overlay processes (screenshot UI, Spotlight, …) come and go without
        // meaning an app switch — never dismiss or rebind for them.
        WorkspaceEventKind::Activated if !application.is_regular => None,
        WorkspaceEventKind::Activated => Some(MacSignal::ApplicationActivated(application)),
        // AQBot's panel may transiently activate the process. The paired source-app
        // deactivation does not mean its AX element is gone; hide/terminate events do.
        WorkspaceEventKind::Deactivated => None,
        WorkspaceEventKind::Dismissed => Some(MacSignal::ApplicationDismissed(application.pid)),
    }
}

fn workspace_application_from_notification(
    notification: &NSNotification,
) -> Option<WorkspaceApplication> {
    let user_info = notification.userInfo()?;
    let user_info = unsafe { user_info.cast_unchecked::<NSString, AnyObject>() };
    let application = user_info
        .objectForKey(unsafe { NSWorkspaceApplicationKey })?
        .downcast::<NSRunningApplication>()
        .ok()?;
    workspace_application(&application)
}

fn workspace_application(application: &NSRunningApplication) -> Option<WorkspaceApplication> {
    let pid = application.processIdentifier();
    if pid <= 0 {
        return None;
    }
    let source_app = application
        .bundleIdentifier()
        .or_else(|| application.localizedName())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("pid:{pid}"));
    Some(WorkspaceApplication {
        pid,
        source_app,
        is_regular: application.activationPolicy() == NSApplicationActivationPolicy::Regular,
    })
}

fn workspace_application_for_pid(pid: i32) -> Option<WorkspaceApplication> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .as_deref()
        .and_then(workspace_application)
}

async fn run_monitor(
    sender: UnboundedSender<PlatformEvent>,
    mac_sender: UnboundedSender<MacSignal>,
    mut mac_receiver: UnboundedReceiver<MacSignal>,
    mut stop_rx: oneshot::Receiver<()>,
    ready: mpsc::SyncSender<Result<(), PlatformStartError>>,
    overlay_active: Arc<AtomicBool>,
) {
    let system = match SystemWideElement::new() {
        Some(system) => system,
        None => {
            let _ = ready.send(Err(start_error(
                "macos_system_element_unavailable",
                "Could not create the macOS system accessibility element",
            )));
            return;
        }
    };
    let (workspace_observer, initial_application) =
        WorkspaceObserver::new(mac_sender.clone(), overlay_active);
    let _workspace_observer = workspace_observer;
    let own_pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    let mut lifecycle = MonitorLifecycle::default();
    let mut active = initial_application
        .and_then(|application| bind_application(application, own_pid, &mut lifecycle));

    let _ = ready.send(Ok(()));
    if let Some(active) = active.as_ref() {
        emit_current_selection(active, &sender);
    }

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            signal = mac_receiver.recv() => {
                let Some(signal) = signal else {
                    break;
                };
                if !is_process_trusted() {
                    let _ = sender.send(PlatformEvent::Error(RuntimeError {
                        code: "macos_accessibility_permission_revoked".into(),
                        message: "macOS Accessibility permission was revoked while monitoring".into(),
                    }));
                    break;
                }
                handle_mac_signal(
                    signal,
                    &system,
                    own_pid,
                    &sender,
                    &mac_sender,
                    &mut lifecycle,
                    &mut active,
                );
            }
            event = wait_notification(active.as_ref().and_then(|value| value.subscriptions.focused_element.as_ref())) => {
                if event.is_some() {
                    refresh_active_application(&mut active, &mut lifecycle);
                    if let Some(active) = active.as_ref() {
                        emit_current_selection(active, &sender);
                    }
                } else if let Some(active) = active.as_mut() {
                    active.subscriptions.focused_element = None;
                }
            }
            event = wait_notification(active.as_ref().and_then(|value| value.subscriptions.focused_window.as_ref())) => {
                if event.is_some() {
                    refresh_active_application(&mut active, &mut lifecycle);
                    if let Some(active) = active.as_ref() {
                        emit_current_selection(active, &sender);
                    }
                } else if let Some(active) = active.as_mut() {
                    active.subscriptions.focused_window = None;
                }
            }
            event = wait_notification(active.as_ref().and_then(|value| value.subscriptions.window_minimized.as_ref())) => {
                if event.is_some() {
                    let _ = sender.send(PlatformEvent::Dismiss(DismissReason::AppChanged));
                    refresh_active_application(&mut active, &mut lifecycle);
                } else if let Some(active) = active.as_mut() {
                    active.subscriptions.window_minimized = None;
                }
            }
            event = wait_notification(active.as_ref().and_then(|value| value.subscriptions.app_selection.as_ref())) => {
                if let Some(event) = event {
                    emit_event_selection(active.as_ref(), &event, &sender);
                } else if let Some(active) = active.as_mut() {
                    active.subscriptions.app_selection = None;
                }
            }
            event = wait_notification(active.as_ref().and_then(|value| value.subscriptions.element_selection.as_ref())) => {
                if let Some(event) = event {
                    emit_event_selection(active.as_ref(), &event, &sender);
                } else if let Some(active) = active.as_mut() {
                    active.subscriptions.element_selection = None;
                }
            }
        }
    }
}

fn handle_mac_signal(
    signal: MacSignal,
    system: &SystemWideElement,
    own_pid: i32,
    sender: &UnboundedSender<PlatformEvent>,
    mac_sender: &UnboundedSender<MacSignal>,
    lifecycle: &mut MonitorLifecycle,
    active: &mut Option<ActiveApplication>,
) {
    match signal {
        MacSignal::ApplicationActivated(application) => {
            tracing::debug!(
                pid = application.pid,
                source_app = %application.source_app,
                "macOS foreground application activated"
            );
            // Showing or clicking the non-activating toolbar must not dismiss the session.
            // Keep the previous external-app AX subscription until a real third-party app
            // becomes frontmost (or the source app is dismissed).
            if application.pid == own_pid {
                tracing::debug!("Ignoring own-application activation for selection toolbar");
                return;
            }
            let _ = sender.send(PlatformEvent::Dismiss(DismissReason::AppChanged));
            *active = bind_application(application, own_pid, lifecycle);
            if let Some(active) = active.as_ref() {
                emit_current_selection(active, sender);
            }
        }
        MacSignal::ApplicationDismissed(pid) => {
            let dismissed = lifecycle.dismiss(pid);
            if dismissed {
                *active = None;
                let _ = sender.send(PlatformEvent::Dismiss(DismissReason::AppChanged));
            }
        }
        MacSignal::SelectionProbeRequested(point) => {
            tracing::debug!(
                pid = active.as_ref().map(|active| active.info.pid),
                point_x = point.x,
                point_y = point.y,
                "Scheduling macOS mouse selection probe"
            );
            schedule_selection_probe(mac_sender, point, 0);
        }
        MacSignal::SelectionProbeReady { point, attempt } => {
            probe_selection(system, active, lifecycle, own_pid, point, attempt, sender, mac_sender);
        }
    }
}

fn schedule_selection_probe(sender: &UnboundedSender<MacSignal>, point: LogicalPoint, attempt: usize) {
    let Some(delay_ms) = SELECTION_PROBE_DELAYS_MS.get(attempt).copied() else {
        return;
    };
    let delayed_sender = sender.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        let _ = delayed_sender.send(MacSignal::SelectionProbeReady { point, attempt });
    });
}

fn is_last_probe_attempt(attempt: usize) -> bool {
    attempt + 1 >= SELECTION_PROBE_DELAYS_MS.len()
}

struct ActiveApplication {
    info: WorkspaceApplication,
    element: AXUIElement,
    subscriptions: FocusedSubscriptions,
    generation: u64,
}

#[derive(Default)]
struct FocusedSubscriptions {
    focused_element: Option<AXNotificationStream>,
    focused_window: Option<AXNotificationStream>,
    window_minimized: Option<AXNotificationStream>,
    app_selection: Option<AXNotificationStream>,
    element_selection: Option<AXNotificationStream>,
}

/// Bundle-id prefixes of Chromium-family browsers that predate the
/// `AXManualAccessibility` switch or still honour the VoiceOver-era flag.
/// `AXEnhancedUserInterface` has window-animation side effects in unrelated
/// apps, so it is only asserted for this allowlist.
const ENHANCED_UI_BUNDLE_PREFIXES: &[&str] = &[
    "com.google.Chrome",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "org.chromium",
    "com.vivaldi",
    "com.operasoftware",
    "company.thebrowser.Browser",
    "ru.yandex.desktop.yandex-browser",
];

/// Chromium builds its accessibility tree lazily and only for detected
/// assistive clients, so `AXSelectedText` reads return nothing until the tree
/// is switched on. `AXManualAccessibility` (Chromium ≥ 90 / Electron) is safe
/// to assert on every app — non-Chromium targets answer AttributeUnsupported.
fn enable_browser_accessibility(application: &AXUIElement, source_app: &str) {
    if let Err(error) = application.set_bool_attribute("AXManualAccessibility", true) {
        tracing::trace!(source_app, %error, "AXManualAccessibility is unavailable");
    }
    if ENHANCED_UI_BUNDLE_PREFIXES
        .iter()
        .any(|prefix| source_app.starts_with(prefix))
    {
        if let Err(error) = application.set_bool_attribute("AXEnhancedUserInterface", true) {
            tracing::trace!(source_app, %error, "AXEnhancedUserInterface is unavailable");
        }
    }
}

fn bind_application(
    application: WorkspaceApplication,
    own_pid: i32,
    lifecycle: &mut MonitorLifecycle,
) -> Option<ActiveApplication> {
    let generation = lifecycle.activate(application.pid, own_pid)?;
    let Some(element) = AXUIElement::from_pid(application.pid) else {
        lifecycle.dismiss(application.pid);
        tracing::debug!(
            pid = application.pid,
            "Could not create an accessibility element for the active macOS application"
        );
        return None;
    };
    // Flip on Chromium's lazily-built AX tree before subscribing, so the
    // selection attributes exist by the time the user selects text.
    enable_browser_accessibility(&element, &application.source_app);
    let subscriptions = subscribe_focused(&element);
    tracing::debug!(
        pid = application.pid,
        generation,
        "Bound macOS accessibility subscriptions to foreground application"
    );
    Some(ActiveApplication {
        info: application,
        element,
        subscriptions,
        generation,
    })
}

fn subscribe_focused(application: &AXUIElement) -> FocusedSubscriptions {
    let element = application
        .element_attribute(AX_FOCUSED_UI_ELEMENT_ATTRIBUTE)
        .ok()
        .flatten();
    let window = application
        .element_attribute(AX_FOCUSED_WINDOW_ATTRIBUTE)
        .ok()
        .flatten();
    FocusedSubscriptions {
        focused_element: subscribe_optional(
            Some(application),
            AX_FOCUSED_UI_ELEMENT_CHANGED_NOTIFICATION,
        ),
        focused_window: subscribe_optional(
            Some(application),
            AX_FOCUSED_WINDOW_CHANGED_NOTIFICATION,
        ),
        window_minimized: subscribe_optional(window.as_ref(), AX_WINDOW_MINIATURIZED_NOTIFICATION),
        app_selection: subscribe_optional(Some(application), AX_SELECTED_TEXT_CHANGED_NOTIFICATION),
        element_selection: subscribe_optional(
            element.as_ref(),
            AX_SELECTED_TEXT_CHANGED_NOTIFICATION,
        ),
    }
}

fn subscribe_optional(
    element: Option<&AXUIElement>,
    notification: &str,
) -> Option<AXNotificationStream> {
    let element = element?;
    AXNotificationStream::subscribe_many(element, &[notification], 32)
        .map_err(|error| {
            tracing::debug!(notification, %error, "macOS AX notification is unavailable");
        })
        .ok()
}

fn refresh_active_application(
    active: &mut Option<ActiveApplication>,
    lifecycle: &mut MonitorLifecycle,
) {
    if let Some(active) = active.as_mut() {
        active.subscriptions = subscribe_focused(&active.element);
        if let Some(generation) = lifecycle.refresh(active.info.pid) {
            active.generation = generation;
        }
    }
}

async fn wait_notification(stream: Option<&AXNotificationStream>) -> Option<AXObserverEvent> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

fn emit_current_selection(active: &ActiveApplication, sender: &UnboundedSender<PlatformEvent>) {
    match active
        .element
        .element_attribute(AX_FOCUSED_UI_ELEMENT_ATTRIBUTE)
    {
        Ok(Some(element)) => {
            emit_selection_from_candidates_with_pointer(active, [element], sender, None, true);
        }
        Ok(None) => {
            let _ = sender.send(PlatformEvent::Clear);
        }
        Err(error) => {
            tracing::debug!(
                pid = active.info.pid,
                %error,
                "Could not read the focused macOS accessibility element"
            );
        }
    }
}

fn emit_event_selection(
    active: Option<&ActiveApplication>,
    event: &AXObserverEvent,
    sender: &UnboundedSender<PlatformEvent>,
) {
    let Some(active) = active else {
        return;
    };
    let focused = active
        .element
        .element_attribute(AX_FOCUSED_UI_ELEMENT_ATTRIBUTE)
        .ok()
        .flatten();
    // Chromium may deliver the notification from a renderer/XPC element while
    // exposing the actual selection only on the browser application's focused
    // element. Resolve both candidates before deciding that the selection cleared.
    emit_selection_from_candidates_with_pointer(
        active,
        std::iter::once(event.element.clone()).chain(focused),
        sender,
        None,
        true,
    );
}

#[allow(clippy::too_many_arguments)]
fn probe_selection(
    system: &SystemWideElement,
    active: &mut Option<ActiveApplication>,
    lifecycle: &mut MonitorLifecycle,
    own_pid: i32,
    point: LogicalPoint,
    attempt: usize,
    sender: &UnboundedSender<PlatformEvent>,
    mac_sender: &UnboundedSender<MacSignal>,
) {
    match system.element_at_position(point.x as f32, point.y as f32) {
        Ok(Some(element)) => {
            let hit_pid = element.pid().ok();
            match selection_probe_action(
                active.as_ref().map(|value| value.info.pid),
                hit_pid,
                own_pid,
            ) {
                SelectionProbeAction::Ignore => {
                    tracing::debug!(
                        hit_pid,
                        own = hit_pid == Some(own_pid),
                        source_pid = active.as_ref().map(|value| value.info.pid),
                        "Ignoring macOS selection probe for AQBot or an invalid element"
                    );
                    return;
                }
                SelectionProbeAction::Reuse => {}
                SelectionProbeAction::Rebind(pid) => {
                    let Some(application) = workspace_application_for_pid(pid) else {
                        tracing::error!(
                            pid,
                            "Could not resolve the macOS application hit by selection probe"
                        );
                        return;
                    };
                    if !application.is_regular {
                        // Never steal the binding for overlay processes
                        // (screenshot UI, Spotlight, input methods).
                        tracing::debug!(
                            pid,
                            source_app = %application.source_app,
                            "Ignoring macOS selection probe over an overlay application"
                        );
                        return;
                    }
                    tracing::debug!(
                        previous_pid = active.as_ref().map(|value| value.info.pid),
                        pid,
                        "Rebinding macOS accessibility subscriptions to mouse-hit application"
                    );
                    *active = bind_application(application, own_pid, lifecycle);
                }
            }
            let Some(active) = active.as_ref() else {
                tracing::error!(
                    hit_pid,
                    "macOS selection probe has no bound source application"
                );
                return;
            };
            tracing::debug!(
                pid = active.info.pid,
                "Reading macOS selection from mouse hit-test element"
            );
            // Prefer the mouse-up point so the toolbar appears near the user's hand,
            // not at the first glyph of a long selection (TextGO / pot pattern).
            let focused = active
                .element
                .element_attribute(AX_FOCUSED_UI_ELEMENT_ATTRIBUTE)
                .ok()
                .flatten();
            let found = emit_selection_from_candidates_with_pointer(
                active,
                std::iter::once(element).chain(focused),
                sender,
                Some(ScreenPoint {
                    x: point.x,
                    y: point.y,
                }),
                // Chromium/WebKit may still be propagating the selection; only the
                // final failed attempt is allowed to mean "deselected".
                is_last_probe_attempt(attempt),
            );
            if !found && !is_last_probe_attempt(attempt) {
                tracing::debug!(
                    pid = active.info.pid,
                    attempt,
                    "macOS selection probe found no selection yet; retrying"
                );
                schedule_selection_probe(mac_sender, point, attempt + 1);
            }
        }
        Ok(None) => {
            // Do not Clear: empty hit-tests include toolbar clicks and UI chrome.
            // Real deselection is delivered via AX SelectedTextChanged / outside click.
            tracing::debug!(
                pid = active.as_ref().map(|value| value.info.pid),
                "Ignoring macOS selection probe with no hit-test element"
            );
            schedule_selection_probe(mac_sender, point, attempt + 1);
        }
        Err(error) => {
            tracing::debug!(
                pid = active.as_ref().map(|value| value.info.pid),
                %error,
                "Could not hit-test the macOS selection endpoint"
            );
            schedule_selection_probe(mac_sender, point, attempt + 1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionProbeAction {
    Ignore,
    Reuse,
    Rebind(i32),
}

fn selection_probe_action(
    active_pid: Option<i32>,
    hit_pid: Option<i32>,
    own_pid: i32,
) -> SelectionProbeAction {
    match hit_pid {
        Some(pid) if pid == own_pid => SelectionProbeAction::Ignore,
        Some(pid) if active_pid == Some(pid) => SelectionProbeAction::Reuse,
        Some(pid) => SelectionProbeAction::Rebind(pid),
        None => SelectionProbeAction::Ignore,
    }
}

fn emit_selection_from_candidates_with_pointer(
    active: &ActiveApplication,
    candidates: impl IntoIterator<Item = AXUIElement>,
    sender: &UnboundedSender<PlatformEvent>,
    pointer: Option<ScreenPoint>,
    clear_on_empty: bool,
) -> bool {
    let payload = first_value_in_candidate_chains(
        candidates,
        MAX_SELECTION_ANCESTORS,
        read_selection_payload,
        |candidate| {
            candidate
                .element_attribute(AX_PARENT_ATTRIBUTE)
                .ok()
                .flatten()
        },
    );
    match payload {
        Some(mut payload) => {
            let pointer_anchored = pointer.is_some();
            if let Some(pointer) = pointer {
                // Keep a small rect at the release point so place_surface still centers
                // and flips above/below correctly.
                payload.anchor = ScreenRect {
                    x: pointer.x,
                    y: pointer.y,
                    width: 1.0,
                    height: 1.0,
                };
                payload.anchor_kind = SelectionAnchorKind::Pointer;
            }
            tracing::debug!(
                pid = active.info.pid,
                text_len = payload.text.chars().count(),
                pointer_anchored,
                "macOS accessibility selection read succeeded"
            );
            let observation = selection_observation(active, payload);
            let _ = sender.send(PlatformEvent::Selection(observation));
            true
        }
        None => {
            tracing::debug!(
                pid = active.info.pid,
                "macOS accessibility element did not expose a selection"
            );
            // AX notification path may legitimately clear; the mouse probe only
            // clears on its final attempt when the hit element is the source app,
            // so a persistent empty selection means deselect.
            if clear_on_empty {
                let _ = sender.send(PlatformEvent::Clear);
            }
            false
        }
    }
}

fn first_value_in_ancestor_chain<T, U>(
    mut current: T,
    max_depth: usize,
    mut read: impl FnMut(&T) -> Option<U>,
    mut parent: impl FnMut(&T) -> Option<T>,
) -> Option<U> {
    for _ in 0..max_depth {
        if let Some(value) = read(&current) {
            return Some(value);
        }
        current = parent(&current)?;
    }
    None
}

fn first_value_in_candidate_chains<T, U>(
    candidates: impl IntoIterator<Item = T>,
    max_depth: usize,
    mut read: impl FnMut(&T) -> Option<U>,
    mut parent: impl FnMut(&T) -> Option<T>,
) -> Option<U> {
    for candidate in candidates {
        if let Some(value) = first_value_in_ancestor_chain(
            candidate,
            max_depth,
            |current| read(current),
            |current| parent(current),
        ) {
            return Some(value);
        }
    }
    None
}

struct SelectionPayload {
    text: String,
    range_signature: String,
    anchor: ScreenRect,
    anchor_kind: SelectionAnchorKind,
}

fn read_selection_payload(element: &AXUIElement) -> Option<SelectionPayload> {
    read_range_selection(element).or_else(|| read_marker_selection(element))
}

fn read_range_selection(element: &AXUIElement) -> Option<SelectionPayload> {
    let text = read_string_attribute(element, AX_SELECTED_TEXT_ATTRIBUTE)?;
    if text.trim().is_empty() {
        return None;
    }
    let range = match element.range_attribute(AX_SELECTED_TEXT_RANGE_ATTRIBUTE) {
        Ok(range) => range?,
        Err(error) => {
            trace_ax_read_error(element, AX_SELECTED_TEXT_RANGE_ATTRIBUTE, &error);
            return None;
        }
    };
    if range.length <= 0 {
        return None;
    }
    let first_character = AXValue::from_range(first_character_range(range)?)?;
    let rect = match element.parameterized_attribute(
        AX_BOUNDS_FOR_RANGE_PARAMETERIZED_ATTRIBUTE,
        &first_character,
    ) {
        Ok(value) => usable_selection_rect(value?.as_rect()?)?,
        Err(error) => {
            trace_ax_read_error(element, AX_BOUNDS_FOR_RANGE_PARAMETERIZED_ATTRIBUTE, &error);
            return None;
        }
    };
    Some(SelectionPayload {
        text,
        range_signature: format!("range:{}:{}", range.location, range.length),
        anchor: ScreenRect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        },
        anchor_kind: SelectionAnchorKind::SelectionRect,
    })
}

fn first_character_range(range: AXRange) -> Option<AXRange> {
    (range.length > 0).then_some(AXRange {
        location: range.location,
        length: 1,
    })
}

fn read_marker_selection(element: &AXUIElement) -> Option<SelectionPayload> {
    let selected_range =
        match element.text_marker_range_attribute(AX_SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE) {
            Ok(range) => range?,
            Err(error) => {
                trace_ax_read_error(element, AX_SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE, &error);
                return None;
            }
        };
    let selected_range = ordered_marker_range(element, &selected_range).unwrap_or(selected_range);
    let selected_value = AXValue::from_text_marker_range(&selected_range)?;
    let text = match element.parameterized_attribute(
        AX_STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        &selected_value,
    ) {
        Ok(value) => value?.as_string()?,
        Err(error) => {
            trace_ax_read_error(
                element,
                AX_STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
                &error,
            );
            return None;
        }
    };
    if text.trim().is_empty() {
        return None;
    }
    let rect = first_marker_rect(element, &selected_range)
        .or_else(|| marker_range_rect(element, &selected_range))
        .and_then(usable_selection_rect)?;
    Some(SelectionPayload {
        text,
        range_signature: marker_range_signature(&selected_range),
        anchor: ScreenRect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        },
        anchor_kind: SelectionAnchorKind::SelectionRect,
    })
}

fn ordered_marker_range(
    element: &AXUIElement,
    range: &AXTextMarkerRange,
) -> Option<AXTextMarkerRange> {
    let start = AXValue::from_text_marker(&range.start_marker())?;
    let end = AXValue::from_text_marker(&range.end_marker())?;
    let markers = AXValue::from_array(&[&start, &end])?;
    element
        .parameterized_attribute(
            AX_TEXT_MARKER_RANGE_FOR_UNORDERED_TEXT_MARKERS_PARAMETERIZED_ATTRIBUTE,
            &markers,
        )
        .ok()
        .flatten()
        .and_then(|value| value.as_text_marker_range())
}

fn first_marker_rect(
    element: &AXUIElement,
    range: &AXTextMarkerRange,
) -> Option<axuielement::AXRect> {
    let start = range.start_marker();
    let start_value = AXValue::from_text_marker(&start)?;
    let next = element
        .parameterized_attribute(
            AX_NEXT_TEXT_MARKER_FOR_TEXT_MARKER_PARAMETERIZED_ATTRIBUTE,
            &start_value,
        )
        .ok()
        .flatten()
        .and_then(|value| value.as_text_marker())?;
    let first_range = AXTextMarkerRange::new(&start, &next)?;
    marker_range_rect(element, &first_range)
}

fn marker_range_rect(
    element: &AXUIElement,
    range: &AXTextMarkerRange,
) -> Option<axuielement::AXRect> {
    let range = AXValue::from_text_marker_range(range)?;
    element
        .parameterized_attribute(
            AX_BOUNDS_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
            &range,
        )
        .ok()
        .flatten()
        .and_then(|value| value.as_rect())
}

fn usable_selection_rect(rect: axuielement::AXRect) -> Option<axuielement::AXRect> {
    (rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width > 0.0
        && rect.size.height > 0.0)
        .then_some(rect)
}

fn marker_range_signature(range: &AXTextMarkerRange) -> String {
    let mut hasher = DefaultHasher::new();
    range.start_marker().bytes().hash(&mut hasher);
    range.end_marker().bytes().hash(&mut hasher);
    format!("marker:{:016x}", hasher.finish())
}

fn read_string_attribute(element: &AXUIElement, attribute: &str) -> Option<String> {
    match element.string_attribute(attribute) {
        Ok(value) => value,
        Err(error) => {
            trace_ax_read_error(element, attribute, &error);
            None
        }
    }
}

fn trace_ax_read_error(element: &AXUIElement, attribute: &str, error: &axuielement::AXError) {
    tracing::debug!(
        pid = element.pid().unwrap_or_default(),
        attribute,
        %error,
        "macOS accessibility selection attribute is unavailable"
    );
}

fn selection_observation(
    active: &ActiveApplication,
    payload: SelectionPayload,
) -> SelectionObservation {
    let source_window = active
        .element
        .element_attribute(AX_FOCUSED_WINDOW_ATTRIBUTE)
        .ok()
        .flatten()
        .and_then(|window| window.string_attribute(AX_TITLE_ATTRIBUTE).ok().flatten())
        .unwrap_or_default();
    SelectionObservation {
        text: payload.text,
        source_app: active.info.source_app.clone(),
        source_window,
        range_signature: payload.range_signature,
        anchor: payload.anchor,
        anchor_kind: payload.anchor_kind,
    }
}

fn screen_point_from_cg(point: CGPoint) -> ScreenPoint {
    ScreenPoint {
        x: point.x,
        y: point.y,
    }
}

fn start_error(code: &str, message: &str) -> PlatformStartError {
    PlatformStartError {
        permission: PermissionState::Granted,
        error: RuntimeError {
            code: code.into(),
            message: message.into(),
        },
    }
}

const MODERN_ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";
const LEGACY_ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionAction {
    OpenPermissionPane,
    OpenForManualAdd,
}

fn permission_action(bundled_app: bool) -> PermissionAction {
    if bundled_app {
        PermissionAction::OpenPermissionPane
    } else {
        PermissionAction::OpenForManualAdd
    }
}

fn is_bundled_app_executable(executable: &Path) -> bool {
    executable.ancestors().any(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension == "app")
    })
}

fn open_accessibility_settings() -> Result<(), String> {
    open::that(MODERN_ACCESSIBILITY_SETTINGS_URL)
        .or_else(|_| open::that(LEGACY_ACCESSIBILITY_SETTINGS_URL))
        .map_err(|error| error.to_string())
}

pub fn open_permission_settings() -> Result<PermissionSettingsOutcome, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    open_accessibility_settings()?;
    match permission_action(is_bundled_app_executable(&executable)) {
        PermissionAction::OpenForManualAdd => Ok(PermissionSettingsOutcome::ManualAddRequired {
            executable_path: executable.to_string_lossy().into_owned(),
        }),
        PermissionAction::OpenPermissionPane => Ok(PermissionSettingsOutcome::PermissionPaneOpened),
    }
}

pub fn request_permission() -> Result<PermissionState, String> {
    let _ = is_process_trusted_with_prompt();
    Ok(permission_state())
}

#[cfg(test)]
mod macos_tests {
    use std::path::Path;
    use std::time::Duration;

    use super::{
        event_tap_disable_reason, first_character_range, first_value_in_ancestor_chain,
        first_value_in_candidate_chains, is_bundled_app_executable, marker_range_signature,
        permission_action, screen_point_from_cg, selection_probe_action, usable_selection_rect,
        workspace_signal, MacSignal, MonitorLifecycle, PermissionAction, SelectionProbeAction,
        WorkspaceApplication, WorkspaceEventKind,
    };
    use axuielement::{AXPoint, AXRange, AXRect, AXSize, AXTextMarkerRange};
    use core_graphics::event::CGEventType;
    use core_graphics::geometry::CGPoint;

    #[test]
    fn macos_event_points_stay_in_tauri_logical_coordinates() {
        assert_eq!(
            screen_point_from_cg(CGPoint::new(1058.5, 598.25)),
            crate::selection_toolbar::ScreenPoint {
                x: 1058.5,
                y: 598.25,
            }
        );
    }

    #[test]
    fn monitor_lifecycle_starts_without_an_active_application() {
        let mut lifecycle = MonitorLifecycle::default();

        assert_eq!(lifecycle.active_pid, None);
        let generation = lifecycle.activate(42, 7).expect("external app binds");

        assert!(lifecycle.accepts(42, generation));
    }

    #[test]
    fn monitor_lifecycle_ignores_self_and_stale_application_events() {
        let mut lifecycle = MonitorLifecycle::default();

        assert_eq!(lifecycle.activate(7, 7), None);
        let old_generation = lifecycle.activate(42, 7).expect("first app binds");
        let current_generation = lifecycle.activate(84, 7).expect("second app binds");

        assert!(!lifecycle.dismiss(42));
        assert!(!lifecycle.accepts(42, old_generation));
        assert!(lifecycle.accepts(84, current_generation));
    }

    #[test]
    fn own_application_activation_keeps_external_subscription() {
        let mut lifecycle = MonitorLifecycle::default();
        let generation = lifecycle.activate(42, 7).expect("external app binds");

        assert_eq!(lifecycle.activate(7, 7), None);
        assert!(lifecycle.accepts(42, generation));
    }

    #[test]
    fn mouse_hit_rebinds_a_stale_external_application() {
        assert_eq!(
            selection_probe_action(Some(42), Some(84), 7),
            SelectionProbeAction::Rebind(84)
        );
        assert_eq!(
            selection_probe_action(Some(42), Some(7), 7),
            SelectionProbeAction::Ignore
        );
    }

    #[test]
    fn source_app_deactivation_does_not_clear_selection_subscription() {
        let application = WorkspaceApplication {
            pid: 42,
            source_app: "TextEdit".into(),
            is_regular: true,
        };

        assert!(workspace_signal(WorkspaceEventKind::Deactivated, application).is_none());
    }

    #[test]
    fn overlay_application_activation_is_ignored() {
        let screenshot_ui = WorkspaceApplication {
            pid: 77,
            source_app: "com.apple.screencaptureui".into(),
            is_regular: false,
        };
        let browser = WorkspaceApplication {
            pid: 78,
            source_app: "com.google.Chrome".into(),
            is_regular: true,
        };

        assert!(workspace_signal(WorkspaceEventKind::Activated, screenshot_ui).is_none());
        assert!(matches!(
            workspace_signal(WorkspaceEventKind::Activated, browser),
            Some(MacSignal::ApplicationActivated(_))
        ));
    }

    #[test]
    fn disabled_global_event_tap_requests_reenable() {
        assert_eq!(
            event_tap_disable_reason(CGEventType::TapDisabledByTimeout),
            Some("timeout")
        );
        assert_eq!(
            event_tap_disable_reason(CGEventType::TapDisabledByUserInput),
            Some("user_input")
        );
        assert_eq!(event_tap_disable_reason(CGEventType::LeftMouseUp), None);
    }

    #[test]
    fn selection_candidate_walks_to_the_first_readable_parent() {
        let selected = first_value_in_ancestor_chain(
            0,
            16,
            |node| (*node == 3).then_some("selection"),
            |node| Some(node + 1),
        );

        assert_eq!(selected, Some("selection"));
    }

    #[test]
    fn selection_candidate_walk_is_bounded() {
        let selected = first_value_in_ancestor_chain(
            0,
            3,
            |node| (*node == 3).then_some("selection"),
            |node| Some(node + 1),
        );

        assert_eq!(selected, None);
    }

    #[test]
    fn selection_candidate_walk_uses_focused_element_after_xpc_event_element() {
        let selected = first_value_in_candidate_chains(
            [0, 10],
            3,
            |node| (*node == 12).then_some("selection"),
            |node| Some(node + 1),
        );

        assert_eq!(selected, Some("selection"));
    }

    #[test]
    fn range_selection_anchors_to_the_first_character() {
        assert_eq!(
            first_character_range(AXRange {
                location: 19,
                length: 8,
            }),
            Some(AXRange {
                location: 19,
                length: 1,
            })
        );
        assert_eq!(
            first_character_range(AXRange {
                location: 19,
                length: 0,
            }),
            None
        );
    }

    #[test]
    fn browser_placeholder_bounds_fall_through_to_text_markers() {
        assert_eq!(
            usable_selection_rect(AXRect {
                origin: AXPoint { x: 0.0, y: 1440.0 },
                size: AXSize {
                    width: 0.0,
                    height: 0.0,
                },
            }),
            None
        );
        assert!(usable_selection_rect(AXRect {
            origin: AXPoint { x: 600.0, y: 729.0 },
            size: AXSize {
                width: 5.0,
                height: 16.0,
            },
        })
        .is_some());
    }

    #[test]
    fn text_marker_signature_uses_both_marker_boundaries() {
        let first = AXTextMarkerRange::from_bytes(&[1, 2], &[3, 4]).expect("marker range");
        let second = AXTextMarkerRange::from_bytes(&[1, 2], &[3, 5]).expect("marker range");

        assert_ne!(
            marker_range_signature(&first),
            marker_range_signature(&second)
        );
    }

    #[tokio::test]
    async fn mouse_up_probe_is_scheduled_before_an_application_is_bound() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let point = super::LogicalPoint { x: 320.0, y: 180.0 };

        super::schedule_selection_probe(&sender, point, 0);

        let signal = tokio::time::timeout(
            Duration::from_millis(super::SELECTION_PROBE_DELAYS_MS[0] + 100),
            receiver.recv(),
        )
        .await
        .expect("selection probe should settle")
        .expect("selection probe channel should stay open");
        match signal {
            super::MacSignal::SelectionProbeReady { point: actual, attempt } => {
                assert_eq!(actual.x, point.x);
                assert_eq!(actual.y, point.y);
                assert_eq!(attempt, 0);
            }
            other => panic!("unexpected macOS signal: {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_retries_stop_after_the_last_configured_attempt() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let point = super::LogicalPoint { x: 320.0, y: 180.0 };
        let last_attempt = super::SELECTION_PROBE_DELAYS_MS.len() - 1;

        assert!(super::is_last_probe_attempt(last_attempt));
        assert!(!super::is_last_probe_attempt(0));

        super::schedule_selection_probe(&sender, point, super::SELECTION_PROBE_DELAYS_MS.len());
        drop(sender);
        assert!(receiver.recv().await.is_none());
    }

    #[test]
    fn packaged_app_opens_the_permission_pane() {
        assert_eq!(
            permission_action(true),
            PermissionAction::OpenPermissionPane
        );
    }

    #[test]
    fn unbundled_development_binary_exposes_a_manual_add_path() {
        assert_eq!(permission_action(false), PermissionAction::OpenForManualAdd);
    }

    #[test]
    fn app_bundle_detection_distinguishes_tauri_dev_from_packaged_apps() {
        assert!(is_bundled_app_executable(Path::new(
            "/Applications/AQBot.app/Contents/MacOS/AQBot"
        )));
        assert!(!is_bundled_app_executable(Path::new(
            "/workspace/src-tauri/target/debug/AQBot"
        )));
    }
}
