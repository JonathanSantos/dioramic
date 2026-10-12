//! Edges: style resolution, arrival ports (spread and kept visible), paths and projected heads.
//!
//! Order of work: every head end asks for a border angle toward the other node; the angles of all heads
//! arriving at the same node are spread apart inside the allowed arc (visible faces for tall nodes);
//! a head still covered by another node's silhouette is nudged along the arc; then each edge gets its
//! path (curved / straight / orthogonal / rails) ending at the head bases, tangent to the heads.
//!
//! Rails (T-406/T-407) are elbows with the fewest bends: a rails head may arrive at any face, and on a face
//! hidden behind its node it stands where the rail comes out of the node's silhouette.
//!
//! Fans (T-400): ends at one node keep the angular order of the nodes they point to (ends clamped to
//! the same corner of a tall node are ordered by how far behind they come from); the route cost
//! charges every crossing, and heavily a crossing between edges that share a node; a pair that still
//! crosses swaps its ports at the shared node and re-routes.

use std::cell::OnceCell;
use std::f64::consts::FRAC_PI_2;

use dioramic_spec::{ArrowEnds, Edge, EdgeColors, EdgeKind, Model, Routing, ValueSource};

use crate::color::Color;
use crate::geometry::{PlanPoint, PlanRect, PlanVector, Projection, Rect, ScreenPoint};
use crate::layout::PlanLayout;
use crate::theme::{ArrowPosition, LinePattern, Theme};

use super::elbow::{DrawnRail, ElbowObstacles, RailPort, elbow_corners, glued_length, plain_elbow};
use super::labels::measure_node_labels;
use super::legend::class_look;
use super::lint::{ALONG_NODE_MIN_PX, THROUGH_NODE_MIN_PX};
use super::midway::midway_heads;
use super::nodes::{DrawnNodes, NodeGeometry, inside_convex, outside_distance};
use super::ports::{
    ArrivalArc, ArrivalPort, PORT_GAP, angle_of, border_point, direction_at, face_normal, spread_arrivals,
    triangles_overlap,
};
use super::rails::{
    BEND_COST as RAIL_BEND_COST, FaceEnd, ORTHOGONAL_BEND_RADIUS, RAIL_BEND_RADIUS, RAIL_FACE_USE, RailGeometry,
    share_faces,
};
use super::routing::{ArmScale, PathEnd, RouteGrid, cubic_path, orthogonal_path_on, scaled_cubic_path};
use super::visibility::{
    Cover, GRAZE_CLEARANCE_PX, MIN_VISIBLE_EDGE_PX, Trace, coincident_length, crossing_count, touches,
};
use super::{ARROW_HALF_WIDTH, ARROW_LENGTH, ArrowHead, EDGE_Z, EdgePath, RAIL_LANE, SceneEdge};
use crate::icons::Footprint;

/// Angular step and number of steps tried when a head is hidden behind another node.
const UNHIDE_STEP: f64 = std::f64::consts::PI / 18.0;
const UNHIDE_MAX_STEPS: i32 = 18;
/// A head point closer than this to a silhouette side still counts as visible (px).
const SILHOUETTE_INSET_PX: f64 = 0.5;
/// Half-angle (radians) between the two ends of a self-loop, around the node's front.
const SELF_LOOP_HALF_ANGLE: f64 = 0.55;
/// Clearance (px) between the silhouette of a node and the tip of a rails head pushed out of it (T-407).
const RAIL_TIP_CLEARANCE_PX: f64 = 1.0;
/// Bisection steps when pushing a rails head out of a silhouette (far below a px).
const RAIL_TIP_SEARCH_STEPS: usize = 40;
/// Farthest (tiles) a rails head is pushed out of a silhouette (taller than any icon).
const RAIL_TIP_MAX_PUSH: f64 = 16.0;
/// Samples of a face's slide range when finding how far out the lead of a hidden face must start.
const RAIL_TIP_LATERAL_SAMPLES: usize = 9;
/// Samples of a crowded face when looking for the spot farthest from the ports already on it: fine enough that
/// three ports a lane apart fit on a face just three lanes wide (T-413).
const ROOMIEST_SPOT_SAMPLES: usize = 41;
/// Cost per tile of a rail running back over the body of a node it connects (T-407): worse than any detour.
const THROUGH_OWN_NODE_COST: f64 = 2000.0;
/// Inset (tiles) of an end node's footprint when measuring a rail over it (its ports lie on the border).
const OWN_BODY_INSET: f64 = 1e-3;
/// Cost of a rail head reaching a face hidden behind its node (the head at the silhouette, T-407): more
/// than [`RAIL_FACE_TURN_COST`], so a visible face wins when it costs no extra bend, and less than a bend
/// minus that, so a hidden face that saves a bend wins (fewest bends first, no detours).
const HIDDEN_FACE_COST: f64 = 24.0;
const _: () =
    assert!(HIDDEN_FACE_COST > RAIL_FACE_TURN_COST && HIDDEN_FACE_COST < RAIL_FACE_TURN_COST + RAIL_BEND_COST);
/// Cost per tile of a rail running parallel to another rail closer than one lane ([`RAIL_LANE`]): moving one
/// lane over is always cheaper (T-407).
const GLUED_RAIL_COST: f64 = 1000.0;

/// Which node an edge end belongs to and whether it carries a head.
#[derive(Debug, Clone, Copy)]
struct EndSpec {
    edge: usize,
    at_start: bool,
    node: usize,
    other: usize,
    head: bool,
}

/// Builds the scene edges in spec order (labels are placed later).
///
/// Every edge first gets its natural route; then the edges a reader would misread (running through a
/// node, hidden behind one, or on top of another edge) try a few variants of their ends and control arms
/// and keep the cheapest one ([`Router::improve`]).
pub fn build_edges(
    model: &Model,
    turned: &PlanLayout,
    drawn: &DrawnNodes,
    theme: &Theme,
    projection: &Projection,
    kinds: &[EdgeKind],
) -> Vec<SceneEdge> {
    let ends = edge_ends(model, drawn);
    let angles = arrival_angles(model, &ends, drawn, projection);
    let router = Router {
        model,
        ends: &ends,
        drawn,
        projection,
        obstacles: drawn.geometry.iter().map(footprint_rect).collect(),
        bounds: turned.bounds,
        covers: drawn.silhouettes.iter().map(|silhouette| Cover::new(silhouette)).collect(),
        label_columns: label_columns(model, drawn, theme, turned.params.label_lift_px),
        grid: OnceCell::new(),
        order: routing_order(model, drawn),
        heads_midway: theme.edge_drawing.arrow_position == ArrowPosition::Middle,
    };
    // Each edge is first routed among the ones before it (rails avoid the rails already drawn).
    let mut routes: Vec<Option<Route>> = vec![None; model.edges.len()];
    for &index in &router.order {
        let natural = Variant { rails: model.edges[index].route.is_elbow(), ..Variant::NATURAL };
        routes[index] = router.route(index, &angles, natural, &routes);
    }
    let explored = vec![false; model.edges.len()];
    let mut state = RouteState { routes, base: angles.clone(), angles, explored };
    router.improve(&mut state);
    router.untangle_fans(&mut state);
    if model.edges.iter().any(|edge| edge.route.is_elbow()) {
        // A rail avoids the rails drawn when it is routed: after the fans are untangled, the others get
        // another look at the final picture (T-406).
        router.improve(&mut state);
        router.reroute_detours(&mut state);
    }
    let mut edges = Vec::with_capacity(model.edges.len());
    for ((edge, route), kind) in model.edges.iter().zip(state.routes).zip(kinds.iter().copied()) {
        let Some(route) = route else { continue };
        let (color, width, dashed) = resolve_style(model, edge, theme, kind);
        let midway = match theme.edge_drawing.arrow_position {
            ArrowPosition::End => Vec::new(),
            ArrowPosition::Middle => midway_heads(&route.path, &route.heads, projection),
        };
        edges.push(SceneEdge {
            id: edge.id.clone(),
            from: edge.from.clone(),
            to: edge.to.clone(),
            path: route.path,
            heads: route.heads,
            midway,
            kind,
            color,
            width,
            dashed,
            flow: edge.flow,
            label: None,
        });
    }
    edges
}

/// Turns (radians) tried on an end's border angle, natural first.
const END_TURNS: [f64; 7] = [0.0, 0.35, -0.35, 0.7, -0.7, 1.05, -1.05];
/// Plan angles of the four face axes (+x, +y, −x, −y).
const FACE_AXES: [f64; 4] = [0.0, FRAC_PI_2, std::f64::consts::PI, -FRAC_PI_2];
/// Control arm scales tried (see [`ArmScale`]).
const ARM_SCALES: [ArmScale; 4] = [
    ArmScale::NORMAL,
    ArmScale { leave: 1.7, arrive: 1.0 },
    ArmScale { leave: 1.0, arrive: 1.7 },
    ArmScale { leave: 1.7, arrive: 1.7 },
];
/// Cost per px of stroke inside another node's silhouette.
const THROUGH_NODE_COST: f64 = 8.0;
/// Cost per px of visible stroke hugging another node's outline (reads as passing under it, T-381).
const GRAZING_COST: f64 = 4.0;
/// Cost per px missing from [`MIN_VISIBLE_EDGE_PX`] of visible stroke.
const HIDDEN_COST: f64 = 10.0;
/// Flat cost of an edge showing less than [`MIN_VISIBLE_EDGE_PX`]: a stroke that does not read as a line
/// is worse than one hugging a node outline ([`GRAZING_COST`]), whatever the missing length.
const UNREADABLE_EDGE_COST: f64 = 200.0;
/// Cost per px of stroke running on top of another edge.
const COINCIDENT_COST: f64 = 5.0;
/// Flat cost per node the route would be linted for (`W_EDGE_THROUGH_NODE`: through it, or along its
/// outline): above [`SHARED_NODE_CROSSING_COST`], so untangling a fan never earns a lint (T-400).
const LINTED_NODE_COST: f64 = 400.0;
/// [`LINTED_NODE_COST`] on rails: above a tile of rail detour ([`super::rails`]) and an extra bend, so keeping
/// to the fewest bends never makes a rail read as under a node (T-407).
const RAIL_LINTED_NODE_COST: f64 = 2000.0;
/// Cost per crossing with an edge that shares no node with this one: a crossing is sometimes unavoidable,
/// so it only breaks ties between otherwise clean routes.
const CROSSING_COST: f64 = 12.0;
/// Cost per crossing with an edge that shares a node: the fan reads as an "X" (T-400). Above
/// [`UNREADABLE_EDGE_COST`] so no other defect is ever traded for it.
const SHARED_NODE_CROSSING_COST: f64 = 300.0;
/// [`SHARED_NODE_CROSSING_COST`] on rails: a rail can always detour on its own lane (T-406), so a fan that
/// crosses is worth more than a long way round through a label column.
const RAIL_SHARED_NODE_CROSSING_COST: f64 = 1500.0;
/// Cost of a rail crossing another edge twice or more: the two strokes read as plaited (T-425: two rails of
/// the AI assistant's orchestrator that swap sides twice). More than a bend and a little length, so either
/// rail takes another face.
const RAIL_PLAIT_COST: f64 = 120.0;
/// Cost per px of visible stroke in the column above a labelled node, where its label and leader go.
const LABEL_COLUMN_COST: f64 = 2.0;
/// [`LABEL_COLUMN_COST`] on rails: a label moves aside more easily than a rail detours round its column
/// (fewest bends, no detours first, T-407).
const RAIL_LABEL_COLUMN_COST: f64 = 0.5;
/// [`RAIL_LABEL_COLUMN_COST`] in the column of a node the rail does not connect (T-422): that label can only
/// move off the rail, and in a crowded hub it has nowhere to go.
const RAIL_FOREIGN_LABEL_COLUMN_COST: f64 = 1.5;
/// Preference for the natural route: cost per radian of end turn and per unit of extra arm length.
const TURN_COST: f64 = 40.0;
/// Preference for the natural face of a rail end: leaving or arriving by another face costs less than one
/// bend ([`super::rails`]), so a face that saves a bend wins (fewest bends first, T-406).
const RAIL_FACE_TURN_COST: f64 = 12.0;
const ARM_COST: f64 = 30.0;
/// A curved edge drawn as an orthogonal route looks different: only when nothing curved works.
const ORTHOGONAL_FALLBACK_COST: f64 = 160.0;
/// Misreading cost under which an edge keeps its natural route (a few px in a label column).
const MISREADING_WORTH_FIXING: f64 = 40.0;
/// Relative margin of the early exit of [`Router::misreading`].
const EARLY_EXIT_MARGIN: f64 = 1e-9;
/// Passes of [`Router::untangle_fans`] (a swap can uncover another crossing).
const UNTANGLE_PASSES: usize = 2;
/// Improvement passes over all edges: each pass sees the previous choices, and a fan untangled by one edge
/// can let its sibling go back to a cheaper route (stops early when a pass changes nothing).
const IMPROVE_PASSES: usize = 4;
/// Passes of [`Router::reroute_detours`] (lifting one sibling can free the way for another rail).
const REROUTE_PASSES: usize = 3;

/// One way of drawing an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Variant {
    start_turn: f64,
    end_turn: f64,
    arms: ArmScale,
    orthogonal: bool,
    /// A rails variant (its face turns are priced per turned end, not per radian).
    rails: bool,
}

impl Variant {
    const NATURAL: Variant =
        Variant { start_turn: 0.0, end_turn: 0.0, arms: ArmScale::NORMAL, orthogonal: false, rails: false };

    /// Cost of leaving the natural route.
    fn deviation(&self) -> f64 {
        let turns = if self.rails {
            let turned = [self.start_turn, self.end_turn].iter().filter(|turn| **turn != 0.0).count();
            RAIL_FACE_TURN_COST * turned as f64
        } else {
            TURN_COST * (self.start_turn.abs() + self.end_turn.abs())
        };
        let arms = ARM_COST * ((self.arms.leave - 1.0) + (self.arms.arrive - 1.0));
        let fallback = if self.orthogonal { ORTHOGONAL_FALLBACK_COST } else { 0.0 };
        turns + arms + fallback
    }
}

/// A routed edge.
#[derive(Debug, Clone)]
struct Route {
    path: EdgePath,
    heads: Vec<ArrowHead>,
    trace: Trace,
    variant: Variant,
    /// Geometry cost of a rail (length, bends, clearance; 0 for other routings).
    rail_cost: f64,
    /// Plan corners of a rail (empty for other routings).
    corners: Vec<PlanPoint>,
}

/// Routes and end angles while edges are being improved.
#[derive(Clone)]
struct RouteState {
    /// Natural angle of every end (turns of the variants are relative to it).
    base: Vec<f64>,
    /// Angle of every end as currently routed.
    angles: Vec<f64>,
    routes: Vec<Option<Route>>,
    /// Edges whose variants were all compared at least once.
    explored: Vec<bool>,
}

/// Everything needed to route and judge edges.
struct Router<'a> {
    model: &'a Model,
    ends: &'a [EndSpec],
    drawn: &'a DrawnNodes,
    projection: &'a Projection,
    obstacles: Vec<PlanRect>,
    bounds: PlanRect,
    covers: Vec<Cover<'a>>,
    /// Column above each labelled node (its position, the box) where its label will most likely go.
    label_columns: Vec<(usize, Rect)>,
    /// Orthogonal routing grid, built on first use.
    grid: OnceCell<RouteGrid>,
    /// Order in which edges are routed and improved ([`routing_order`]).
    order: Vec<usize>,
    /// True when the theme draws heads midway along the edge (`schematic`): a port then needs no room for a
    /// head's half width, so a crowded face may use all of its length.
    heads_midway: bool,
}

impl Router<'_> {
    /// Path and heads of edge `index` drawn as `variant` (turns relative to `angles`).
    fn route(&self, index: usize, angles: &[f64], variant: Variant, routes: &[Option<Route>]) -> Option<Route> {
        let [start, end] = ends_of(self.ends, index)?;
        let routing = self.model.edges[index].route;
        let projection = self.projection;
        let mut start_end = path_end(
            &self.ends[start],
            angles[start] + variant.start_turn,
            self.drawn,
            projection,
            self.head_placement(routing),
        );
        let mut end_end = path_end(
            &self.ends[end],
            angles[end] + variant.end_turn,
            self.drawn,
            projection,
            self.head_placement(routing),
        );
        let grid = || self.grid.get_or_init(|| RouteGrid::new(&self.obstacles, self.bounds));
        let orthogonal = || {
            orthogonal_path_on(grid(), projection, &start_end.path, &end_end.path)
                .unwrap_or_else(|| cubic_path(projection, &start_end.path, &end_end.path, false))
        };
        let mut rail_cost = 0.0;
        let mut corners = Vec::new();
        let path = match routing {
            Routing::Straight => cubic_path(projection, &start_end.path, &end_end.path, true),
            Routing::Curved if variant.orthogonal => orthogonal(),
            Routing::Curved => scaled_cubic_path(projection, &start_end.path, &end_end.path, false, variant.arms),
            Routing::Rails | Routing::Orthogonal => {
                let drawn: Vec<DrawnRail<'_>> = routes
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .filter_map(|(other, route)| {
                        let route = route.as_ref().filter(|route| !route.corners.is_empty())?;
                        Some(DrawnRail { corners: &route.corners, shares_node: self.share_node(index, other) })
                    })
                    .collect();
                let heights: Vec<f64> = self.drawn.geometry.iter().map(|node| node.z + node.height).collect();
                let own = [start_end.node, end_end.node];
                let obstacles = ElbowObstacles { nodes: &self.obstacles, heights: &heights, rails: &drawn, own };
                let (from, to) = (self.rail_port(&start_end, start, routes), self.rail_port(&end_end, end, routes));
                let mut found = elbow_corners(&from, &to, &obstacles).unwrap_or_else(|| plain_elbow(&from, &to));
                // The ports may have slid along their faces: the ends go where the rail ends.
                let mut hidden_faces = 0;
                if let Some(first) = found.first_mut() {
                    hidden_faces += usize::from(self.settle_rail_end(&mut start_end, first));
                }
                if let Some(last) = found.last_mut() {
                    hidden_faces += usize::from(self.settle_rail_end(&mut end_end, last));
                }
                let rail = RailGeometry::from_corners_with_radius(found.clone(), bend_radius(routing));
                let others: Vec<PlanRect> = (0..self.obstacles.len())
                    .filter(|&node| node != start_end.node && node != end_end.node)
                    .map(|node| self.obstacles[node])
                    .collect();
                // Nodes closer than two leads may leave the search no room but to cut back over an end node.
                let own: Vec<PlanRect> = [start_end.node, end_end.node]
                    .iter()
                    .map(|&node| self.obstacles[node].inflate(-OWN_BODY_INSET))
                    .collect();
                let glued: f64 = drawn.iter().map(|other| glued_length(&rail.corners, other.corners)).sum();
                let ends = [self.obstacles[start_end.node], self.obstacles[end_end.node]];
                // The stroke runs on past the base of a head to its tip.
                let reach = [&start_end, &end_end].map(|end| if end.port.is_some() { ARROW_LENGTH } else { 0.0 });
                rail_cost = rail.cost(&others, ends, reach)
                    + HIDDEN_FACE_COST * hidden_faces as f64
                    + THROUGH_OWN_NODE_COST * rail.length_inside(&own)
                    + GLUED_RAIL_COST * glued;
                // Priced as routed; drawn reaching the outline of a floating body (T-425).
                let reached = [
                    found.first_mut().is_some_and(|first| self.reach_floating_body(&mut start_end, first)),
                    found.last_mut().is_some_and(|last| self.reach_floating_body(&mut end_end, last)),
                ];
                let rail = if reached.contains(&true) {
                    RailGeometry::from_corners_with_radius(found, bend_radius(routing))
                } else {
                    rail
                };
                corners.clone_from(&rail.corners);
                rail.project(projection)
            }
        };
        let heads = [&end_end, &start_end]
            .into_iter()
            .filter_map(|resolved| {
                resolved.port.map(|port| {
                    project_head(projection, &port, &self.drawn.geometry[resolved.node], routing.is_elbow())
                })
            })
            .collect();
        let trace = Trace::of(&path);
        Some(Route { path, heads, trace, variant, rail_cost, corners })
    }

    /// A rail end that may slide along its face, inside the share of the face rails use (a head keeps
    /// its half width clear of the corners) and inside its own slot among the other rail ends on that face
    /// ([`Self::slot_range`]), so two ports never meet and their order holds.
    fn rail_port(&self, end: &ResolvedEnd, spec: usize, routes: &[Option<Route>]) -> RailPort {
        let geometry = &self.drawn.geometry[end.node];
        let normal = end.path.inward * -1.0;
        let (half_width, half_depth) = geometry.footprint.half_extents();
        let across_x = normal.x.abs() > normal.y.abs();
        let (center, half_face) =
            if across_x { (geometry.center.y, half_depth) } else { (geometry.center.x, half_width) };
        // A head drawn midway (`schematic`) leaves nothing at the port that needs room (T-421: its half width
        // narrowed the face enough to force a step just before the port).
        let margin = if end.port.is_some() && !self.heads_midway { ARROW_HALF_WIDTH } else { 0.0 };
        // A round body's "face" is the tangent line square to the axis, where its rails start (T-407: they
        // slide along it too, so two of them never share a port).
        let reach = (half_face * RAIL_FACE_USE - margin).max(0.0);
        let face = (center - reach, center + reach);
        // A crowded face may spill its slots towards the corners before two ports come within a lane (T-413).
        let whole_reach = (half_face - margin).max(reach);
        let whole_face = (center - whole_reach, center + whole_reach);
        let lateral = match self.slot_range(spec, routes, normal) {
            Some((low, high)) if low <= face.1 && high >= face.0 => (face.0.max(low), face.1.min(high)),
            Some((low, high)) if low <= whole_face.1 && high >= whole_face.0 => {
                (whole_face.0.max(low), whole_face.1.min(high))
            }
            // No slot a lane from every neighbour in fan order: the spot of the whole face farthest from them.
            _ => {
                let own = lateral_across(end.path.point, normal);
                let spot = self.roomiest_spot(spec, routes, normal, whole_face).unwrap_or(own);
                (spot, spot)
            }
        };
        RailPort { point: self.rail_lead_point(end, normal, lateral), inward: end.path.inward, lateral }
    }

    /// Range of the cross coordinate the rail end `spec` may slide over on the face with outward `normal`
    /// without coming within a lane of the ends of the rails already drawn from that face, on the side of
    /// each that keeps the fan in the order of the nodes it leads to (the order of [`share_faces`]), so two
    /// ports never meet and their rails leave a lane apart. `None` when that leaves no room on the face.
    fn slot_range(&self, spec: usize, routes: &[Option<Route>], normal: PlanVector) -> Option<(f64, f64)> {
        let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
        // Sign of the lateral coordinate along the face tangent (−n.y, n.x), in which fans are ordered.
        let tangent_sign = if normal.x.abs() > normal.y.abs() { normal.x.signum() } else { -normal.y.signum() };
        let fan_key = |end: usize| self.fan_key(end, normal);
        for (other, theirs) in self.ports_on_face_of(spec, routes, normal) {
            let before = fan_key(spec).total_cmp(&fan_key(other)).then(spec.cmp(&other)).is_lt();
            if before == (tangent_sign > 0.0) {
                high = high.min(theirs - RAIL_LANE);
            } else {
                low = low.max(theirs + RAIL_LANE);
            }
        }
        (low <= high).then_some((low, high))
    }

    /// The spot of the face range `face` farthest from the ports of the rails already drawn from that face.
    fn roomiest_spot(
        &self,
        spec: usize,
        routes: &[Option<Route>],
        normal: PlanVector,
        face: (f64, f64),
    ) -> Option<f64> {
        let taken = self.ports_on_face(spec, routes, normal);
        let room = |spot: f64| taken.iter().map(|taken| (taken - spot).abs()).fold(f64::INFINITY, f64::min);
        (0..ROOMIEST_SPOT_SAMPLES)
            .map(|sample| face.0 + (face.1 - face.0) * sample as f64 / (ROOMIEST_SPOT_SAMPLES - 1) as f64)
            .max_by(|a, b| room(*a).total_cmp(&room(*b)))
    }

    /// Cross coordinates of the ports of the rails already drawn from the face with outward `normal` of the
    /// node of rail end `spec` (other than its own), with the positions of their ends in `ends`.
    fn ports_on_face(&self, spec: usize, routes: &[Option<Route>], normal: PlanVector) -> Vec<f64> {
        self.ports_on_face_of(spec, routes, normal).into_iter().map(|(_, lateral)| lateral).collect()
    }

    /// [`Self::ports_on_face`] with the position in `ends` of each.
    fn ports_on_face_of(&self, spec: usize, routes: &[Option<Route>], normal: PlanVector) -> Vec<(usize, f64)> {
        let node = self.ends[spec].node;
        (0..self.ends.len())
            .filter(|&other| other != spec && self.ends[other].node == node)
            .filter_map(|other| {
                let sibling = &self.ends[other];
                let corners = routes[sibling.edge].as_ref()?.corners.as_slice();
                let (port, next) = match (sibling.at_start, corners) {
                    (true, [port, next, ..]) => (*port, *next),
                    (false, [.., next, port]) => (*port, *next),
                    _ => return None,
                };
                (port.to(next).normalized().dot(normal) >= SAME_FACE_COSINE)
                    .then(|| (other, lateral_across(port, normal)))
            })
            .collect()
    }

    /// Order key of a rail end in the fan of its face: where the node at the other end lies, seen from
    /// the face (as in [`share_faces`]).
    fn fan_key(&self, end: usize, normal: PlanVector) -> f64 {
        let spec = &self.ends[end];
        let here = self.drawn.geometry[spec.node].center;
        let toward = angle_of(here.to(self.drawn.geometry[spec.other].center));
        let turn = (toward - angle_of(normal)).rem_euclid(std::f64::consts::TAU);
        if turn > std::f64::consts::PI { turn - std::f64::consts::TAU } else { turn }
    }

    /// Where the rail of `end` stops before its port. A head on a face hidden behind its node sits where the
    /// rail comes out of the silhouette, which depends on where it lands along the face: the search starts
    /// from the farthest of those spots, so the head always finds room on the last leg ([`Self::settle_rail_head`]
    /// then moves it in to its own spot).
    fn rail_lead_point(&self, end: &ResolvedEnd, normal: PlanVector, lateral: (f64, f64)) -> PlanPoint {
        let Some(face) = end.face else { return end.path.point };
        let silhouette = &self.drawn.silhouettes[end.node];
        let head = end.port.is_some();
        if head && self.heads_midway {
            // A head drawn midway leaves the port bare: nothing there is pushed out of the silhouette.
            return end.path.point;
        }
        let push =
            |lateral: f64| rail_push(slid_across(face, normal, lateral), normal, head, silhouette, self.projection);
        let own = rail_push(face, normal, head, silhouette, self.projection);
        let farthest = (0..RAIL_TIP_LATERAL_SAMPLES)
            .map(|sample| {
                let t = sample as f64 / (RAIL_TIP_LATERAL_SAMPLES - 1) as f64;
                push(lateral.0 + (lateral.1 - lateral.0) * t)
            })
            .fold(own, f64::max);
        end.path.point + normal * (farthest - own)
    }

    /// Puts the end of a rail where it lands at `corner` (its first or last corner), after the ports slid
    /// along their faces: on its own spot of the face, pushed out of the silhouette on a hidden face, and
    /// `corner` moves there along the same leg (to the head's base for a head). True when a head arrives at
    /// a face hidden behind its node (a plain end there is not charged: it only starts out of the silhouette).
    fn settle_rail_end(&self, end: &mut ResolvedEnd, corner: &mut PlanPoint) -> bool {
        let Some(face) = end.face else { return false };
        let normal = end.path.inward * -1.0;
        let face = slid_across(face, normal, lateral_across(*corner, normal));
        let head = end.port.is_some();
        if head && self.heads_midway {
            // Nothing is painted at the port: the stroke may end on a hidden face, under the body that hides it
            // (T-425), and the face still costs as hidden.
            let settled = ArrivalPort { tip: face + normal * rail_base_push(head), aim: normal * -1.0 };
            *corner = settled.base();
            end.port = Some(settled);
            return is_back_face(normal);
        }
        let push = rail_push(face, normal, head, &self.drawn.silhouettes[end.node], self.projection);
        if head {
            let settled = ArrivalPort { tip: face + normal * push, aim: normal * -1.0 };
            *corner = settled.base();
            end.port = Some(settled);
        } else {
            *corner = face + normal * push;
        }
        head && push > rail_base_push(head) + PUSH_EPSILON
    }

    /// How rails heads are placed: pushed out of the silhouette on a hidden face, unless the theme draws its heads
    /// midway (nothing painted at the port).
    fn head_placement(&self, routing: Routing) -> HeadPlacement {
        HeadPlacement { routing, pushed_out: !self.heads_midway }
    }

    /// Pulls the end of a rail at `corner` (its first or last corner) in under a body drawn above its footprint
    /// (a floating cloud) until the stroke meets the body's outline on screen: on the floor at the face the stroke
    /// stops short of the body and reads as passing under it (T-425). Only on a face in front of the body (a back
    /// face is drawn over by it) and when no head is painted at the port (a head there would be hidden). True when
    /// the end moved.
    fn reach_floating_body(&self, end: &mut ResolvedEnd, corner: &mut PlanPoint) -> bool {
        let Some(face) = end.face else { return false };
        if end.port.is_some() && !self.heads_midway {
            return false;
        }
        let normal = end.path.inward * -1.0;
        let face = slid_across(face, normal, lateral_across(*corner, normal));
        let tip = end.port.map_or(*corner, |port| port.tip);
        let geometry = &self.drawn.geometry[end.node];
        let push = face.to(tip).dot(normal);
        let depth = face.to(geometry.center).dot(normal);
        let Some(pull) = pull_to_body(face, normal, (depth, push), &self.drawn.bodies[end.node], self.projection)
        else {
            return false;
        };
        let tip = face + normal * pull;
        match end.port.as_mut() {
            Some(port) => {
                port.tip = tip;
                *corner = port.base();
            }
            None => *corner = tip,
        }
        true
    }

    /// How badly edge `index` reads as `route` among the other `routes` (0 = clean), without the deviation.
    /// Stops adding once the cost passes `budget` (the caller only needs to know it lost).
    fn misreading(&self, index: usize, route: &Route, routes: &[Option<Route>], budget: f64) -> f64 {
        let Some([start, end]) = ends_of(self.ends, index) else { return 0.0 };
        let (source, target) = (self.ends[start].node, self.ends[end].node);
        let bounds = route.trace.bounds;
        let reach = bounds.inflate(GRAZE_CLEARANCE_PX);
        let covers: Vec<(usize, &Cover<'_>)> =
            self.covers.iter().enumerate().filter(|(_, cover)| touches(&cover.bounds, &reach)).collect();
        let others = |&&(node, _): &&(usize, &Cover<'_>)| node != source && node != target;
        let columns: Vec<(bool, &Rect)> = self
            .label_columns
            .iter()
            .filter(|(_, column)| touches(column, &bounds))
            .map(|(node, column)| (*node == source || *node == target, column))
            .collect();
        let linted_node_cost =
            if self.model.edges[index].route.is_elbow() { RAIL_LINTED_NODE_COST } else { LINTED_NODE_COST };
        // A rail may pass under the label of its own node (T-313: the label still names it), less so under the
        // label of another node, which then has nowhere to go but over it (T-422).
        let (label_column_cost, foreign_label_column_cost) = if self.model.edges[index].route.is_elbow() {
            (RAIL_LABEL_COLUMN_COST, RAIL_FOREIGN_LABEL_COLUMN_COST)
        } else {
            (LABEL_COLUMN_COST, LABEL_COLUMN_COST)
        };
        let (mut through, mut visible, mut in_label_columns, mut in_foreign_label_columns, mut grazing) =
            (0.0, 0.0, 0.0, 0.0, 0.0);
        // Per cover (same order as `covers`): stroke inside it and stroke hugging it, for the lint's thresholds.
        let mut inside_by_node = vec![0.0; covers.len()];
        let mut along_by_node = vec![0.0; covers.len()];
        for (middle, _, length) in route.trace.pieces() {
            let hidden = covers.iter().any(|(_, cover)| cover.hides(middle));
            if !hidden {
                visible += length;
                // Under several columns at once, the foreign one counts.
                match columns.iter().filter(|(_, column)| column.contains(middle)).map(|(own, _)| *own).min() {
                    Some(false) => in_foreign_label_columns += length,
                    Some(true) => in_label_columns += length,
                    None => {}
                }
                let mut grazed = false;
                // No cover hides this piece, so hugging an outline is just being near it.
                for slot in (0..covers.len()).filter(|&slot| others(&&covers[slot]) && covers[slot].1.hugs(middle)) {
                    along_by_node[slot] += length;
                    grazed = true;
                }
                if grazed {
                    grazing += length;
                }
            } else {
                let mut foreign = false;
                for slot in (0..covers.len()).filter(|&slot| others(&&covers[slot]) && covers[slot].1.hides(middle)) {
                    inside_by_node[slot] += length;
                    foreign = true;
                }
                if foreign {
                    through += length;
                }
            }
            // Every term of the cost is non-negative: once what is already known passes the budget (with a
            // margin far above the rounding of a different order of sums), the route has lost (the caller only
            // needs to know that), whatever the rest adds.
            let known = THROUGH_NODE_COST * through
                + label_column_cost * in_label_columns
                + foreign_label_column_cost * in_foreign_label_columns
                + route.rail_cost;
            if known > budget + EARLY_EXIT_MARGIN * (1.0 + budget.abs()) {
                return known;
            }
        }
        let hidden = (MIN_VISIBLE_EDGE_PX - visible).max(0.0);
        // A short edge may need the stretch beside a node to be seen at all: only what it shows beyond the
        // minimum is charged for hugging, so the router never hides an edge to keep it off an outline.
        let charged_grazing = grazing.min((visible - MIN_VISIBLE_EDGE_PX).max(0.0));
        let linted = inside_by_node
            .iter()
            .zip(&along_by_node)
            .filter(|&(&inside, &along)| inside >= THROUGH_NODE_MIN_PX || along >= ALONG_NODE_MIN_PX)
            .count() as f64;
        let mut cost = THROUGH_NODE_COST * through
            + HIDDEN_COST * hidden
            + if hidden > 0.0 { UNREADABLE_EDGE_COST } else { 0.0 }
            + label_column_cost * in_label_columns
            + foreign_label_column_cost * in_foreign_label_columns
            + GRAZING_COST * charged_grazing
            + linted_node_cost * linted
            + route.rail_cost;
        for (other_index, other) in routes.iter().enumerate() {
            let Some(other) = other.as_ref().filter(|_| other_index != index) else { continue };
            if cost > budget {
                break;
            }
            cost += COINCIDENT_COST * coincident_length(&route.trace, &other.trace);
            let crossings = crossing_count(&route.trace, &other.trace) as f64;
            if crossings > 0.0 {
                let unit = if !self.share_node(index, other_index) {
                    CROSSING_COST
                } else if self.model.edges[index].route.is_elbow() {
                    RAIL_SHARED_NODE_CROSSING_COST
                } else {
                    SHARED_NODE_CROSSING_COST
                };
                cost += unit * crossings;
                if crossings > 1.0 && self.model.edges[index].route.is_elbow() {
                    cost += RAIL_PLAIT_COST;
                }
            }
        }
        cost
    }

    /// The start and end nodes of edge `index` (positions in the drawn nodes).
    fn edge_nodes(&self, index: usize) -> Option<[usize; 2]> {
        let [start, end] = ends_of(self.ends, index)?;
        Some([self.ends[start].node, self.ends[end].node])
    }

    /// True when edges `a` and `b` have an end node in common.
    fn share_node(&self, a: usize, b: usize) -> bool {
        let nodes = |edge: usize| self.ends.iter().filter(move |end| end.edge == edge).map(|end| end.node);
        nodes(a).any(|node| nodes(b).any(|other| other == node))
    }

    /// Re-routes the edges that are misread, keeping the cheapest variant (spec order, a few passes).
    fn improve(&self, state: &mut RouteState) {
        for _ in 0..IMPROVE_PASSES {
            let mut changed = false;
            for &index in &self.order {
                changed |= self.improve_edge(index, state);
            }
            if !changed {
                break;
            }
        }
    }

    /// Last resort for fans that still read as an "X": two edges that share a node and cross swap their
    /// ports at that node (same kind of end only, so a head never moves to a hidden spot), when the pair
    /// reads better afterwards (T-400).
    fn untangle_fans(&self, state: &mut RouteState) {
        for _ in 0..UNTANGLE_PASSES {
            let mut changed = false;
            for first in 0..state.routes.len() {
                for second in first + 1..state.routes.len() {
                    changed |= self.untangle_pair(first, second, state);
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn untangle_pair(&self, first: usize, second: usize, state: &mut RouteState) -> bool {
        let (Some(route_a), Some(route_b)) = (state.routes[first].as_ref(), state.routes[second].as_ref()) else {
            return false;
        };
        if crossing_count(&route_a.trace, &route_b.trace) == 0 {
            return false;
        }
        let current = self.pair_cost([first, second], &state.routes);
        for (end_a, end_b) in self.shared_ends(first, second) {
            let mut trial = state.clone();
            trial.base.swap(end_a, end_b);
            trial.angles[end_a] = trial.base[end_a];
            trial.angles[end_b] = trial.base[end_b];
            for index in [first, second] {
                let natural = Variant { rails: self.model.edges[index].route.is_elbow(), ..Variant::NATURAL };
                trial.routes[index] = self.route(index, &trial.base, natural, &trial.routes);
                if let Some([start, end]) = ends_of(self.ends, index) {
                    trial.angles[start] = trial.base[start];
                    trial.angles[end] = trial.base[end];
                }
            }
            for index in [first, second] {
                self.improve_edge(index, &mut trial);
            }
            if self.pair_cost([first, second], &trial.routes) < current - f64::EPSILON {
                *state = trial;
                return true;
            }
        }
        false
    }

    /// Rails that still take a detour or too many bends ([`RailGeometry::reads_as_detour`]) are often hemmed
    /// in by a rail of the same node drawn before them, which they must not cross: the sibling is lifted,
    /// the rail re-routed without it, and the sibling routed again around the new rail. Kept when the pair
    /// reads better (T-407).
    fn reroute_detours(&self, state: &mut RouteState) {
        for _ in 0..REROUTE_PASSES {
            if !self.reroute_detours_once(state) {
                break;
            }
        }
    }

    /// One pass of [`Self::reroute_detours`]; true when a pair changed.
    fn reroute_detours_once(&self, state: &mut RouteState) -> bool {
        let mut changed = false;
        for &index in &self.order {
            let Some(corners) = state.routes[index].as_ref().map(|route| route.corners.as_slice()) else { continue };
            let siblings: Vec<usize> = (0..state.routes.len())
                .filter(|&other| other != index && state.routes[other].is_some() && self.share_node(index, other))
                .filter(|&other| self.model.edges[other].route.is_elbow())
                .collect();
            // A rail glued to a sibling (two ports of a face too narrow for both lanes, where the first one took
            // the middle) gets the same treatment as a detour: the sibling moves over once it is drawn (T-413).
            let glued_to = |other: &usize| {
                state.routes[*other].as_ref().is_some_and(|route| glued_length(corners, &route.corners) > 0.0)
            };
            let detour = self.edge_nodes(index).is_some_and(|[from, to]| {
                RailGeometry::reads_as_detour(corners, [self.obstacles[from], self.obstacles[to]])
            });
            let siblings: Vec<usize> = if detour { siblings } else { siblings.into_iter().filter(glued_to).collect() };
            if siblings.is_empty() {
                continue;
            }
            for other in siblings {
                if self.reroute_around(index, other, state) {
                    changed = true;
                    break;
                }
            }
        }
        changed
    }

    /// Re-routes rail `index` without rail `other`, then `other` around it; true when the pair reads better.
    fn reroute_around(&self, index: usize, other: usize, state: &mut RouteState) -> bool {
        let current = self.pair_cost([index, other], &state.routes);
        let mut trial = state.clone();
        trial.routes[other] = None;
        trial.explored[index] = false;
        self.improve_edge(index, &mut trial);
        let natural = Variant { rails: true, ..Variant::NATURAL };
        trial.routes[other] = self.route(other, &trial.base, natural, &trial.routes);
        if let Some([start, end]) = ends_of(self.ends, other) {
            trial.angles[start] = trial.base[start];
            trial.angles[end] = trial.base[end];
        }
        trial.explored[other] = false;
        self.improve_edge(other, &mut trial);
        if trial.routes[other].is_none() || self.pair_cost([index, other], &trial.routes) >= current - f64::EPSILON {
            return false;
        }
        *state = trial;
        true
    }

    /// Misreading plus deviation of two edges among `routes`.
    fn pair_cost(&self, pair: [usize; 2], routes: &[Option<Route>]) -> f64 {
        pair.into_iter()
            .filter_map(|index| {
                let route = routes[index].as_ref()?;
                Some(self.misreading(index, route, routes, f64::INFINITY) + route.variant.deviation())
            })
            .sum()
    }

    /// Pairs of ends (positions in `ends`) of edges `a` and `b` at a node they share, of the same kind.
    fn shared_ends(&self, a: usize, b: usize) -> Vec<(usize, usize)> {
        let ends_of_edge = |edge: usize| (0..self.ends.len()).filter(move |&end| self.ends[end].edge == edge);
        ends_of_edge(a)
            .flat_map(|end_a| ends_of_edge(b).map(move |end_b| (end_a, end_b)))
            .filter(|&(end_a, end_b)| {
                let (spec_a, spec_b) = (&self.ends[end_a], &self.ends[end_b]);
                spec_a.node == spec_b.node && spec_a.head == spec_b.head && spec_a.node != spec_a.other
            })
            .collect()
    }

    fn improve_edge(&self, index: usize, state: &mut RouteState) -> bool {
        let Some(current) = state.routes[index].as_ref() else { return false };
        let misreading = self.misreading(index, current, &state.routes, f64::INFINITY);
        // A rail compares its faces once (the cheapest depends on the geometry, not only on defects); after
        // that, like any edge, only when it reads badly.
        let rails = self.model.edges[index].route.is_elbow();
        let first_look = rails && !state.explored[index];
        state.explored[index] = true;
        if !first_look && misreading <= MISREADING_WORTH_FIXING {
            return false;
        }
        let current_cost = misreading + current.variant.deviation();
        let Some([start, end]) = ends_of(self.ends, index) else { return false };
        let priced = |variant: Variant, best: &Option<(f64, Route)>| {
            let budget = best.as_ref().map_or(f64::INFINITY, |(cost, _)| cost - variant.deviation());
            let route = self.route(index, &state.base, variant, &state.routes)?;
            Some((self.misreading(index, &route, &state.routes, budget) + variant.deviation(), route))
        };
        let mut curved = self.variants(index, [start, end], state);
        // Rails search their own detours ([`elbow_corners`]): no orthogonal fallback.
        let may_fall_back = !curved.is_empty() && !rails;
        // Cheapest deviation first: once a route costs less than the next variant's deviation alone,
        // nothing after it can win.
        curved.sort_by(|a, b| a.deviation().total_cmp(&b.deviation()));
        let mut best: Option<(f64, Route)> = None;
        for variant in curved {
            if best.as_ref().is_some_and(|(cost, _)| *cost <= variant.deviation()) {
                break;
            }
            let candidate = priced(variant, &best);
            best = cheapest(best.into_iter().chain(candidate));
        }
        // The orthogonal fallback (an A* search) costs at least its deviation: only worth it when every
        // curve is worse than that.
        let fallback_can_win = best.as_ref().is_none_or(|(cost, _)| *cost > ORTHOGONAL_FALLBACK_COST);
        if may_fall_back && fallback_can_win {
            let fallback = priced(Variant { orthogonal: true, ..Variant::NATURAL }, &best);
            best = cheapest(best.into_iter().chain(fallback));
        }
        let Some((cost, route)) = best else { return false };
        if cost >= current_cost - f64::EPSILON {
            return false;
        }
        state.angles[start] = state.base[start] + route.variant.start_turn;
        state.angles[end] = state.base[end] + route.variant.end_turn;
        state.routes[index] = Some(route);
        true
    }

    /// Curved variants of edge `index` worth trying: end turns × arm scales (none for other routings). Head ends only turn where the head stays visible and clear of sibling heads.
    fn variants(&self, index: usize, [start, end]: [usize; 2], state: &RouteState) -> Vec<Variant> {
        match self.model.edges[index].route {
            Routing::Curved => {}
            Routing::Rails | Routing::Orthogonal => return self.rail_variants([start, end], state),
            Routing::Straight => return Vec::new(),
        }
        let start_turns = self.allowed_turns(start, state);
        let end_turns = self.allowed_turns(end, state);
        // One end turns at a time, or both the same way (a swing to one side); every arm scale.
        let mut turns: Vec<(f64, f64)> = start_turns.iter().map(|&turn| (turn, 0.0)).collect();
        turns.extend(end_turns.iter().filter(|turn| **turn != 0.0).map(|&turn| (0.0, turn)));
        turns.extend(
            start_turns.iter().filter(|turn| **turn != 0.0 && end_turns.contains(&-**turn)).map(|&turn| (turn, -turn)),
        );
        let mut variants = Vec::new();
        for (start_turn, end_turn) in turns {
            for arms in ARM_SCALES {
                variants.push(Variant { start_turn, end_turn, arms, ..Variant::NATURAL });
            }
        }
        variants
    }

    /// Rail variants: leaving and arriving by the natural face or an adjacent one.
    fn rail_variants(&self, [start, end]: [usize; 2], state: &RouteState) -> Vec<Variant> {
        let start_turns = self.allowed_turns(start, state);
        let end_turns = self.allowed_turns(end, state);
        start_turns
            .iter()
            .flat_map(|&start_turn| {
                end_turns.iter().map(move |&end_turn| Variant { start_turn, end_turn, rails: true, ..Variant::NATURAL })
            })
            .collect()
    }

    /// End turns that keep a head end visible and apart from the other heads at its node.
    fn allowed_turns(&self, end: usize, state: &RouteState) -> Vec<f64> {
        let spec = &self.ends[end];
        let rail_turns;
        let candidates: &[f64] = match self.model.edges[spec.edge].route {
            Routing::Rails | Routing::Orthogonal => {
                rail_turns = rail_face_turns(&self.drawn.geometry[spec.node].footprint, state.base[end]);
                &rail_turns
            }
            Routing::Curved | Routing::Straight => &END_TURNS,
        };
        if !spec.head {
            return candidates.to_vec();
        }
        let geometry = &self.drawn.geometry[spec.node];
        let arc = ArrivalArc::for_node(&geometry.footprint, geometry.tall);
        // A rails head may take any face: on a hidden one it stands where the rail leaves the silhouette.
        let head_at = |edge: usize, angle: f64| match self.model.edges[edge].route {
            Routing::Rails | Routing::Orthogonal => {
                rail_head_port(spec.node, angle, self.drawn, self.projection, !self.heads_midway).0
            }
            _ => ArrivalPort::at_angle(geometry.center, &geometry.footprint, angle),
        };
        let any_face = self.model.edges[spec.edge].route.is_elbow();
        let siblings: Vec<[PlanPoint; 3]> = (0..self.ends.len())
            .filter(|&other| other != end && self.ends[other].head && self.ends[other].node == spec.node)
            // A sibling lifted for a re-route ([`Self::reroute_around`]) has no head to keep clear of yet.
            .filter(|&other| state.routes[self.ends[other].edge].is_some())
            .map(|other| head_at(self.ends[other].edge, state.angles[other]).triangle())
            .collect();
        candidates
            .iter()
            .copied()
            .filter(|&turn| {
                let angle = state.base[end] + turn;
                if turn == 0.0 {
                    return true;
                }
                let triangle = head_at(spec.edge, angle).triangle();
                // Rails heads sharing a face take lanes of their own ([`Self::rail_port`]): the spot aimed at
                // (the middle of the face) says nothing about where they land (T-421).
                // A head drawn midway is not painted at the port: it need not show there (T-425).
                let shows = (any_face && self.heads_midway)
                    || head_is_visible(&triangle, spec.node, self.drawn, self.projection);
                (any_face || arc.contains(angle))
                    && shows
                    && (any_face || siblings.iter().all(|sibling| !triangles_overlap(&triangle, sibling)))
            })
            .collect()
    }
}

/// Turns tried on a rail end at border angle `base`: none (the natural face, on its lane) and one to the
/// middle of each other face. A quarter turn of the angle does not always change faces (a wide face spans
/// more than a quarter turn), so the other faces are aimed at directly (T-407).
fn rail_face_turns(footprint: &Footprint, base: f64) -> Vec<f64> {
    let natural = face_normal(footprint, direction_at(base));
    let mut turns = vec![0.0];
    for axis in FACE_AXES {
        if face_normal(footprint, direction_at(axis)) != natural {
            turns.push((axis - base + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI);
        }
    }
    turns
}

/// The routings drawn as elbows ([`super::elbow`]): `rails`, and `orthogonal` with tighter corners
/// (T-407: both get the same ports, lanes and nudging).
trait Elbow {
    /// True for `rails` and `orthogonal`.
    fn is_elbow(&self) -> bool;
}

impl Elbow for Routing {
    fn is_elbow(&self) -> bool {
        matches!(self, Routing::Rails | Routing::Orthogonal)
    }
}

/// Radius (tiles) of the rounded bends of an elbow routing.
fn bend_radius(routing: Routing) -> f64 {
    if routing == Routing::Orthogonal { ORTHOGONAL_BEND_RADIUS } else { RAIL_BEND_RADIUS }
}

/// Order in which edges are routed: spec order, except that rails go shortest first (T-407). A rail avoids
/// the rails drawn before it, so the short ones, which a detour would double, take the direct way and the
/// long ones, which can afford it, go round them. Other routings keep spec order, ahead of the rails.
fn routing_order(model: &Model, drawn: &DrawnNodes) -> Vec<usize> {
    let span = |index: usize| {
        let edge = &model.edges[index];
        if !edge.route.is_elbow() {
            return 0.0;
        }
        match (drawn.index_of(&edge.from), drawn.index_of(&edge.to)) {
            (Some(from), Some(to)) => {
                let offset = drawn.geometry[from].center.to(drawn.geometry[to].center);
                offset.x.abs() + offset.y.abs()
            }
            _ => 0.0,
        }
    };
    let mut order: Vec<usize> = (0..model.edges.len()).collect();
    order.sort_by(|&a, &b| span(a).total_cmp(&span(b)).then(a.cmp(&b)));
    order
}

/// The cheapest priced route (first wins ties, so the natural order of variants is the preference).
fn cheapest(priced: impl Iterator<Item = (f64, Route)>) -> Option<(f64, Route)> {
    priced.fold(None, |best: Option<(f64, Route)>, candidate| match best {
        Some(current) if current.0 <= candidate.0 => Some(current),
        _ => Some(candidate),
    })
}

/// Screen boxes above each labelled node where its label and leader will most likely go: the label box
/// centered over the node at the base lift, down to the top of the node.
fn label_columns(model: &Model, drawn: &DrawnNodes, theme: &Theme, lift_px: f64) -> Vec<(usize, Rect)> {
    drawn
        .label_anchors
        .iter()
        .zip(measure_node_labels(model, theme, &drawn.nodes))
        .enumerate()
        .filter_map(|(node, (&anchor, text))| {
            let text = text?;
            let half_width = text.width / 2.0;
            let column =
                Rect::new(anchor.x - half_width, anchor.y - lift_px - text.height, anchor.x + half_width, anchor.y);
            Some((node, column))
        })
        .collect()
}

/// True when no corner of a head triangle is covered by another node's silhouette.
fn head_is_visible(triangle: &[PlanPoint; 3], node: usize, drawn: &DrawnNodes, projection: &Projection) -> bool {
    triangle.iter().all(|point| {
        let screen = projection.project(*point, EDGE_Z);
        drawn
            .silhouettes
            .iter()
            .enumerate()
            .all(|(other, polygon)| other == node || !inside_convex(polygon, screen, SILHOUETTE_INSET_PX))
    })
}

fn footprint_rect(geometry: &NodeGeometry) -> PlanRect {
    let (half_width, half_depth) = geometry.footprint.half_extents();
    PlanRect::around(geometry.center, half_width, half_depth)
}

/// Both ends of every edge whose nodes exist (spec validation guarantees it; others are skipped).
fn edge_ends(model: &Model, drawn: &DrawnNodes) -> Vec<EndSpec> {
    let mut ends = Vec::with_capacity(model.edges.len() * 2);
    for (index, edge) in model.edges.iter().enumerate() {
        let (Some(source), Some(target)) = (drawn.index_of(&edge.from), drawn.index_of(&edge.to)) else { continue };
        let (head_at_start, head_at_end) = match edge.arrow {
            ArrowEnds::End => (false, true),
            ArrowEnds::Start => (true, false),
            ArrowEnds::Both => (true, true),
            ArrowEnds::None => (false, false),
        };
        ends.push(EndSpec { edge: index, at_start: true, node: source, other: target, head: head_at_start });
        ends.push(EndSpec { edge: index, at_start: false, node: target, other: source, head: head_at_end });
    }
    ends
}

/// Positions in `ends` of the start and end of edge `edge`.
fn ends_of(ends: &[EndSpec], edge: usize) -> Option<[usize; 2]> {
    let start = ends.iter().position(|end| end.edge == edge && end.at_start)?;
    let end = ends.iter().position(|end| end.edge == edge && !end.at_start)?;
    Some([start, end])
}

/// Border angle wanted by an end: toward the other node (axis-snapped for orthogonal routes).
fn desired_angle(model: &Model, end: &EndSpec, drawn: &DrawnNodes) -> f64 {
    let here = &drawn.geometry[end.node];
    let arc = ArrivalArc::for_node(&here.footprint, here.tall);
    if end.node == end.other {
        let front = (arc.low + arc.high) / 2.0;
        return if end.at_start { front - SELF_LOOP_HALF_ANGLE } else { front + SELF_LOOP_HALF_ANGLE };
    }
    let toward = angle_of(here.center.to(drawn.geometry[end.other].center));
    // Only a head must stay on a visible face; a plain end leaves by the face toward the other node, even
    // when that face is behind the node (doc 06, rule 4). A rails head takes the natural face too: on a
    // hidden one it stands where the rail comes out of the silhouette (T-407).
    let routing = model.edges[end.edge].route;
    let snap_arc = if end.head && !routing.is_elbow() { arc } else { ArrivalArc::FULL };
    match routing {
        Routing::Orthogonal | Routing::Rails => snap_to_axis(toward, &snap_arc),
        Routing::Curved | Routing::Straight => toward,
    }
}

/// Nearest axis direction allowed by the arc (orthogonal routes leave and arrive square to a face).
fn snap_to_axis(angle: f64, arc: &ArrivalArc) -> f64 {
    let axes = [0.0, FRAC_PI_2, std::f64::consts::PI, -FRAC_PI_2];
    let distance = |axis: f64| {
        let delta = (angle - axis).rem_euclid(std::f64::consts::TAU);
        delta.min(std::f64::consts::TAU - delta)
    };
    axes.into_iter()
        .filter(|axis| arc.contains(*axis))
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
        .unwrap_or_else(|| arc.clamp(angle))
}

/// Final border angle of every head end (index-aligned with `ends`; plain ends keep their desired angle).
fn arrival_angles(model: &Model, ends: &[EndSpec], drawn: &DrawnNodes, projection: &Projection) -> Vec<f64> {
    let mut angles: Vec<f64> = ends.iter().map(|end| desired_angle(model, end, drawn)).collect();
    for node in 0..drawn.geometry.len() {
        let geometry = &drawn.geometry[node];
        let arc = ArrivalArc::for_node(&geometry.footprint, geometry.tall);
        let on_rails = |index: usize| model.edges[ends[index].edge].route.is_elbow();
        let (rail_heads, heads): (Vec<usize>, Vec<usize>) = (0..ends.len())
            .filter(|&index| ends[index].head && ends[index].node == node)
            .partition(|&index| on_rails(index));
        spread_exits(node, ends, &mut angles, &geometry.footprint);
        spread_members(&heads, &mut angles, &geometry.footprint, arc);
        // Rails heads take any face (T-407); they never need unhiding: a hidden face pushes them out.
        spread_members(&rail_heads, &mut angles, &geometry.footprint, ArrivalArc::FULL);
        share_rail_faces(model, node, ends, &mut angles, drawn);
        if !heads.is_empty() {
            unhide_heads(node, &heads, &mut angles, drawn, projection, arc);
        }
    }
    angles
}

/// Spreads the plain ends (no head) leaving one node, so edges fanning out toward the same side start
/// apart instead of as one thick bundle. Self-loops keep their symmetric pair.
fn spread_exits(node: usize, ends: &[EndSpec], angles: &mut [f64], footprint: &crate::icons::Footprint) {
    let members: Vec<usize> = (0..ends.len())
        .filter(|&index| !ends[index].head && ends[index].node == node && ends[index].other != node)
        .collect();
    if members.len() > 1 {
        spread_members(&members, angles, footprint, ArrivalArc::FULL);
    }
}

/// Rails ends of one node that share a face get a lane each, arrivals and departures together
/// ([`share_faces`]); heads and plain ends are spread separately before, and could land on the same spot.
fn share_rail_faces(model: &Model, node: usize, ends: &[EndSpec], angles: &mut [f64], drawn: &DrawnNodes) {
    let members: Vec<usize> = (0..ends.len())
        .filter(|&index| {
            let end = &ends[index];
            end.node == node && end.other != node && model.edges[end.edge].route.is_elbow()
        })
        .collect();
    if members.len() < 2 {
        return;
    }
    let here = &drawn.geometry[node];
    let face_ends: Vec<FaceEnd> = members
        .iter()
        .map(|&index| FaceEnd {
            angle: angles[index],
            toward: angle_of(here.center.to(drawn.geometry[ends[index].other].center)),
        })
        .collect();
    for (&index, angle) in members.iter().zip(share_faces(&here.footprint, &face_ends)) {
        angles[index] = angle;
    }
}

/// Spreads the angles of `members` (positions in `angles`) apart inside `arc` ([`spread_arrivals`]).
fn spread_members(members: &[usize], angles: &mut [f64], footprint: &crate::icons::Footprint, arc: ArrivalArc) {
    if members.is_empty() {
        return;
    }
    let desired: Vec<f64> = members.iter().map(|&index| angles[index]).collect();
    for (&index, angle) in members.iter().zip(spread_arrivals(footprint, arc, &desired)) {
        angles[index] = angle;
    }
}

/// Nudges heads covered by another node's silhouette along the arc, without overlapping sibling heads.
fn unhide_heads(
    node: usize,
    members: &[usize],
    angles: &mut [f64],
    drawn: &DrawnNodes,
    projection: &Projection,
    arc: ArrivalArc,
) {
    let geometry = &drawn.geometry[node];
    let triangle_at = |angle: f64| ArrivalPort::at_angle(geometry.center, &geometry.footprint, angle).triangle();
    let visible = |triangle: &[PlanPoint; 3]| head_is_visible(triangle, node, drawn, projection);
    for (position, &index) in members.iter().enumerate() {
        if visible(&triangle_at(angles[index])) {
            continue;
        }
        let siblings: Vec<[PlanPoint; 3]> = members
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != position)
            .map(|(_, &other)| triangle_at(angles[other]))
            .collect();
        let original = angles[index];
        let replacement = (1..=UNHIDE_MAX_STEPS)
            .flat_map(|step| [original + f64::from(step) * UNHIDE_STEP, original - f64::from(step) * UNHIDE_STEP])
            .filter(|candidate| arc.contains(*candidate))
            .find(|candidate| {
                let triangle = triangle_at(*candidate);
                visible(&triangle) && siblings.iter().all(|sibling| !triangles_overlap(&triangle, sibling))
            });
        if let Some(angle) = replacement {
            angles[index] = arc.clamp(angle);
        }
    }
}

/// The routing of an edge and whether its rails heads are pushed out of the silhouette on a hidden face (they
/// are not when the theme draws heads midway: nothing is painted at the port).
#[derive(Debug, Clone, Copy)]
struct HeadPlacement {
    routing: Routing,
    pushed_out: bool,
}

/// True when the face with outward `normal` looks away from the viewer (−x or −y): the body hides it.
fn is_back_face(normal: PlanVector) -> bool {
    normal.x < -PUSH_EPSILON || normal.y < -PUSH_EPSILON
}

/// An end resolved to a path end, plus its port when it carries a head.
struct ResolvedEnd {
    node: usize,
    path: PathEnd,
    port: Option<ArrivalPort>,
    /// Point of the face under a rails head (its tip is pushed out from there).
    face: Option<PlanPoint>,
}

/// Path end of `end` at border angle `angle`. A plain end leaves along the border direction, or square to
/// its face on rails (T-403), so rails from one face run parallel.
fn path_end(
    end: &EndSpec,
    angle: f64,
    drawn: &DrawnNodes,
    projection: &Projection,
    placement: HeadPlacement,
) -> ResolvedEnd {
    let HeadPlacement { routing, pushed_out } = placement;
    let geometry = &drawn.geometry[end.node];
    if end.head {
        let (port, face) = if routing.is_elbow() {
            let (port, face) = rail_head_port(end.node, angle, drawn, projection, pushed_out);
            (port, Some(face))
        } else {
            (ArrivalPort::at_angle(geometry.center, &geometry.footprint, angle), None)
        };
        return ResolvedEnd {
            node: end.node,
            path: PathEnd { point: port.base(), inward: port.aim },
            port: Some(port),
            face,
        };
    }
    let direction = direction_at(angle);
    let (mut border, _) = border_point(geometry.center, &geometry.footprint, direction);
    let outward = if routing.is_elbow() { face_normal(&geometry.footprint, direction) } else { direction };
    if routing.is_elbow()
        && let Footprint::Circle { radius } = geometry.footprint
    {
        // A rail leaves a round body from the tangent line square to its axis, so its lead clears the body.
        let tangent = PlanVector::new(-outward.y, outward.x);
        border = geometry.center + outward * radius + tangent * (direction.dot(tangent) * radius);
    }
    if !routing.is_elbow() {
        return ResolvedEnd {
            node: end.node,
            path: PathEnd { point: border, inward: outward * -1.0 },
            port: None,
            face: None,
        };
    }
    // A rail leaving a face hidden behind its node starts where it comes out of the silhouette (T-407).
    let push = rail_push(border, outward, false, &drawn.silhouettes[end.node], projection);
    let path = PathEnd { point: border + outward * push, inward: outward * -1.0 };
    ResolvedEnd { node: end.node, path, port: None, face: Some(border) }
}

/// Rails head of node `node` at border angle `angle`, square to its face ([`ArrivalPort::square_to_face`]),
/// pushed out of the node's silhouette when that face is hidden ([`rail_push`]); and the face point
/// under it.
fn rail_head_port(
    node: usize,
    angle: f64,
    drawn: &DrawnNodes,
    projection: &Projection,
    pushed_out: bool,
) -> (ArrivalPort, PlanPoint) {
    let geometry = &drawn.geometry[node];
    let square = ArrivalPort::square_to_face(geometry.center, &geometry.footprint, angle);
    let normal = square.aim * -1.0;
    let face = square.tip - normal * PORT_GAP;
    let push = if pushed_out {
        rail_push(face, normal, true, &drawn.silhouettes[node], projection)
    } else {
        rail_base_push(true)
    };
    (ArrivalPort { tip: face + normal * push, aim: normal * -1.0 }, face)
}

/// Cosine above which the lead of a rail end runs along a face normal (it leaves from that face).
const SAME_FACE_COSINE: f64 = 0.99;

/// Floor distances (tiles) below this count as the same push.
const PUSH_EPSILON: f64 = 1e-6;

/// How far (tiles) out of its face a rail end stands when the rail leaves (or reaches) the face point `face`
/// with outward plan `normal`: a head's tip [`PORT_GAP`] out and a plain end on the face itself, when that
/// spot shows on screen. On a face hidden behind the body the end stands where the rail comes out of the
/// node's screen `silhouette` (T-407): the visible stroke starts or ends there, and a head lies on the
/// floor pointing along the last leg, never hidden behind the body.
fn rail_push(
    face: PlanPoint,
    normal: PlanVector,
    head: bool,
    silhouette: &[ScreenPoint],
    projection: &Projection,
) -> f64 {
    let base = rail_base_push(head);
    let on_screen = |push: f64| projection.project(face + normal * push, EDGE_Z);
    // Only the back faces (−x, −y) are drawn over by the body; a front face shows, even where the lifted
    // stroke touches the bottom edge of its outline.
    if !is_back_face(normal) || silhouette.len() < 3 || outside_distance(silhouette, on_screen(base)) >= 0.0 {
        return base;
    }
    let shows = |push: f64| {
        if !head {
            return outside_distance(silhouette, on_screen(push)) >= RAIL_TIP_CLEARANCE_PX;
        }
        let port = ArrivalPort { tip: face + normal * push, aim: normal * -1.0 };
        let [tip, left, right] = port.triangle().map(|point| projection.project(point, EDGE_Z));
        outside_distance(silhouette, tip) >= RAIL_TIP_CLEARANCE_PX
            && outside_distance(silhouette, left) >= 0.0
            && outside_distance(silhouette, right) >= 0.0
    };
    // Along the floor the stroke leaves the convex silhouette once and stays out: bisect for that spot.
    let (mut hidden, mut shown) = (base, base + RAIL_TIP_MAX_PUSH);
    if !shows(shown) {
        return base;
    }
    for _ in 0..RAIL_TIP_SEARCH_STEPS {
        let middle = (hidden + shown) / 2.0;
        if shows(middle) { shown = middle } else { hidden = middle }
    }
    shown
}

/// A rail end farther than this (px) outside its body's outline on screen stops visibly short of it.
const RAIL_BODY_GAP_PX: f64 = 2.0;
/// Share of the way from a face to the body's center a rail end may be pulled in to meet a floating body.
const RAIL_BODY_REACH: f64 = 0.75;

/// Push (tiles along `normal` from `face`, negative under the body) at which a rail end on a front face meets the
/// outline of a body drawn above its footprint, between `depth` (the body's center) and `push` (where it ends
/// now). `None` when the end already touches the outline or on a back face; [`RAIL_BODY_REACH`] of the way to the
/// center when the outline is farther in.
fn pull_to_body(
    face: PlanPoint,
    normal: PlanVector,
    (depth, push): (f64, f64),
    silhouette: &[ScreenPoint],
    projection: &Projection,
) -> Option<f64> {
    let back_face = is_back_face(normal);
    let gap = |push: f64| outside_distance(silhouette, projection.project(face + normal * push, EDGE_Z));
    if back_face || silhouette.len() < 3 || gap(push) <= RAIL_BODY_GAP_PX {
        return None;
    }
    // Never all the way to the center, where the head would lose its direction.
    let deepest = depth * RAIL_BODY_REACH;
    if gap(deepest) > 0.0 {
        return Some(deepest);
    }
    // From the center out the floor point leaves the convex outline once: bisect for that spot.
    let (mut inside, mut outside) = (deepest, push);
    for _ in 0..RAIL_TIP_SEARCH_STEPS {
        let middle = (inside + outside) / 2.0;
        if gap(middle) > 0.0 { outside = middle } else { inside = middle }
    }
    Some(inside)
}

/// Push of a rail end on a visible face: a head's tip stands [`PORT_GAP`] out, a plain end on the face.
fn rail_base_push(head: bool) -> f64 {
    if head { PORT_GAP } else { 0.0 }
}

/// The coordinate of `point` across the face with outward axis `normal` (y on an x face, x on a y face).
fn lateral_across(point: PlanPoint, normal: PlanVector) -> f64 {
    if normal.x.abs() > normal.y.abs() { point.y } else { point.x }
}

/// `point` moved along the face with outward axis `normal` to the cross coordinate `lateral`.
fn slid_across(point: PlanPoint, normal: PlanVector, lateral: f64) -> PlanPoint {
    if normal.x.abs() > normal.y.abs() { PlanPoint::new(point.x, lateral) } else { PlanPoint::new(lateral, point.y) }
}

/// Screen head of `port`. A rails head points square into its face, which on a port off the middle of the
/// face is not at the center: it aims at the point of the body straight ahead of it instead.
fn project_head(projection: &Projection, port: &ArrivalPort, target: &NodeGeometry, square: bool) -> ArrowHead {
    let [tip, left, right] = port.triangle().map(|point| projection.project(point, EDGE_Z));
    let aimed_at = if square { port.tip + port.aim * port.tip.to(target.center).dot(port.aim) } else { target.center };
    ArrowHead { tip, base: [left, right], target_center: projection.project(aimed_at, EDGE_Z) }
}

/// Stroke color, width and dash of an edge in the theme's edge coloring (`layout.edge_colors`, T-409).
/// `color` written on the edge always wins; `semantic` colors the edges without a style of their own by
/// class; `mono` collapses the theme's built-in styles to one color.
fn resolve_style(model: &Model, edge: &Edge, theme: &Theme, kind: EdgeKind) -> (Color, f64, bool) {
    let by_class = edge.style_source == ValueSource::Default || EdgeKind::ALL.iter().any(|k| k.as_str() == edge.style);
    match theme.edge_colors(model.layout.edge_colors) {
        EdgeColors::Semantic if by_class => class_style(model, edge, theme, kind),
        EdgeColors::Mono => {
            let (color, width, dashed) = named_style(model, edge, theme);
            let written =
                edge.color.is_some() || model.styles.get(edge.style.as_str()).is_some_and(|s| s.color.is_some());
            (if written { color } else { theme.edges.primary }, width, dashed)
        }
        EdgeColors::Semantic | EdgeColors::Style => named_style(model, edge, theme),
    }
}

/// The class look ([`legend::class_look`]: theme palette, `styles.<class>`) under the edge's own fields.
fn class_style(model: &Model, edge: &Edge, theme: &Theme, kind: EdgeKind) -> (Color, f64, bool) {
    let (color, line, width) = class_look(model, theme, kind);
    let color = edge.color.as_ref().map_or(color, |written| theme.resolve(written));
    let dashed = edge.dashed.unwrap_or(line == LinePattern::Dashed);
    (color, edge.width.unwrap_or(width), dashed)
}

/// Stroke color, width and dash: edge fields → named style → built-in style → theme default.
fn named_style(model: &Model, edge: &Edge, theme: &Theme) -> (Color, f64, bool) {
    let named = model.styles.get(edge.style.as_str());
    let color = edge
        .color
        .as_ref()
        .or_else(|| named.and_then(|style| style.color.as_ref()))
        .map(|color| theme.resolve(color))
        .or_else(|| theme.builtin_edge_color(&edge.style))
        .unwrap_or(theme.edges.primary);
    let width = edge.width.or_else(|| named.and_then(|style| style.width)).unwrap_or(theme.edge_width);
    let dashed = edge.dashed.or_else(|| named.and_then(|style| style.dashed)).unwrap_or(false);
    (color, width, dashed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::nodes::silhouette;

    fn tall_box() -> (NodeGeometry, Vec<ScreenPoint>, Projection) {
        let projection = Projection::new(56.0);
        let geometry = NodeGeometry {
            center: PlanPoint::new(2.0, 1.0),
            z: 0.0,
            footprint: Footprint::Rect { half_width: 0.42, half_depth: 0.42 },
            height: 1.2,
            tall: true,
            replica_offset: PlanVector::default(),
        };
        let hull = silhouette(&projection, &geometry);
        (geometry, hull, projection)
    }

    #[test]
    fn a_rail_end_is_pulled_in_under_a_floating_body_until_it_meets_its_outline() {
        let projection = Projection::new(56.0);
        let center = PlanPoint::new(2.0, 1.0);
        // An outline floating a tile above the floor point of the center (a cloud).
        let lifted = projection.project(center, 1.0);
        let outline: Vec<ScreenPoint> = (0..16)
            .map(|index| {
                let angle = std::f64::consts::TAU * f64::from(index) / 16.0;
                ScreenPoint::new(lifted.x + 40.0 * angle.cos(), lifted.y + 40.0 * angle.sin())
            })
            .collect();
        let normal = PlanVector::new(1.0, 0.0);
        let face = center + normal * 0.5;
        let pulled = pull_to_body(face, normal, (-0.5, 0.08), &outline, &projection).unwrap_or_else(|| panic!("pull"));
        assert!((-0.5 * RAIL_BODY_REACH - 1e-9..0.08).contains(&pulled), "{pulled}");
        assert!(pull_to_body(face, normal * -1.0, (0.5, -0.08), &outline, &projection).is_none(), "a back face");
        let (geometry, hull, projection) = tall_box();
        let face = geometry.center + normal * 0.42;
        assert!(pull_to_body(face, normal, (-0.42, PORT_GAP), &hull, &projection).is_none(), "a body on the floor");
    }

    #[test]
    fn a_rails_head_on_a_visible_face_stays_at_the_port_gap() {
        let (geometry, hull, projection) = tall_box();
        let normal = PlanVector::new(1.0, 0.0);
        let face = geometry.center + normal * 0.42;
        assert!((rail_push(face, normal, true, &hull, &projection) - PORT_GAP).abs() < 1e-9);
        assert_eq!(rail_push(face, normal, false, &hull, &projection), 0.0, "a plain end leaves from the face");
    }

    #[test]
    fn a_rails_head_on_a_hidden_face_stands_where_the_rail_leaves_the_silhouette() {
        let (geometry, hull, projection) = tall_box();
        let normal = PlanVector::new(-1.0, 0.0);
        let face = geometry.center + normal * 0.42;
        let push = rail_push(face, normal, true, &hull, &projection);
        let port = ArrivalPort { tip: face + normal * push, aim: normal * -1.0 };
        assert!(push > PORT_GAP + 0.1, "pushed out of the body: {push}");
        assert_eq!(port.aim, normal * -1.0, "points along the last leg, square into the face");
        let on_screen = port.triangle().map(|point| projection.project(point, EDGE_Z));
        let tip_gap = outside_distance(&hull, on_screen[0]);
        assert!((RAIL_TIP_CLEARANCE_PX - 1e-3..RAIL_TIP_CLEARANCE_PX + 0.5).contains(&tip_gap), "tip {tip_gap} px out");
        assert!(on_screen[1..].iter().all(|&corner| outside_distance(&hull, corner) >= 0.0));
        let exit = rail_push(face, normal, false, &hull, &projection);
        assert!(exit > 0.1, "a plain end on a hidden face starts out of the silhouette: {exit}");
        let start = projection.project(face + normal * exit, EDGE_Z);
        assert!(outside_distance(&hull, start) >= RAIL_TIP_CLEARANCE_PX - 1e-3);
        // A little closer to the face and the tip would be behind the body.
        let closer = projection.project(face + normal * (push - 0.05), EDGE_Z);
        assert!(outside_distance(&hull, closer) < RAIL_TIP_CLEARANCE_PX);
    }

    #[test]
    fn a_rail_keeps_out_from_under_the_label_of_a_node_it_passes() {
        // `b` stands just beside the straight way from `a` to `c`: its label goes above it on screen, right
        // where that rail would run (T-422: the stroke vanished under "Billing Worker").
        const SPEC: &str = "diorama: 1\ntheme: schematic\nnodes:\n  - { id: a, at: [0, 0] }\n  \
                            - { id: b, at: [2, 2], label: A rather long label }\n  - { id: c, at: [5, 0] }\n\
                            edges:\n  - a -> c\n";
        let diagram = crate::compile_source(SPEC, dioramic_spec::SourceFormat::Yaml, &crate::CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let scene = diagram.scene;
        let label = scene.nodes.iter().find(|node| node.id.as_str() == "b").and_then(|node| node.label.clone());
        let Some(label) = label else { panic!("b has a label") };
        let edge = &scene.edges[0];
        let under = edge.path.sample(64, 1.0).into_iter().filter(|point| label.rect.contains(*point)).count();
        assert_eq!(under, 0, "{:?} runs under {:?}", edge.path, label.rect);
    }

    #[test]
    fn faces_are_crossed_by_the_lateral_coordinate() {
        let point = PlanPoint::new(1.0, 2.0);
        assert_eq!(lateral_across(point, PlanVector::new(1.0, 0.0)), 2.0);
        assert_eq!(lateral_across(point, PlanVector::new(0.0, -1.0)), 1.0);
        assert_eq!(slid_across(point, PlanVector::new(-1.0, 0.0), 5.0), PlanPoint::new(1.0, 5.0));
        assert_eq!(slid_across(point, PlanVector::new(0.0, 1.0), 5.0), PlanPoint::new(5.0, 2.0));
    }
}
