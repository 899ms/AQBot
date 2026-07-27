use std::{cell::RefCell, thread};

use tokio::sync::mpsc::UnboundedSender;
use uiautomation::{
    events::{
        CustomEventHandlerFn, CustomFocusChangedEventHandlerFn, UIEventHandler, UIEventType,
        UIFocusChangedEventHandler,
    },
    patterns::{UITextPattern, UITextRange},
    types::TreeScope,
    variants::SafeArray,
    UIAutomation, UIElement,
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, LPARAM, LRESULT, WPARAM},
    System::{
        Threading::{
            GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        Variant::VT_R8,
    },
    UI::{
        Input::KeyboardAndMouse::VK_ESCAPE,
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
            UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_KEYBOARD_LL,
            WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN,
        },
    },
};
use windows::core::PWSTR;

use super::{DismissReason, PlatformEvent, PlatformMonitorHandle, PlatformStartError};
use crate::selection_toolbar::{
    PermissionSettingsOutcome, PermissionState, RuntimeError, ScreenPoint, ScreenRect,
    SelectionAnchorKind, SelectionObservation,
};

thread_local! {
    static GLOBAL_EVENT_SENDER: RefCell<Option<UnboundedSender<PlatformEvent>>> =
        const { RefCell::new(None) };
}

pub fn start_monitor(
    sender: UnboundedSender<PlatformEvent>,
) -> Result<PlatformMonitorHandle, PlatformStartError> {
    let (stop_sender, stop_receiver) = std::sync::mpsc::channel();
    let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
    let uia_sender = sender.clone();
    let thread = thread::Builder::new()
        .name("selection-toolbar-uia".into())
        .spawn(move || {
            let automation = match UIAutomation::new() {
                Ok(automation) => automation,
                Err(error) => {
                    let _ = ready_sender.send(Err(error.to_string()));
                    return;
                }
            };
            let root = match automation.get_root_element() {
                Ok(root) => root,
                Err(error) => {
                    let _ = ready_sender.send(Err(error.to_string()));
                    return;
                }
            };

            let event_sender = uia_sender.clone();
            let event_handler_fn: Box<CustomEventHandlerFn> = Box::new(move |element, _| {
                publish_selection(element, &event_sender);
                Ok(())
            });
            let event_handler = UIEventHandler::from(event_handler_fn);
            if let Err(error) = automation.add_automation_event_handler(
                UIEventType::Text_TextSelectionChanged,
                &root,
                TreeScope::Subtree,
                None,
                &event_handler,
            ) {
                let _ = ready_sender.send(Err(error.to_string()));
                return;
            }

            let focus_sender = uia_sender;
            let focus_handler_fn: Box<CustomFocusChangedEventHandlerFn> =
                Box::new(move |element| {
                    publish_selection(element, &focus_sender);
                    Ok(())
                });
            let focus_handler = UIFocusChangedEventHandler::from(focus_handler_fn);
            if let Err(error) = automation.add_focus_changed_event_handler(None, &focus_handler) {
                let _ = automation.remove_automation_event_handler(
                    UIEventType::Text_TextSelectionChanged,
                    &root,
                    &event_handler,
                );
                let _ = ready_sender.send(Err(error.to_string()));
                return;
            }
            let _ = ready_sender.send(Ok(()));

            let _ = stop_receiver.recv();
            let _ = automation.remove_focus_changed_event_handler(&focus_handler);
            let _ = automation.remove_automation_event_handler(
                UIEventType::Text_TextSelectionChanged,
                &root,
                &event_handler,
            );
        })
        .map_err(|error| start_error("uia_thread_failed", error.to_string()))?;

    match ready_receiver.recv() {
        Ok(Ok(())) => {
            let (global_stop, global_thread) = match start_global_dismiss_listener(sender) {
                Ok(listener) => listener,
                Err(error) => {
                    let _ = stop_sender.send(());
                    let _ = thread.join();
                    return Err(error);
                }
            };
            Ok(PlatformMonitorHandle::new(move || {
                let _ = stop_sender.send(());
                global_stop();
                let _ = thread.join();
                let _ = global_thread.join();
            }))
        }
        Ok(Err(message)) => {
            let _ = thread.join();
            Err(start_error("uia_unavailable", message))
        }
        Err(error) => {
            let _ = thread.join();
            Err(start_error("uia_start_failed", error.to_string()))
        }
    }
}

fn start_global_dismiss_listener(
    sender: UnboundedSender<PlatformEvent>,
) -> Result<(impl FnOnce() + Send + 'static, thread::JoinHandle<()>), PlatformStartError> {
    let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("selection-toolbar-global-events".into())
        .spawn(move || {
            GLOBAL_EVENT_SENDER.with(|slot| {
                *slot.borrow_mut() = Some(sender);
            });
            let keyboard_hook =
                match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) } {
                    Ok(hook) => hook,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
            let mouse_hook =
                match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) } {
                    Ok(hook) => hook,
                    Err(error) => {
                        let _ = unsafe { UnhookWindowsHookEx(keyboard_hook) };
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            let thread_id = unsafe { GetCurrentThreadId() };
            let _ = ready_sender.send(Ok(thread_id));
            while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {}
            let _ = unsafe { UnhookWindowsHookEx(mouse_hook) };
            let _ = unsafe { UnhookWindowsHookEx(keyboard_hook) };
            GLOBAL_EVENT_SENDER.with(|slot| {
                *slot.borrow_mut() = None;
            });
        })
        .map_err(|error| start_error("windows_global_event_thread_failed", error.to_string()))?;
    let thread_id = ready_receiver
        .recv()
        .map_err(|error| start_error("windows_global_event_start_failed", error.to_string()))?
        .map_err(|message| start_error("windows_global_event_unavailable", message))?;
    let stop = move || unsafe {
        let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
    };
    Ok((stop, thread))
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 as u32 == WM_KEYDOWN {
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if event.vkCode == u32::from(VK_ESCAPE.0) {
            GLOBAL_EVENT_SENDER.with(|slot| {
                if let Some(sender) = slot.borrow().as_ref() {
                    let _ = sender.send(PlatformEvent::Dismiss(DismissReason::Escape));
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            wparam.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        )
    {
        let event = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        GLOBAL_EVENT_SENDER.with(|slot| {
            if let Some(sender) = slot.borrow().as_ref() {
                let _ = sender.send(PlatformEvent::GlobalPointerDown(ScreenPoint {
                    x: f64::from(event.pt.x),
                    y: f64::from(event.pt.y),
                }));
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub fn open_permission_settings() -> Result<PermissionSettingsOutcome, String> {
    Ok(PermissionSettingsOutcome::PermissionPaneOpened)
}

pub fn permission_state() -> PermissionState {
    PermissionState::NotRequired
}

pub fn request_permission() -> Result<PermissionState, String> {
    Ok(PermissionState::NotRequired)
}

fn publish_selection(element: &UIElement, sender: &UnboundedSender<PlatformEvent>) {
    match read_selection(element) {
        Ok(Some(observation)) => {
            let _ = sender.send(PlatformEvent::Selection(observation));
        }
        Ok(None) => {
            let _ = sender.send(PlatformEvent::Clear);
        }
        Err(error) => {
            let _ = sender.send(PlatformEvent::Error(RuntimeError {
                code: "uia_selection_failed".into(),
                message: error.to_string(),
            }));
        }
    }
}

fn read_selection(element: &UIElement) -> uiautomation::Result<Option<SelectionObservation>> {
    let pattern = match element.get_pattern::<UITextPattern>() {
        Ok(pattern) => pattern,
        Err(_) => return Ok(None),
    };
    let Some(range) = pattern.get_selection()?.into_iter().next() else {
        return Ok(None);
    };
    let text = range.get_text(-1)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let Some(anchor) = first_bounding_rect(&range)? else {
        return Ok(None);
    };
    let process_id = element.get_process_id()?;
    let source_window = element
        .get_name()
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("window:{process_id}"));
    let runtime_id = element
        .get_runtime_id()?
        .into_iter()
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(".");
    let source_app = process_image_basename(process_id)
        .unwrap_or_else(|| format!("process:{process_id}"));

    Ok(Some(SelectionObservation {
        text,
        source_app,
        source_window,
        range_signature: runtime_id,
        anchor,
        anchor_kind: SelectionAnchorKind::SelectionRect,
    }))
}

/// Stable filter key for Windows: lower-case executable basename (e.g. `notepad.exe`).
fn process_image_basename(process_id: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        let path = process_image_path(handle);
        let _ = CloseHandle(handle);
        let path = path?;
        std::path::Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase())
    }
}

fn process_image_path(handle: HANDLE) -> Option<String> {
    use windows::Win32::System::Threading::QueryFullProcessImageNameW;
    unsafe {
        let mut buffer = vec![0u16; 1024];
        let mut size = buffer.len() as u32;
        QueryFullProcessImageNameW(handle, Default::default(), PWSTR(buffer.as_mut_ptr()), &mut size)
            .ok()?;
        if size == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }
}

fn first_bounding_rect(range: &UITextRange) -> uiautomation::Result<Option<ScreenRect>> {
    let raw = unsafe { range.as_ref().GetBoundingRectangles()? };
    let values: Vec<f64> = SafeArray::from(raw).into_vector(VT_R8)?;
    Ok(values.chunks_exact(4).next().and_then(|rect| {
        (rect[2] > 0.0 && rect[3] > 0.0).then_some(ScreenRect {
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
        })
    }))
}

fn start_error(code: &str, message: String) -> PlatformStartError {
    PlatformStartError {
        permission: PermissionState::NotRequired,
        error: RuntimeError {
            code: code.into(),
            message,
        },
    }
}
