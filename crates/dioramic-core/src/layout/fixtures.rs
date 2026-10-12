//! Test-only model builders. `dioramic-spec` normalization (T-101) is not available to this crate's unit
//! tests yet, so the examples are read with a tiny reader that understands the flow-style subset they use.

use std::path::PathBuf;

use dioramic_spec::{
    Arrange, ArrowEnds, BorderStyle, Edge, EdgeId, FlowAxis, FlowMode, LayoutAlgorithm, LayoutOptions, Model, Node,
    NodeId, NodeSize, PlanCoord, Routing, ThemeName, ValueSource, ViewOptions, Wrap, Zone, ZoneId,
};
use indexmap::IndexMap;

/// Fluent builder of a normalized [`Model`].
#[derive(Debug, Clone)]
pub(super) struct ModelBuilder {
    model: Model,
}

impl Default for ModelBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn zone(id: &str) -> Zone {
    Zone {
        id: ZoneId::from(id),
        label: None,
        color: None,
        parent: None,
        arrange: Arrange::Auto,
        at: None,
        size: None,
        border: BorderStyle::Dashed,
        path: format!("zones.{id}"),
    }
}

pub(super) fn node(id: &str, zone: Option<&str>) -> Node {
    Node {
        id: NodeId::from(id),
        icon: None,
        label: None,
        zone: zone.map(ZoneId::from),
        color: None,
        replicas: 1,
        badge: None,
        at: None,
        z: 0.0,
        size: NodeSize::M,
        note: None,
        path: format!("nodes.{id}"),
    }
}

pub(super) fn edge(from: &str, to: &str) -> Edge {
    Edge {
        id: EdgeId(format!("{from}->{to}")),
        from: NodeId::from(from),
        to: NodeId::from(to),
        label: None,
        style: "default".to_owned(),
        style_source: ValueSource::Spec,
        kind: None,
        color: None,
        width: None,
        dashed: None,
        arrow: ArrowEnds::End,
        route: Routing::Curved,
        route_source: ValueSource::Default,
        flow: FlowMode::Off,
        path: format!("edges.{from}->{to}"),
    }
}

impl ModelBuilder {
    pub fn new() -> Self {
        Self {
            model: Model {
                title: None,
                subtitle: None,
                theme: ThemeName::default(),
                layout: LayoutOptions::default(),
                view: ViewOptions::default(),
                styles: IndexMap::new(),
                zones: Vec::new(),
                nodes: Vec::new(),
                edges: Vec::new(),
                animation: None,
            },
        }
    }

    /// Zone-less nodes.
    pub fn nodes(mut self, ids: &[&str]) -> Self {
        self.model.nodes.extend(ids.iter().map(|id| node(id, None)));
        self
    }

    /// Nodes inside `zone`.
    pub fn nodes_in(mut self, zone: &str, ids: &[&str]) -> Self {
        self.model.nodes.extend(ids.iter().map(|id| node(id, Some(zone))));
        self
    }

    /// Any zone.
    pub fn zone(mut self, zone: Zone) -> Self {
        self.model.zones.push(zone);
        self
    }

    /// Any node.
    pub fn node(mut self, node: Node) -> Self {
        self.model.nodes.push(node);
        self
    }

    /// Edges written as `from <op> to[: label]`.
    pub fn edges(mut self, shorthands: &[&str]) -> Self {
        self.model.edges.extend(shorthands.iter().filter_map(|text| parse_edge(text)));
        self
    }

    pub fn flow(mut self, flow: FlowAxis) -> Self {
        self.model.layout.flow = flow;
        self
    }

    pub fn algorithm(mut self, algorithm: LayoutAlgorithm) -> Self {
        self.model.layout.algorithm = algorithm;
        self
    }

    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.model.layout.wrap = wrap;
        self
    }

    pub fn build(self) -> Model {
        self.model
    }
}

fn parse_edge(text: &str) -> Option<Edge> {
    let mut words = text.split_whitespace();
    let from = words.next()?;
    let _operator = words.next()?;
    let to = words.next()?.trim_end_matches(':');
    Some(edge(from, to))
}

/// Names of the `examples/*.diorama.yaml` files, sorted.
pub(super) fn example_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(examples_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".diorama.yaml").map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Reads `examples/<name>.diorama.yaml` (zones, nodes, edges and `layout.flow` / `layout.wrap`).
pub(super) fn example(name: &str) -> Model {
    let path = examples_dir().join(format!("{name}.diorama.yaml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    read_example(&source)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Other,
    Zones,
    Nodes,
    Edges,
}

fn read_example(source: &str) -> Model {
    let mut builder = ModelBuilder::new();
    let mut section = Section::Other;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            section = match trimmed.split(':').next() {
                Some("zones") => Section::Zones,
                Some("nodes") => Section::Nodes,
                Some("edges") => Section::Edges,
                _ => Section::Other,
            };
            if let Some(options) = trimmed.strip_prefix("layout:") {
                builder = apply_layout_options(builder, &mapping(options));
            }
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ") else { continue };
        builder = match section {
            Section::Zones => builder.zone(read_zone(&mapping(item))),
            Section::Nodes => builder.node(read_node(&mapping(item))),
            Section::Edges => builder.edges(&[item]),
            Section::Other => builder,
        };
    }
    builder.build()
}

fn apply_layout_options(mut builder: ModelBuilder, fields: &[(String, String)]) -> ModelBuilder {
    for (key, value) in fields {
        match (key.as_str(), value.as_str()) {
            ("flow", "y") => builder = builder.flow(FlowAxis::Y),
            ("algorithm", "hub") => builder = builder.algorithm(LayoutAlgorithm::Hub),
            ("wrap", count) => {
                if let Ok(count) = count.parse() {
                    builder = builder.wrap(Wrap::Count(count));
                }
            }
            _ => {}
        }
    }
    builder
}

fn read_zone(fields: &[(String, String)]) -> Zone {
    let mut result = zone(&field(fields, "id").unwrap_or_default());
    result.label = field(fields, "label");
    result.parent = field(fields, "parent").map(ZoneId);
    result.border = match field(fields, "border").as_deref() {
        Some("none") => BorderStyle::None,
        Some("solid") => BorderStyle::Solid,
        _ => BorderStyle::Dashed,
    };
    result.arrange = match field(fields, "arrange").as_deref() {
        Some("row") => Arrange::Row,
        Some("column") => Arrange::Column,
        Some("grid") => Arrange::Grid,
        _ => Arrange::Auto,
    };
    result.at = field(fields, "at").and_then(|at| coord(&at));
    result.size = field(fields, "size").and_then(|size| coord(&size)).map(|size| [size.x, size.y]);
    result
}

fn read_node(fields: &[(String, String)]) -> Node {
    let mut result = node(&field(fields, "id").unwrap_or_default(), field(fields, "zone").as_deref());
    result.icon = field(fields, "icon");
    result.label = field(fields, "label");
    result.replicas = field(fields, "replicas").and_then(|count| count.parse().ok()).unwrap_or(1);
    result.at = field(fields, "at").and_then(|at| coord(&at));
    result.size = match field(fields, "size").as_deref() {
        Some("s") => NodeSize::S,
        Some("l") => NodeSize::L,
        _ => NodeSize::M,
    };
    result
}

fn field(fields: &[(String, String)], key: &str) -> Option<String> {
    fields.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone())
}

fn coord(text: &str) -> Option<PlanCoord> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.split(',').map(|part| part.trim().parse::<f64>());
    Some(PlanCoord { x: parts.next()?.ok()?, y: parts.next()?.ok()? })
}

/// `{ key: value, key: "quoted, value", at: [1, 2] }` → pairs.
fn mapping(text: &str) -> Vec<(String, String)> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    let mut pairs = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut bracket_depth = 0_u32;
    for character in inner.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            '[' if !in_quotes => bracket_depth += 1,
            ']' if !in_quotes => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if !in_quotes && bracket_depth == 0 => {
                pairs.extend(pair(&current));
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    pairs.extend(pair(&current));
    pairs
}

fn pair(text: &str) -> Option<(String, String)> {
    let (key, value) = text.split_once(':')?;
    Some((key.trim().to_owned(), value.trim().trim_matches('"').to_owned()))
}
