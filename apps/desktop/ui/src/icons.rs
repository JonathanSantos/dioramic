//! UI icons: 16 px line icons drawn for Dioramic (1.6 px strokes, round caps, `currentColor`),
//! with an isometric twist where it fits the brand (files and density are little iso tiles).

use leptos::prelude::*;

/// Named icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Folder.
    Folder,
    /// Open folder (call to action).
    FolderOpen,
    /// Spec file: a small isometric cube.
    Cube,
    /// Disclosure chevron.
    Chevron,
    /// Rotate counter-clockwise.
    RotateLeft,
    /// Rotate clockwise.
    RotateRight,
    /// Automatic camera.
    Auto,
    /// Spaced layout (tiles apart).
    Spaced,
    /// Compact layout (tiles together).
    Compact,
    /// Pin the view into the spec.
    Pin,
    /// Export.
    Export,
    /// Light UI.
    Sun,
    /// Dark UI.
    Moon,
    /// UI follows the system.
    System,
    /// Zoom in.
    Plus,
    /// Zoom out.
    Minus,
    /// Fit to view.
    Fit,
    /// Close.
    Close,
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Hint.
    Hint,
    /// Success.
    Check,
    /// Reload from disk.
    Reload,
    /// Reveal in file manager.
    Reveal,
    /// Image formats.
    Image,
    /// Vector format.
    Vector,
    /// Animation format.
    Film,
    /// Paused / stale render.
    Pause,
    /// Show or hide the file sidebar.
    Sidebar,
    /// Show or hide the editor (preview mode).
    EditorPanel,
    /// Soft wrap of long lines.
    Wrap,
    /// Curved connections.
    Curves,
    /// Isometric rails: straight legs, rounded 90° bends.
    Rails,
    /// Flow layout: nodes in a row along the flow.
    Flow,
    /// Hub layout: the busiest node in the middle, its neighbours around it.
    Hub,
    /// Background grid with lines (also the grid button).
    Grid,
    /// Background grid of dots.
    GridDots,
    /// No background grid.
    GridNone,
    /// Preview in motion (packets and icons move).
    Motion,
    /// View options (density, layout, connections) folded into one popover: two sliders.
    View,
}

impl Icon {
    fn paths(self) -> &'static str {
        match self {
            Self::Folder => {
                r#"<path d="M2 4.5c0-.8.6-1.5 1.5-1.5h3l1.5 1.5h4.5c.8 0 1.5.7 1.5 1.5v5.5c0 .8-.7 1.5-1.5 1.5h-9C2.6 13 2 12.3 2 11.5z"/>"#
            }
            Self::FolderOpen => {
                r#"<path d="M2 11.5V4.5C2 3.7 2.6 3 3.5 3h3L8 4.5h4c.8 0 1.5.7 1.5 1.5v1"/><path d="M2.2 12.2 4 7.6c.2-.4.6-.6 1-.6h8.6c.7 0 1.1.7.9 1.3l-1.6 3.9c-.2.5-.6.8-1.1.8H3.2c-.7 0-1.2-.5-1-1.1z"/>"#
            }
            Self::Cube => {
                r#"<path d="M8 1.8 13.4 4.9v6.2L8 14.2 2.6 11.1V4.9z"/><path d="M2.6 4.9 8 8l5.4-3.1M8 8v6.2"/>"#
            }
            Self::Chevron => r#"<path d="m6 4 4 4-4 4"/>"#,
            Self::RotateLeft => r#"<path d="M3.2 6.4A5 5 0 1 1 3 9.6"/><path d="M2.6 2.8v3.8h3.8"/>"#,
            Self::RotateRight => r#"<path d="M12.8 6.4A5 5 0 1 0 13 9.6"/><path d="M13.4 2.8v3.8H9.6"/>"#,
            Self::Auto => {
                r#"<path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2"/><circle cx="8" cy="8" r="3.2"/><path d="m8 6.3.5 1.2 1.2.5-1.2.5L8 9.7l-.5-1.2L6.3 8l1.2-.5z" fill="currentColor" stroke="none"/>"#
            }
            Self::Spaced => {
                r#"<path d="M4.5 2.2 7 3.6 4.5 5 2 3.6zM11.5 2.2 14 3.6 11.5 5 9 3.6zM4.5 9.2 7 10.6 4.5 12 2 10.6zM11.5 9.2 14 10.6 11.5 12 9 10.6z"/>"#
            }
            Self::Compact => {
                r#"<path d="M8 2.5 13.5 5.6 8 8.7 2.5 5.6z"/><path d="M2.5 5.6v3.6L8 12.3l5.5-3.1V5.6M8 8.7v3.6"/>"#
            }
            Self::Pin => {
                r#"<path d="M9.8 1.8 14.2 6.2l-1.6.6-2.6 2.6-.3 2.6-1.2 1.2-2.4-2.4L3 14m3.1-3.6L3.7 8l1.2-1.2 2.6-.3 2.6-2.6z"/>"#
            }
            Self::Export => {
                r#"<path d="M8 10V2.5M5 5.3l3-3 3 3"/><path d="M3 8.5v3.8c0 .6.5 1.2 1.2 1.2h7.6c.7 0 1.2-.6 1.2-1.2V8.5"/>"#
            }
            Self::Sun => {
                r#"<circle cx="8" cy="8" r="2.8"/><path d="M8 1.5v1.3M8 13.2v1.3M1.5 8h1.3M13.2 8h1.3M3.4 3.4l.9.9M11.7 11.7l.9.9M3.4 12.6l.9-.9M11.7 4.3l.9-.9"/>"#
            }
            Self::Moon => r#"<path d="M13.6 9.6A5.8 5.8 0 0 1 6.4 2.4a5.8 5.8 0 1 0 7.2 7.2z"/>"#,
            Self::System => {
                r#"<rect x="2" y="2.8" width="12" height="8.4" rx="1.4"/><path d="M5.5 13.6h5M8 11.2v2.4"/><path d="M8 4.6v4.8a2.4 2.4 0 0 0 0-4.8z" fill="currentColor" stroke="none"/>"#
            }
            Self::Plus => r#"<path d="M8 3.5v9M3.5 8h9"/>"#,
            Self::Minus => r#"<path d="M3.5 8h9"/>"#,
            Self::Fit => {
                r#"<path d="M2.5 5.5v-2c0-.6.4-1 1-1h2M10.5 2.5h2c.6 0 1 .4 1 1v2M13.5 10.5v2c0 .6-.4 1-1 1h-2M5.5 13.5h-2c-.6 0-1-.4-1-1v-2"/><path d="M8 5.6 10.4 7 8 8.4 5.6 7z"/>"#
            }
            Self::Close => r#"<path d="m4 4 8 8M12 4l-8 8"/>"#,
            Self::Error => {
                r#"<circle cx="8" cy="8" r="6"/><path d="M8 4.8v3.6"/><circle cx="8" cy="10.9" r=".8" fill="currentColor" stroke="none"/>"#
            }
            Self::Warning => {
                r#"<path d="M7.1 2.6 1.9 11.8c-.4.7.1 1.6.9 1.6h10.4c.8 0 1.3-.9.9-1.6L8.9 2.6c-.4-.7-1.4-.7-1.8 0z"/><path d="M8 6v3"/><circle cx="8" cy="11.1" r=".8" fill="currentColor" stroke="none"/>"#
            }
            Self::Hint => {
                r#"<path d="M6 12.5h4M6.6 14.5h2.8"/><path d="M8 1.6a4.4 4.4 0 0 0-2.6 8c.4.3.6.8.6 1.3v.1h4v-.1c0-.5.2-1 .6-1.3A4.4 4.4 0 0 0 8 1.6z"/>"#
            }
            Self::Check => r#"<path d="m3.2 8.4 3 3 6.6-6.6"/>"#,
            Self::Reload => {
                r#"<path d="M13.2 7.2A5.2 5.2 0 0 0 3.6 5"/><path d="M3.2 2.4V5.2H6"/><path d="M2.8 8.8A5.2 5.2 0 0 0 12.4 11"/><path d="M12.8 13.6v-2.8H10"/>"#
            }
            Self::Reveal => {
                r#"<path d="M9.5 2.5h4v4M13.5 2.5 8 8"/><path d="M12 9.5v3c0 .6-.4 1-1 1H3.5c-.6 0-1-.4-1-1V5c0-.6.4-1 1-1h3"/>"#
            }
            Self::Image => {
                r#"<rect x="2" y="2.5" width="12" height="11" rx="1.6"/><circle cx="5.6" cy="6" r="1.2"/><path d="m2.4 12 3.8-3.6 2.4 2.2 2-1.8 3.2 3"/>"#
            }
            Self::Vector => {
                r#"<path d="M3 13 8 3l5 10"/><rect x="6.6" y="1.6" width="2.8" height="2.8" rx=".5"/><rect x="1.6" y="11.6" width="2.8" height="2.8" rx=".5"/><rect x="11.6" y="11.6" width="2.8" height="2.8" rx=".5"/>"#
            }
            Self::Film => {
                r#"<rect x="2" y="2.5" width="12" height="11" rx="1.6"/><path d="M5 2.5v11M11 2.5v11M2 6h3M2 10h3M11 6h3M11 10h3"/><path d="m7.2 6.6 2.2 1.4-2.2 1.4z" fill="currentColor" stroke="none"/>"#
            }
            Self::Pause => r#"<circle cx="8" cy="8" r="6"/><path d="M6.6 5.8v4.4M9.4 5.8v4.4"/>"#,
            Self::Sidebar => {
                r#"<rect x="2" y="2.8" width="12" height="10.4" rx="1.6"/><path d="M6 2.8v10.4"/><path d="M3.6 5.4h1M3.6 7.2h1" stroke-width="1.2"/>"#
            }
            Self::EditorPanel => {
                r#"<rect x="2" y="2.8" width="12" height="10.4" rx="1.6"/><path d="M8.4 2.8v10.4"/><path d="M4 5.6h2.4M4 7.6h1.6M4 9.6h2" stroke-width="1.2"/>"#
            }
            Self::Curves => {
                r#"<path d="M3.4 12.6c5.2 0 4-9.2 9.2-9.2"/><circle cx="3" cy="12.6" r="1.3" fill="currentColor" stroke="none"/><circle cx="13" cy="3.4" r="1.3" fill="currentColor" stroke="none"/>"#
            }
            Self::Rails => {
                r#"<path d="M3.4 12.6h2.2c.5 0 1-.2 1.4-.6l2-2c.4-.4.9-.6 1.4-.6h.6V5c0-.9.7-1.6 1.6-1.6h.6"/><circle cx="3" cy="12.6" r="1.3" fill="currentColor" stroke="none"/><circle cx="13" cy="3.4" r="1.3" fill="currentColor" stroke="none"/>"#
            }
            Self::Flow => {
                r#"<circle cx="2.8" cy="8" r="1.6"/><circle cx="8" cy="8" r="1.6"/><circle cx="13.2" cy="8" r="1.6"/><path d="M4.4 8h2M9.6 8h2"/>"#
            }
            Self::Hub => {
                r#"<circle cx="8" cy="8" r="2"/><circle cx="2.8" cy="3.2" r="1.3"/><circle cx="13.2" cy="3.2" r="1.3"/><circle cx="2.8" cy="12.8" r="1.3"/><circle cx="13.2" cy="12.8" r="1.3"/><path d="M3.8 4.2 6.5 6.6M12.2 4.2 9.5 6.6M3.8 11.8l2.7-2.4M12.2 11.8 9.5 9.4"/>"#
            }
            Self::Grid => {
                r#"<path d="M8 2.6 14.2 6.2 8 9.8 1.8 6.2z"/><path d="M4.9 4.4 11.1 8M11.1 4.4 4.9 8" stroke-width="1.2"/><path d="M1.8 9.6 8 13.2l6.2-3.6" stroke-width="1.2" opacity=".55"/>"#
            }
            Self::GridDots => {
                r#"<g fill="currentColor" stroke="none"><circle cx="8" cy="3" r="1.05"/><circle cx="4.6" cy="5" r="1.05"/><circle cx="11.4" cy="5" r="1.05"/><circle cx="1.6" cy="7" r=".95"/><circle cx="8" cy="7" r="1.05"/><circle cx="14.4" cy="7" r=".95"/><circle cx="4.6" cy="9" r="1.05"/><circle cx="11.4" cy="9" r="1.05"/><circle cx="8" cy="11" r="1.05"/></g>"#
            }
            Self::GridNone => r#"<path d="M8 2.6 14.2 6.2 8 9.8 1.8 6.2z" stroke-dasharray="1.6 2.2"/>"#,
            Self::Motion => {
                r#"<path d="M1.8 5.2h3.4M1.2 8h3.2M1.8 10.8h3.4" stroke-width="1.3"/><circle cx="10.4" cy="8" r="3.6"/><circle cx="10.4" cy="8" r="1.3" fill="currentColor" stroke="none"/>"#
            }
            Self::View => {
                r#"<path d="M2.4 5h3.4M9.4 5h4.2M2.4 11h6.2M12.2 11h1.4"/><circle cx="7.6" cy="5" r="1.8"/><circle cx="10.4" cy="11" r="1.8"/>"#
            }
            Self::Wrap => {
                r#"<path d="M2.5 4h11M2.5 8h9a2 2 0 0 1 0 4H8.5"/><path d="m10 10.4-1.6 1.6 1.6 1.6"/><path d="M2.5 12h3.5"/>"#
            }
        }
    }

    /// SVG markup of the icon.
    pub fn svg(self) -> String {
        format!(
            r#"<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{}</svg>"#,
            self.paths()
        )
    }
}

/// Inline icon element.
#[component]
pub fn IconView(
    /// Which icon.
    icon: Icon,
    /// Extra classes.
    #[prop(optional)]
    class: &'static str,
) -> impl IntoView {
    view! { <span class=format!("icon {class}") inner_html=icon.svg()></span> }
}
