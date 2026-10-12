//! `Engine::render` end to end on `examples/checkout.diorama.yaml` (needs the T-104 scene/SVG pipeline).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::PathBuf;

use dioramic_render::{
    Engine, GridRequest, OutputFormat, PreviewKind, PreviewMode, RenderError, RenderRequest, RenderResult,
};
use dioramic_spec::{GridStyle, LayoutAlgorithm, RotationSpec, Routing, ThemeName};

/// Small GIF width keeps the debug-build test fast; the 1600 px budget is measured in `gif_quality`.
const TEST_GIF_WIDTH_PX: u32 = 480;

fn checkout_request(format: OutputFormat) -> RenderRequest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/checkout.diorama.yaml");
    let mut request = RenderRequest::from_text(std::fs::read_to_string(path).unwrap());
    request.format = format;
    request
}

fn render(request: &RenderRequest) -> RenderResult {
    Engine::shared().render(request).unwrap()
}

fn assert_preview_fits(result: &RenderResult, kind: PreviewKind) {
    let preview = result.preview.as_ref().expect("preview requested by default");
    assert_eq!(preview.kind, kind);
    assert!(preview.width.max(preview.height) <= dioramic_render::PREVIEW_MAX_SIDE_PX);
    let decoded = png::Decoder::new(std::io::Cursor::new(&preview.png)).read_info().unwrap();
    assert_eq!((decoded.info().width, decoded.info().height), (preview.width, preview.height));
}

#[test]
fn png_is_decodable_with_summary_and_preview() {
    let result = render(&checkout_request(OutputFormat::Png));
    let decoder = png::Decoder::new(std::io::Cursor::new(&result.output)).read_info().unwrap();
    let summary = &result.summary;
    assert_eq!((decoder.info().width, decoder.info().height), (summary.width, summary.height));
    assert_eq!(summary.bytes, result.output.len() as u64);
    assert_eq!((summary.frames, summary.duration_ms), (None, None));
    assert_preview_fits(&result, PreviewKind::Image);
}

#[test]
fn png_is_deterministic() {
    let request = checkout_request(OutputFormat::Png);
    assert_eq!(render(&request).output, render(&request).output);
}

#[test]
fn png_width_override_sets_output_width() {
    let mut request = checkout_request(OutputFormat::Png);
    request.width_px = Some(640);
    request.preview = PreviewMode::None;
    let result = render(&request);
    assert_eq!(result.summary.width, 640);
    assert!(result.preview.is_none());
}

#[test]
fn svg_uses_outlines_by_default() {
    let result = render(&checkout_request(OutputFormat::Svg));
    let svg = String::from_utf8(result.output.clone()).unwrap();
    assert!(svg.trim_start().starts_with("<svg") || svg.trim_start().starts_with("<?xml"));
    assert!(!svg.contains("<text"), "outlines mode must not emit <text>");
    assert_preview_fits(&result, PreviewKind::Image);
}

#[test]
fn svg_animated_reports_loop_and_filmstrip() {
    let result = render(&checkout_request(OutputFormat::SvgAnimated));
    assert_eq!(result.summary.frames, Some(100));
    assert_eq!(result.summary.duration_ms, Some(2000));
    assert_preview_fits(&result, PreviewKind::Filmstrip);
}

#[test]
fn gif_has_all_frames_and_filmstrip() {
    let mut request = checkout_request(OutputFormat::Gif);
    request.width_px = Some(TEST_GIF_WIDTH_PX);
    let result = render(&request);
    let mut decoder = gif::DecodeOptions::new().read_info(std::io::Cursor::new(&result.output)).unwrap();
    assert_eq!(u32::from(decoder.width()), TEST_GIF_WIDTH_PX);
    let mut frames = 0;
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        assert_eq!(frame.delay, 2);
        frames += 1;
    }
    assert_eq!(Some(frames), result.summary.frames);
    assert_eq!(result.summary.width, TEST_GIF_WIDTH_PX);
    assert_preview_fits(&result, PreviewKind::Filmstrip);
}

#[test]
fn auto_rotation_reports_scores_and_overrides_apply() {
    let mut request = checkout_request(OutputFormat::Png);
    request.rotation = Some(RotationSpec::Auto);
    request.theme = Some(ThemeName::Midnight);
    request.preview = PreviewMode::None;
    let view = render(&request).summary.view;
    assert_eq!(view.theme, ThemeName::Midnight);
    let scores = view.rotation_scores.expect("auto fills rotation_scores");
    assert_eq!(scores.keys().map(String::as_str).collect::<Vec<_>>(), ["0", "180", "270", "90"]);
}

#[test]
fn invalid_spec_returns_diagnostics() {
    let request = RenderRequest::from_text("diorama: 1\nnodes:\n  - { id: a, icon: not-an-icon }\n");
    match Engine::shared().render(&request) {
        Err(RenderError::Invalid(diagnostics)) => assert!(diagnostics.has_errors()),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn out_of_range_scale_is_rejected() {
    let mut request = checkout_request(OutputFormat::Png);
    request.scale = 10.0;
    assert!(matches!(Engine::shared().render(&request), Err(RenderError::Request(_))));
}

#[test]
fn too_many_frames_is_a_limit_error() {
    let mut request = checkout_request(OutputFormat::Gif);
    request.animation.duration_s = Some(20.0);
    assert!(matches!(Engine::shared().render(&request), Err(RenderError::Limit { what: "frames", .. })));
}

fn svg_of(request: &RenderRequest) -> String {
    String::from_utf8(render(request).output).unwrap()
}

#[test]
fn spec_overrides_equal_to_the_spec_change_nothing() {
    let plain = checkout_request(OutputFormat::Svg);
    let mut explicit = checkout_request(OutputFormat::Svg);
    explicit.routing = Some(Routing::Curved);
    explicit.grid.style = Some(GridStyle::Lines);
    explicit.grid.size = Some(1.0);
    explicit.icon_animation = Some(true);
    explicit.layout_algorithm = Some(LayoutAlgorithm::Flow);
    assert_eq!(svg_of(&plain), svg_of(&explicit), "override path must compile exactly like the spec");
}

#[test]
fn routing_and_grid_overrides_reach_the_drawing_and_the_summary() {
    let mut request = checkout_request(OutputFormat::Svg);
    request.preview = PreviewMode::None;
    request.routing = Some(Routing::Rails);
    request.grid = GridRequest { size: Some(0.5), style: Some(GridStyle::Dots), opacity: Some(0.6), color: None };
    let result = render(&request);
    let view = &result.summary.view;
    assert_eq!(view.routing, Routing::Rails);
    assert_eq!((view.grid.size, view.grid.style, view.grid.opacity), (0.5, GridStyle::Dots, 0.6));
    assert_eq!(view.icon_animation, None, "static formats do not report icon motion");
    assert_ne!(result.output, render(&checkout_request(OutputFormat::Svg)).output);
    let json = serde_json::to_value(&result.summary).unwrap();
    assert_eq!(json["view"]["routing"], "rails");
    assert_eq!(json["view"]["grid"]["style"], "dots");
}

#[test]
fn still_icons_drop_their_motion_only_in_animated_output() {
    let mut moving = checkout_request(OutputFormat::SvgAnimated);
    moving.preview = PreviewMode::None;
    let mut still = moving.clone();
    still.icon_animation = Some(false);
    let (moving_result, still_result) = (render(&moving), render(&still));
    assert_eq!(moving_result.summary.view.icon_animation, Some(true));
    assert_eq!(still_result.summary.view.icon_animation, Some(false));
    let animations = |result: &RenderResult| String::from_utf8_lossy(&result.output).matches("<animate").count();
    assert!(animations(&still_result) < animations(&moving_result));

    let png = |icon_animation| {
        let mut request = checkout_request(OutputFormat::Png);
        request.preview = PreviewMode::None;
        request.icon_animation = icon_animation;
        render(&request).output
    };
    assert_eq!(png(None), png(Some(false)), "a still PNG is the same PNG");
}

#[test]
fn bad_grid_override_is_a_spec_error() {
    let mut request = checkout_request(OutputFormat::Png);
    request.grid.size = Some(9.0);
    match Engine::shared().render(&request) {
        Err(RenderError::Invalid(diagnostics)) => {
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.code.as_str() == "E_BAD_VALUE"), "{diagnostics:?}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn render_request_round_trips_through_json_with_the_new_fields() {
    let mut request = checkout_request(OutputFormat::Gif);
    request.routing = Some(Routing::Rails);
    request.grid.style = Some(GridStyle::None);
    request.icon_animation = Some(false);
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["routing"], "rails");
    let back: RenderRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back, request);
    let legacy: RenderRequest =
        serde_json::from_value(serde_json::json!({ "source": { "text": { "text": "diorama: 1" } } })).unwrap();
    assert!(!legacy.has_spec_overrides());
}

#[test]
fn schematic_lays_out_with_hub_unless_the_request_says_otherwise() {
    let summary_of = |theme, algorithm| {
        let mut request = checkout_request(OutputFormat::Svg);
        request.preview = PreviewMode::None;
        request.theme = Some(theme);
        request.layout_algorithm = algorithm;
        render(&request)
    };
    let schematic = summary_of(ThemeName::Schematic, None);
    assert_eq!(schematic.summary.view.layout_algorithm, LayoutAlgorithm::Hub);
    assert_eq!(serde_json::to_value(&schematic.summary).unwrap()["view"]["layout_algorithm"], "hub");
    let flow = summary_of(ThemeName::Schematic, Some(LayoutAlgorithm::Flow));
    assert_eq!(flow.summary.view.layout_algorithm, LayoutAlgorithm::Flow);
    assert_ne!(schematic.output, flow.output, "the override changes the layout");
    let blueprint = summary_of(ThemeName::Blueprint, None);
    assert_eq!(blueprint.summary.view.layout_algorithm, LayoutAlgorithm::Flow, "classic themes keep flow");
    let hub = summary_of(ThemeName::Blueprint, Some(LayoutAlgorithm::Hub));
    assert_eq!(hub.summary.view.layout_algorithm, LayoutAlgorithm::Hub);
}
