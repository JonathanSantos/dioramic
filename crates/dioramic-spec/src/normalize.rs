//! [`SpecV1`] → [`Model`]: expands edge shorthand (string and single-key map), applies defaults,
//! assigns edge ids (`from->to`, `from->to#2` for parallel edges), expands `animation.steps[].path`,
//! and parses durations. Never consults the icon catalog.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;

use crate::common::{ArrowEnds, ColorRef, Density, EdgeKind, RotationSpec, Routing, ValueSource};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::model::{
    Animation, AnimationPreset, AnimationStep, Edge, EdgeId, EdgeStyle, FlowMode, GridOptions, LayoutOptions, Model,
    Node, NodeId, PlanCoord, ViewOptions, Zone, ZoneId,
};
use crate::raw::{
    AnimationSpec, AnimationStepSpec, DurationSpec, EdgeDetails, EdgeEntry, EdgeKeyedValue, EdgeSpec, EdgeStyleSpec,
    FlowDirection, FlowSpec, NodeSpec, SpecV1, ViewSpec, ZoneSpec,
};
use crate::shorthand::{EdgeShorthand, parse_edge_shorthand};

/// Style used when an edge names none (a theme style, always available).
pub(crate) const DEFAULT_EDGE_STYLE: &str = "primary";
/// Raster frames per second when `animation.fps` is absent (`docs/07-animacao.md`: 50 fps is the GIF maximum).
const DEFAULT_FPS: u32 = 50;
/// Loop and story-step duration when none is given.
const DEFAULT_DURATION_SECONDS: f64 = 2.0;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;
/// Separator between the base edge id and the parallel-edge counter (`a->b#2`).
const PARALLEL_EDGE_SEPARATOR: char = '#';
/// Field names of the long form, used to recognize a malformed long-form edge.
const LONG_FORM_FROM: &str = "from";
const LONG_FORM_TO: &str = "to";

/// Normalizes a parsed spec. Fails with `E_EDGE_SYNTAX` / `E_BAD_VALUE` diagnostics
/// (all of them, not just the first).
pub fn normalize(spec: SpecV1) -> Result<Model, Diagnostics> {
    let mut diagnostics = Diagnostics::new();
    let layout = layout_options(&spec);
    let edges = normalize_edges(spec.edges, layout.routing, &mut diagnostics);
    let animation = spec.animation.map(|animation| normalize_animation(animation, &edges, &mut diagnostics));
    if diagnostics.has_errors() {
        diagnostics.sort();
        return Err(diagnostics);
    }
    Ok(Model {
        title: spec.title,
        subtitle: spec.subtitle,
        theme: spec.theme.unwrap_or_default(),
        layout,
        view: normalize_view(spec.view, layout.density),
        styles: normalize_styles(spec.styles),
        zones: spec.zones.into_iter().enumerate().map(|(index, zone)| normalize_zone(index, zone)).collect(),
        nodes: spec.nodes.into_iter().enumerate().map(|(index, node)| normalize_node(index, node)).collect(),
        edges,
        animation,
    })
}

fn layout_options(spec: &SpecV1) -> LayoutOptions {
    let layout = spec.layout.clone().unwrap_or_default();
    LayoutOptions {
        flow: layout.flow.unwrap_or_default(),
        density: layout.density.unwrap_or_default(),
        wrap: layout.wrap.unwrap_or_default(),
        routing: layout.routing.unwrap_or_default(),
        routing_source: source_of(layout.routing.as_ref()),
        edge_colors: layout.edge_colors,
        algorithm: layout.algorithm.unwrap_or_default(),
        algorithm_source: source_of(layout.algorithm.as_ref()),
    }
}

/// [`ValueSource::Spec`] when the field was written.
fn source_of<T>(value: Option<&T>) -> ValueSource {
    if value.is_some() { ValueSource::Spec } else { ValueSource::Default }
}

fn normalize_styles(styles: IndexMap<String, EdgeStyleSpec>) -> IndexMap<String, EdgeStyle> {
    styles
        .into_iter()
        .map(|(name, style)| (name, EdgeStyle { color: style.color, width: style.width, dashed: style.dashed }))
        .collect()
}

fn plan_coord(at: Option<[f64; 2]>) -> Option<PlanCoord> {
    at.map(|[x, y]| PlanCoord { x, y })
}

fn normalize_zone(index: usize, zone: ZoneSpec) -> Zone {
    Zone {
        id: ZoneId(zone.id),
        label: zone.label,
        color: zone.color,
        parent: zone.parent.map(ZoneId),
        arrange: zone.arrange.unwrap_or_default(),
        at: plan_coord(zone.at),
        size: zone.size,
        border: zone.border.unwrap_or_default(),
        path: format!("zones[{index}]"),
    }
}

fn normalize_node(index: usize, node: NodeSpec) -> Node {
    Node {
        id: NodeId(node.id),
        icon: node.icon,
        label: node.label,
        zone: node.zone.map(ZoneId),
        color: node.color,
        replicas: node.replicas.unwrap_or(1),
        badge: node.badge,
        at: plan_coord(node.at),
        z: node.z.unwrap_or(0.0),
        size: node.size.unwrap_or_default(),
        note: node.note,
        path: format!("nodes[{index}]"),
    }
}

/// An edge after its shape (string, single-key map, long form) has been resolved, before ids are assigned.
struct EdgeDraft {
    id: Option<String>,
    from: String,
    to: String,
    label: Option<String>,
    style: Option<String>,
    kind: Option<EdgeKind>,
    color: Option<ColorRef>,
    width: Option<f64>,
    dashed: Option<bool>,
    arrow: ArrowEnds,
    route: Option<Routing>,
    flow: Option<FlowSpec>,
}

impl EdgeDraft {
    fn from_shorthand(shorthand: EdgeShorthand) -> Self {
        Self {
            id: None,
            dashed: shorthand.op.is_dashed().then_some(true),
            arrow: shorthand.op.arrow(),
            from: shorthand.from,
            to: shorthand.to,
            label: shorthand.label,
            style: shorthand.style,
            kind: None,
            color: None,
            width: None,
            route: None,
            flow: None,
        }
    }

    fn from_long(edge: EdgeSpec) -> Self {
        Self {
            id: edge.id,
            from: edge.from,
            to: edge.to,
            label: edge.label,
            style: edge.style,
            kind: edge.kind,
            color: edge.color,
            width: edge.width,
            dashed: edge.dashed,
            arrow: edge.arrow.unwrap_or_default(),
            route: edge.route,
            flow: edge.flow,
        }
    }

    /// Explicit fields of a single-key map value win over what the operator implied.
    fn apply(mut self, details: EdgeDetails) -> Self {
        self.id = details.id.or(self.id);
        self.label = details.label.or(self.label);
        self.style = details.style.or(self.style);
        self.kind = details.kind.or(self.kind);
        self.color = details.color.or(self.color);
        self.width = details.width.or(self.width);
        self.dashed = details.dashed.or(self.dashed);
        self.arrow = details.arrow.unwrap_or(self.arrow);
        self.route = details.route.or(self.route);
        self.flow = details.flow.or(self.flow);
        self
    }
}

/// `view` with defaults: rotation by density, grid of v1.
fn normalize_view(view: Option<ViewSpec>, density: Density) -> ViewOptions {
    let view = view.unwrap_or_default();
    let grid = view.grid.unwrap_or_default();
    let defaults = GridOptions::default();
    ViewOptions {
        rotation: view.rotation.unwrap_or_else(|| RotationSpec::default_for(density)),
        grid: GridOptions {
            size: grid.size.unwrap_or(defaults.size),
            style: grid.style.unwrap_or(defaults.style),
            opacity: grid.opacity.unwrap_or(defaults.opacity),
            color: grid.color,
        },
        legend: view.legend.unwrap_or_default(),
    }
}

fn normalize_edges(entries: Vec<EdgeEntry>, routing: Routing, diagnostics: &mut Diagnostics) -> Vec<Edge> {
    let mut ids = EdgeIdAllocator::default();
    entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let path = format!("edges[{index}]");
            match edge_draft(entry) {
                Ok(draft) => Some(finish_edge(draft, path, routing, &mut ids)),
                Err(message) => {
                    diagnostics.push(edge_syntax_diagnostic(path, message));
                    None
                }
            }
        })
        .collect()
}

fn edge_syntax_diagnostic(path: String, message: String) -> Diagnostic {
    Diagnostic::new(DiagnosticCode::EdgeSyntax, path, message).with_hint("write edges as \"from -> to: label @style\"")
}

fn edge_draft(entry: EdgeEntry) -> Result<EdgeDraft, String> {
    match entry {
        EdgeEntry::Shorthand(text) => shorthand_draft(&text),
        EdgeEntry::Long(edge) => Ok(EdgeDraft::from_long(edge)),
        EdgeEntry::Keyed(map) => keyed_draft(map),
    }
}

fn shorthand_draft(text: &str) -> Result<EdgeDraft, String> {
    parse_edge_shorthand(text)
        .map(EdgeDraft::from_shorthand)
        .map_err(|error| format!("cannot parse edge '{text}': {error}"))
}

/// `{ "a -> b": value }` is what YAML makes of an unquoted shorthand; rebuilding the string gives the
/// exact same edge as the quoted form.
fn keyed_draft(map: IndexMap<String, Option<EdgeKeyedValue>>) -> Result<EdgeDraft, String> {
    if map.contains_key(LONG_FORM_FROM) || map.contains_key(LONG_FORM_TO) {
        // serde fell back to the map shape because the long form did not deserialize.
        return Err("long-form edge has a missing, unknown or invalid field; allowed: id, from, to, label, style, \
             kind, color, width, dashed, arrow, route, flow"
            .to_owned());
    }
    if map.len() != 1 {
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        return Err(format!(
            "an edge map must have exactly one key 'from -> to', found {} ({})",
            keys.len(),
            keys.join(", ")
        ));
    }
    let Some((key, value)) = map.into_iter().next() else {
        return Err("empty edge map".to_owned());
    };
    match value {
        None => shorthand_draft(&key),
        Some(EdgeKeyedValue::Text(text)) => shorthand_draft(&format!("{key}: {text}")),
        Some(EdgeKeyedValue::Number(number)) => shorthand_draft(&format!("{key}: {number}")),
        Some(EdgeKeyedValue::Details(details)) => shorthand_draft(&key).map(|draft| draft.apply(details)),
    }
}

fn finish_edge(draft: EdgeDraft, path: String, routing: Routing, ids: &mut EdgeIdAllocator) -> Edge {
    let id = match draft.id {
        Some(explicit) => ids.reserve(explicit),
        None => ids.allocate(&draft.from, &draft.to),
    };
    Edge {
        id: EdgeId(id),
        from: NodeId(draft.from),
        to: NodeId(draft.to),
        label: draft.label,
        style_source: source_of(draft.style.as_ref()),
        style: draft.style.unwrap_or_else(|| DEFAULT_EDGE_STYLE.to_owned()),
        kind: draft.kind,
        color: draft.color,
        width: draft.width,
        dashed: draft.dashed,
        arrow: draft.arrow,
        route_source: source_of(draft.route.as_ref()),
        route: draft.route.unwrap_or(routing),
        flow: flow_mode(draft.flow),
        path,
    }
}

fn flow_mode(flow: Option<FlowSpec>) -> FlowMode {
    match flow {
        None | Some(FlowSpec::Enabled(false)) => FlowMode::Off,
        Some(FlowSpec::Enabled(true) | FlowSpec::Direction(FlowDirection::Forward)) => FlowMode::Forward,
        Some(FlowSpec::Direction(FlowDirection::Reverse)) => FlowMode::Reverse,
    }
}

/// Hands out `from->to`, `from->to#2`… in spec order. Explicit ids are recorded so generated ones never collide
/// with them; duplicated explicit ids are left for `validate` to report.
#[derive(Default)]
struct EdgeIdAllocator {
    taken: BTreeSet<String>,
    parallel_count: BTreeMap<String, u32>,
}

impl EdgeIdAllocator {
    fn reserve(&mut self, id: String) -> String {
        self.taken.insert(id.clone());
        id
    }

    fn allocate(&mut self, from: &str, to: &str) -> String {
        let base = format!("{from}->{to}");
        loop {
            let count = self.parallel_count.entry(base.clone()).or_insert(0);
            *count += 1;
            let candidate = if *count == 1 { base.clone() } else { format!("{base}{PARALLEL_EDGE_SEPARATOR}{count}") };
            if self.taken.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

fn normalize_animation(animation: AnimationSpec, edges: &[Edge], diagnostics: &mut Diagnostics) -> Animation {
    let preset = animation.preset.unwrap_or_default();
    let duration_s = resolve_duration(animation.duration.as_ref(), "animation.duration", diagnostics)
        .unwrap_or(DEFAULT_DURATION_SECONDS);
    let steps = animation
        .steps
        .into_iter()
        .enumerate()
        .map(|(index, step)| normalize_step(index, step, edges, diagnostics))
        .collect();
    Animation {
        preset,
        fps: animation.fps.unwrap_or(DEFAULT_FPS),
        duration_s,
        looped: animation.looped.unwrap_or(loops_by_default(preset)),
        icons: animation.icons.unwrap_or(true),
        steps,
    }
}

/// `flow` and `pulse` are continuous effects; `build` holds its last frame and `story` loops only on request.
fn loops_by_default(preset: AnimationPreset) -> bool {
    matches!(preset, AnimationPreset::Flow | AnimationPreset::Pulse)
}

fn normalize_step(
    index: usize,
    step: AnimationStepSpec,
    edges: &[Edge],
    diagnostics: &mut Diagnostics,
) -> AnimationStep {
    let path = format!("animation.steps[{index}].duration");
    let duration_s = resolve_duration(step.duration.as_ref(), &path, diagnostics).unwrap_or(DEFAULT_DURATION_SECONDS);
    let mut step_edges: Vec<EdgeId> =
        step.path.windows(2).map(|pair| edge_between(edges, &pair[0], &pair[1])).collect();
    step_edges.extend(step.edges.into_iter().map(EdgeId));
    let mut step_nodes: Vec<NodeId> = step.path.into_iter().map(NodeId).collect();
    step_nodes.extend(step.nodes.into_iter().map(NodeId));
    AnimationStep {
        edges: dedup_in_order(step_edges),
        nodes: dedup_in_order(step_nodes),
        effect: step.effect,
        caption: step.caption,
        duration_s,
    }
}

/// The edge a story path walks from `from` to `to`: same direction first, then the reverse one. When no edge
/// links them, the would-be id is kept so `validate` reports it as an unknown reference.
fn edge_between(edges: &[Edge], from: &str, to: &str) -> EdgeId {
    let links = |edge: &&Edge, a: &str, b: &str| edge.from.as_str() == a && edge.to.as_str() == b;
    edges
        .iter()
        .find(|edge| links(edge, from, to))
        .or_else(|| edges.iter().find(|edge| links(edge, to, from)))
        .map(|edge| edge.id.clone())
        .unwrap_or_else(|| EdgeId(format!("{from}->{to}")))
}

fn dedup_in_order<T: Ord + Clone>(items: Vec<T>) -> Vec<T> {
    let mut seen = BTreeSet::new();
    items.into_iter().filter(|item| seen.insert(item.clone())).collect()
}

/// `None` when absent or malformed (malformed also pushes `E_BAD_VALUE`).
fn resolve_duration(duration: Option<&DurationSpec>, path: &str, diagnostics: &mut Diagnostics) -> Option<f64> {
    let duration = duration?;
    match parse_duration(duration) {
        Some(seconds) => Some(seconds),
        None => {
            diagnostics.push(
                Diagnostic::new(DiagnosticCode::BadValue, path, format!("invalid duration {}", describe(duration)))
                    .with_hint("use seconds (2), \"2.5s\" or \"800ms\""),
            );
            None
        }
    }
}

fn describe(duration: &DurationSpec) -> String {
    match duration {
        DurationSpec::Seconds(seconds) => seconds.to_string(),
        DurationSpec::Text(text) => format!("'{text}'"),
    }
}

/// Seconds from `2`, `"2"`, `"2.5s"` or `"800ms"`. Range checks (> 0) are done by `validate`.
fn parse_duration(duration: &DurationSpec) -> Option<f64> {
    let seconds = match duration {
        DurationSpec::Seconds(seconds) => *seconds,
        DurationSpec::Text(text) => parse_duration_text(text.trim())?,
    };
    seconds.is_finite().then_some(seconds)
}

fn parse_duration_text(text: &str) -> Option<f64> {
    if let Some(milliseconds) = text.strip_suffix("ms") {
        return milliseconds.trim().parse::<f64>().ok().map(|value| value / MILLISECONDS_PER_SECOND);
    }
    text.strip_suffix('s').unwrap_or(text).trim().parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::LayoutAlgorithm;
    use crate::common::Rotation;
    use crate::parse::{SourceFormat, parse};

    fn model(yaml: &str) -> Model {
        let spec = parse(yaml, SourceFormat::Yaml).unwrap_or_else(|error| panic!("{error}"));
        normalize(spec).unwrap_or_else(|error| panic!("{error}"))
    }

    fn errors(yaml: &str) -> Diagnostics {
        let spec = parse(yaml, SourceFormat::Yaml).unwrap_or_else(|error| panic!("{error}"));
        match normalize(spec) {
            Ok(model) => panic!("expected errors, got {model:?}"),
            Err(diagnostics) => diagnostics,
        }
    }

    const NODES: &str = "diorama: 1\nnodes: [{ id: fw }, { id: core }, { id: lb }, { id: api1 }]\n";

    fn edges_of(edges_yaml: &str) -> Vec<Edge> {
        model(&format!("{NODES}edges:\n{edges_yaml}")).edges
    }

    #[test]
    fn rotation_defaults_to_auto_only_when_compact() {
        let compact = model(&format!("{NODES}layout: {{ density: compact }}\n"));
        assert_eq!(compact.view.rotation, RotationSpec::Auto);
        assert_eq!(model(NODES).view.rotation, RotationSpec::Fixed(Rotation::R0));
        let spaced = model(&format!("{NODES}layout: {{ density: spaced }}\n"));
        assert_eq!(spaced.view.rotation, RotationSpec::Fixed(Rotation::R0));
    }

    #[test]
    fn explicit_rotation_wins_over_the_compact_default() {
        let fixed = model(&format!("{NODES}layout: {{ density: compact }}\nview: {{ rotation: 0 }}\n"));
        assert_eq!(fixed.view.rotation, RotationSpec::Fixed(Rotation::R0));
        let turned = model(&format!("{NODES}layout: {{ density: compact }}\nview: {{ rotation: 270 }}\n"));
        assert_eq!(turned.view.rotation, RotationSpec::Fixed(Rotation::R270));
    }

    #[test]
    fn unquoted_map_equals_quoted_string() {
        let edges = edges_of("  - fw -> core: 1Gbps trunk @trunk\n  - \"fw -> core: 1Gbps trunk @trunk\"\n");
        assert_eq!(edges[0].label.as_deref(), Some("1Gbps trunk"));
        assert_eq!(edges[0].style, "trunk");
        let same_except_identity = |edge: &Edge| Edge { id: EdgeId::from("x"), path: String::new(), ..edge.clone() };
        assert_eq!(same_except_identity(&edges[0]), same_except_identity(&edges[1]));
    }

    #[test]
    fn keyed_values_number_null_and_details() {
        let edges = edges_of(
            "  - fw -> core: 443\n  - lb -> api1 @primary\n  - lb -> api1:\n  - fw <- core: { label: x, style: s, flow: true, arrow: both }\n",
        );
        assert_eq!(edges[0].label.as_deref(), Some("443"));
        assert_eq!(edges[1].style, "primary");
        assert_eq!(edges[2].label, None);
        assert_eq!(edges[3].label.as_deref(), Some("x"));
        assert_eq!(edges[3].style, "s");
        assert_eq!(edges[3].flow, FlowMode::Forward);
        assert_eq!(edges[3].arrow, ArrowEnds::Both);
    }

    #[test]
    fn multi_key_map_is_edge_syntax_error() {
        let found = errors(&format!("{NODES}edges:\n  - {{ \"fw -> core\": a, \"lb -> api1\": b }}\n"));
        assert_eq!(found.0[0].code, DiagnosticCode::EdgeSyntax);
        assert_eq!(found.0[0].path, "edges[0]");
    }

    #[test]
    fn malformed_long_form_is_edge_syntax_error() {
        let found = errors(&format!("{NODES}edges:\n  - {{ from: fw, to: core, colour: red }}\n"));
        assert_eq!(found.0[0].code, DiagnosticCode::EdgeSyntax);
        assert!(found.0[0].message.contains("long-form"));
    }

    #[test]
    fn bad_shorthand_is_edge_syntax_error_and_all_are_reported() {
        let found = errors(&format!("{NODES}edges:\n  - \"fw core\"\n  - \"lb -> api1 @two words\"\n"));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|diagnostic| diagnostic.code == DiagnosticCode::EdgeSyntax));
    }

    #[test]
    fn operators_set_arrow_and_dash_and_overrides_win() {
        let edges = edges_of(
            "  - \"fw <- core\"\n  - \"fw ..> core\"\n  - \"fw .. core\"\n  - fw ..> core: { dashed: false, arrow: none }\n  - { from: fw, to: core }\n",
        );
        assert_eq!((edges[0].from.as_str(), edges[0].to.as_str()), ("fw", "core"));
        assert_eq!(edges[0].arrow, ArrowEnds::Start);
        assert_eq!(edges[0].dashed, None);
        assert_eq!((edges[1].arrow, edges[1].dashed), (ArrowEnds::End, Some(true)));
        assert_eq!((edges[2].arrow, edges[2].dashed), (ArrowEnds::None, Some(true)));
        assert_eq!((edges[3].arrow, edges[3].dashed), (ArrowEnds::None, Some(false)));
        assert_eq!((edges[4].arrow, edges[4].dashed), (ArrowEnds::End, None));
    }

    #[test]
    fn parallel_edges_get_numbered_ids_and_paths() {
        let edges = edges_of(
            "  - fw -> core\n  - fw -> core\n  - { id: fw->core#3, from: lb, to: api1 }\n  - fw -> core\n  - core -> fw\n",
        );
        let ids: Vec<&str> = edges.iter().map(|edge| edge.id.as_str()).collect();
        assert_eq!(ids, ["fw->core", "fw->core#2", "fw->core#3", "fw->core#4", "core->fw"]);
        assert_eq!(edges[3].path, "edges[3]");
    }

    #[test]
    fn layout_algorithm_defaults_to_flow_and_records_its_source() {
        let plain = model(NODES);
        assert_eq!(
            (plain.layout.algorithm, plain.layout.algorithm_source),
            (LayoutAlgorithm::Flow, ValueSource::Default)
        );
        let hub = model(&format!("{NODES}layout: {{ algorithm: hub }}\n"));
        assert_eq!((hub.layout.algorithm, hub.layout.algorithm_source), (LayoutAlgorithm::Hub, ValueSource::Spec));
        let serialized = serde_json::to_value(plain.layout).unwrap_or_default();
        assert!(serialized.get("algorithm").is_none(), "the default stays out of serialized models");
    }

    #[test]
    fn defaults_are_applied() {
        let model = model(&format!(
            "{NODES}layout: {{ routing: orthogonal }}\nzones: [{{ id: z }}]\nedges:\n  - fw -> core\n  - {{ from: lb, to: api1, route: straight, flow: reverse }}\n"
        ));
        assert_eq!(model.nodes[0].replicas, 1);
        assert_eq!(model.nodes[1].path, "nodes[1]");
        assert_eq!(model.zones[0].path, "zones[0]");
        assert_eq!(model.edges[0].style, DEFAULT_EDGE_STYLE);
        assert_eq!(model.edges[0].route, Routing::Orthogonal);
        assert_eq!(model.edges[1].route, Routing::Straight);
        let rails = self::tests::model(&format!("{NODES}layout: {{ routing: rails }}\nedges:\n  - fw -> core\n"));
        assert_eq!(rails.edges[0].route, Routing::Rails);
        let elbow = self::tests::model(&format!("{NODES}layout: {{ routing: elbow }}\nedges:\n  - fw -> core\n"));
        assert_eq!(elbow.edges[0].route, Routing::Rails, "elbow is another name for rails");
        assert_eq!(rails.view.grid, GridOptions::default());
        let gridded = self::tests::model(&format!("{NODES}view: {{ grid: {{ size: 2, style: dots }} }}\n"));
        assert_eq!((gridded.view.grid.size, gridded.view.grid.style), (2.0, crate::GridStyle::Dots));
        assert_eq!(gridded.view.grid.opacity, 1.0);
        assert_eq!(model.edges[1].flow, FlowMode::Reverse);
        assert_eq!(model.animation, None);
    }

    #[test]
    fn animation_defaults_and_durations() {
        let flow = model(&format!("{NODES}animation: {{ preset: flow }}\n")).animation;
        assert_eq!(
            flow.as_ref().map(|animation| (animation.fps, animation.duration_s, animation.looped)),
            Some((50, 2.0, true))
        );
        assert_eq!(flow.as_ref().map(|animation| animation.icons), Some(true), "icons move by default");
        let still = model(&format!("{NODES}animation: {{ preset: flow, icons: false }}\n")).animation;
        assert_eq!(still.map(|animation| animation.icons), Some(false));
        let build = model(&format!("{NODES}animation: {{ preset: build, duration: 800ms }}\n")).animation;
        assert_eq!(build.as_ref().map(|animation| (animation.duration_s, animation.looped)), Some((0.8, false)));
        for (text, seconds) in [("2", 2.0), ("\"2.5s\"", 2.5), ("\"800ms\"", 0.8), ("\"3\"", 3.0), ("1.5", 1.5)] {
            let animation = model(&format!("{NODES}animation: {{ duration: {text} }}\n")).animation;
            assert_eq!(animation.map(|animation| animation.duration_s), Some(seconds), "{text}");
        }
    }

    #[test]
    fn malformed_duration_is_bad_value() {
        let found = errors(&format!("{NODES}animation: {{ steps: [{{ nodes: [fw], duration: soon }}] }}\n"));
        assert_eq!(found.0[0].code, DiagnosticCode::BadValue);
        assert_eq!(found.0[0].path, "animation.steps[0].duration");
    }

    #[test]
    fn story_path_expands_to_edge_ids() {
        let model = model(&format!(
            "{NODES}edges:\n  - fw -> core\n  - lb -> core\n  - core -> api1\nanimation:\n  preset: story\n  steps:\n    - {{ path: [fw, core, lb], edges: [core->api1], nodes: [api1], duration: 2.5s }}\n    - {{ path: [api1, fw] }}\n"
        ));
        let Some(animation) = model.animation else { panic!("animation missing") };
        let ids = |step: &AnimationStep| step.edges.iter().map(|edge| edge.as_str().to_owned()).collect::<Vec<_>>();
        assert_eq!(ids(&animation.steps[0]), ["fw->core", "lb->core", "core->api1"]);
        let nodes: Vec<&str> = animation.steps[0].nodes.iter().map(NodeId::as_str).collect();
        assert_eq!(nodes, ["fw", "core", "lb", "api1"]);
        assert_eq!(animation.steps[0].duration_s, 2.5);
        assert_eq!(animation.steps[1].duration_s, DEFAULT_DURATION_SECONDS);
        assert_eq!(ids(&animation.steps[1]), ["api1->fw"]);
        assert!(!animation.looped);
    }
}
