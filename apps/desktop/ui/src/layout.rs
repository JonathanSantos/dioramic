//! Workbench layout: column widths (files · editor · preview), collapsible panels and the problems
//! panel height. The math is pure (and tested); [`Layout`] holds the signals and persists choices.

use leptos::prelude::*;

use crate::storage;

/// Default width of the file sidebar (px).
pub const SIDEBAR_DEFAULT_PX: f64 = 216.0;
/// Narrowest sidebar the splitter allows (px).
pub const SIDEBAR_MIN_PX: f64 = 176.0;
/// Widest sidebar the splitter allows (px).
pub const SIDEBAR_MAX_PX: f64 = 360.0;
/// Default editor share of the room right of the sidebar. The preview is the product, so it gets
/// the larger part: about 68 % of that room, ≥ 58 % of a 1440 px window.
pub const EDITOR_DEFAULT_RATIO: f64 = 0.31;
/// Narrowest editor (px): about 40 columns of YAML next to the gutter.
pub const EDITOR_MIN_PX: f64 = 300.0;
/// Narrowest preview (px): the floating toolbar always fits with icons only.
pub const PREVIEW_MIN_PX: f64 = 460.0;
/// Below this viewport width (CSS px, e.g. interface zoom 150 %) the sidebar folds away by itself.
pub const NARROW_VIEWPORT_PX: f64 = 1100.0;
/// Default problems panel height as a share of the editor column.
pub const PROBLEMS_DEFAULT_SHARE: f64 = 0.3;
/// Smallest / largest problems panel height (px / share of the editor column).
pub const PROBLEMS_MIN_PX: f64 = 72.0;
/// Largest problems panel height as a share of the editor column.
pub const PROBLEMS_MAX_SHARE: f64 = 0.7;
/// Splitter step of an arrow key (px); Shift multiplies it.
pub const SPLITTER_STEP_PX: f64 = 16.0;

/// Widths of the three workbench columns (px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Columns {
    /// File sidebar (0 when folded).
    pub sidebar: f64,
    /// YAML editor (0 when folded: preview mode).
    pub editor: f64,
    /// Live preview: everything left.
    pub preview: f64,
}

/// Clamps a sidebar width to the splitter range.
pub fn clamp_sidebar(width: f64) -> f64 {
    width.clamp(SIDEBAR_MIN_PX, SIDEBAR_MAX_PX)
}

/// Clamps an editor ratio to something sensible (the px limits are applied in [`columns`]).
pub fn clamp_ratio(ratio: f64) -> f64 {
    ratio.clamp(0.1, 0.9)
}

/// Splits the viewport. `sidebar` / `editor_ratio` are `None` when that panel is folded.
/// The preview never drops below [`PREVIEW_MIN_PX`] while the editor can shrink to [`EDITOR_MIN_PX`];
/// when even that does not fit, the editor gives way first.
pub fn columns(viewport: f64, sidebar: Option<f64>, editor_ratio: Option<f64>) -> Columns {
    let viewport = viewport.max(0.0);
    let sidebar = sidebar.map_or(0.0, clamp_sidebar).min((viewport - PREVIEW_MIN_PX).max(0.0));
    let room = viewport - sidebar;
    let editor = editor_ratio.map_or(0.0, |ratio| {
        let wanted = room * clamp_ratio(ratio);
        let most = (room - PREVIEW_MIN_PX).max(0.0);
        wanted.max(EDITOR_MIN_PX).min(most)
    });
    Columns { sidebar, editor, preview: room - editor }
}

/// Editor ratio that puts the editor's right edge at `x` (client px), given the sidebar width.
pub fn ratio_at(x: f64, viewport: f64, sidebar: f64) -> f64 {
    let room = (viewport - sidebar).max(1.0);
    clamp_ratio((x - sidebar) / room)
}

/// Problems panel height for a drag to `y` inside an editor column spanning `top..bottom`.
pub fn problems_height_at(y: f64, top: f64, bottom: f64) -> f64 {
    clamp_problems(bottom - y, bottom - top)
}

/// Clamps a problems panel height to the editor column.
pub fn clamp_problems(height: f64, column: f64) -> f64 {
    height.clamp(PROBLEMS_MIN_PX, (column * PROBLEMS_MAX_SHARE).max(PROBLEMS_MIN_PX))
}

/// Layout signals shared by the workbench. `Copy`: signals are handles.
#[derive(Clone, Copy)]
pub struct Layout {
    /// Viewport width (CSS px), kept current by the app's resize listener.
    pub viewport: RwSignal<f64>,
    /// The user wants the sidebar (wide windows).
    pub sidebar_wanted: RwSignal<bool>,
    /// The user opened the sidebar while the window is narrow.
    pub sidebar_peek: RwSignal<bool>,
    /// Sidebar width (px).
    pub sidebar_width: RwSignal<f64>,
    /// Editor shown (false = preview mode).
    pub editor_open: RwSignal<bool>,
    /// Editor share of the room right of the sidebar.
    pub editor_ratio: RwSignal<f64>,
    /// Problems panel height (px); `None` = default share.
    pub problems_height: RwSignal<Option<f64>>,
    /// Soft wrap in the editor.
    pub soft_wrap: RwSignal<bool>,
}

impl Layout {
    /// Reads the saved choices.
    pub fn restore() -> Self {
        let number = |key| storage::get(key).and_then(|value| value.parse::<f64>().ok());
        let flag = |key, default| storage::get(key).map_or(default, |value| value == "1");
        Self {
            viewport: RwSignal::new(viewport_width()),
            sidebar_wanted: RwSignal::new(flag(storage::SIDEBAR_OPEN, true)),
            sidebar_peek: RwSignal::new(false),
            sidebar_width: RwSignal::new(number(storage::SIDEBAR_WIDTH).map_or(SIDEBAR_DEFAULT_PX, clamp_sidebar)),
            editor_open: RwSignal::new(flag(storage::EDITOR_OPEN, true)),
            editor_ratio: RwSignal::new(number(storage::EDITOR_RATIO).map_or(EDITOR_DEFAULT_RATIO, clamp_ratio)),
            problems_height: RwSignal::new(number(storage::PROBLEMS_HEIGHT)),
            soft_wrap: RwSignal::new(flag(storage::SOFT_WRAP, false)),
        }
    }

    /// True when the window is too narrow for the sidebar to stay open by default.
    pub fn is_narrow(&self) -> bool {
        self.viewport.get() < NARROW_VIEWPORT_PX
    }

    /// Sidebar visible right now.
    pub fn sidebar_visible(&self) -> bool {
        if self.is_narrow() { self.sidebar_peek.get() } else { self.sidebar_wanted.get() }
    }

    /// ⌘B.
    pub fn toggle_sidebar(&self) {
        if self.viewport.get_untracked() < NARROW_VIEWPORT_PX {
            self.sidebar_peek.update(|open| *open = !*open);
        } else {
            self.sidebar_wanted.update(|open| *open = !*open);
            storage::set(storage::SIDEBAR_OPEN, if self.sidebar_wanted.get_untracked() { "1" } else { "0" });
        }
    }

    /// ⌘\ (preview mode).
    pub fn toggle_editor(&self) {
        self.editor_open.update(|open| *open = !*open);
        storage::set(storage::EDITOR_OPEN, if self.editor_open.get_untracked() { "1" } else { "0" });
    }

    /// ⌥Z.
    pub fn toggle_soft_wrap(&self) {
        self.soft_wrap.update(|wrap| *wrap = !*wrap);
        storage::set(storage::SOFT_WRAP, if self.soft_wrap.get_untracked() { "1" } else { "0" });
    }

    /// Current column widths.
    pub fn columns(&self, has_file: bool) -> Columns {
        let sidebar = self.sidebar_visible().then(|| self.sidebar_width.get());
        let editor = (has_file && self.editor_open.get()).then(|| self.editor_ratio.get());
        columns(self.viewport.get(), sidebar, editor)
    }

    /// Sets and saves the sidebar width.
    pub fn set_sidebar_width(&self, width: f64) {
        let width = clamp_sidebar(width);
        self.sidebar_width.set(width);
        storage::set(storage::SIDEBAR_WIDTH, &format!("{width:.0}"));
    }

    /// Sets and saves the editor ratio.
    pub fn set_editor_ratio(&self, ratio: f64) {
        let ratio = clamp_ratio(ratio);
        self.editor_ratio.set(ratio);
        storage::set(storage::EDITOR_RATIO, &format!("{ratio:.4}"));
    }

    /// Sets and saves the problems panel height (`None` = default).
    pub fn set_problems_height(&self, height: Option<f64>) {
        self.problems_height.set(height);
        match height {
            Some(height) => storage::set(storage::PROBLEMS_HEIGHT, &format!("{height:.0}")),
            None => storage::remove(storage::PROBLEMS_HEIGHT),
        }
    }
}

/// `window.innerWidth`, or a desktop-sized default outside a browser.
pub fn viewport_width() -> f64 {
    web_sys::window().and_then(|window| window.inner_width().ok()).and_then(|width| width.as_f64()).unwrap_or(1440.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_split_gives_the_preview_most_of_a_wide_window() {
        let split = columns(1440.0, Some(SIDEBAR_DEFAULT_PX), Some(EDITOR_DEFAULT_RATIO));
        assert!((split.sidebar + split.editor + split.preview - 1440.0).abs() < 1e-9);
        assert!(split.preview / 1440.0 >= 0.58, "preview share {}", split.preview / 1440.0);
        assert!(split.editor >= EDITOR_MIN_PX);
    }

    #[test]
    fn editor_keeps_its_minimum_on_the_smallest_window() {
        let split = columns(1100.0, Some(SIDEBAR_DEFAULT_PX), Some(EDITOR_DEFAULT_RATIO));
        assert!((split.editor - EDITOR_MIN_PX).abs() < 1e-9);
        assert!(split.preview >= PREVIEW_MIN_PX);
    }

    #[test]
    fn preview_never_drops_below_its_minimum() {
        for viewport in [600.0, 960.0, 1100.0, 1920.0] {
            let split = columns(viewport, Some(SIDEBAR_MAX_PX), Some(0.9));
            assert!(split.preview >= PREVIEW_MIN_PX - 1e-9, "{viewport}: {split:?}");
            assert!(split.sidebar >= 0.0 && split.editor >= 0.0);
        }
    }

    #[test]
    fn folded_panels_give_their_room_to_the_preview() {
        let split = columns(1440.0, None, None);
        assert_eq!(split, Columns { sidebar: 0.0, editor: 0.0, preview: 1440.0 });
        let preview_mode = columns(1440.0, Some(200.0), None);
        assert!((preview_mode.preview - 1240.0).abs() < 1e-9);
    }

    #[test]
    fn ratio_round_trips_through_the_splitter_position() {
        let split = columns(1440.0, Some(216.0), Some(0.4));
        let ratio = ratio_at(split.sidebar + split.editor, 1440.0, 216.0);
        assert!((ratio - 0.4).abs() < 1e-9);
        assert!((ratio_at(-500.0, 1440.0, 216.0) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn problems_height_is_clamped_to_the_column() {
        assert!((problems_height_at(500.0, 100.0, 700.0) - 200.0).abs() < 1e-9);
        assert!((problems_height_at(690.0, 100.0, 700.0) - PROBLEMS_MIN_PX).abs() < 1e-9);
        assert!((problems_height_at(0.0, 100.0, 700.0) - 600.0 * PROBLEMS_MAX_SHARE).abs() < 1e-9);
        assert!((clamp_sidebar(10.0) - SIDEBAR_MIN_PX).abs() < 1e-9);
    }
}
