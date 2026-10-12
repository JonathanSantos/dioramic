//! App state (Leptos signals) and the actions the components trigger.

use std::collections::BTreeMap;
use std::time::Duration;

use dioramic_desktop_ipc::{
    DiagnosticView, DiagramTheme, ExportFormat, ExportOutcome, ExportRequest, PinRequest, PreviewRequest,
    PreviewResponse, PreviewStatus, ResolvedView, SaveRequest, Severity, ViewDensity, ViewGrid, ViewLayout,
    ViewRotation, ViewRouting, ViewState, Workspace,
};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend;
use crate::components::preview::FitIntent;
use crate::highlight::LineMark;
use crate::layout::Layout;
use crate::locate::{focus_key, locate};
use crate::motion::prefers_reduced_motion;
use crate::storage;

/// Pause after the last keystroke before the preview re-renders.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(150);
/// How long a toast stays on screen.
const TOAST_LIFETIME: Duration = Duration::from_millis(4200);

/// Light/dark choice for the app chrome (the diagram theme is separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiTheme {
    /// Follow the OS.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl UiTheme {
    /// Next choice of the header toggle.
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    /// Stored value / `data-theme` attribute (`None` = follow the OS).
    pub fn attribute(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::Light => Some("light"),
            Self::Dark => Some("dark"),
        }
    }

    /// Parses the stored value.
    pub fn from_attribute(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    /// Tooltip label.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Appearance: System",
            Self::Light => "Appearance: Light",
            Self::Dark => "Appearance: Dark",
        }
    }
}

/// Toast tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastTone {
    /// Something worked.
    Success,
    /// Something failed.
    Error,
    /// Neutral news.
    Info,
}

/// Transient message in the corner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// Unique id.
    pub id: u32,
    /// Tone.
    pub tone: ToastTone,
    /// Message.
    pub message: String,
    /// File to reveal, when the toast is about an exported file.
    pub reveal: Option<String>,
}

/// Export popover state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportState {
    /// Hidden.
    Closed,
    /// Choosing format and size.
    Choosing,
    /// Rendering (job id and format).
    Running {
        /// Job id.
        job_id: u32,
        /// Format.
        format: ExportFormat,
    },
}

/// Everything the UI shares. `Copy`: signals are handles.
#[derive(Clone, Copy)]
pub struct AppState {
    /// Open folder.
    pub workspace: RwSignal<Option<Workspace>>,
    /// Open spec path.
    pub open_path: RwSignal<Option<String>>,
    /// Editor text.
    pub text: RwSignal<String>,
    /// Text as last read from / written to disk.
    pub saved_text: RwSignal<String>,
    /// The open file changed on disk while the editor had unsaved edits.
    pub disk_conflict: RwSignal<bool>,
    /// View overrides (rotation, density, theme).
    pub view: RwSignal<ViewState>,
    /// Last preview response.
    pub preview: RwSignal<Option<PreviewResponse>>,
    /// Last SVG that rendered (kept while the spec is temporarily broken).
    pub last_svg: RwSignal<Option<String>>,
    /// A preview request is in flight.
    pub rendering: RwSignal<bool>,
    /// Diagnostic the user clicked.
    pub selected_diagnostic: RwSignal<Option<usize>>,
    /// Editor line to reveal (1-based), consumed by the editor.
    pub jump_to_line: RwSignal<Option<u32>>,
    /// Caret position (1-based line, column).
    pub cursor: RwSignal<(u32, u32)>,
    /// Preview zoom factor (for the status bar).
    pub zoom: RwSignal<f64>,
    /// Bumped to ask the preview to fit the diagram that is on screen now (`0`, double click).
    pub fit_request: RwSignal<u32>,
    /// How the preview frames the next SVG (open, rotate, density… ask for a fresh fit).
    pub fit_intent: StoredValue<FitIntent>,
    /// Size of the diagram on screen (its viewBox), for the status bar and the export summary.
    pub diagram_size: RwSignal<Option<(f64, f64)>>,
    /// Last view the engine resolved, kept while the spec is broken (stable toolbar labels).
    pub last_view: RwSignal<Option<ResolvedView>>,
    /// Panel sizes and visibility.
    pub layout: Layout,
    /// App chrome theme.
    pub ui_theme: RwSignal<UiTheme>,
    /// Export popover.
    pub export: RwSignal<ExportState>,
    /// Grid popover open.
    pub grid_open: RwSignal<bool>,
    /// View popover open (density, layout and connections, folded there when the preview is narrow).
    pub view_open: RwSignal<bool>,
    /// Diagram theme menu open.
    pub theme_open: RwSignal<bool>,
    /// The user wants the preview in motion (remembered across files and launches).
    pub motion: RwSignal<bool>,
    /// The system asks for reduced motion: the preview stays still whatever `motion` says.
    pub reduced_motion: RwSignal<bool>,
    /// Toasts.
    pub toasts: RwSignal<Vec<Toast>>,
    /// View the spec asks for by itself (rendered with no overrides), with the text it was read from.
    /// Picking that value again drops the override instead of pinning a no-op.
    spec_view: StoredValue<Option<(String, ResolvedView)>>,
    preview_generation: StoredValue<u64>,
    preview_timer: StoredValue<Option<TimeoutHandle>>,
    next_id: StoredValue<u32>,
}

impl AppState {
    /// Fresh state.
    pub fn new() -> Self {
        Self {
            workspace: RwSignal::new(None),
            open_path: RwSignal::new(None),
            text: RwSignal::new(String::new()),
            saved_text: RwSignal::new(String::new()),
            disk_conflict: RwSignal::new(false),
            view: RwSignal::new(ViewState::default()),
            preview: RwSignal::new(None),
            last_svg: RwSignal::new(None),
            rendering: RwSignal::new(false),
            selected_diagnostic: RwSignal::new(None),
            jump_to_line: RwSignal::new(None),
            cursor: RwSignal::new((1, 1)),
            zoom: RwSignal::new(1.0),
            fit_request: RwSignal::new(0),
            fit_intent: StoredValue::new(FitIntent::Always),
            diagram_size: RwSignal::new(None),
            last_view: RwSignal::new(None),
            layout: Layout::restore(),
            ui_theme: RwSignal::new(UiTheme::from_attribute(storage::get(storage::UI_THEME).as_deref())),
            export: RwSignal::new(ExportState::Closed),
            grid_open: RwSignal::new(false),
            view_open: RwSignal::new(false),
            theme_open: RwSignal::new(false),
            motion: RwSignal::new(storage::get(storage::MOTION).as_deref() == Some("1")),
            reduced_motion: RwSignal::new(prefers_reduced_motion()),
            toasts: RwSignal::new(Vec::new()),
            spec_view: StoredValue::new(None),
            preview_generation: StoredValue::new(0),
            preview_timer: StoredValue::new(None),
            next_id: StoredValue::new(1),
        }
    }

    fn fresh_id(&self) -> u32 {
        let id = self.next_id.get_value();
        self.next_id.set_value(id.wrapping_add(1));
        id
    }

    /// True when the editor has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.open_path.with(Option::is_some) && self.text.with(|text| self.saved_text.with(|saved| text != saved))
    }

    /// Diagnostics of the last preview.
    pub fn diagnostics(&self) -> Vec<DiagnosticView> {
        self.preview.with(|preview| preview.as_ref().map(|preview| preview.diagnostics.clone()).unwrap_or_default())
    }

    /// Editor line of a diagnostic: its source position, else where its path points.
    pub fn line_of(&self, diagnostic: &DiagnosticView) -> Option<u32> {
        diagnostic.line.or_else(|| self.text.with(|text| locate(text, &diagnostic.path)))
    }

    /// Worst diagnostic per editor line, for the inline lenses (one lens per line, "+N" for the rest).
    pub fn line_marks(&self) -> BTreeMap<u32, LineMark> {
        let mut marks: BTreeMap<u32, LineMark> = BTreeMap::new();
        for diagnostic in self.diagnostics() {
            let Some(line) = self.line_of(&diagnostic) else { continue };
            let mark = LineMark {
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                focus: focus_key(&diagnostic.path),
                others: 0,
            };
            match marks.get_mut(&line) {
                None => {
                    marks.insert(line, mark);
                }
                Some(existing) => {
                    let others = existing.others + 1;
                    if diagnostic.severity < existing.severity {
                        *existing = mark;
                    }
                    existing.others = others;
                }
            }
        }
        marks
    }

    /// Count of diagnostics per severity.
    pub fn severity_count(&self, severity: Severity) -> usize {
        self.preview.with(|preview| {
            preview.as_ref().map_or(0, |preview| preview.diagnostics.iter().filter(|d| d.severity == severity).count())
        })
    }

    // ── toasts ──────────────────────────────────────────────────────────────

    /// Shows a toast that disappears by itself.
    pub fn toast(&self, tone: ToastTone, message: impl Into<String>, reveal: Option<String>) {
        let id = self.fresh_id();
        self.toasts.update(|toasts| toasts.push(Toast { id, tone, message: message.into(), reveal }));
        let toasts = self.toasts;
        set_timeout(move || toasts.update(|toasts| toasts.retain(|toast| toast.id != id)), TOAST_LIFETIME);
    }

    /// Dismisses a toast.
    pub fn dismiss_toast(&self, id: u32) {
        self.toasts.update(|toasts| toasts.retain(|toast| toast.id != id));
    }

    // ── workspace and files ─────────────────────────────────────────────────

    /// Opens the folder picker.
    pub fn pick_workspace(self) {
        spawn_local(async move {
            match backend::pick_workspace().await {
                Ok(Some(workspace)) => self.adopt_workspace(workspace),
                Ok(None) => {}
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// Opens a folder by path.
    pub fn open_workspace(self, path: String) {
        spawn_local(async move {
            match backend::open_workspace(&path).await {
                Ok(workspace) => self.adopt_workspace(workspace),
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    fn adopt_workspace(self, workspace: Workspace) {
        storage::set(storage::LAST_WORKSPACE, &workspace.root);
        let remembered = storage::get(storage::LAST_FILE).filter(|path| path.starts_with(&workspace.root));
        let first = remembered.or_else(|| workspace.first_spec().map(|node| node.path.clone()));
        self.workspace.set(Some(workspace));
        if let Some(path) = first {
            self.open_file(path);
        }
    }

    /// Replaces the tree after a change on disk (keeps the open file).
    pub fn refresh_workspace(&self, workspace: Workspace) {
        self.workspace.set(Some(workspace));
    }

    /// Opens a spec in the editor.
    pub fn open_file(self, path: String) {
        if self.open_path.with(|open| open.as_deref() == Some(path.as_str())) {
            return;
        }
        if self.is_dirty() {
            // Keep it simple and safe: save before switching rather than losing edits.
            self.save();
        }
        spawn_local(async move {
            match backend::read_spec(&path).await {
                Ok(text) => {
                    storage::set(storage::LAST_FILE, &path);
                    self.selected_diagnostic.set(None);
                    self.disk_conflict.set(false);
                    self.last_svg.set(None);
                    self.preview.set(None);
                    self.view.set(ViewState::default());
                    self.saved_text.set(text.clone());
                    self.text.set(text);
                    self.open_path.set(Some(path));
                    self.request_fit(FitIntent::Always);
                    self.render_now();
                }
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// The file changed on disk (AI or another editor): follow it unless there are local edits.
    pub fn on_disk_change(self, path: String) {
        if self.open_path.with(|open| open.as_deref() != Some(path.as_str())) {
            return;
        }
        spawn_local(async move {
            let Ok(disk) = backend::read_spec(&path).await else { return };
            if self.text.with(|text| *text == disk) {
                self.saved_text.set(disk);
                return;
            }
            if self.is_dirty() {
                self.disk_conflict.set(true);
            } else {
                self.saved_text.set(disk.clone());
                self.text.set(disk);
                self.request_fit(FitIntent::IfResized);
                self.schedule_preview();
                let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
                self.toast(ToastTone::Info, format!("{name} was updated on disk"), None);
            }
        });
    }

    /// Drops local edits and loads the file from disk.
    pub fn reload_from_disk(self) {
        let Some(path) = self.open_path.get_untracked() else { return };
        spawn_local(async move {
            match backend::read_spec(&path).await {
                Ok(disk) => {
                    self.disk_conflict.set(false);
                    self.saved_text.set(disk.clone());
                    self.text.set(disk);
                    self.request_fit(FitIntent::IfResized);
                    self.schedule_preview();
                }
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// Saves the editor text verbatim.
    pub fn save(self) {
        let Some(path) = self.open_path.get_untracked() else { return };
        let text = self.text.get_untracked();
        spawn_local(async move {
            match backend::save_spec(&SaveRequest { path, text: text.clone() }).await {
                Ok(()) => {
                    self.saved_text.set(text);
                    self.disk_conflict.set(false);
                }
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// Editor input.
    pub fn edit(&self, text: String) {
        self.text.set(text);
        self.schedule_preview();
    }

    // ── preview ─────────────────────────────────────────────────────────────

    /// Re-renders after the debounce pause.
    pub fn schedule_preview(&self) {
        if let Some(handle) = self.preview_timer.get_value() {
            handle.clear();
        }
        let state = *self;
        let handle = set_timeout_with_handle(move || state.render_now(), PREVIEW_DEBOUNCE).ok();
        self.preview_timer.set_value(handle);
    }

    /// Re-renders now. Responses of superseded requests are dropped, so typing fast never shows
    /// an older diagram after a newer one.
    pub fn render_now(self) {
        if self.open_path.get_untracked().is_none() {
            return;
        }
        let generation = self.preview_generation.get_value() + 1;
        self.preview_generation.set_value(generation);
        let request = PreviewRequest {
            text: self.text.get_untracked(),
            view: self.view.get_untracked(),
            spec_path: self.open_path.get_untracked(),
            motion: self.motion_active_untracked(),
        };
        let spec_only = request.view == ViewState::default();
        self.rendering.set(true);
        spawn_local(async move {
            let response = backend::preview(&request).await;
            if self.preview_generation.get_value() != generation {
                return;
            }
            self.rendering.set(false);
            match response {
                Ok(response) => {
                    if let Some(view) = response.view {
                        self.last_view.set(Some(view));
                        if spec_only {
                            self.spec_view.set_value(Some((request.text, view)));
                        }
                    }
                    if let Some(svg) = &response.svg {
                        self.last_svg.set(Some(svg.clone()));
                    }
                    self.selected_diagnostic.set(None);
                    self.preview.set(Some(response));
                }
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// True when the engine said it cannot render yet.
    pub fn engine_unavailable(&self) -> Option<String> {
        self.preview.with(|preview| match preview.as_ref().map(|preview| &preview.status) {
            Some(PreviewStatus::EngineUnavailable { reason }) => Some(reason.clone()),
            _ => None,
        })
    }

    // ── view ────────────────────────────────────────────────────────────────

    /// Asks the preview to frame the next SVG (the strongest pending intent wins).
    pub fn request_fit(&self, intent: FitIntent) {
        self.fit_intent.update_value(|pending| *pending = (*pending).max(intent));
    }

    fn set_view(&self, intent: FitIntent, change: impl FnOnce(&mut ViewState)) {
        self.view.update(change);
        self.request_fit(intent);
        self.render_now();
    }

    /// A field of the view the spec asks for by itself, while the text is the one it was read from.
    fn spec_value<T>(&self, field: impl Fn(&ResolvedView) -> T) -> Option<T> {
        self.spec_view.with_value(|spec| {
            let (text, view) = spec.as_ref()?;
            self.text.with_untracked(|current| current == text).then(|| field(view))
        })
    }

    /// View the engine used last (kept while the spec is broken).
    fn resolved_view(&self) -> Option<ResolvedView> {
        self.preview.with(|preview| preview.as_ref().and_then(|p| p.view)).or_else(|| self.last_view.get())
    }

    /// Current rotation in degrees (override, else what the engine used, else 0).
    pub fn rotation_degrees(&self) -> u16 {
        let fixed = self.view.with(|view| view.rotation.and_then(ViewRotation::degrees));
        fixed.or_else(|| self.resolved_view().map(|view| view.rotation_degrees)).unwrap_or(0)
    }

    /// ⟲ / ⟳.
    pub fn rotate(&self, quarter_turns: i32) {
        let degrees = self.rotation_degrees();
        self.set_view(FitIntent::Always, |view| view.rotation = Some(ViewRotation::turned(degrees, quarter_turns)));
    }

    /// Auto camera on/off.
    pub fn toggle_auto(&self) {
        let degrees = self.rotation_degrees();
        self.set_view(FitIntent::Always, |view| {
            view.rotation = match view.rotation {
                Some(ViewRotation::Auto) => ViewRotation::from_degrees(degrees),
                _ => Some(ViewRotation::Auto),
            };
        });
    }

    /// Density in effect.
    pub fn density(&self) -> ViewDensity {
        self.view
            .with(|view| view.density)
            .or_else(|| self.resolved_view().map(|view| view.density))
            .unwrap_or(ViewDensity::Spaced)
    }

    /// Spaced ↔ compact.
    pub fn toggle_density(&self) {
        let next = self.density().toggled();
        let density = override_unless_spec(next, self.spec_value(|view| view.density));
        self.set_view(FitIntent::Always, |view| view.density = density);
    }

    /// Theme in effect (override, else the last one the engine used, else the default).
    pub fn diagram_theme(&self) -> DiagramTheme {
        self.view
            .with(|view| view.theme)
            .or_else(|| self.resolved_view().map(|view| view.theme))
            .unwrap_or(DiagramTheme::Blueprint)
    }

    /// Picks a diagram theme.
    pub fn set_theme(&self, theme: DiagramTheme) {
        let theme = override_unless_spec(theme, self.spec_value(|view| view.theme));
        self.set_view(FitIntent::IfResized, |view| view.theme = theme);
    }

    /// Placement algorithm in effect (override, else what the engine used, else flow).
    pub fn layout(&self) -> ViewLayout {
        self.view.with(|view| view.layout).or_else(|| self.resolved_view().map(|view| view.layout)).unwrap_or_default()
    }

    /// Picks the placement algorithm.
    pub fn set_layout(&self, layout: ViewLayout) {
        if self.layout() != layout {
            let layout = override_unless_spec(layout, self.spec_value(|view| view.layout));
            self.set_view(FitIntent::IfResized, |view| view.layout = layout);
        }
    }

    /// Flow ⟷ hub (`L`).
    pub fn toggle_layout(&self) {
        self.set_layout(self.layout().toggled());
    }

    /// Edge routing in effect (override, else what the engine used, else curves).
    pub fn routing(&self) -> ViewRouting {
        self.view
            .with(|view| view.routing)
            .or_else(|| self.resolved_view().map(|view| view.routing))
            .unwrap_or_default()
    }

    /// Picks the edge routing.
    pub fn set_routing(&self, routing: ViewRouting) {
        if self.routing() != routing {
            let routing = override_unless_spec(routing, self.spec_value(|view| view.routing));
            self.set_view(FitIntent::IfResized, |view| view.routing = routing);
        }
    }

    /// Rails ⟷ curves (`R`).
    pub fn toggle_routing(&self) {
        self.set_routing(self.routing().toggled());
    }

    /// Background grid in effect (override, else what the engine used, else the default).
    pub fn grid(&self) -> ViewGrid {
        self.view.with(|view| view.grid).or_else(|| self.resolved_view().map(|view| view.grid)).unwrap_or_default()
    }

    /// Changes the background grid, starting from the one in effect.
    pub fn update_grid(&self, change: impl FnOnce(ViewGrid) -> ViewGrid) {
        let next = change(self.grid());
        if next != self.grid() {
            self.set_view(FitIntent::Keep, |view| view.grid = Some(next));
        }
    }

    /// Drops the grid override: back to what the spec says.
    pub fn reset_grid(&self) {
        if self.view.with(|view| view.grid.is_some()) {
            self.set_view(FitIntent::Keep, |view| view.grid = None);
        }
    }

    /// Opens or closes the grid popover (`G`); the other popovers make room.
    pub fn toggle_grid_popover(&self) {
        self.toggle_popover(self.grid_open);
    }

    /// Opens or closes one toolbar popover; opening it closes the others (one at a time).
    pub fn toggle_popover(&self, popover: RwSignal<bool>) {
        let open = !popover.get_untracked();
        self.close_popovers();
        popover.set(open);
    }

    /// Closes every toolbar popover and menu (Esc). A running export keeps its progress card.
    pub fn close_popovers(&self) {
        if matches!(self.export.get_untracked(), ExportState::Choosing) {
            self.export.set(ExportState::Closed);
        }
        for popover in [self.grid_open, self.view_open, self.theme_open] {
            if popover.get_untracked() {
                popover.set(false);
            }
        }
    }

    /// The preview moves: the user asked for it and the system does not ask for reduced motion.
    pub fn motion_active(&self) -> bool {
        self.motion.get() && !self.reduced_motion.get()
    }

    fn motion_active_untracked(&self) -> bool {
        self.motion.get_untracked() && !self.reduced_motion.get_untracked()
    }

    /// Motion preview on/off (`M`). Ignored while the system asks for reduced motion.
    pub fn toggle_motion(&self) {
        if self.reduced_motion.get_untracked() {
            self.toast(ToastTone::Info, "Motion is off while the system asks to reduce motion", None);
            return;
        }
        let next = !self.motion.get_untracked();
        self.motion.set(next);
        storage::set(storage::MOTION, if next { "1" } else { "0" });
        self.request_fit(FitIntent::Keep);
        self.render_now();
    }

    /// The system's reduced-motion setting changed.
    pub fn set_reduced_motion(&self, reduced: bool) {
        if self.reduced_motion.get_untracked() != reduced {
            self.reduced_motion.set(reduced);
            if self.motion.get_untracked() {
                self.render_now();
            }
        }
    }

    /// Spec fields that pinning would write (`view.rotation: 270`), one per override.
    pub fn pending_pin_fields(&self) -> Vec<String> {
        self.view.with(|view| {
            let mut fields = Vec::new();
            if let Some(rotation) = view.rotation {
                fields.push(format!("view.rotation: {}", rotation.spec_value()));
            }
            if let Some(density) = view.density {
                fields.push(format!("layout.density: {}", density.spec_value()));
            }
            if let Some(theme) = view.theme {
                fields.push(format!("theme: {}", theme.spec_value()));
            }
            if let Some(layout) = view.layout {
                fields.push(format!("layout.algorithm: {}", layout.spec_value()));
            }
            if let Some(routing) = view.routing {
                fields.push(format!("layout.routing: {}", routing.spec_value()));
            }
            if let Some(grid) = view.grid {
                fields.push(format!("view.grid: {}", grid_summary(grid)));
            }
            fields
        })
    }

    /// True when the view has overrides not yet written into the spec.
    pub fn has_view_overrides(&self) -> bool {
        self.view.with(|view| *view != ViewState::default())
    }

    /// Writes the view overrides into the spec text (surgical edit) and clears them.
    pub fn pin_view(self) {
        let request = PinRequest { text: self.text.get_untracked(), view: self.view.get_untracked() };
        spawn_local(async move {
            match backend::pin_view(&request).await {
                Ok(text) => {
                    self.view.set(ViewState::default());
                    self.grid_open.set(false);
                    self.edit(text);
                    self.toast(ToastTone::Success, "View pinned to the spec — save to keep it", None);
                }
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    // ── diagnostics ─────────────────────────────────────────────────────────

    /// Selects a diagnostic: reveals its line and highlights its element.
    pub fn select_diagnostic(&self, index: usize) {
        self.selected_diagnostic.set(Some(index));
        let line = self.diagnostics().get(index).and_then(|diagnostic| self.line_of(diagnostic));
        if let Some(line) = line {
            self.jump_to_line.set(Some(line));
        }
    }

    /// `data-id` to highlight in the preview.
    pub fn highlighted_target(&self) -> Option<String> {
        let index = self.selected_diagnostic.get()?;
        self.preview.with(|preview| preview.as_ref()?.diagnostics.get(index)?.target_id.clone())
    }

    // ── export ──────────────────────────────────────────────────────────────

    /// Starts an export.
    pub fn export(self, format: ExportFormat, scale: f64) {
        let job_id = self.fresh_id();
        let request = ExportRequest {
            job_id,
            text: self.text.get_untracked(),
            view: self.view.get_untracked(),
            format,
            scale,
            spec_path: self.open_path.get_untracked(),
        };
        self.export.set(ExportState::Running { job_id, format });
        spawn_local(async move {
            let outcome = backend::export_diagram(&request).await;
            let still_running =
                matches!(self.export.get_untracked(), ExportState::Running { job_id: id, .. } if id == job_id);
            if !still_running {
                return;
            }
            self.export.set(ExportState::Closed);
            match outcome {
                Ok(ExportOutcome::Saved { path, bytes }) => {
                    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
                    self.toast(ToastTone::Success, format!("Exported {name} · {}", human_bytes(bytes)), Some(path));
                }
                Ok(ExportOutcome::Cancelled) => {}
                Err(error) => self.toast(ToastTone::Error, error, None),
            }
        });
    }

    /// Cancels the running export.
    pub fn cancel_export(&self) {
        if let ExportState::Running { job_id, .. } = self.export.get_untracked() {
            self.export.set(ExportState::Closed);
            spawn_local(backend::cancel_export(job_id));
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// `{ size: 0.5, style: dots }` (the pin tooltip): the fields that differ from the default grid, like the pin
/// writes them over a spec without `view.grid`; the whole grid when it is the default one.
pub fn grid_summary(grid: ViewGrid) -> String {
    let default = ViewGrid::default();
    let all = [
        (grid.size != default.size, format!("size: {}", grid.size)),
        (grid.style != default.style, format!("style: {}", grid.style.spec_value())),
        (grid.opacity != default.opacity, format!("opacity: {}", grid.opacity)),
    ];
    let changed: Vec<&str> = all.iter().filter(|(differs, _)| *differs).map(|(_, text)| text.as_str()).collect();
    let fields = if changed.is_empty() { all.iter().map(|(_, text)| text.as_str()).collect() } else { changed };
    format!("{{ {} }}", fields.join(", "))
}

/// Override to store when the user picks `wanted`: none when the spec already asks for it, so choosing
/// the spec's own value (Flow → Hub on a hub spec) leaves nothing to pin.
pub fn override_unless_spec<T: PartialEq>(wanted: T, spec: Option<T>) -> Option<T> {
    (spec.as_ref() != Some(&wanted)).then_some(wanted)
}

/// `412 KB`, `1.4 MB`.
pub fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    #[allow(clippy::cast_precision_loss)] // display only
    let value = bytes as f64;
    if value < KIB {
        format!("{bytes} B")
    } else if value < KIB * KIB {
        format!("{:.0} KB", value / KIB)
    } else {
        format!("{:.1} MB", value / KIB / KIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_the_spec_value_drops_the_override() {
        assert_eq!(override_unless_spec(ViewLayout::Hub, Some(ViewLayout::Hub)), None);
        assert_eq!(override_unless_spec(ViewLayout::Flow, Some(ViewLayout::Hub)), Some(ViewLayout::Flow));
        assert_eq!(override_unless_spec(ViewRouting::Rails, None), Some(ViewRouting::Rails));
    }

    #[test]
    fn human_bytes_picks_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(421_888), "412 KB");
        assert_eq!(human_bytes(1_468_006), "1.4 MB");
    }

    #[test]
    fn grid_summary_reads_like_the_spec() {
        let grid = ViewGrid { size: 0.5, style: dioramic_desktop_ipc::GridStyle::Dots, opacity: 0.6 };
        assert_eq!(grid_summary(grid), "{ size: 0.5, style: dots, opacity: 0.6 }");
        let dots = ViewGrid { style: dioramic_desktop_ipc::GridStyle::Dots, ..ViewGrid::default() };
        assert_eq!(grid_summary(dots), "{ style: dots }");
        assert_eq!(grid_summary(ViewGrid::default()), "{ size: 1, style: lines, opacity: 1 }");
    }

    #[test]
    fn ui_theme_cycles_and_round_trips() {
        assert_eq!(UiTheme::System.next().next().next(), UiTheme::System);
        for theme in [UiTheme::System, UiTheme::Light, UiTheme::Dark] {
            assert_eq!(UiTheme::from_attribute(theme.attribute()), theme);
        }
    }
}
