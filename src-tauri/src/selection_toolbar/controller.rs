use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;

use aqbot_core::types::{AppSettings, SelectionToolbarBuiltinAiKey, SelectionToolbarTool};
use tauri::{AppHandle, Emitter, Manager, Theme};
use tokio::sync::{mpsc, Mutex};

use super::{
    normalize_permission_status,
    platform::{self, DismissReason, PlatformEvent, PlatformMonitorHandle},
    runtime::SessionView,
    window, PermissionSettingsOutcome, PermissionState, RuntimeError, RuntimeSnapshot,
    RuntimeState, RuntimeStatus, RuntimeStore, ScreenPoint, SelectionChange, SelectionDebouncer,
    SelectionObservation, SelectionPlatform, SurfaceSize, ToolbarToolView,
};

pub struct SelectionToolbarRuntime {
    store: Mutex<RuntimeStore>,
    monitor: Mutex<Option<PlatformMonitorHandle>>,
    event_sender: Mutex<Option<mpsc::UnboundedSender<PlatformEvent>>>,
    generation: AtomicU64,
    debounce_clock: Instant,
    debouncer: Mutex<SelectionDebouncer>,
    surface: Mutex<SurfaceSize>,
    last_window_position: Mutex<Option<ScreenPoint>>,
    dragged_for_session: AtomicBool,
    /// True while a tool is running or the pointer is interacting with the toolbar.
    interaction_lock: AtomicBool,
    /// Selection-toolbar webview has registered event listeners.
    frontend_ready: AtomicBool,
    /// Session emitted before the frontend was ready.
    pending_session: Mutex<Option<SessionView>>,
}

impl SelectionToolbarRuntime {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(RuntimeStore::new(SelectionPlatform::current())),
            monitor: Mutex::new(None),
            event_sender: Mutex::new(None),
            generation: AtomicU64::new(0),
            debounce_clock: Instant::now(),
            debouncer: Mutex::new(SelectionDebouncer::new(200)),
            surface: Mutex::new(SurfaceSize::Toolbar),
            last_window_position: Mutex::new(None),
            dragged_for_session: AtomicBool::new(false),
            interaction_lock: AtomicBool::new(false),
            frontend_ready: AtomicBool::new(false),
            pending_session: Mutex::new(None),
        }
    }

    pub async fn snapshot(&self) -> RuntimeSnapshot {
        let _ = self.refresh_permission_status().await;
        self.store.lock().await.snapshot()
    }

    pub async fn status(&self) -> RuntimeStatus {
        self.refresh_permission_status().await
    }

    pub async fn reconcile(self: &Arc<Self>, app: &AppHandle, settings: &AppSettings) {
        if let Err(message) = settings.selection_toolbar.validate() {
            self.set_error("invalid_settings", message).await;
            return;
        }
        if !settings.selection_toolbar.enabled {
            self.stop(app).await;
            return;
        }
        if self.monitor.lock().await.is_some() {
            if self.status().await.state == RuntimeState::PermissionRequired {
                return;
            }
            self.refresh_session(app, settings).await;
            return;
        }

        self.set_runtime_state(RuntimeState::Starting, PermissionState::Unknown, None)
            .await;

        #[cfg(target_os = "macos")]
        if let Err(error) = window::precreate(app) {
            tracing::error!(%error, "Could not precreate selection toolbar panel");
            self.set_error("window_precreate_failed", error).await;
            return;
        }

        let sender = self.ensure_event_loop(app).await;
        match platform::start_monitor(sender) {
            Ok(handle) => {
                *self.monitor.lock().await = Some(handle);
                self.set_runtime_state(RuntimeState::Running, platform::permission_state(), None)
                    .await;
                let _ = self.refresh_permission_status().await;

                #[cfg(not(target_os = "macos"))]
                // Warm the toolbar webview/panel so the first selection is not a cold load.
                if let Err(error) = window::precreate(app) {
                    tracing::warn!(%error, "Could not precreate selection toolbar window");
                }
            }
            Err(start_error) => {
                let state = if start_error.permission == PermissionState::Denied {
                    RuntimeState::PermissionRequired
                } else {
                    RuntimeState::Unavailable
                };
                self.set_runtime_state(state, start_error.permission, Some(start_error.error))
                    .await;
            }
        }
    }

    pub async fn retry(self: &Arc<Self>, app: &AppHandle) -> Result<RuntimeStatus, String> {
        if let Some(handle) = self.monitor.lock().await.take() {
            handle.stop();
        }
        let settings =
            aqbot_core::repo::settings::get_settings(&app.state::<crate::AppState>().sea_db)
                .await
                .map_err(|error| error.to_string())?;
        self.reconcile(app, &settings).await;
        Ok(self.status().await)
    }

    pub fn open_permission_settings(&self) -> Result<PermissionSettingsOutcome, String> {
        platform::open_permission_settings()
    }

    pub fn request_permission(&self) -> Result<PermissionState, String> {
        platform::request_permission()
    }

    pub async fn shutdown(&self, app: &AppHandle) {
        if let Some(handle) = self.monitor.lock().await.take() {
            handle.stop();
        }
        let _ = self.hide(app, "application_exit").await;
    }

    pub async fn set_surface(&self, app: &AppHandle, surface: SurfaceSize) -> Result<(), String> {
        let anchor = {
            let store = self.store.lock().await;
            let snapshot = store.snapshot();
            snapshot
                .session
                .and_then(|session| store.selection_observation(&session.selection_id).cloned())
                .map(|observation| (observation.anchor, observation.anchor_kind))
        };
        let previous_surface = {
            let mut current_surface = self.surface.lock().await;
            let previous_surface = *current_surface;
            *current_surface = surface;
            previous_surface
        };
        let current_position = window::current_screen_position(app);
        let previous_position = *self.last_window_position.lock().await;
        if current_position
            .zip(previous_position)
            .is_some_and(|(current, previous)| position_changed(current, previous))
        {
            self.dragged_for_session.store(true, Ordering::Relaxed);
        }
        let preserve_current_position = self.dragged_for_session.load(Ordering::Relaxed)
            || matches!(surface, SurfaceSize::Result);
        let position = if preserve_current_position {
            match current_position {
                Some(position) => {
                    let (previous_width, _) = previous_surface.dimensions();
                    let (next_width, _) = surface.dimensions();
                    let centered_position = ScreenPoint {
                        x: position.x - (next_width - previous_width) / 2.0,
                        y: position.y,
                    };
                    Some(window::show_surface_at_position(
                        app,
                        centered_position,
                        surface,
                    )?)
                }
                None => anchor
                    .map(|(anchor, kind)| window::show_surface(app, anchor, kind, surface))
                    .transpose()?,
            }
        } else {
            anchor
                .map(|(anchor, kind)| window::show_surface(app, anchor, kind, surface))
                .transpose()?
        };
        if let Some(position) = position {
            *self.last_window_position.lock().await = Some(position);
        }
        if matches!(surface, SurfaceSize::Result) {
            // The result panel appears under a stationary cursor, so the
            // hover→make-key path may never fire; focus it explicitly so its
            // buttons respond to the first click.
            if let Err(error) = window::focus_surface(app) {
                tracing::warn!(%error, "Could not focus the selection toolbar result surface");
            }
        }
        Ok(())
    }

    pub async fn hide(&self, app: &AppHandle, reason: &str) -> Result<(), String> {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.debouncer.lock().await.clear();
        self.store.lock().await.clear();
        self.dragged_for_session.store(false, Ordering::Relaxed);
        self.interaction_lock.store(false, Ordering::Relaxed);
        *self.last_window_position.lock().await = None;
        *self.pending_session.lock().await = None;
        window::hide(app)?;
        tracing::debug!(reason, "selection toolbar hide");
        let _ = app.emit_to(
            window::SELECTION_TOOLBAR_WINDOW_LABEL,
            "selection-toolbar://hidden",
            reason,
        );
        Ok(())
    }

    pub fn lock_interaction(&self) {
        self.interaction_lock.store(true, Ordering::Relaxed);
    }

    pub fn unlock_interaction(&self) {
        self.interaction_lock.store(false, Ordering::Relaxed);
    }

    /// Called when the selection-toolbar webview has finished wiring event listeners.
    pub async fn mark_frontend_ready(&self, app: &AppHandle) {
        self.frontend_ready.store(true, Ordering::Relaxed);
        if let Some(session) = self.pending_session.lock().await.take() {
            tracing::debug!(
                selection_id = %session.selection_id,
                "Flushing pending selection toolbar session after frontend ready"
            );
            let _ = app.emit_to(
                window::SELECTION_TOOLBAR_WINDOW_LABEL,
                "selection-toolbar://session",
                session,
            );
        }
    }

    fn should_suppress_clear(&self, app: &AppHandle) -> bool {
        if self.interaction_lock.load(Ordering::Relaxed) {
            return true;
        }
        // While the session is live and the toolbar window is up, treat empty AX
        // clears as noise unless an outside click / Dismiss decides otherwise.
        let session_live = self
            .store
            .try_lock()
            .map(|store| store.snapshot().session.is_some())
            .unwrap_or(true);
        session_live && window::is_toolbar_visible_for_suppress(app)
    }

    pub async fn selection_text(&self, selection_id: &str) -> Option<String> {
        self.store
            .lock()
            .await
            .selection_text(selection_id)
            .map(str::to_string)
    }

    pub async fn begin_run(
        &self,
        selection_id: &str,
        tool_id: &str,
    ) -> Result<(String, Arc<std::sync::atomic::AtomicBool>), String> {
        self.store.lock().await.begin_run(selection_id, tool_id)
    }

    pub async fn append_delta(&self, request_id: &str, delta: &str) -> bool {
        self.store.lock().await.append_delta(request_id, delta)
    }

    pub async fn complete_run(&self, request_id: &str) -> bool {
        self.store.lock().await.complete_run(request_id)
    }

    pub async fn stop_run(&self, request_id: &str) -> bool {
        self.store.lock().await.stop_run(request_id)
    }

    pub async fn fail_run(&self, request_id: &str, error: String) -> bool {
        self.store.lock().await.fail_run(request_id, error)
    }

    pub async fn run_output(&self, request_id: &str) -> Option<String> {
        self.store
            .lock()
            .await
            .run_output(request_id)
            .map(str::to_string)
    }

    pub async fn replace_output(&self, request_id: &str, output: String) -> bool {
        self.store.lock().await.replace_output(request_id, output)
    }

    async fn ensure_event_loop(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> mpsc::UnboundedSender<PlatformEvent> {
        if let Some(sender) = self.event_sender.lock().await.clone() {
            return sender;
        }
        let (sender, mut receiver) = mpsc::unbounded_channel();
        *self.event_sender.lock().await = Some(sender.clone());
        let runtime = Arc::clone(self);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                runtime.handle_platform_event(&app, event).await;
            }
        });
        sender
    }

    async fn handle_platform_event(self: &Arc<Self>, app: &AppHandle, event: PlatformEvent) {
        match event {
            PlatformEvent::Selection(observation) => {
                tracing::debug!(
                    source_app = %observation.source_app,
                    text_len = observation.text.chars().count(),
                    "selection event received"
                );
                let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
                self.debouncer
                    .lock()
                    .await
                    .push(observation, self.elapsed_ms());
                let runtime = Arc::clone(self);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let current_generation = runtime.generation.load(Ordering::Relaxed);
                    if current_generation == generation {
                        let change = {
                            let mut debouncer = runtime.debouncer.lock().await;
                            debouncer.take_ready(runtime.elapsed_ms())
                        };
                        match change {
                            Some(SelectionChange::Selected(observation)) => {
                                runtime.publish_selection(&app, observation).await;
                            }
                            Some(SelectionChange::Cleared) => {
                                if runtime.should_suppress_clear(&app) {
                                    tracing::debug!(
                                        "Suppressing selection_cleared while toolbar interaction is active"
                                    );
                                } else {
                                    let _ = runtime.hide(&app, "selection_cleared").await;
                                }
                            }
                            None => {}
                        }
                    }
                });
            }
            PlatformEvent::Clear => {
                tracing::debug!("clear event received");
                if self.should_suppress_clear(app) {
                    tracing::debug!(
                        "Suppressing platform Clear while toolbar interaction is active"
                    );
                } else {
                    let _ = self.hide(app, "platform").await;
                }
            }
            PlatformEvent::Dismiss(reason) => {
                tracing::debug!(?reason, "dismiss event received");
                // Esc always closes. App switch / hide / minimize must not close
                // the toolbar while the user is interacting with it or a result
                // panel is open — only an outside click, Esc or the close button.
                if reason == DismissReason::AppChanged && self.sticky_interaction_active().await {
                    tracing::debug!(
                        "Keeping selection toolbar open across an app change while interacting"
                    );
                } else {
                    let _ = self.hide(app, "platform").await;
                }
            }
            PlatformEvent::GlobalPointerDown(point) => {
                // An outside click always closes — even while a tool is
                // streaming. Only clicks on the toolbar itself keep it alive.
                if window::is_pointer_over_toolbar(app, point) {
                    tracing::debug!("Ignoring global pointer down over selection toolbar");
                    self.interaction_lock.store(true, Ordering::Relaxed);
                } else {
                    let _ = self.hide(app, "outside_click").await;
                }
            }
            PlatformEvent::Error(error) => {
                self.set_runtime_state(
                    RuntimeState::Error,
                    self.status().await.permission,
                    Some(error),
                )
                .await;
                let _ = self.hide(app, "monitor_error").await;
            }
        }
    }

    /// True while a tool is running, the pointer is interacting with the
    /// toolbar, or the result panel is open — states in which the toolbar must
    /// survive app switches and duplicate selection announcements.
    async fn sticky_interaction_active(&self) -> bool {
        if self.interaction_lock.load(Ordering::Relaxed) {
            return true;
        }
        matches!(*self.surface.lock().await, SurfaceSize::Result)
    }

    /// A selection can be announced by more than one platform path (mouse-up
    /// probe, AX notification) with different anchors. Re-publishing mints a new
    /// session id, cancels any active run and resets the surface — so keep the
    /// live session for duplicates, and never replace a session the user is
    /// actively interacting with.
    async fn should_skip_publish(&self, observation: &SelectionObservation) -> bool {
        let (session_live, duplicate) = {
            let store = self.store.lock().await;
            match store.snapshot().session {
                Some(session) => (
                    true,
                    // range_signature is unstable across read paths (range vs
                    // text-marker vs hit-test candidate), so the duplicate key
                    // is app + text only.
                    store
                        .selection_observation(&session.selection_id)
                        .is_some_and(|current| {
                            current.source_app == observation.source_app
                                && current.text == observation.text
                        }),
                ),
                None => (false, false),
            }
        };
        if !session_live {
            return false;
        }
        if duplicate {
            tracing::debug!("Skipping duplicate selection publish for the live session");
            return true;
        }
        if self.sticky_interaction_active().await {
            tracing::debug!("Skipping selection publish while toolbar interaction is active");
            return true;
        }
        false
    }

    async fn publish_selection(&self, app: &AppHandle, observation: SelectionObservation) {
        tracing::debug!(
            source_app = %observation.source_app,
            text_len = observation.text.chars().count(),
            "publishing selection"
        );
        if self.should_skip_publish(&observation).await {
            return;
        }
        let settings =
            match aqbot_core::repo::settings::get_settings(&app.state::<crate::AppState>().sea_db)
                .await
            {
                Ok(settings) if settings.selection_toolbar.enabled => settings,
                _ => return,
            };
        if !settings
            .selection_toolbar
            .allows_source_app(&observation.source_app)
        {
            tracing::debug!(
                source_app = %observation.source_app,
                mode = ?settings.selection_toolbar.app_filter_mode,
                "selection ignored by app filter"
            );
            return;
        }
        let status = self.status().await;
        if status.state != RuntimeState::Running {
            self.set_runtime_state(RuntimeState::Running, status.permission, None)
                .await;
        }
        let tools = toolbar_tool_views(&settings);
        if tools.is_empty() {
            let _ = self.hide(app, "no_enabled_tools").await;
            return;
        }
        let theme = toolbar_theme(app, &settings);
        let anchor = observation.anchor;
        let anchor_kind = observation.anchor_kind;
        let session = {
            let mut store = self.store.lock().await;
            let id = store.accept_selection(
                observation,
                tools,
                theme,
                &settings.language,
                settings
                    .selection_toolbar
                    .translate_target_language
                    .as_deref(),
            );
            store
                .snapshot()
                .session
                .filter(|session| session.selection_id == id)
        };
        *self.surface.lock().await = SurfaceSize::Toolbar;
        self.dragged_for_session.store(false, Ordering::Relaxed);
        let position = match window::show_surface(app, anchor, anchor_kind, SurfaceSize::Toolbar) {
            Ok(position) => position,
            Err(error) => {
                self.set_error("window_show_failed", error).await;
                return;
            }
        };
        tracing::info!(
            position_x = position.x,
            position_y = position.y,
            "selection toolbar window shown"
        );
        *self.last_window_position.lock().await = Some(position);
        if let Some(session) = session {
            tracing::debug!(
                selection_id = %session.selection_id,
                frontend_ready = self.frontend_ready.load(Ordering::Relaxed),
                "selection toolbar show"
            );
            if self.frontend_ready.load(Ordering::Relaxed) {
                let _ = app.emit_to(
                    window::SELECTION_TOOLBAR_WINDOW_LABEL,
                    "selection-toolbar://session",
                    session,
                );
            } else {
                *self.pending_session.lock().await = Some(session);
            }
        }
    }

    async fn refresh_session(&self, app: &AppHandle, settings: &AppSettings) {
        let tools = toolbar_tool_views(settings);
        if tools.is_empty() {
            let _ = self.hide(app, "no_enabled_tools").await;
            return;
        }
        let theme = toolbar_theme(app, settings).to_string();
        let session = {
            let mut store = self.store.lock().await;
            store.refresh_session(
                tools,
                &theme,
                &settings.language,
                settings
                    .selection_toolbar
                    .translate_target_language
                    .as_deref(),
            );
            store.snapshot().session
        };
        if let Some(session) = session {
            let _ = app.emit_to(
                window::SELECTION_TOOLBAR_WINDOW_LABEL,
                "selection-toolbar://session",
                session,
            );
        }
    }

    async fn stop(&self, app: &AppHandle) {
        if let Some(handle) = self.monitor.lock().await.take() {
            handle.stop();
        }
        let _ = self.hide(app, "disabled").await;
        self.set_runtime_state(RuntimeState::Disabled, platform::permission_state(), None)
            .await;
    }

    async fn set_error(&self, code: &str, message: String) {
        self.set_runtime_state(
            RuntimeState::Error,
            self.status().await.permission,
            Some(RuntimeError {
                code: code.into(),
                message,
            }),
        )
        .await;
    }

    fn elapsed_ms(&self) -> u64 {
        self.debounce_clock.elapsed().as_millis() as u64
    }

    async fn set_runtime_state(
        &self,
        state: RuntimeState,
        permission: PermissionState,
        last_error: Option<RuntimeError>,
    ) {
        self.store.lock().await.set_status(RuntimeStatus {
            state,
            platform: SelectionPlatform::current(),
            permission,
            last_error,
            global_dismissal_supported: matches!(
                SelectionPlatform::current(),
                SelectionPlatform::Macos | SelectionPlatform::Windows
            ),
        });
    }

    async fn refresh_permission_status(&self) -> RuntimeStatus {
        let permission = platform::permission_state();
        let (status, permission_revoked) = {
            let mut store = self.store.lock().await;
            let previous = store.status();
            let permission_revoked = permission == PermissionState::Denied
                && matches!(
                    previous.state,
                    RuntimeState::Starting | RuntimeState::Running
                );
            let status = normalize_permission_status(previous, permission);
            store.set_status(status.clone());
            (status, permission_revoked)
        };
        if permission_revoked {
            if let Some(handle) = self.monitor.lock().await.take() {
                handle.stop();
            }
        }
        status
    }
}

fn position_changed(current: ScreenPoint, previous: ScreenPoint) -> bool {
    (current.x - previous.x).abs() > 2.0 || (current.y - previous.y).abs() > 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection_toolbar::{ScreenRect, SelectionAnchorKind};

    fn observation(text: &str, source_app: &str) -> SelectionObservation {
        SelectionObservation {
            text: text.into(),
            source_app: source_app.into(),
            source_window: "window".into(),
            range_signature: "range:0:4".into(),
            anchor: ScreenRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 12.0,
            },
            anchor_kind: SelectionAnchorKind::SelectionRect,
        }
    }

    async fn runtime_with_live_selection(text: &str) -> Arc<SelectionToolbarRuntime> {
        let runtime = Arc::new(SelectionToolbarRuntime::new());
        runtime.store.lock().await.accept_selection(
            observation(text, "com.example.editor"),
            vec![],
            "light",
            "en-US",
            None,
        );
        runtime
    }

    #[tokio::test]
    async fn re_announced_selection_does_not_replace_the_live_session() {
        let runtime = runtime_with_live_selection("hello").await;

        // Same app + text with a different anchor/signature (probe vs AX path).
        let mut duplicate = observation("hello", "com.example.editor");
        duplicate.range_signature = "marker:deadbeef".into();
        duplicate.anchor.x = 500.0;
        duplicate.anchor_kind = SelectionAnchorKind::Pointer;

        assert!(runtime.should_skip_publish(&duplicate).await);
        assert!(
            !runtime
                .should_skip_publish(&observation("different", "com.example.editor"))
                .await
        );
    }

    #[tokio::test]
    async fn no_selection_is_published_while_the_user_interacts_with_the_toolbar() {
        let runtime = runtime_with_live_selection("hello").await;
        runtime.lock_interaction();

        assert!(
            runtime
                .should_skip_publish(&observation("different", "com.example.editor"))
                .await
        );

        runtime.unlock_interaction();
        assert!(
            !runtime
                .should_skip_publish(&observation("different", "com.example.editor"))
                .await
        );
    }

    #[tokio::test]
    async fn result_surface_blocks_replacement_and_app_change_dismissal() {
        let runtime = runtime_with_live_selection("hello").await;
        assert!(!runtime.sticky_interaction_active().await);

        *runtime.surface.lock().await = SurfaceSize::Result;

        assert!(runtime.sticky_interaction_active().await);
        assert!(
            runtime
                .should_skip_publish(&observation("different", "com.example.editor"))
                .await
        );
    }

    #[tokio::test]
    async fn without_a_live_session_every_selection_publishes() {
        let runtime = Arc::new(SelectionToolbarRuntime::new());
        runtime.lock_interaction();

        assert!(
            !runtime
                .should_skip_publish(&observation("hello", "com.example.editor"))
                .await
        );
    }
}

fn toolbar_tool_views(settings: &AppSettings) -> Vec<ToolbarToolView> {
    settings
        .selection_toolbar
        .tools
        .iter()
        .filter(|tool| tool.enabled())
        .map(|tool| match tool {
            SelectionToolbarTool::BuiltinAi { builtin_key, .. } => ToolbarToolView::ai(
                builtin_key.as_str(),
                Some(builtin_key.as_str()),
                None,
                match builtin_key {
                    SelectionToolbarBuiltinAiKey::Translate => "languages",
                    SelectionToolbarBuiltinAiKey::Polish => "spell-check",
                    SelectionToolbarBuiltinAiKey::Summarize => "list-collapse",
                },
            ),
            SelectionToolbarTool::BuiltinAction { builtin_key, .. } => {
                ToolbarToolView::action(builtin_key.as_str(), builtin_key.as_str())
            }
            SelectionToolbarTool::CustomAi { id, name, icon, .. } => {
                ToolbarToolView::ai(id, None, Some(name), icon)
            }
        })
        .collect()
}

fn toolbar_theme(app: &AppHandle, settings: &AppSettings) -> &'static str {
    if settings.selection_toolbar.theme_follow || settings.theme_mode == "system" {
        return match app
            .get_webview_window("main")
            .and_then(|window| window.theme().ok())
        {
            Some(Theme::Dark) => "dark",
            _ => "light",
        };
    }
    if settings.theme_mode == "dark" {
        "dark"
    } else {
        "light"
    }
}
