//! Structural validation of a [`Model`]: ids, references, cycles, colors, ranges and limits.
//! Icon existence is NOT checked here (see `dioramic_core::icons::validate_icons`).

use std::collections::{BTreeMap, BTreeSet};

use crate::common::{ColorRef, EdgeKind, Wrap};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::model::{GridOptions, Model, Zone};
use crate::normalize::DEFAULT_EDGE_STYLE;
use crate::parse::{SourceFormat, parse};

/// Hard limit of nodes (`E_LIMIT`), `docs/05-mcp.md#segurança`.
pub const MAX_NODES: usize = 500;
/// Above this, `W_TOO_MANY_NODES`.
pub const RECOMMENDED_MAX_NODES: usize = 40;
/// Above this, `W_LABEL_LONG` (the label wraps to 2 lines).
pub const RECOMMENDED_MAX_LABEL_CHARS: usize = 28;

/// Edge styles every theme defines (`docs/04-spec.md#edge`).
const THEME_STYLES: [&str; 5] = [DEFAULT_EDGE_STYLE, "secondary", "muted", "danger", "success"];
/// Candidates farther than this (normalized Levenshtein similarity below it) are not suggested.
const MIN_SUGGESTION_SIMILARITY: f64 = 0.5;

/// Validates a model. Returns every finding (errors, warnings, hints), sorted with [`Diagnostics::sort`].
///
/// Checks: `E_BAD_ID`, `E_DUPLICATE_ID`, `E_UNKNOWN_REF` (with "did you mean" via `strsim`),
/// `E_ZONE_CYCLE`, `E_BAD_COLOR`, `E_BAD_VALUE`, `E_LIMIT`, `W_LABEL_LONG`, `W_ISOLATED`,
/// `W_TOO_MANY_NODES`, `W_SELF_LOOP`.
pub fn validate(model: &Model) -> Diagnostics {
    let mut checker = Checker::new(model);
    checker.check_document();
    checker.check_view();
    checker.check_zones();
    checker.check_nodes();
    checker.check_styles();
    checker.check_edges();
    checker.check_animation();
    let mut diagnostics = checker.diagnostics;
    diagnostics.sort();
    diagnostics
}

/// A model that passed structural validation, with its non-error diagnostics.
#[derive(Debug, Clone)]
pub struct Validated {
    /// The model.
    pub model: Model,
    /// Warnings and hints (never errors).
    pub warnings: Diagnostics,
}

/// `parse` → `normalize` → `validate`. Errors stop the pipeline; warnings ride along.
pub fn load(source: &str, format: SourceFormat) -> Result<Validated, Diagnostics> {
    let spec = parse(source, format)?;
    let model = crate::normalize::normalize(spec)?;
    let diagnostics = validate(&model);
    if diagnostics.has_errors() {
        return Err(diagnostics);
    }
    Ok(Validated { model, warnings: diagnostics })
}

/// Walks the model once per section and collects every finding.
struct Checker<'a> {
    model: &'a Model,
    node_ids: BTreeSet<&'a str>,
    zone_ids: BTreeSet<&'a str>,
    diagnostics: Diagnostics,
}

impl<'a> Checker<'a> {
    fn new(model: &'a Model) -> Self {
        Self {
            model,
            node_ids: model.nodes.iter().map(|node| node.id.as_str()).collect(),
            zone_ids: model.zones.iter().map(|zone| zone.id.as_str()).collect(),
            diagnostics: Diagnostics::new(),
        }
    }

    fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn check_document(&mut self) {
        let node_count = self.model.nodes.len();
        if node_count == 0 {
            self.push(
                Diagnostic::new(DiagnosticCode::NoNodes, "nodes", "the diagram has no nodes")
                    .with_hint("add at least one node: { id: api, icon: server }"),
            );
        } else if node_count > MAX_NODES {
            self.push(
                Diagnostic::new(
                    DiagnosticCode::Limit,
                    "nodes",
                    format!("{node_count} nodes exceed the limit of {MAX_NODES}"),
                )
                .with_hint("split the system into several diagrams"),
            );
        } else if node_count > RECOMMENDED_MAX_NODES {
            self.push(
                Diagnostic::new(
                    DiagnosticCode::TooManyNodes,
                    "nodes",
                    format!(
                        "{node_count} nodes; consider splitting into {} diagrams",
                        node_count.div_ceil(RECOMMENDED_MAX_NODES)
                    ),
                )
                .with_hint(format!("keep each diagram under about {RECOMMENDED_MAX_NODES} nodes")),
            );
        }
        if self.model.layout.wrap == Wrap::Count(0) {
            self.push(bad_value("layout.wrap", "wrap must be at least 1", "use a positive integer or auto"));
        }
    }

    fn check_view(&mut self) {
        let grid = &self.model.view.grid;
        if !(grid.size.is_finite() && (GridOptions::MIN_SIZE..=GridOptions::MAX_SIZE).contains(&grid.size)) {
            self.push(bad_value(
                "view.grid.size",
                &format!("grid size must be between {} and {} tiles", GridOptions::MIN_SIZE, GridOptions::MAX_SIZE),
                "use 0.5, 1 or 2 (cells per tile side)",
            ));
        }
        if !(grid.opacity.is_finite() && (0.0..=1.0).contains(&grid.opacity)) {
            self.push(bad_value("view.grid.opacity", "grid opacity must be between 0 and 1", "e.g. 0.5"));
        }
        if let Some(color) = &grid.color {
            self.check_color(color, "view.grid.color");
        }
    }

    fn check_zones(&mut self) {
        let model = self.model;
        self.check_unique_ids(model.zones.iter().map(|zone| (zone.id.as_str(), zone.path.as_str())), "zone");
        let zone_ids = self.zone_ids.clone();
        for zone in &model.zones {
            self.check_id(zone.id.as_str(), &format!("{}.id", zone.path));
            if let Some(color) = &zone.color {
                self.check_color(color, &format!("{}.color", zone.path));
            }
            if let Some(parent) = &zone.parent {
                self.check_ref(parent.as_str(), &format!("{}.parent", zone.path), "zone", &zone_ids);
            }
            if let Some([width, depth]) = zone.size
                && !(is_positive(width) && is_positive(depth))
            {
                self.push(bad_value(
                    &format!("{}.size", zone.path),
                    "zone size must be positive",
                    "use [width, depth] in tiles, e.g. [4, 3]",
                ));
            }
            if let Some(at) = zone.at {
                self.check_coord(at.x, at.y, &format!("{}.at", zone.path));
            }
            self.check_label(zone.label.as_deref(), &zone.path, zone.id.as_str());
        }
        self.check_zone_cycles();
    }

    fn check_zone_cycles(&mut self) {
        let parents: BTreeMap<&str, &str> = self
            .model
            .zones
            .iter()
            .filter_map(|zone| zone.parent.as_ref().map(|parent| (zone.id.as_str(), parent.as_str())))
            .collect();
        let cyclic: Vec<&Zone> =
            self.model.zones.iter().filter(|zone| is_own_ancestor(zone.id.as_str(), &parents)).collect();
        for zone in cyclic {
            self.push(
                Diagnostic::new(
                    DiagnosticCode::ZoneCycle,
                    format!("{}.parent", zone.path),
                    format!("zone '{}' is its own ancestor", zone.id),
                )
                .with_hint("remove one parent link in the cycle"),
            );
        }
    }

    fn check_nodes(&mut self) {
        let model = self.model;
        self.check_unique_ids(model.nodes.iter().map(|node| (node.id.as_str(), node.path.as_str())), "node");
        let connected: BTreeSet<&str> =
            model.edges.iter().flat_map(|edge| [edge.from.as_str(), edge.to.as_str()]).collect();
        let zone_ids = self.zone_ids.clone();
        for node in &model.nodes {
            self.check_id(node.id.as_str(), &format!("{}.id", node.path));
            if let Some(zone) = &node.zone {
                self.check_ref(zone.as_str(), &format!("{}.zone", node.path), "zone", &zone_ids);
            }
            if let Some(color) = &node.color {
                self.check_color(color, &format!("{}.color", node.path));
            }
            if node.replicas == 0 {
                self.push(bad_value(
                    &format!("{}.replicas", node.path),
                    "replicas must be at least 1",
                    "omit replicas for a single instance",
                ));
            }
            if !node.z.is_finite() || node.z < 0.0 {
                self.push(bad_value(
                    &format!("{}.z", node.path),
                    "z must be zero or positive",
                    "z is the elevation in tiles",
                ));
            }
            if let Some(at) = node.at {
                self.check_coord(at.x, at.y, &format!("{}.at", node.path));
            }
            self.check_label(node.label.as_deref(), &node.path, node.id.as_str());
            if !connected.contains(node.id.as_str()) && model.nodes.len() > 1 {
                self.push(
                    Diagnostic::new(
                        DiagnosticCode::Isolated,
                        node.path.clone(),
                        format!("node '{}' has no edges", node.id),
                    )
                    .with_hint("connect it or remove it"),
                );
            }
        }
    }

    fn check_styles(&mut self) {
        let model = self.model;
        for (name, style) in &model.styles {
            let path = format!("styles.{name}");
            if let Some(color) = &style.color {
                self.check_color(color, &format!("{path}.color"));
            }
            if let Some(width) = style.width {
                self.check_width(width, &format!("{path}.width"));
            }
        }
    }

    fn check_edges(&mut self) {
        let model = self.model;
        self.check_unique_ids(model.edges.iter().map(|edge| (edge.id.as_str(), edge.path.as_str())), "edge");
        // A connection class name (`@data`) is a style too: it sets the edge's class.
        let class_names = EdgeKind::ALL.map(EdgeKind::as_str);
        let style_names: BTreeSet<&str> =
            model.styles.keys().map(String::as_str).chain(THEME_STYLES).chain(class_names).collect();
        let node_ids = self.node_ids.clone();
        for edge in &model.edges {
            self.check_ref(edge.from.as_str(), &format!("{}.from", edge.path), "node", &node_ids);
            self.check_ref(edge.to.as_str(), &format!("{}.to", edge.path), "node", &node_ids);
            self.check_ref(&edge.style, &format!("{}.style", edge.path), "style", &style_names);
            if let Some(color) = &edge.color {
                self.check_color(color, &format!("{}.color", edge.path));
            }
            if let Some(width) = edge.width {
                self.check_width(width, &format!("{}.width", edge.path));
            }
            if edge.from == edge.to {
                self.push(
                    Diagnostic::new(
                        DiagnosticCode::SelfLoop,
                        edge.path.clone(),
                        format!("edge '{}' connects '{}' to itself", edge.id, edge.from),
                    )
                    .with_hint("self loops are not drawn; remove the edge"),
                );
            }
            if let Some(label) = &edge.label
                && label.chars().count() > RECOMMENDED_MAX_LABEL_CHARS
            {
                self.push(long_label(&format!("{}.label", edge.path), &format!("edge '{}'", edge.id), label));
            }
        }
    }

    fn check_animation(&mut self) {
        let Some(animation) = &self.model.animation else { return };
        if animation.fps == 0 {
            self.push(bad_value("animation.fps", "fps must be at least 1", "omit fps for the default of 50"));
        }
        if !is_positive(animation.duration_s) {
            self.push(bad_value(
                "animation.duration",
                "duration must be positive",
                "use seconds (2), \"2.5s\" or \"800ms\"",
            ));
        }
        let edge_ids: BTreeSet<&str> = self.model.edges.iter().map(|edge| edge.id.as_str()).collect();
        let node_ids = self.node_ids.clone();
        for (index, step) in animation.steps.iter().enumerate() {
            let path = format!("animation.steps[{index}]");
            for edge in &step.edges {
                self.check_ref(edge.as_str(), &format!("{path}.edges"), "edge", &edge_ids);
            }
            for node in &step.nodes {
                self.check_ref(node.as_str(), &format!("{path}.nodes"), "node", &node_ids);
            }
            if !is_positive(step.duration_s) {
                self.push(bad_value(
                    &format!("{path}.duration"),
                    "duration must be positive",
                    "use seconds (2), \"2.5s\" or \"800ms\"",
                ));
            }
        }
    }

    /// Reports every repeated id, pointing at the first occurrence.
    fn check_unique_ids<'i>(&mut self, items: impl Iterator<Item = (&'i str, &'i str)>, kind: &str) {
        let mut first_path: BTreeMap<&str, &str> = BTreeMap::new();
        for (id, path) in items {
            match first_path.get(id) {
                Some(first) => self.push(
                    Diagnostic::new(
                        DiagnosticCode::DuplicateId,
                        format!("{path}.id"),
                        format!("{kind} id '{id}' already exists in {first}"),
                    )
                    .with_hint("ids must be unique; rename one of them"),
                ),
                None => {
                    first_path.insert(id, path);
                }
            }
        }
    }

    fn check_id(&mut self, id: &str, path: &str) {
        if !is_valid_id(id) {
            self.push(
                Diagnostic::new(DiagnosticCode::BadId, path, format!("invalid id '{id}'"))
                    .with_hint("start with a letter; use only letters, digits, _, - and /, e.g. core_switch"),
            );
        }
    }

    fn check_ref(&mut self, name: &str, path: &str, kind: &str, known: &BTreeSet<&str>) {
        if known.contains(name) {
            return;
        }
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::UnknownRef, path, format!("{kind} '{name}' does not exist"));
        if let Some(suggestion) = closest(name, known) {
            diagnostic = diagnostic.with_hint(format!("did you mean '{suggestion}'?"));
        }
        self.push(diagnostic);
    }

    fn check_color(&mut self, color: &ColorRef, path: &str) {
        if let ColorRef::Hex(hex) = color
            && hex.rgb().is_none()
        {
            self.push(
                Diagnostic::new(DiagnosticCode::BadColor, path, format!("invalid color '{}'", hex.0))
                    .with_hint("use a token (blue, orange, teal, green, violet, pink, red, gray) or #rrggbb"),
            );
        }
    }

    fn check_width(&mut self, width: f64, path: &str) {
        if !is_positive(width) {
            self.push(bad_value(path, "width must be positive", "edge width is in px, e.g. 2.5"));
        }
    }

    fn check_coord(&mut self, x: f64, y: f64, path: &str) {
        if !(x.is_finite() && y.is_finite()) {
            self.push(bad_value(path, "coordinates must be finite numbers", "use [x, y] in tiles"));
        }
    }

    fn check_label(&mut self, label: Option<&str>, owner_path: &str, owner_id: &str) {
        if let Some(label) = label
            && label.chars().count() > RECOMMENDED_MAX_LABEL_CHARS
        {
            self.push(long_label(&format!("{owner_path}.label"), &format!("'{owner_id}'"), label));
        }
    }
}

fn bad_value(path: &str, message: &str, hint: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticCode::BadValue, path, message).with_hint(hint)
}

fn long_label(path: &str, owner: &str, label: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::LabelLong,
        path,
        format!("label of {owner} has {} characters; it will wrap to 2 lines", label.chars().count()),
    )
    .with_hint(format!("keep labels at most {RECOMMENDED_MAX_LABEL_CHARS} characters"))
}

fn is_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Letters, digits, `_`, `-`, `/`; must start with a letter (`DiagnosticCode::BadId`).
fn is_valid_id(id: &str) -> bool {
    let mut characters = id.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/'))
}

/// Follows parent links from `start`; true when they come back to it. Bounded by the number of links,
/// so a cycle that does not include `start` still terminates.
fn is_own_ancestor(start: &str, parents: &BTreeMap<&str, &str>) -> bool {
    let mut current = start;
    for _ in 0..parents.len() {
        match parents.get(current) {
            Some(&parent) if parent == start => return true,
            Some(&parent) => current = parent,
            None => return false,
        }
    }
    false
}

/// Most similar known name, if similar enough to be a plausible typo. Ties go to the first in sorted order.
fn closest<'k>(name: &str, known: &BTreeSet<&'k str>) -> Option<&'k str> {
    known
        .iter()
        .map(|candidate| (strsim::normalized_levenshtein(name, candidate), *candidate))
        .filter(|(similarity, _)| *similarity >= MIN_SUGGESTION_SIMILARITY)
        .fold(None, |best: Option<(f64, &str)>, current| match best {
            Some(best) if best.0 >= current.0 => Some(best),
            _ => Some(current),
        })
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics(yaml: &str) -> Diagnostics {
        let spec = parse(yaml, SourceFormat::Yaml).unwrap_or_else(|error| panic!("{error}"));
        let model = crate::normalize::normalize(spec).unwrap_or_else(|error| panic!("{error}"));
        validate(&model)
    }

    fn find(found: &Diagnostics, code: DiagnosticCode) -> Diagnostic {
        found
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .cloned()
            .unwrap_or_else(|| panic!("no {code} in:\n{found}"))
    }

    const BASE: &str = "diorama: 1\n";

    #[test]
    fn clean_spec_has_no_findings() {
        let found =
            diagnostics("diorama: 1\nzones: [{ id: z }]\nnodes: [{ id: a, zone: z }, { id: b }]\nedges: [a -> b]\n");
        assert!(found.is_empty(), "{found}");
    }

    #[test]
    fn bad_id() {
        let found = diagnostics(&format!("{BASE}nodes: [{{ id: 1st }}, {{ id: \"has space\" }}]\n"));
        assert_eq!(found.iter().filter(|diagnostic| diagnostic.code == DiagnosticCode::BadId).count(), 2);
        assert_eq!(find(&found, DiagnosticCode::BadId).path, "nodes[0].id");
    }

    #[test]
    fn duplicate_id_points_at_first() {
        let found =
            diagnostics(&format!("{BASE}nodes: [{{ id: core }}, {{ id: x }}, {{ id: core }}]\nedges: [core -> x]\n"));
        let diagnostic = find(&found, DiagnosticCode::DuplicateId);
        assert_eq!(diagnostic.path, "nodes[2].id");
        assert!(diagnostic.message.contains("nodes[0]"), "{}", diagnostic.message);
    }

    #[test]
    fn duplicate_explicit_edge_id() {
        let found = diagnostics(&format!(
            "{BASE}nodes: [{{ id: a }}, {{ id: b }}]\nedges: [{{ id: e, from: a, to: b }}, {{ id: e, from: b, to: a }}]\n"
        ));
        assert_eq!(find(&found, DiagnosticCode::DuplicateId).path, "edges[1].id");
    }

    #[test]
    fn unknown_refs_with_did_you_mean() {
        let found = diagnostics(&format!(
            "{BASE}styles: {{ trunk: {{ width: 3 }} }}\nzones: [{{ id: comms }}, {{ id: hall, parent: hal }}]\nnodes: [{{ id: core, zone: comm }}, {{ id: nas, zone: comms }}]\nedges:\n  - core_sw -> nas: x @trunc\n  - core -> naz\nanimation: {{ preset: story, steps: [{{ path: [core, nas], nodes: [cor] }}, {{ edges: [core->nass] }}] }}\n"
        ));
        let hints: BTreeMap<String, Option<String>> = found
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::UnknownRef)
            .map(|diagnostic| (diagnostic.path.clone(), diagnostic.hint.clone()))
            .collect();
        let hint = |path: &str| hints.get(path).cloned().flatten();
        assert_eq!(hint("zones[1].parent").as_deref(), Some("did you mean 'hall'?"));
        assert_eq!(hint("nodes[0].zone").as_deref(), Some("did you mean 'comms'?"));
        assert_eq!(hint("edges[0].from").as_deref(), Some("did you mean 'core'?"));
        assert_eq!(hint("edges[0].style").as_deref(), Some("did you mean 'trunk'?"));
        assert_eq!(hint("edges[1].to").as_deref(), Some("did you mean 'nas'?"));
        assert_eq!(hint("animation.steps[0].edges").as_deref(), Some("did you mean 'core->naz'?"));
        assert_eq!(hint("animation.steps[0].nodes").as_deref(), Some("did you mean 'core'?"));
        assert!(hints.contains_key("animation.steps[1].edges"));
    }

    #[test]
    fn unknown_ref_without_plausible_suggestion_has_no_hint() {
        let found =
            diagnostics(&format!("{BASE}nodes: [{{ id: alpha }}, {{ id: beta }}]\nedges: [alpha -> zzzzzzzz]\n"));
        assert_eq!(find(&found, DiagnosticCode::UnknownRef).hint, None);
    }

    #[test]
    fn zone_cycle() {
        let found = diagnostics(&format!(
            "{BASE}zones: [{{ id: a, parent: b }}, {{ id: b, parent: a }}, {{ id: c, parent: c }}, {{ id: d, parent: a }}]\nnodes: [{{ id: n }}]\n"
        ));
        let paths: Vec<&str> = found
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::ZoneCycle)
            .map(|diagnostic| diagnostic.path.as_str())
            .collect();
        assert_eq!(paths, ["zones[0].parent", "zones[1].parent", "zones[2].parent"]);
    }

    #[test]
    fn bad_color() {
        let found = diagnostics(&format!(
            "{BASE}styles: {{ s: {{ color: \"#12\" }} }}\nzones: [{{ id: z, color: bleu }}]\nnodes: [{{ id: a, color: \"#abc\" }}, {{ id: b, color: \"#gggggg\" }}]\nedges: [{{ from: a, to: b, color: red }}]\n"
        ));
        let paths: Vec<&str> = found
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::BadColor)
            .map(|diagnostic| diagnostic.path.as_str())
            .collect();
        assert_eq!(paths, ["nodes[1].color", "styles.s.color", "zones[0].color"]);
    }

    #[test]
    fn grid_values_are_checked() {
        let found = diagnostics(&format!(
            "{BASE}view: {{ grid: {{ size: 0.1, opacity: 1.5, color: \"#12\" }} }}\nnodes: [{{ id: a }}]\n"
        ));
        let codes: BTreeSet<(DiagnosticCode, &str)> =
            found.iter().map(|diagnostic| (diagnostic.code, diagnostic.path.as_str())).collect();
        assert!(codes.contains(&(DiagnosticCode::BadValue, "view.grid.size")), "{found:?}");
        assert!(codes.contains(&(DiagnosticCode::BadValue, "view.grid.opacity")), "{found:?}");
        assert!(codes.contains(&(DiagnosticCode::BadColor, "view.grid.color")), "{found:?}");
        let fine = diagnostics(&format!(
            "{BASE}view: {{ grid: {{ size: 0.5, style: dots, opacity: 0.4, color: teal }} }}\nnodes: [{{ id: a }}]\n"
        ));
        assert!(fine.iter().all(|diagnostic| !diagnostic.path.starts_with("view")), "{fine:?}");
    }

    #[test]
    fn bad_values() {
        let found = diagnostics(&format!(
            "{BASE}layout: {{ wrap: 0 }}\nstyles: {{ s: {{ width: -1 }} }}\nzones: [{{ id: z, size: [0, 2] }}]\nnodes: [{{ id: a, replicas: 0, z: -1 }}, {{ id: b }}]\nedges: [{{ from: a, to: b, width: 0 }}]\nanimation: {{ fps: 0, duration: 0, steps: [{{ nodes: [a], duration: -2 }}] }}\n"
        ));
        let paths: BTreeSet<&str> = found
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::BadValue)
            .map(|diagnostic| diagnostic.path.as_str())
            .collect();
        let expected: BTreeSet<&str> = [
            "layout.wrap",
            "styles.s.width",
            "zones[0].size",
            "nodes[0].replicas",
            "nodes[0].z",
            "edges[0].width",
            "animation.fps",
            "animation.duration",
            "animation.steps[0].duration",
        ]
        .into_iter()
        .collect();
        assert_eq!(paths, expected);
    }

    fn many_nodes(count: usize) -> String {
        let nodes: Vec<String> = (0..count).map(|index| format!("{{ id: n{index} }}")).collect();
        format!("{BASE}nodes: [{}]\n", nodes.join(", "))
    }

    #[test]
    fn node_limits() {
        assert!(find(&diagnostics(&many_nodes(MAX_NODES + 1)), DiagnosticCode::Limit).is_error());
        let warning = find(&diagnostics(&many_nodes(RECOMMENDED_MAX_NODES + 12)), DiagnosticCode::TooManyNodes);
        assert!(!warning.is_error());
        assert!(warning.message.contains("52 nodes"));
        assert!(
            !diagnostics(&many_nodes(RECOMMENDED_MAX_NODES))
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TooManyNodes)
        );
    }

    #[test]
    fn no_nodes() {
        assert!(find(&diagnostics(&format!("{BASE}nodes: []\n")), DiagnosticCode::NoNodes).is_error());
    }

    #[test]
    fn long_labels_warn() {
        let long = "Branch Manager Workstation Two";
        let found = diagnostics(&format!(
            "{BASE}zones: [{{ id: z, label: {long} }}]\nnodes: [{{ id: a, label: {long} }}, {{ id: b, label: \"{short}\" }}]\nedges: [\"a -> b: {long}\"]\n",
            short = "x".repeat(RECOMMENDED_MAX_LABEL_CHARS)
        ));
        let paths: Vec<&str> = found
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::LabelLong)
            .map(|diagnostic| diagnostic.path.as_str())
            .collect();
        assert_eq!(paths, ["edges[0].label", "nodes[0].label", "zones[0].label"]);
        assert!(!found.has_errors());
    }

    #[test]
    fn isolated_is_a_hint() {
        let found =
            diagnostics(&format!("{BASE}nodes: [{{ id: a }}, {{ id: b }}, {{ id: printer }}]\nedges: [a -> b]\n"));
        let diagnostic = find(&found, DiagnosticCode::Isolated);
        assert_eq!(diagnostic.severity, crate::diagnostic::Severity::Hint);
        assert_eq!(diagnostic.path, "nodes[2]");
        assert!(diagnostics(&format!("{BASE}nodes: [{{ id: solo }}]\n")).is_empty());
    }

    #[test]
    fn self_loop_warns() {
        let found = diagnostics(&format!("{BASE}nodes: [{{ id: a }}]\nedges: [a -> a]\n"));
        assert!(!find(&found, DiagnosticCode::SelfLoop).is_error());
    }

    #[test]
    fn findings_are_sorted_errors_first() {
        let found = diagnostics(&format!("{BASE}nodes: [{{ id: a }}, {{ id: b }}]\nedges: [a -> a, a -> c]\n"));
        let severities: Vec<_> = found.iter().map(|diagnostic| diagnostic.severity).collect();
        let mut sorted = severities.clone();
        sorted.sort();
        assert_eq!(severities, sorted);
    }

    #[test]
    fn load_stops_on_errors_and_keeps_warnings() {
        assert!(load("diorama: 1\nnodes: [{ id: a }]\nedges: [a -> b]\n", SourceFormat::Yaml).is_err());
        let loaded = load("diorama: 1\nnodes: [{ id: a }, { id: b }]\n", SourceFormat::Yaml);
        assert_eq!(loaded.map(|validated| validated.warnings.len()).ok(), Some(2));
    }
}
