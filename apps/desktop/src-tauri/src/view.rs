//! Conversions between the UI view state ([`dioramic_desktop_ipc`]) and the engine types.

use dioramic_desktop_ipc::{
    DiagramTheme, GridStyle, ResolvedView, ViewDensity, ViewGrid, ViewLayout, ViewRotation, ViewRouting, ViewState,
};
use dioramic_render::{
    GridRequest, OutputFormat, PreviewMode, RenderRequest, SourceText, SpecSource, TextFormat, ViewSummary,
};
use dioramic_spec::{Density, GridOptions, LayoutAlgorithm, Rotation, RotationSpec, Routing, ThemeName};

/// Builds a render request for the editor text with the view overrides applied.
pub fn render_request(text: &str, view: &ViewState, format: OutputFormat) -> RenderRequest {
    let source = SpecSource::Text(SourceText { text: text.to_owned(), format: Some(sniff_format(text)) });
    let mut request = RenderRequest::new(source);
    request.format = format;
    request.rotation = view.rotation.map(rotation_spec);
    request.density = view.density.map(density);
    request.theme = view.theme.map(theme_name);
    request.layout_algorithm = view.layout.map(layout_algorithm);
    request.routing = view.routing.map(routing);
    request.grid = view.grid.map(grid_request).unwrap_or_default();
    request
}

/// Request used by the live preview: SVG keeping `<text>` (the webview has Inter via `@font-face`)
/// and no PNG preview, which only the AI needs. In motion, the animated SVG (CSS/SMIL, which the webview plays).
pub fn preview_request(text: &str, view: &ViewState, motion: bool) -> RenderRequest {
    let format = if motion { OutputFormat::SvgAnimated } else { OutputFormat::Svg };
    let mut request = render_request(text, view, format);
    request.svg_text_as_outlines = false;
    request.preview = PreviewMode::None;
    request
}

/// JSON specs start with `{`; everything else is YAML (JSON is valid YAML anyway).
fn sniff_format(text: &str) -> TextFormat {
    if text.trim_start().starts_with('{') { TextFormat::Json } else { TextFormat::Yaml }
}

fn rotation_spec(rotation: ViewRotation) -> RotationSpec {
    match rotation {
        ViewRotation::Deg0 => RotationSpec::Fixed(Rotation::R0),
        ViewRotation::Deg90 => RotationSpec::Fixed(Rotation::R90),
        ViewRotation::Deg180 => RotationSpec::Fixed(Rotation::R180),
        ViewRotation::Deg270 => RotationSpec::Fixed(Rotation::R270),
        ViewRotation::Auto => RotationSpec::Auto,
    }
}

fn density(density: ViewDensity) -> Density {
    match density {
        ViewDensity::Spaced => Density::Spaced,
        ViewDensity::Compact => Density::Compact,
    }
}

fn theme_name(theme: DiagramTheme) -> ThemeName {
    match theme {
        DiagramTheme::Blueprint => ThemeName::Blueprint,
        DiagramTheme::Midnight => ThemeName::Midnight,
        DiagramTheme::Paper => ThemeName::Paper,
        DiagramTheme::Mono => ThemeName::Mono,
        DiagramTheme::Schematic => ThemeName::Schematic,
    }
}

fn layout_algorithm(layout: ViewLayout) -> LayoutAlgorithm {
    match layout {
        ViewLayout::Flow => LayoutAlgorithm::Flow,
        ViewLayout::Hub => LayoutAlgorithm::Hub,
    }
}

fn view_layout(algorithm: LayoutAlgorithm) -> ViewLayout {
    match algorithm {
        LayoutAlgorithm::Flow => ViewLayout::Flow,
        LayoutAlgorithm::Hub => ViewLayout::Hub,
    }
}

fn routing(routing: ViewRouting) -> Routing {
    match routing {
        ViewRouting::Curved => Routing::Curved,
        ViewRouting::Orthogonal => Routing::Orthogonal,
        ViewRouting::Straight => Routing::Straight,
        ViewRouting::Rails => Routing::Rails,
    }
}

fn view_routing(routing: Routing) -> ViewRouting {
    match routing {
        Routing::Curved => ViewRouting::Curved,
        Routing::Orthogonal => ViewRouting::Orthogonal,
        Routing::Straight => ViewRouting::Straight,
        Routing::Rails => ViewRouting::Rails,
    }
}

fn grid_style(style: GridStyle) -> dioramic_spec::GridStyle {
    match style {
        GridStyle::Lines => dioramic_spec::GridStyle::Lines,
        GridStyle::Dots => dioramic_spec::GridStyle::Dots,
        GridStyle::None => dioramic_spec::GridStyle::None,
    }
}

/// The view grid as engine overrides; the spec's color stays.
fn grid_request(grid: ViewGrid) -> GridRequest {
    GridRequest { size: Some(grid.size), style: Some(grid_style(grid.style)), opacity: Some(grid.opacity), color: None }
}

fn view_grid(grid: &GridOptions) -> ViewGrid {
    ViewGrid {
        size: grid.size,
        style: match grid.style {
            dioramic_spec::GridStyle::Lines => GridStyle::Lines,
            dioramic_spec::GridStyle::Dots => GridStyle::Dots,
            dioramic_spec::GridStyle::None => GridStyle::None,
        },
        opacity: grid.opacity,
    }
}

/// The view the engine reports back, in UI terms.
pub fn resolved_view(summary: &ViewSummary) -> ResolvedView {
    ResolvedView {
        rotation_degrees: summary.rotation.degrees(),
        density: match summary.density {
            Density::Spaced => ViewDensity::Spaced,
            Density::Compact => ViewDensity::Compact,
        },
        theme: match summary.theme {
            ThemeName::Blueprint => DiagramTheme::Blueprint,
            ThemeName::Midnight => DiagramTheme::Midnight,
            ThemeName::Paper => DiagramTheme::Paper,
            ThemeName::Mono => DiagramTheme::Mono,
            ThemeName::Schematic => DiagramTheme::Schematic,
        },
        layout: view_layout(summary.layout_algorithm),
        routing: view_routing(summary.routing),
        grid: view_grid(&summary.grid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_view_leaves_the_spec_in_charge() {
        let request = render_request("diorama: 1\n", &ViewState::default(), OutputFormat::Png);
        assert_eq!(request.rotation, None);
        assert_eq!(request.density, None);
        assert_eq!(request.theme, None);
    }

    #[test]
    fn overrides_reach_the_engine() {
        let view = ViewState {
            rotation: Some(ViewRotation::Auto),
            density: Some(ViewDensity::Compact),
            theme: Some(DiagramTheme::Midnight),
            layout: Some(ViewLayout::Hub),
            routing: Some(ViewRouting::Rails),
            grid: Some(ViewGrid { size: 0.5, style: GridStyle::Dots, opacity: 0.6 }),
        };
        let request = preview_request("diorama: 1\n", &view, false);
        assert_eq!(request.rotation, Some(RotationSpec::Auto));
        assert_eq!(request.density, Some(Density::Compact));
        assert_eq!(request.theme, Some(ThemeName::Midnight));
        assert_eq!(request.routing, Some(Routing::Rails));
        assert_eq!(request.layout_algorithm, Some(LayoutAlgorithm::Hub));
        assert_eq!(
            request.grid,
            GridRequest {
                size: Some(0.5),
                style: Some(dioramic_spec::GridStyle::Dots),
                opacity: Some(0.6),
                color: None
            }
        );
        assert_eq!(request.format, OutputFormat::Svg);
        assert!(!request.svg_text_as_outlines);
        assert_eq!(request.preview, PreviewMode::None);
    }

    #[test]
    fn motion_asks_for_the_animated_svg_with_live_text() {
        let request = preview_request("diorama: 1\n", &ViewState::default(), true);
        assert_eq!(request.format, OutputFormat::SvgAnimated);
        assert!(!request.svg_text_as_outlines);
        assert_eq!(request.preview, PreviewMode::None);
        assert!(request.grid.is_empty() && request.routing.is_none());
    }

    #[test]
    fn json_text_is_detected() {
        assert_eq!(sniff_format("  {\"diorama\": 1}"), TextFormat::Json);
        assert_eq!(sniff_format("# comment\ndiorama: 1"), TextFormat::Yaml);
    }
}
