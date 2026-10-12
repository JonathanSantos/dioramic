//! Layout invariants on hand-built models, on every example and on random graphs.

use dioramic_spec::{Arrange, Density, DiagnosticCode, FlowAxis, Model, PlanCoord, Rotation, Wrap};
use proptest::prelude::*;

use super::fixtures::{ModelBuilder, edge, example, example_names, node, zone};
use super::*;

const EPSILON: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn overlaps(a: PlanRect, b: PlanRect) -> bool {
    a.x0 < b.x1 - EPSILON && b.x0 < a.x1 - EPSILON && a.y0 < b.y1 - EPSILON && b.y0 < a.y1 - EPSILON
}

fn inside(inner: PlanRect, outer: PlanRect) -> bool {
    inner.x0 >= outer.x0 - EPSILON
        && inner.y0 >= outer.y0 - EPSILON
        && inner.x1 <= outer.x1 + EPSILON
        && inner.y1 <= outer.y1 + EPSILON
}

fn zone_rect(plan: &PlanLayout, id: &str) -> PlanRect {
    plan.zones.iter().find(|zone| zone.id.as_str() == id).map(|zone| zone.rect).unwrap_or_else(|| panic!("zone {id}"))
}

fn center(plan: &PlanLayout, id: &str) -> PlanPoint {
    plan.nodes.iter().find(|node| node.id.as_str() == id).map(|node| node.center).unwrap_or_else(|| panic!("node {id}"))
}

fn ancestors(model: &Model, zone_id: Option<&str>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = zone_id.map(str::to_owned);
    while let Some(id) = current {
        if chain.contains(&id) {
            break;
        }
        current = model
            .zones
            .iter()
            .find(|zone| zone.id.as_str() == id)
            .and_then(|zone| zone.parent.as_ref())
            .map(|parent| parent.as_str().to_owned());
        chain.push(id);
    }
    chain
}

/// Every invariant of contract C-004 that does not depend on pins.
pub(super) fn assert_invariants(model: &Model, plan: &PlanLayout) {
    assert_eq!(plan.rotation, Rotation::R0);
    assert_eq!(plan.nodes.len(), model.nodes.len());
    assert_eq!(plan.zones.len(), model.zones.len());
    for (index, (place, node)) in plan.nodes.iter().zip(&model.nodes).enumerate() {
        assert_eq!((place.index, &place.id), (index, &node.id));
        assert!(place.center.x.is_finite() && place.center.y.is_finite());
    }
    let footprints: Vec<PlanRect> = plan.nodes.iter().map(footprint_rect).collect();
    for (a, first) in footprints.iter().enumerate() {
        for (b, second) in footprints.iter().enumerate().skip(a + 1) {
            assert!(
                !overlaps(first.inflate(FOOTPRINT_CLEARANCE), second.inflate(FOOTPRINT_CLEARANCE)),
                "footprints of {} and {} overlap",
                plan.nodes[a].id,
                plan.nodes[b].id
            );
        }
    }
    for (place, node) in plan.nodes.iter().zip(&model.nodes) {
        let chain = ancestors(model, node.zone.as_ref().map(|zone| zone.as_str()));
        for zone in &plan.zones {
            let footprint = footprint_rect(place);
            if chain.iter().any(|id| id == zone.id.as_str()) {
                assert!(inside(footprint, zone.rect), "{} is not inside its zone {}", place.id, zone.id);
            } else {
                assert!(!overlaps(footprint, zone.rect), "{} intrudes into zone {}", place.id, zone.id);
            }
        }
    }
    for (position, zone) in plan.zones.iter().enumerate() {
        let parent = model.zones[zone.index].parent.as_ref();
        if let Some(parent) = parent.and_then(|parent| plan.zones.iter().position(|other| &other.id == parent)) {
            assert!(parent < position, "parent of {} comes after it", zone.id);
            assert!(inside(zone.rect, plan.zones[parent].rect), "{} is outside its parent", zone.id);
            assert_eq!(zone.depth, plan.zones[parent].depth + 1);
        } else {
            assert_eq!(zone.depth, 0);
        }
        for other in plan.zones.iter().skip(position + 1) {
            if model.zones[other.index].parent.as_ref() == parent {
                assert!(!overlaps(zone.rect, other.rect), "sibling zones {} and {} overlap", zone.id, other.id);
            }
        }
    }
    for zone in &plan.zones {
        assert!(inside(zone.rect, plan.bounds));
    }
}

fn area(rect: PlanRect) -> f64 {
    rect.width() * rect.depth()
}

fn layout_ok(model: &Model, density: Density) -> PlanLayout {
    layout(model, density).unwrap_or_else(|errors| panic!("layout failed: {errors:?}"))
}

#[test]
fn every_example_satisfies_the_invariants_in_both_densities() {
    let names = example_names();
    assert!(names.len() >= 6, "examples not found: {names:?}");
    for name in names {
        let model = example(&name);
        for density in Density::ALL {
            let plan = layout_ok(&model, density);
            assert_invariants(&model, &plan);
            dump::write(&format!("{name}-{}", density.as_str()), &model, &plan);
        }
    }
}

#[test]
fn compact_checkout_uses_at_most_sixty_percent_of_the_spaced_area() {
    let model = example("checkout");
    let spaced = layout_ok(&model, Density::Spaced);
    let compact = layout_ok(&model, Density::Compact);
    let ratio = area(compact.bounds) / area(spaced.bounds);
    assert!(ratio <= 0.6, "compact/spaced area ratio {ratio:.3}");
}

#[test]
fn checkout_reads_left_to_right_with_replicas_stacked_across() {
    let plan = layout_ok(&example("checkout"), Density::Spaced);
    let order = ["internet", "edge", "app", "data"].map(|id| zone_rect(&plan, id));
    assert!(order.windows(2).all(|pair| pair[0].x1 < pair[1].x0), "zones are not in flow order");
    assert!(order.iter().all(|rect| close(rect.y0, order[0].y0) && close(rect.y1, order[0].y1)), "one aligned row");
    let (api1, api2) = (center(&plan, "api1"), center(&plan, "api2"));
    assert!(close(api1.x, api2.x) && close((api2.y - api1.y).abs(), plan.params.node_gap));
    assert!(close(center(&plan, "lb").y, (api1.y + api2.y) / 2.0), "single node centered on the pair");
}

#[test]
fn compact_folds_zones_in_a_serpentine() {
    let model = ModelBuilder::new()
        .zone(zone("z0"))
        .zone(zone("z1"))
        .zone(zone("z2"))
        .zone(zone("z3"))
        .nodes_in("z0", &["a"])
        .nodes_in("z1", &["b"])
        .nodes_in("z2", &["c"])
        .nodes_in("z3", &["d"])
        .edges(&["a -> b", "b -> c", "c -> d"])
        .build();
    let plan = layout_ok(&model, Density::Compact);
    let [z0, z1, z2, z3] = ["z0", "z1", "z2", "z3"].map(|id| zone_rect(&plan, id));
    assert!(z0.x1 < z1.x0 && close(z0.y0, z1.y0), "first row runs along the flow");
    assert!(z2.y0 > z1.y1 && close(z2.x0, z1.x0), "z2 folds under z1");
    assert!(z3.x1 < z2.x0 && close(z3.x0, z0.x0), "second row runs backwards");
    let spaced = layout_ok(&model, Density::Spaced);
    let rows: Vec<f64> = spaced.zones.iter().map(|zone| zone.rect.y0).collect();
    assert!(rows.iter().all(|y| close(*y, rows[0])), "spaced keeps a single row");
}

#[test]
fn wrap_overrides_zones_per_row() {
    let mut builder = ModelBuilder::new().wrap(Wrap::Count(1));
    for id in ["z0", "z1", "z2"] {
        builder = builder.zone(zone(id)).nodes_in(id, &[&format!("{id}_node")]);
    }
    let plan = layout_ok(&builder.build(), Density::Spaced);
    let [z0, z1, z2] = ["z0", "z1", "z2"].map(|id| zone_rect(&plan, id));
    assert!(z0.y1 < z1.y0 && z1.y1 < z2.y0 && close(z0.x0, z2.x0), "one zone per row");
}

#[test]
fn flow_y_runs_the_zones_along_plan_y() {
    let model = ModelBuilder::new()
        .flow(FlowAxis::Y)
        .zone(zone("a"))
        .zone(zone("b"))
        .nodes_in("a", &["src"])
        .nodes_in("b", &["dst1", "dst2"])
        .edges(&["src -> dst1", "src -> dst2"])
        .build();
    let plan = layout_ok(&model, Density::Spaced);
    assert!(zone_rect(&plan, "a").y1 < zone_rect(&plan, "b").y0);
    let (first, second) = (center(&plan, "dst1"), center(&plan, "dst2"));
    assert!(close(first.y, second.y) && first.x < second.x, "siblings stack across the flow (along x)");
    assert_invariants(&model, &plan);
}

#[test]
fn arrange_modes_shape_the_zone() {
    let build = |arrange: Arrange| {
        let mut container = zone("box");
        container.arrange = arrange;
        let model = ModelBuilder::new().zone(container).nodes_in("box", &["a", "b", "c", "d"]).build();
        let plan = layout_ok(&model, Density::Spaced);
        assert_invariants(&model, &plan);
        let centers: Vec<PlanPoint> = plan.nodes.iter().map(|node| node.center).collect();
        let distinct = |values: Vec<f64>| {
            let mut sorted = values;
            sorted.sort_by(f64::total_cmp);
            sorted.dedup_by(|a, b| close(*a, *b));
            sorted.len()
        };
        (distinct(centers.iter().map(|c| c.x).collect()), distinct(centers.iter().map(|c| c.y).collect()))
    };
    assert_eq!(build(Arrange::Row), (4, 1));
    assert_eq!(build(Arrange::Column), (1, 4));
    assert_eq!(build(Arrange::Grid), (2, 2));
    assert_eq!(build(Arrange::Auto), (1, 4), "unconnected siblings share a rank and stack across");
}

#[test]
fn auto_folds_a_wide_fan_out_into_several_lanes() {
    let model = ModelBuilder::new()
        .zone(zone("box"))
        .nodes_in("box", &["hub", "a", "b", "c", "d", "e"])
        .edges(&["hub -> a", "hub -> b", "hub -> c", "hub -> d", "hub -> e"])
        .build();
    let plan = layout_ok(&model, Density::Spaced);
    let lanes: Vec<f64> = ["a", "b", "c", "d", "e"].iter().map(|id| center(&plan, id).x).collect();
    assert!(close(lanes[0], lanes[2]) && close(lanes[3], lanes[4]) && lanes[3] > lanes[0], "3 + 2 lanes: {lanes:?}");
}

#[test]
fn nested_zones_sit_inside_their_parent_with_padding() {
    let model = example("kubernetes-platform");
    let plan = layout_ok(&model, Density::Spaced);
    let cluster = zone_rect(&plan, "cluster");
    let padding = plan.params.zone_padding;
    for child in ["ns_web", "ns_orders"] {
        let rect = zone_rect(&plan, child);
        assert!(inside(rect.inflate(padding - 1e-6), cluster), "{child} lacks padding inside cluster");
        let depth = plan.zones.iter().find(|zone| zone.id.as_str() == child).map(|zone| zone.depth);
        assert_eq!(depth, Some(1));
    }
    assert_invariants(&model, &plan);
}

#[test]
fn empty_zones_and_zone_less_nodes_get_their_own_room() {
    let model = ModelBuilder::new()
        .zone(zone("empty"))
        .zone(zone("full"))
        .nodes_in("full", &["a"])
        .nodes(&["free"])
        .edges(&["free -> a"])
        .build();
    for density in Density::ALL {
        let plan = layout_ok(&model, density);
        assert_invariants(&model, &plan);
        let empty = zone_rect(&plan, "empty");
        assert!(empty.width() > 0.0 && empty.depth() > 0.0);
    }
    let plan = layout_ok(&model, Density::Spaced);
    let full = zone_rect(&plan, "full");
    let free = center(&plan, "free");
    assert!(free.y < full.y0 && (full.x0..=full.x1).contains(&free.x), "a zone-less node sits beside its zone");
    assert!(full.x1 < zone_rect(&plan, "empty").x0, "empty zones go last");
}

#[test]
fn zone_less_model_still_flows() {
    let model = example("minimal");
    let plan = layout_ok(&model, Density::Spaced);
    let xs: Vec<f64> = ["users", "lb", "api", "db"].iter().map(|id| center(&plan, id).x).collect();
    assert!(xs.windows(2).all(|pair| close(pair[1] - pair[0], plan.params.node_gap)), "{xs:?}");
    assert!(close(plan.bounds.x0, 0.0) && close(plan.bounds.y0, 0.0), "unpinned plans start at the origin");
}

#[test]
fn pinned_node_is_exact_and_the_rest_moves_around_it() {
    let mut pinned = node("pinned", Some("app"));
    pinned.at = Some(PlanCoord { x: 3.5, y: 2.0 });
    let mut free = node("free_pin", None);
    free.at = Some(PlanCoord { x: -4.0, y: 0.5 });
    let model = ModelBuilder::new()
        .zone(zone("edge"))
        .zone(zone("app"))
        .nodes_in("edge", &["lb"])
        .nodes_in("app", &["api"])
        .node(pinned)
        .node(free)
        .edges(&["lb -> api", "lb -> pinned"])
        .build();
    for density in Density::ALL {
        let plan = layout_ok(&model, density);
        assert_eq!(center(&plan, "pinned"), PlanPoint::new(3.5, 2.0));
        assert_eq!(center(&plan, "free_pin"), PlanPoint::new(-4.0, 0.5));
        assert_invariants(&model, &plan);
    }
}

#[test]
fn pinned_zone_keeps_its_corner_and_size() {
    let mut fixed = zone("fixed");
    fixed.at = Some(PlanCoord { x: 10.0, y: 4.5 });
    fixed.size = Some([6.0, 4.0]);
    let mut sized = zone("sized");
    sized.size = Some([7.5, 5.0]);
    let mut cornered = zone("cornered");
    cornered.at = Some(PlanCoord { x: -9.0, y: -9.0 });
    let model = ModelBuilder::new()
        .zone(zone("auto"))
        .zone(fixed)
        .zone(sized)
        .zone(cornered)
        .nodes_in("auto", &["a"])
        .nodes_in("fixed", &["b", "c"])
        .nodes_in("sized", &["d"])
        .nodes_in("cornered", &["e"])
        .edges(&["a -> b", "b -> d"])
        .flow(FlowAxis::Y)
        .build();
    for density in Density::ALL {
        let plan = layout_ok(&model, density);
        assert_eq!(zone_rect(&plan, "fixed"), PlanRect::new(10.0, 4.5, 16.0, 8.5));
        let sized = zone_rect(&plan, "sized");
        assert!(close(sized.width(), 7.5) && close(sized.depth(), 5.0));
        let cornered = zone_rect(&plan, "cornered");
        assert!(close(cornered.x0, -9.0) && close(cornered.y0, -9.0));
        assert_invariants(&model, &plan);
    }
}

#[test]
fn unknown_icon_is_reported_as_a_diagnostic() {
    let mut broken = node("thing", None);
    broken.icon = Some("no-such-icon".to_owned());
    let model = ModelBuilder::new().node(broken).build();
    let errors = layout(&model, Density::Spaced).err().unwrap_or_default();
    assert!(errors.iter().any(|diagnostic| diagnostic.code == DiagnosticCode::UnknownIcon), "{errors:?}");
}

#[test]
fn layout_is_deterministic() {
    for name in example_names() {
        let model = example(&name);
        for density in Density::ALL {
            assert_eq!(layout_ok(&model, density), layout_ok(&model, density), "{name}");
        }
    }
}

#[test]
fn ranks_are_published_on_the_nodes() {
    let plan = layout_ok(&example("checkout"), Density::Spaced);
    let ranks: Vec<u32> = plan.nodes.iter().map(|node| node.rank).collect();
    assert_eq!(ranks, vec![0, 1, 2, 2, 3, 3]);
}

#[test]
fn four_quarter_turns_are_the_identity() {
    for name in example_names() {
        let base = layout_ok(&example(&name), Density::Compact);
        let mut turned = base.clone();
        for _ in 0..4 {
            turned = rotate(&turned, Rotation::R90);
        }
        assert_eq!(turned.rotation, Rotation::R0);
        for (a, b) in base.nodes.iter().zip(&turned.nodes) {
            assert!(close(a.center.x, b.center.x) && close(a.center.y, b.center.y), "{name}: {}", a.id);
            assert_eq!(a.metrics, b.metrics);
        }
        for (a, b) in base.zones.iter().zip(&turned.zones) {
            assert!(close(a.rect.x0, b.rect.x0) && close(a.rect.y1, b.rect.y1), "{name}: {}", a.id);
        }
    }
}

#[test]
fn quarter_turn_swaps_footprints_and_rectangles() {
    let base = layout_ok(&example("bank-branch"), Density::Spaced);
    let turned = rotate(&base, Rotation::R90);
    assert_eq!(turned.rotation, Rotation::R90);
    for (before, after) in base.nodes.iter().zip(&turned.nodes) {
        assert_eq!(after.metrics, icons::metrics(after.icon, after.size, Rotation::R90));
        let (width, depth) = before.metrics.footprint.half_extents();
        let (turned_width, turned_depth) =
            icons::metrics(before.icon, before.size, Rotation::R90).footprint.half_extents();
        assert!(close(width, turned_depth) && close(depth, turned_width), "{}", before.id);
    }
    for (before, after) in base.zones.iter().zip(&turned.zones) {
        assert!(close(before.rect.width(), after.rect.depth()) && close(before.rect.depth(), after.rect.width()));
    }
    assert!(close(turned.bounds.width(), base.bounds.depth()));
}

/// A random but structurally valid model: zones form a forest, nodes point at existing zones.
pub(super) fn random_model() -> impl Strategy<Value = (Model, Density)> {
    let zones = prop::collection::vec((prop::option::of(0_usize..8), 0_u8..4), 0..6);
    let nodes = prop::collection::vec((prop::option::of(0_usize..8), 0_u8..3), 1..14);
    let edges = prop::collection::vec((0_usize..14, 0_usize..14), 0..20);
    let options = (any::<bool>(), 0_u32..4, any::<bool>());
    (zones, nodes, edges, options).prop_map(|(zones, nodes, edges, (flow_y, wrap, compact))| {
        let mut builder = ModelBuilder::new();
        let zone_count = zones.len();
        for (index, (parent, arrange)) in zones.into_iter().enumerate() {
            let mut entry = zone(&format!("z{index}"));
            entry.parent = parent.filter(|parent| *parent < index).map(|parent| format!("z{parent}").as_str().into());
            entry.arrange = [Arrange::Auto, Arrange::Row, Arrange::Column, Arrange::Grid][usize::from(arrange)];
            builder = builder.zone(entry);
        }
        let node_count = nodes.len();
        for (index, (zone_choice, size)) in nodes.into_iter().enumerate() {
            let zone_id = zone_choice.filter(|zone| *zone < zone_count).map(|zone| format!("z{zone}"));
            let mut entry = node(&format!("n{index}"), zone_id.as_deref());
            entry.size = [NodeSize::S, NodeSize::M, NodeSize::L][usize::from(size)];
            builder = builder.node(entry);
        }
        let mut model = builder.build();
        model.edges = edges
            .into_iter()
            .filter(|(from, to)| *from < node_count && *to < node_count)
            .map(|(from, to)| edge(&format!("n{from}"), &format!("n{to}")))
            .collect();
        if flow_y {
            model.layout.flow = FlowAxis::Y;
        }
        if wrap > 0 {
            model.layout.wrap = Wrap::Count(wrap);
        }
        (model, if compact { Density::Compact } else { Density::Spaced })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_models_never_overlap((model, density) in random_model()) {
        let plan = layout_ok(&model, density);
        assert_invariants(&model, &plan);
    }
}

/// Debug aid: `DIORAMIC_LAYOUT_DUMP=<dir> cargo test -p dioramic-core layout` writes a plain isometric
/// sketch of every example layout (zones as floor diamonds, nodes as footprints) to look at.
pub(super) mod dump {
    use std::fmt::Write as _;

    use super::*;
    use crate::geometry::{COS_30, SIN_30};

    const TILE: f64 = 28.0;

    fn project(point: PlanPoint) -> (f64, f64) {
        ((point.x - point.y) * COS_30 * TILE, (point.x + point.y) * SIN_30 * TILE)
    }

    fn polygon(rect: PlanRect, style: &str) -> String {
        let points: Vec<String> = rect
            .corners()
            .iter()
            .map(|corner| {
                let (x, y) = project(*corner);
                format!("{x:.1},{y:.1}")
            })
            .collect();
        format!("<polygon points=\"{}\" {style}/>", points.join(" "))
    }

    pub fn write(name: &str, model: &Model, plan: &PlanLayout) {
        let Ok(directory) = std::env::var("DIORAMIC_LAYOUT_DUMP") else { return };
        let corners = plan.bounds.inflate(1.0).corners().map(project);
        let (x0, x1) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (x, _)| (lo.min(*x), hi.max(*x)));
        let (y0, y1) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, y)| (lo.min(*y), hi.max(*y)));
        // Fit a 700 px square (the size of the Quick Look thumbnails used to eyeball them).
        let scale = 700.0 / (x1 - x0).max(y1 - y0);
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{x0:.1} {y0:.1} {:.1} {:.1}\" width=\"{:.0}\" height=\"{:.0}\" style=\"background:#0f1b2d;font:9px sans-serif\">",
            x1 - x0,
            y1 - y0,
            (x1 - x0) * scale,
            (y1 - y0) * scale
        );
        for zone in &plan.zones {
            let fill = ["#1d3557", "#274472", "#3a5a8c"][(zone.depth as usize).min(2)];
            svg.push_str(&polygon(zone.rect, &format!("fill=\"{fill}\" stroke=\"#8ab4f8\" stroke-dasharray=\"4 3\"")));
            let (x, y) = project(PlanPoint::new(zone.rect.x0, zone.rect.y1));
            let _ = write!(svg, "<text x=\"{x:.1}\" y=\"{:.1}\" fill=\"#ffd166\">{}</text>", y + 10.0, zone.id);
        }
        for edge in &model.edges {
            let (Some(from), Some(to)) = (plan.node(&edge.from), plan.node(&edge.to)) else { continue };
            let ((ax, ay), (bx, by)) = (project(from.center), project(to.center));
            let _ = write!(
                svg,
                "<line x1=\"{ax:.1}\" y1=\"{ay:.1}\" x2=\"{bx:.1}\" y2=\"{by:.1}\" stroke=\"#f4a261\" stroke-width=\"1.5\"/>"
            );
        }
        for node in &plan.nodes {
            svg.push_str(&polygon(footprint_rect(node), "fill=\"#e9eef5\" stroke=\"#0f1b2d\""));
            let (x, y) = project(node.center);
            let _ = write!(
                svg,
                "<text x=\"{x:.1}\" y=\"{:.1}\" fill=\"#fff\" text-anchor=\"middle\">{} r{}</text>",
                y - 14.0,
                node.id,
                node.rank
            );
        }
        svg.push_str("</svg>");
        let _ = std::fs::write(format!("{directory}/{name}.svg"), svg);
    }
}

/// Whether the segment `from → to` crosses `rect` (Liang–Barsky clipping).
fn segment_crosses(from: PlanPoint, to: PlanPoint, rect: PlanRect) -> bool {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let mut range = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, from.x - rect.x0), (dx, rect.x1 - from.x), (-dy, from.y - rect.y0), (dy, rect.y1 - from.y)] {
        if p.abs() < EPSILON {
            if q < 0.0 {
                return false;
            }
        } else if p < 0.0 {
            range.0 = range.0.max(q / p);
        } else {
            range.1 = range.1.min(q / p);
        }
    }
    range.0 < range.1
}

fn place<'p>(plan: &'p PlanLayout, id: &str) -> &'p NodePlace {
    plan.nodes.iter().find(|node| node.id.as_str() == id).unwrap_or_else(|| panic!("node {id}"))
}

/// Free floor between two footprints (0 when they touch; the larger of the gaps along x and y).
fn footprint_gap(a: &NodePlace, b: &NodePlace) -> f64 {
    let (a, b) = (footprint_rect(a), footprint_rect(b));
    let along_x = (b.x0 - a.x1).max(a.x0 - b.x1);
    let along_y = (b.y0 - a.y1).max(a.y0 - b.y1);
    along_x.max(along_y).max(0.0)
}

#[test]
fn spaced_keeps_a_legible_flow_in_one_row() {
    for name in ["checkout", "event-driven", "minimal"] {
        let plan = layout_ok(&example(name), Density::Spaced);
        let rows: Vec<f64> =
            plan.zones.iter().filter(|zone| zone.depth == 0).map(|zone| zone.rect.center().y).collect();
        assert!(rows.iter().all(|y| close(*y, rows[0])), "{name}: one row");
    }
}

#[test]
fn spaced_folds_a_long_diagonal_into_a_balanced_block() {
    for name in ["bank-branch", "ml-pipeline"] {
        let plan = layout_ok(&example(name), Density::Spaced);
        let top_level: Vec<PlanRect> = plan.zones.iter().filter(|zone| zone.depth == 0).map(|zone| zone.rect).collect();
        let rows = top_level.iter().filter(|rect| top_level.iter().all(|other| rect.y0 <= other.y1)).count();
        assert!(rows < top_level.len(), "{name}: the zones are folded in more than one row");
        let ratio = plan.bounds.width() / plan.bounds.depth();
        assert!((0.5..=2.5).contains(&ratio), "{name}: plan {:.1} × {:.1}", plan.bounds.width(), plan.bounds.depth());
    }
}

#[test]
fn spaced_fold_follows_a_serpentine_flow() {
    let plan = layout_ok(&example("ml-pipeline"), Density::Spaced);
    let [ingest, features, training, serving, ops] =
        ["ingest", "features", "training", "serving", "ops"].map(|id| zone_rect(&plan, id));
    assert!(ingest.x1 < features.x0 && features.x1 < training.x0, "the first row runs along the flow");
    assert!(serving.y0 > training.y1 && ops.x1 < serving.x0, "the second row comes back");
}

#[test]
fn zone_less_node_sits_beside_the_zone_it_talks_to() {
    let plan = layout_ok(&example("bank-branch"), Density::Spaced);
    let comms = zone_rect(&plan, "comms");
    let rack = footprint_rect(place(&plan, "rack"));
    assert!(rack.x1 > comms.x0 && rack.x0 < comms.x1, "rack shares the comms column");
    assert!(rack.y1 <= comms.y0 && comms.y0 - rack.y1 < plan.params.node_gap, "rack is right behind comms");
}

#[test]
fn zone_less_nodes_stay_off_the_edges_between_zones() {
    for name in ["bank-branch", "ml-pipeline"] {
        let model = example(name);
        for density in Density::ALL {
            let base = layout_ok(&model, density);
            for rotation in Rotation::ALL {
                let plan = rotate(&base, rotation);
                let zoned = |id: &str| model.nodes.iter().any(|node| node.id.as_str() == id && node.zone.is_some());
                for free in model.nodes.iter().filter(|node| node.zone.is_none()) {
                    let footprint = footprint_rect(place(&plan, free.id.as_str()));
                    for edge in model.edges.iter().filter(|edge| zoned(edge.from.as_str()) && zoned(edge.to.as_str())) {
                        let (from, to) = (place(&plan, edge.from.as_str()), place(&plan, edge.to.as_str()));
                        assert!(
                            !segment_crosses(from.center, to.center, footprint),
                            "{name} {density:?} {rotation:?}: {} -> {} crosses {}",
                            edge.from,
                            edge.to,
                            free.id
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn zone_less_node_linked_to_one_zone_is_hosted_beside_it() {
    let model = ModelBuilder::new()
        .zone(zone("a"))
        .zone(zone("b"))
        .nodes_in("a", &["a1"])
        .nodes_in("b", &["b1"])
        .nodes(&["client", "loner"])
        .edges(&["a1 -> b1", "client -> b1"])
        .build();
    for density in Density::ALL {
        let plan = layout_ok(&model, density);
        assert_invariants(&model, &plan);
        let b = zone_rect(&plan, "b");
        let client = center(&plan, "client");
        let across = client.y < b.y0 || client.y > b.y1;
        assert!(
            b.x0 < client.x && client.x < b.x1 && across,
            "{density:?}: client is hosted beside b, across the flow"
        );
        assert!(!segment_crosses(center(&plan, "a1"), center(&plan, "b1"), footprint_rect(place(&plan, "client"))));
    }
}

#[test]
fn compact_icons_leave_their_zone_border_free_for_the_label() {
    for name in example_names() {
        let model = example(&name);
        let plan = layout_ok(&model, Density::Compact);
        for (place, node) in plan.nodes.iter().zip(&model.nodes) {
            let Some(zone) = &node.zone else { continue };
            let room = footprint_rect(place).inflate(COMPACT_BORDER_ROOM - 1e-6);
            assert!(inside(room, zone_rect(&plan, zone.as_str())), "{name}: {} crowds the border of {zone}", place.id);
        }
    }
}

#[test]
fn compact_neighbours_along_the_flow_keep_a_visible_edge_run() {
    for name in example_names() {
        let model = example(&name);
        let plan = layout_ok(&model, Density::Compact);
        for edge in &model.edges {
            let (from, to) = (place(&plan, edge.from.as_str()), place(&plan, edge.to.as_str()));
            let same_stack = (from.center.x - to.center.x).abs() < EPSILON;
            if same_stack {
                continue;
            }
            let gap = footprint_gap(from, to);
            assert!(gap >= COMPACT_EDGE_RUN - 1e-6, "{name}: {} -> {} has {gap:.2} tile of floor", edge.from, edge.to);
        }
    }
}

#[test]
fn compact_zone_padding_grows_with_its_largest_icon() {
    let model = ModelBuilder::new()
        .zone(zone("small"))
        .zone(zone("large"))
        .node(node_with_icon("phone", "small", "voip-phone"))
        .node(node_with_icon("cloud", "large", "cloud"))
        .edges(&["phone -> cloud"])
        .build();
    let plan = layout_ok(&model, Density::Compact);
    let (small, large) = (zone_rect(&plan, "small"), zone_rect(&plan, "large"));
    assert!(large.width() > small.width(), "a cloud needs a larger zone than a phone");
    assert!(close(small.width(), 2.0 * plan.params.zone_padding.max(0.36 + COMPACT_BORDER_ROOM)));
}

fn node_with_icon(id: &str, zone_id: &str, icon: &str) -> dioramic_spec::Node {
    let mut node = node(id, Some(zone_id));
    node.icon = Some(icon.to_owned());
    node
}

/// A hub switch with six links inside its zone; `labelled` names the one link that carries a pill.
fn hub_zone(labelled: Option<&str>) -> Model {
    let mut model = ModelBuilder::new()
        .zone(zone("room"))
        .nodes_in("room", &["hub", "a", "b", "c", "d", "e", "f"])
        .edges(&["hub -> a", "hub -> b", "hub -> c", "hub -> d", "hub -> e", "hub -> f"])
        .build();
    for edge in &mut model.edges {
        if Some(edge.to.as_str()) == labelled {
            edge.label = Some("1Gbps Cat6".to_owned());
        }
    }
    model
}

#[test]
fn spaced_hub_leaves_room_for_the_pill_of_a_labelled_link_inside_its_zone() {
    let lane_gap = |model: &Model| {
        let plan = layout_ok(model, Density::Spaced);
        center(&plan, "a").x - center(&plan, "hub").x
    };
    let plain = lane_gap(&hub_zone(None));
    let labelled = lane_gap(&hub_zone(Some("a")));
    assert!(close(labelled - plain, SPACED_PILL_RUN), "plain {plain}, labelled {labelled}");
}

#[test]
fn spaced_pill_room_is_only_for_hubs() {
    let model = ModelBuilder::new().zone(zone("room")).nodes_in("room", &["a", "b"]).edges(&["a -> b: gRPC"]).build();
    let mut labelled = model.clone();
    labelled.edges[0].label = Some("gRPC".to_owned());
    let gap = |model: &Model| {
        let plan = layout_ok(model, Density::Spaced);
        center(&plan, "b").x - center(&plan, "a").x
    };
    assert!(close(gap(&model), gap(&labelled)));
}

#[test]
fn compact_siblings_across_the_flow_leave_room_for_their_labels() {
    let model =
        ModelBuilder::new().zone(zone("z")).nodes_in("z", &["src", "a", "b"]).edges(&["src -> a", "src -> b"]).build();
    let plan = layout_ok(&model, Density::Compact);
    let across = (center(&plan, "a").y - center(&plan, "b").y).abs();
    assert!(across >= plan.params.node_gap + COMPACT_LABEL_ROOM_ACROSS - EPSILON, "across {across}");
}
