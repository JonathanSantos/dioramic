//! High-level API: `Model` → [`Diagram`] → SVG. Contract: C-005. Owner: T-104.

use std::borrow::Cow;

use dioramic_spec::{Density, Diagnostics, Model, RotationSpec, SourceFormat, ThemeName, ValueSource};

use crate::animation::{AnimationOverrides, AnimationPlan};
use crate::geometry::DEFAULT_TILE_PX;
use crate::icons::{self, IconRegistry};
use crate::scene::{Scene, SceneOptions, build_scene_with};
use crate::svg::{SvgOptions, emit_svg};
use crate::theme::Theme;

/// Caller overrides of what the spec says (MCP params, CLI flags, desktop toolbar).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompileOptions {
    /// Theme override.
    pub theme: Option<ThemeName>,
    /// Rotation override (fixed or auto).
    pub rotation: Option<RotationSpec>,
    /// Density override.
    pub density: Option<Density>,
    /// Tile size override (px).
    pub tile_px: Option<f64>,
    /// Animation overrides.
    pub animation: AnimationOverrides,
    /// Flat icons (provider packs, project icons) loaded by the caller. Empty: built-in icons only.
    pub icons: IconRegistry,
}

/// A compiled diagram: model, scene, animation plan and every non-error diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagram {
    /// Normalized model.
    pub model: Model,
    /// Scene.
    pub scene: Scene,
    /// Animation, if the spec or the overrides ask for one.
    pub animation: Option<AnimationPlan>,
    /// Warnings and hints (spec validation + scene lint), sorted.
    pub diagnostics: Diagnostics,
}

impl CompileOptions {
    /// Scene options for a model with these overrides applied.
    pub fn scene_options(&self, model: &Model) -> SceneOptions {
        let base = SceneOptions::from_model(model);
        SceneOptions {
            theme: self.theme.unwrap_or(base.theme),
            rotation: self.rotation.unwrap_or(base.rotation),
            density: self.density.unwrap_or(base.density),
            tile_px: self.tile_px.unwrap_or(DEFAULT_TILE_PX),
            ..base
        }
    }
}

/// Compiles with the options written in the spec.
pub fn compile(model: &Model) -> Result<Diagram, Diagnostics> {
    compile_with(model, &CompileOptions::default())
}

/// Compiles with overrides: icon resolution (`E_UNKNOWN_ICON`, `W_ICON_PACK_MISSING`) → layout → scene →
/// animation plan.
pub fn compile_with(model: &Model, options: &CompileOptions) -> Result<Diagram, Diagnostics> {
    let theme = Theme::builtin(options.theme.unwrap_or(model.theme));
    let themed = with_theme_defaults(model, &theme);
    let model = themed.as_ref();
    let icon_findings = icons::validate_icons_with(model, &options.icons);
    if icon_findings.has_errors() {
        return Err(icon_findings);
    }
    let scene = build_scene_with(model, &options.scene_options(model), &options.icons)?;
    let animation = AnimationPlan::from_model(model, &options.animation);
    let mut diagnostics = icon_findings;
    diagnostics.extend(scene.diagnostics.clone());
    diagnostics.sort();
    Ok(Diagram { model: model.clone(), scene, animation, diagnostics })
}

/// The model with the theme's defaults for what the spec left unwritten: the routing (`schematic` routes on
/// `rails`; edges that set their own `route` keep it) and the placement algorithm (`schematic` lays out with
/// `hub`). Themes without defaults borrow the model untouched, so their output does not change.
fn with_theme_defaults<'m>(model: &'m Model, theme: &Theme) -> Cow<'m, Model> {
    let routing = theme
        .edge_drawing
        .default_routing
        .filter(|routing| model.layout.routing_source != ValueSource::Spec && model.layout.routing != *routing);
    let algorithm = theme
        .default_layout
        .filter(|algorithm| model.layout.algorithm_source != ValueSource::Spec && model.layout.algorithm != *algorithm);
    if routing.is_none() && algorithm.is_none() {
        return Cow::Borrowed(model);
    }
    let mut themed = model.clone();
    if let Some(routing) = routing {
        themed.layout.routing = routing;
        for edge in themed.edges.iter_mut().filter(|edge| edge.route_source == ValueSource::Default) {
            edge.route = routing;
        }
    }
    if let Some(algorithm) = algorithm {
        themed.layout.algorithm = algorithm;
    }
    Cow::Owned(themed)
}

/// Text → [`Diagram`]: `dioramic_spec::load` (parse, normalize, validate) then [`compile_with`].
/// Spec warnings are merged into [`Diagram::diagnostics`].
pub fn compile_source(source: &str, format: SourceFormat, options: &CompileOptions) -> Result<Diagram, Diagnostics> {
    let validated = dioramic_spec::load(source, format)?;
    let mut diagram = compile_with(&validated.model, options)?;
    let mut diagnostics = validated.warnings;
    diagnostics.extend(diagram.diagnostics);
    diagnostics.sort();
    diagram.diagnostics = diagnostics;
    Ok(diagram)
}

/// Emits the diagram as SVG.
pub fn render_svg(diagram: &Diagram, options: &SvgOptions) -> String {
    emit_svg(&diagram.scene, diagram.animation.as_ref(), options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioramic_spec::{LayoutAlgorithm, Routing};

    fn model(extra: &str) -> Model {
        let source = format!(
            "diorama: 1\n{extra}nodes: [{{ id: web }}, {{ id: db, icon: database }}]\nedges:\n  - web -> db\n  - {{ from: db, to: web, route: straight }}\n"
        );
        match dioramic_spec::load(&source, SourceFormat::Yaml) {
            Ok(validated) => validated.model,
            Err(diagnostics) => panic!("{diagnostics:?}"),
        }
    }

    fn routes(model: &Model, theme: ThemeName) -> (Routing, Vec<Routing>) {
        let themed = with_theme_defaults(model, &Theme::builtin(theme));
        (themed.layout.routing, themed.edges.iter().map(|edge| edge.route).collect())
    }

    #[test]
    fn schematic_routes_on_rails_unless_the_spec_says_otherwise() {
        let unwritten = model("");
        assert_eq!(routes(&unwritten, ThemeName::Schematic), (Routing::Rails, vec![Routing::Rails, Routing::Straight]));
        assert_eq!(
            routes(&unwritten, ThemeName::Blueprint),
            (Routing::Curved, vec![Routing::Curved, Routing::Straight])
        );
        let written = model("layout: { routing: curved }\n");
        assert_eq!(routes(&written, ThemeName::Schematic).0, Routing::Curved, "a written routing wins");
    }

    #[test]
    fn schematic_lays_out_with_hub_unless_the_spec_says_otherwise() {
        let algorithm = |model: &Model, theme| with_theme_defaults(model, &Theme::builtin(theme)).layout.algorithm;
        let unwritten = model("");
        assert_eq!(algorithm(&unwritten, ThemeName::Schematic), LayoutAlgorithm::Hub);
        for theme in [ThemeName::Blueprint, ThemeName::Midnight, ThemeName::Paper, ThemeName::Mono] {
            assert_eq!(algorithm(&unwritten, theme), LayoutAlgorithm::Flow, "{theme:?}");
            assert!(matches!(with_theme_defaults(&unwritten, &Theme::builtin(theme)), Cow::Borrowed(_)));
        }
        let written = model("layout: { algorithm: flow }\n");
        assert_eq!(algorithm(&written, ThemeName::Schematic), LayoutAlgorithm::Flow, "a written algorithm wins");
    }

    #[test]
    fn a_theme_override_brings_its_routing_into_the_diagram() {
        let options = CompileOptions { theme: Some(ThemeName::Schematic), ..CompileOptions::default() };
        let diagram = compile_with(&model(""), &options).unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));
        assert_eq!(diagram.model.layout.routing, Routing::Rails);
    }
}
