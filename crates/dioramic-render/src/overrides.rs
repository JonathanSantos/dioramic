//! Request overrides that live in the spec itself (`layout.routing`, `layout.algorithm`, `view.grid`,
//! `animation.icons`).
//!
//! Theme, rotation and density travel to the core as compile options; these three are spec fields, so the
//! engine writes them into the spec **before** normalization and validation. That keeps one meaning for each
//! value (an override is exactly the spec with that field set, which is what the desktop "Pin" writes) and
//! the usual diagnostics for bad values (`E_BAD_VALUE`, `E_BAD_COLOR`).

use dioramic_core::animation::{DEFAULT_DURATION_S, DEFAULT_FPS};
use dioramic_spec::{Animation, AnimationPreset, AnimationSpec, GridSpec, Model, SpecV1, ValueSource, ViewSpec};

use crate::request::{GridRequest, RenderRequest};

/// Writes the request overrides into a parsed spec.
pub fn apply_to_spec(spec: &mut SpecV1, request: &RenderRequest) {
    if let Some(routing) = request.routing {
        spec.layout.get_or_insert_with(Default::default).routing = Some(routing);
    }
    if let Some(algorithm) = request.layout_algorithm {
        spec.layout.get_or_insert_with(Default::default).algorithm = Some(algorithm);
    }
    if !request.grid.is_empty() {
        let view = spec.view.get_or_insert_with(ViewSpec::default);
        merge_grid(view.grid.get_or_insert_with(GridSpec::default), &request.grid);
    }
    match (spec.animation.as_mut(), request.icon_animation) {
        (Some(animation), Some(moving)) => animation.icons = Some(moving),
        // Icons already move when the spec has no animation block; only "still" needs one.
        (None, Some(false)) => {
            spec.animation = Some(AnimationSpec { icons: Some(false), ..AnimationSpec::default() });
        }
        _ => {}
    }
}

/// Writes the request overrides into an already normalized model. Edges whose route came from the old
/// `layout.routing` follow the new one; edges with another route set it themselves and keep it.
pub fn apply_to_model(model: &mut Model, request: &RenderRequest) {
    if let Some(routing) = request.routing {
        let inherited = model.layout.routing;
        for edge in model.edges.iter_mut().filter(|edge| edge.route == inherited) {
            edge.route = routing;
        }
        model.layout.routing = routing;
        // An override is as good as written: the theme's default routing must not replace it.
        model.layout.routing_source = ValueSource::Spec;
    }
    if let Some(algorithm) = request.layout_algorithm {
        model.layout.algorithm = algorithm;
        // Same as the routing: the theme's default algorithm must not replace an explicit choice.
        model.layout.algorithm_source = ValueSource::Spec;
    }
    let grid = &mut model.view.grid;
    let GridRequest { size, style, opacity, color } = &request.grid;
    grid.size = size.unwrap_or(grid.size);
    grid.style = style.unwrap_or(grid.style);
    grid.opacity = opacity.unwrap_or(grid.opacity);
    if color.is_some() {
        grid.color.clone_from(color);
    }
    match (model.animation.as_mut(), request.icon_animation) {
        (Some(animation), Some(moving)) => animation.icons = moving,
        (None, Some(false)) => model.animation = Some(still_icons_flow()),
        _ => {}
    }
}

fn merge_grid(grid: &mut GridSpec, request: &GridRequest) {
    grid.size = request.size.or(grid.size);
    grid.style = request.style.or(grid.style);
    grid.opacity = request.opacity.or(grid.opacity);
    if request.color.is_some() {
        grid.color.clone_from(&request.color);
    }
}

/// The animation a spec without an `animation` block gets when asked for an animated format (`flow`, 2 s,
/// 50 fps, looping), with the icons still. Same plan as the engine's fallback, so only the icons change.
fn still_icons_flow() -> Animation {
    Animation {
        preset: AnimationPreset::Flow,
        fps: DEFAULT_FPS,
        duration_s: DEFAULT_DURATION_S,
        looped: true,
        icons: false,
        steps: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use dioramic_spec::{GridStyle, LayoutAlgorithm, Routing, SourceFormat, normalize, parse};

    use super::*;

    const SPEC: &str = "diorama: 1\nnodes:\n  - { id: a }\n  - { id: b }\n  - { id: c }\nedges:\n  - a -> b\n  - { from: b, to: c, route: straight }\n";

    fn request() -> RenderRequest {
        RenderRequest::from_text(SPEC)
    }

    fn spec() -> SpecV1 {
        parse(SPEC, SourceFormat::Yaml).unwrap()
    }

    #[test]
    fn spec_overrides_set_only_what_was_asked() {
        let mut request = request();
        request.routing = Some(Routing::Rails);
        request.grid.size = Some(0.5);
        request.grid.style = Some(GridStyle::Dots);
        let mut overridden = spec();
        apply_to_spec(&mut overridden, &request);
        let model = normalize(overridden).unwrap();
        assert_eq!(model.layout.routing, Routing::Rails);
        assert_eq!(model.edges[0].route, Routing::Rails);
        assert_eq!(model.edges[1].route, Routing::Straight, "an edge's own route wins, as in the spec");
        assert_eq!((model.view.grid.size, model.view.grid.style, model.view.grid.opacity), (0.5, GridStyle::Dots, 1.0));
        assert!(model.animation.is_none(), "icons untouched: no animation block appears");
    }

    #[test]
    fn grid_override_keeps_the_spec_fields_it_does_not_name() {
        let mut overridden =
            parse(&format!("{SPEC}view: {{ grid: {{ size: 2, color: teal }} }}\n"), SourceFormat::Yaml).unwrap();
        let mut request = request();
        request.grid.opacity = Some(0.4);
        apply_to_spec(&mut overridden, &request);
        let grid = normalize(overridden).unwrap().view.grid;
        assert_eq!((grid.size, grid.opacity), (2.0, 0.4));
        assert!(grid.color.is_some());
    }

    #[test]
    fn still_icons_need_an_animation_block_and_moving_ones_do_not() {
        let mut still = request();
        still.icon_animation = Some(false);
        let mut overridden = spec();
        apply_to_spec(&mut overridden, &still);
        let animation = normalize(overridden).unwrap().animation.unwrap();
        assert_eq!(animation, still_icons_flow(), "same as the engine's flow fallback, icons still");

        let mut moving = request();
        moving.icon_animation = Some(true);
        let mut untouched = spec();
        apply_to_spec(&mut untouched, &moving);
        assert_eq!(untouched, spec());
    }

    #[test]
    fn model_overrides_follow_the_inherited_route_only() {
        let mut model = normalize(spec()).unwrap();
        let mut request = request();
        request.routing = Some(Routing::Orthogonal);
        request.grid.style = Some(GridStyle::None);
        request.icon_animation = Some(false);
        apply_to_model(&mut model, &request);
        assert_eq!(model.layout.routing, Routing::Orthogonal);
        assert_eq!((model.edges[0].route, model.edges[1].route), (Routing::Orthogonal, Routing::Straight));
        assert_eq!(model.view.grid.style, GridStyle::None);
        assert_eq!(model.animation.map(|animation| animation.icons), Some(false));
    }

    #[test]
    fn layout_algorithm_override_is_written_like_the_spec_field() {
        let mut request = request();
        request.layout_algorithm = Some(LayoutAlgorithm::Hub);
        let mut overridden = spec();
        apply_to_spec(&mut overridden, &request);
        let model = normalize(overridden).unwrap();
        assert_eq!((model.layout.algorithm, model.layout.algorithm_source), (LayoutAlgorithm::Hub, ValueSource::Spec));

        let mut model = normalize(spec()).unwrap();
        request.layout_algorithm = Some(LayoutAlgorithm::Flow);
        apply_to_model(&mut model, &request);
        assert_eq!(model.layout.algorithm_source, ValueSource::Spec, "the theme's default must not replace it");
    }
}
