use serde::{Deserialize, Serialize};

pub const TOOLBAR_WIDTH: f64 = 320.0;
pub const TOOLBAR_HEIGHT: f64 = 36.0;
pub const RESULT_WIDTH: f64 = 400.0;
pub const OVERFLOW_SURFACE_MAX_HEIGHT: f64 = 214.0;
pub const COMPACT_TOOLBAR_BASE_WIDTH: f64 = 52.0;
pub const COMPACT_TOOLBAR_TOOL_WIDTH: f64 = 30.0;
pub const COMPACT_TOOLBAR_MORE_WIDTH: f64 = 28.0;
pub const MAX_VISIBLE_TOOLS: usize = 5;
const SURFACE_GAP: f64 = 8.0;
/// Vertical clearance below a mouse-release point so the surface does not sit
/// under the pointer glyph (macOS/Windows arrow cursors are ~20 logical px tall).
const POINTER_GAP_BELOW: f64 = 18.0;
/// Clearance above the release point when the surface flips upward.
const POINTER_GAP_ABOVE: f64 = 10.0;
const RESULT_PANEL_HEIGHT: f64 = 320.0;
pub const RESULT_HEIGHT: f64 = TOOLBAR_HEIGHT + SURFACE_GAP + RESULT_PANEL_HEIGHT;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    pub fn contains(&self, point: ScreenPoint) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
    }
}

/// What the selection anchor rect represents, which decides surface placement.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionAnchorKind {
    /// Bounds of (the first line of) the selected text: prefer above the text.
    #[default]
    SelectionRect,
    /// Mouse-release point: prefer below the pointer so the toolbar tracks the
    /// user's hand and never covers the line that was just selected.
    Pointer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceSize {
    Toolbar,
    Overflow,
    Result,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverflowDirection {
    Above,
    #[default]
    Below,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct OverflowPlacement {
    pub window_position: ScreenPoint,
    pub toolbar_position: ScreenPoint,
    pub direction: OverflowDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionSettingsOutcome {
    PromptRequested,
    PermissionPaneOpened,
    ManualAddRequired { executable_path: String },
}

impl SurfaceSize {
    pub fn dimensions(self) -> (f64, f64) {
        self.dimensions_with_toolbar_width(TOOLBAR_WIDTH)
    }

    pub fn dimensions_with_toolbar_width(self, toolbar_width: f64) -> (f64, f64) {
        match self {
            Self::Toolbar => (toolbar_width, TOOLBAR_HEIGHT),
            Self::Overflow => (toolbar_width, OVERFLOW_SURFACE_MAX_HEIGHT),
            Self::Result => (RESULT_WIDTH, RESULT_HEIGHT),
        }
    }
}

pub fn compact_toolbar_width(tool_count: usize) -> f64 {
    let visible_count = tool_count.min(MAX_VISIBLE_TOOLS);
    let overflow_width = if tool_count > visible_count {
        COMPACT_TOOLBAR_MORE_WIDTH
    } else {
        0.0
    };
    COMPACT_TOOLBAR_BASE_WIDTH + COMPACT_TOOLBAR_TOOL_WIDTH * visible_count as f64 + overflow_width
}

pub fn place_overflow_from_toolbar(
    toolbar_position: ScreenPoint,
    toolbar_width: f64,
    overflow_height: f64,
    monitor_work_area: ScreenRect,
    scale_factor: f64,
) -> OverflowPlacement {
    let height = overflow_height * scale_factor;
    let toolbar_height = TOOLBAR_HEIGHT * scale_factor;
    let extra_height = (height - toolbar_height).max(0.0);
    let monitor_bottom = monitor_work_area.y + monitor_work_area.height;
    let space_above = toolbar_position.y - monitor_work_area.y;
    let space_below = monitor_bottom - (toolbar_position.y + toolbar_height);
    let direction = if space_below >= extra_height {
        OverflowDirection::Below
    } else if space_above >= extra_height || space_above > space_below {
        OverflowDirection::Above
    } else {
        OverflowDirection::Below
    };
    let requested_y = match direction {
        OverflowDirection::Above => toolbar_position.y - extra_height,
        OverflowDirection::Below => toolbar_position.y,
    };
    let max_x = (monitor_work_area.x + monitor_work_area.width - toolbar_width * scale_factor)
        .max(monitor_work_area.x);
    let max_y = (monitor_work_area.y + monitor_work_area.height - height).max(monitor_work_area.y);
    let window_position = ScreenPoint {
        x: toolbar_position.x.clamp(monitor_work_area.x, max_x),
        y: requested_y.clamp(monitor_work_area.y, max_y),
    };
    let toolbar_position = ScreenPoint {
        x: window_position.x,
        y: match direction {
            OverflowDirection::Above => window_position.y + extra_height,
            OverflowDirection::Below => window_position.y,
        },
    };
    OverflowPlacement {
        window_position,
        toolbar_position,
        direction,
    }
}

#[cfg(test)]
pub fn place_surface(
    anchor: ScreenRect,
    monitor_work_area: ScreenRect,
    surface: SurfaceSize,
) -> ScreenPoint {
    place_surface_scaled(
        anchor,
        SelectionAnchorKind::SelectionRect,
        monitor_work_area,
        surface,
        1.0,
    )
}

#[cfg(test)]
pub fn place_surface_scaled(
    anchor: ScreenRect,
    anchor_kind: SelectionAnchorKind,
    monitor_work_area: ScreenRect,
    surface: SurfaceSize,
    scale_factor: f64,
) -> ScreenPoint {
    place_surface_scaled_with_toolbar_width(
        anchor,
        anchor_kind,
        monitor_work_area,
        surface,
        scale_factor,
        TOOLBAR_WIDTH,
    )
}

pub fn place_surface_scaled_with_toolbar_width(
    anchor: ScreenRect,
    anchor_kind: SelectionAnchorKind,
    monitor_work_area: ScreenRect,
    surface: SurfaceSize,
    scale_factor: f64,
    toolbar_width: f64,
) -> ScreenPoint {
    let (width, height) = surface.dimensions_with_toolbar_width(toolbar_width);
    let width = width * scale_factor;
    let height = height * scale_factor;
    let min_x = monitor_work_area.x;
    let max_x = (monitor_work_area.x + monitor_work_area.width - width).max(min_x);
    let x = (anchor.x + anchor.width / 2.0 - width / 2.0).clamp(min_x, max_x);
    let preferred_y = match anchor_kind {
        SelectionAnchorKind::SelectionRect => {
            let above = anchor.y - SURFACE_GAP - height;
            if above >= monitor_work_area.y {
                above
            } else {
                anchor.y + anchor.height + SURFACE_GAP
            }
        }
        SelectionAnchorKind::Pointer => {
            let below = anchor.y + anchor.height + POINTER_GAP_BELOW * scale_factor;
            if below + height <= monitor_work_area.y + monitor_work_area.height {
                below
            } else {
                anchor.y - POINTER_GAP_ABOVE * scale_factor - height
            }
        }
    };
    let min_y = monitor_work_area.y;
    let max_y = (monitor_work_area.y + monitor_work_area.height - height).max(min_y);
    ScreenPoint {
        x,
        y: preferred_y.clamp(min_y, max_y),
    }
}

#[cfg(test)]
pub fn clamp_surface_position(
    position: ScreenPoint,
    monitor_work_area: ScreenRect,
    surface: SurfaceSize,
    scale_factor: f64,
) -> ScreenPoint {
    clamp_surface_position_with_toolbar_width(
        position,
        monitor_work_area,
        surface,
        scale_factor,
        TOOLBAR_WIDTH,
    )
}

pub fn clamp_surface_position_with_toolbar_width(
    position: ScreenPoint,
    monitor_work_area: ScreenRect,
    surface: SurfaceSize,
    scale_factor: f64,
    toolbar_width: f64,
) -> ScreenPoint {
    let (width, height) = surface.dimensions_with_toolbar_width(toolbar_width);
    let max_x = (monitor_work_area.x + monitor_work_area.width - width * scale_factor)
        .max(monitor_work_area.x);
    let max_y = (monitor_work_area.y + monitor_work_area.height - height * scale_factor)
        .max(monitor_work_area.y);
    ScreenPoint {
        x: position.x.clamp(monitor_work_area.x, max_x),
        y: position.y.clamp(monitor_work_area.y, max_y),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionObservation {
    pub text: String,
    pub source_app: String,
    pub source_window: String,
    pub range_signature: String,
    pub anchor: ScreenRect,
    #[serde(default)]
    pub anchor_kind: SelectionAnchorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectionChange {
    Selected(SelectionObservation),
    Cleared,
}

impl SelectionChange {
    fn fingerprint(&self) -> String {
        match self {
            Self::Cleared => "cleared".into(),
            Self::Selected(observation) => format!(
                "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{:?}",
                observation.source_app,
                observation.source_window,
                observation.range_signature,
                observation.text,
                observation.anchor
            ),
        }
    }
}

pub struct SelectionDebouncer {
    delay_ms: u64,
    pending: Option<(SelectionChange, u64)>,
    last_emission: Option<(String, u64)>,
}

impl SelectionDebouncer {
    pub fn new(delay_ms: u64) -> Self {
        Self {
            delay_ms,
            pending: None,
            last_emission: None,
        }
    }

    pub fn push(&mut self, observation: SelectionObservation, now_ms: u64) {
        let change = if observation.text.trim().is_empty() {
            SelectionChange::Cleared
        } else {
            SelectionChange::Selected(observation)
        };
        self.pending = Some((change, now_ms.saturating_add(self.delay_ms)));
    }

    pub fn take_ready(&mut self, now_ms: u64) -> Option<SelectionChange> {
        let (_, ready_at) = self.pending.as_ref()?;
        if now_ms < *ready_at {
            return None;
        }
        let (change, _) = self.pending.take()?;
        let fingerprint = change.fingerprint();
        let duplicate_window_ms = self.delay_ms.saturating_mul(2);
        if self
            .last_emission
            .as_ref()
            .is_some_and(|(last_fingerprint, emitted_at)| {
                last_fingerprint == &fingerprint
                    && now_ms.saturating_sub(*emitted_at) <= duplicate_window_ms
            })
        {
            return None;
        }
        self.last_emission = Some((fingerprint, now_ms));
        Some(change)
    }

    pub fn clear(&mut self) {
        self.pending = None;
        self.last_emission = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(text: &str, x: f64) -> SelectionObservation {
        SelectionObservation {
            text: text.into(),
            source_app: "editor".into(),
            source_window: "document".into(),
            range_signature: "0:4".into(),
            anchor: ScreenRect {
                x,
                y: 120.0,
                width: 80.0,
                height: 20.0,
            },
            anchor_kind: SelectionAnchorKind::SelectionRect,
        }
    }

    #[test]
    fn placement_flips_below_and_clamps_to_monitor_work_area() {
        let monitor = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let anchor = ScreenRect {
            x: 1880.0,
            y: 2.0,
            width: 80.0,
            height: 20.0,
        };

        assert_eq!(
            place_surface(anchor, monitor, SurfaceSize::Toolbar),
            ScreenPoint { x: 1600.0, y: 30.0 }
        );
    }

    #[test]
    fn placement_preserves_negative_monitor_origins() {
        let monitor = ScreenRect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        assert_eq!(
            place_surface(
                observation("text", -1700.0).anchor,
                monitor,
                SurfaceSize::Result
            ),
            ScreenPoint {
                x: -1860.0,
                y: 148.0
            }
        );
    }

    #[test]
    fn placement_uses_monitor_scale_factor_for_window_bounds() {
        let monitor = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let anchor = ScreenRect {
            x: 1800.0,
            y: 500.0,
            width: 80.0,
            height: 20.0,
        };

        assert_eq!(
            place_surface_scaled(
                anchor,
                SelectionAnchorKind::SelectionRect,
                monitor,
                SurfaceSize::Toolbar,
                2.0
            ),
            ScreenPoint {
                x: 1280.0,
                y: 420.0
            }
        );
    }

    #[test]
    fn pointer_anchor_prefers_below_the_release_point() {
        let monitor = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let pointer = ScreenRect {
            x: 600.0,
            y: 500.0,
            width: 1.0,
            height: 1.0,
        };

        assert_eq!(
            place_surface_scaled(
                pointer,
                SelectionAnchorKind::Pointer,
                monitor,
                SurfaceSize::Toolbar,
                1.0
            ),
            ScreenPoint { x: 440.5, y: 519.0 }
        );
    }

    #[test]
    fn pointer_anchor_flips_above_near_the_bottom_edge() {
        let monitor = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let pointer = ScreenRect {
            x: 600.0,
            y: 1060.0,
            width: 1.0,
            height: 1.0,
        };

        assert_eq!(
            place_surface_scaled(
                pointer,
                SelectionAnchorKind::Pointer,
                monitor,
                SurfaceSize::Toolbar,
                1.0
            ),
            ScreenPoint {
                x: 440.5,
                y: 1014.0
            }
        );
    }

    #[test]
    fn toolbar_surface_uses_the_compact_height() {
        assert_eq!(SurfaceSize::Toolbar.dimensions(), (320.0, 36.0));
        assert_eq!(
            SurfaceSize::Overflow.dimensions_with_toolbar_width(230.0),
            (230.0, OVERFLOW_SURFACE_MAX_HEIGHT)
        );
    }

    #[test]
    fn compact_toolbar_width_tracks_visible_tools_and_overflow() {
        assert_eq!(compact_toolbar_width(1), 82.0);
        assert_eq!(compact_toolbar_width(5), 202.0);
        assert_eq!(compact_toolbar_width(6), 230.0);
        assert_eq!(compact_toolbar_width(20), 230.0);
    }

    #[test]
    fn compact_toolbar_placement_uses_its_session_width() {
        let monitor = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let anchor = ScreenRect {
            x: 600.0,
            y: 500.0,
            width: 80.0,
            height: 20.0,
        };

        assert_eq!(
            place_surface_scaled_with_toolbar_width(
                anchor,
                SelectionAnchorKind::SelectionRect,
                monitor,
                SurfaceSize::Toolbar,
                1.0,
                compact_toolbar_width(1),
            ),
            ScreenPoint { x: 599.0, y: 456.0 }
        );
    }

    #[test]
    fn result_surface_keeps_the_toolbar_above_the_panel() {
        assert_eq!(SurfaceSize::Result.dimensions(), (400.0, 364.0));
    }

    #[test]
    fn dragged_position_is_preserved_and_clamped_for_a_larger_surface() {
        let monitor = ScreenRect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        assert_eq!(
            clamp_surface_position(
                ScreenPoint {
                    x: -350.0,
                    y: 900.0,
                },
                monitor,
                SurfaceSize::Result,
                1.5,
            ),
            ScreenPoint {
                x: -600.0,
                y: 534.0,
            }
        );
    }

    #[test]
    fn overflow_opens_below_without_moving_the_toolbar() {
        let placement = place_overflow_from_toolbar(
            ScreenPoint { x: 500.0, y: 400.0 },
            compact_toolbar_width(6),
            OVERFLOW_SURFACE_MAX_HEIGHT,
            ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            1.0,
        );

        assert_eq!(placement.direction, OverflowDirection::Below);
        assert_eq!(
            placement.window_position,
            ScreenPoint { x: 500.0, y: 400.0 }
        );
        assert_eq!(
            placement.toolbar_position,
            ScreenPoint { x: 500.0, y: 400.0 }
        );
    }

    #[test]
    fn overflow_opens_above_without_moving_the_toolbar() {
        let placement = place_overflow_from_toolbar(
            ScreenPoint {
                x: 500.0,
                y: 1000.0,
            },
            compact_toolbar_width(6),
            OVERFLOW_SURFACE_MAX_HEIGHT,
            ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            1.0,
        );

        assert_eq!(placement.direction, OverflowDirection::Above);
        assert_eq!(
            placement.window_position,
            ScreenPoint { x: 500.0, y: 822.0 }
        );
        assert_eq!(
            placement.toolbar_position,
            ScreenPoint {
                x: 500.0,
                y: 1000.0
            }
        );
    }

    #[test]
    fn short_overflow_stays_below_when_its_content_fits() {
        let placement = place_overflow_from_toolbar(
            ScreenPoint { x: 500.0, y: 900.0 },
            compact_toolbar_width(6),
            119.0,
            ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            1.0,
        );

        assert_eq!(placement.direction, OverflowDirection::Below);
        assert_eq!(
            placement.toolbar_position,
            ScreenPoint { x: 500.0, y: 900.0 }
        );
    }

    #[test]
    fn debounce_uses_source_range_and_bounds_not_text_alone() {
        let mut debouncer = SelectionDebouncer::new(200);
        debouncer.push(observation("same", 100.0), 0);
        assert!(debouncer.take_ready(199).is_none());
        let Some(SelectionChange::Selected(first)) = debouncer.take_ready(200) else {
            panic!("selection should be published");
        };
        assert_eq!(first.anchor.x, 100.0);

        debouncer.push(observation("same", 100.0), 210);
        assert!(debouncer.take_ready(410).is_none());

        debouncer.push(observation("same", 480.0), 420);
        let Some(SelectionChange::Selected(moved)) = debouncer.take_ready(620) else {
            panic!("moved selection should be published");
        };
        assert_eq!(moved.anchor.x, 480.0);
    }

    #[test]
    fn identical_selection_is_published_again_after_the_duplicate_window() {
        let mut debouncer = SelectionDebouncer::new(200);
        debouncer.push(observation("same", 100.0), 0);
        assert!(matches!(
            debouncer.take_ready(200),
            Some(SelectionChange::Selected(_))
        ));

        debouncer.push(observation("same", 100.0), 1_000);

        assert!(matches!(
            debouncer.take_ready(1_200),
            Some(SelectionChange::Selected(_))
        ));
    }

    #[test]
    fn whitespace_selection_is_an_explicit_clear_event() {
        let mut debouncer = SelectionDebouncer::new(200);
        debouncer.push(observation("text", 100.0), 0);
        let _ = debouncer.take_ready(200);
        let mut cleared = observation("  \n", 100.0);
        cleared.range_signature = "4:4".into();
        debouncer.push(cleared, 250);

        assert_eq!(debouncer.take_ready(450), Some(SelectionChange::Cleared));
    }
}
