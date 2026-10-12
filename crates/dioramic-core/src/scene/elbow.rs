//! Elbow search of `routing: rails` (T-406): the fewest 90° bends between two ports.
//!
//! The approach of Excalidraw's elbow arrows, on the floor plan: a non-uniform grid whose lines only sit
//! where a bend may happen (the padded footprints of the nodes, the lead points of the two ports, the
//! channels half way between nodes, and the lanes beside the rails already drawn), and an A* over
//! `(grid point, heading)` whose cost is the Manhattan length plus a heavy price per bend. A path never
//! turns back on itself (no spikes, no hairpins), leaves its port square to the face and arrives square
//! to the target face; crossing another rail costs, a lot when the two share a node, and running on top
//! of one costs per tile, so parallel rails take their own lane.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::geometry::{PlanPoint, PlanRect, PlanVector};

use super::RAIL_LANE;
use super::rails::{RAIL_CLEARANCE, RAIL_LEAD};

/// Grid costs are integers (thousandths of a tile) so the search is exact and deterministic.
const MILLI: f64 = 1000.0;
/// Price of one bend, in thousandths of a tile: one bend saved is worth this much extra length.
const BEND_PENALTY: u64 = 5_000;
/// Price of crossing another rail that shares no node with this one.
const CROSSING_PENALTY: u64 = 4_000;
/// Price of crossing a rail of the same node (a fan that reads as an "X").
const SHARED_CROSSING_PENALTY: u64 = 60_000;
/// Price per tile of a leg drawn behind a node (hidden by its body on screen).
const HIDDEN_PENALTY_PER_TILE: f64 = 12.0;
/// Margin (tiles) around a node's body (on the floor and in height) inside which a rail seems to run along
/// its outline on screen (about the lint's 10 px).
const GRAZE_MARGIN: f64 = 0.2;
/// Price per tile of a leg hugging the outline of a node it does not connect (`W_EDGE_THROUGH_NODE`).
const GRAZE_PENALTY_PER_TILE: f64 = 8.0;
/// Price per tile of running on top of another rail.
const OVERLAP_PENALTY_PER_TILE: f64 = 30.0;
/// Distance (tiles) between a rail and the lane added beside it for the next one: one lane, so parallel
/// rails of different edges never run glued together (T-407).
const LANE_SPACING: f64 = RAIL_LANE;
/// Grid lines this close (tiles) are merged.
const SAME_LINE: f64 = 1e-6;
/// How far (tiles) the grid reaches beyond the two lead points.
const SEARCH_MARGIN: f64 = 3.0;
/// Two parallel legs closer than this (tiles) run glued together (closer than a lane apart).
const OVERLAP_DISTANCE: f64 = RAIL_LANE - 0.01;
/// Samples per tile when measuring how much of a leg is hidden.
const HIDDEN_SAMPLES_PER_TILE: f64 = 4.0;
/// Price per tile of sliding a port along its face away from its lane (the lanes keep the fans in order;
/// a short slide that saves a bend or a jog is cheap).
const SLIDE_PENALTY_PER_TILE: f64 = 0.6;
/// Shortest lead (tiles) a rail may take out of or into a port when the full [`RAIL_LEAD`] would put its bend
/// somewhere worse (behind a node, across a rail) and force a step instead: a shorter stub reads better than an
/// "S" just before the port (T-421).
const SHORT_LEAD: f64 = 0.2;
/// Price of a shortened lead, in thousandths of a tile: less than the two bends of a step, more than a little
/// extra length.
const SHORT_LEAD_PENALTY: u64 = 1_500;
/// Upper bound on expanded states (protects against pathological specs).
const MAX_EXPANSIONS: usize = 120_000;

/// Headings on the plan: +x, +y, −x, −y.
const HEADINGS: [PlanVector; 4] =
    [PlanVector::new(1.0, 0.0), PlanVector::new(0.0, 1.0), PlanVector::new(-1.0, 0.0), PlanVector::new(0.0, -1.0)];

/// A rail already drawn, which the search should neither cross nor run on.
#[derive(Debug, Clone, Copy)]
pub struct DrawnRail<'a> {
    /// Its plan corners.
    pub corners: &'a [PlanPoint],
    /// True when it shares an end node with the rail being searched.
    pub shares_node: bool,
}

/// What the search avoids: node footprints (with [`RAIL_CLEARANCE`]), the floor each node hides on
/// screen, and the rails already drawn.
#[derive(Debug, Clone, Copy)]
pub struct ElbowObstacles<'a> {
    /// Footprints of every node.
    pub nodes: &'a [PlanRect],
    /// Height (tiles) of every node, same order: a body of height `h` hides the floor behind it.
    pub heights: &'a [f64],
    /// Rails already drawn.
    pub rails: &'a [DrawnRail<'a>],
    /// Positions in `nodes` of the two nodes the rail connects (it may run beside those).
    pub own: [usize; 2],
}

/// True when the floor point `point` is drawn over by the body standing on `footprint` with `height`.
/// A body point at height `z` covers the floor point `(x − z, y − z)` on screen (the iso projection), so
/// the hidden floor is the footprint swept toward `(−1, −1)` by the height.
fn hidden_by(point: PlanPoint, footprint: &PlanRect, height: f64) -> bool {
    let low = (footprint.x0 - point.x).max(footprint.y0 - point.y).max(0.0);
    let high = (footprint.x1 - point.x).min(footprint.y1 - point.y).min(height);
    low < high
}

/// One end of a rail: where it leaves (or arrives at) its node and how far it may slide along the face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailPort {
    /// Preferred point (the bare exit point, or the base of the head).
    pub point: PlanPoint,
    /// Unit axis direction the rail travels at this end, pointing into the node.
    pub inward: PlanVector,
    /// Range of the coordinate across `inward` (y for an x face, x for a y face) the port may slide to.
    pub lateral: (f64, f64),
}

impl RailPort {
    fn lateral_of(&self, point: PlanPoint) -> f64 {
        if self.inward.x.abs() > self.inward.y.abs() { point.y } else { point.x }
    }

    /// `point` moved across the face so its lateral coordinate is `lateral`.
    fn slid(&self, point: PlanPoint, lateral: f64) -> PlanPoint {
        if self.inward.x.abs() > self.inward.y.abs() {
            PlanPoint::new(point.x, lateral)
        } else {
            PlanPoint::new(lateral, point.y)
        }
    }

    fn allows(&self, lateral: f64) -> bool {
        lateral >= self.lateral.0 - SAME_LINE && lateral <= self.lateral.1 + SAME_LINE
    }
}

/// Plan corners of the elbow rail from `start` (leaving against `start.inward`) to `end` (arriving along
/// `end.inward`), both included: a straight lead of [`RAIL_LEAD`] at each port and axis legs in between.
/// Each port may slide along its face within its range (the first and last corners say where it went).
/// `None` when the grid holds no path (the caller falls back to a plain elbow).
pub fn elbow_corners(start: &RailPort, end: &RailPort, obstacles: &ElbowObstacles<'_>) -> Option<Vec<PlanPoint>> {
    if let Some(straight) = facing_straight(start, end, obstacles) {
        return Some(straight);
    }
    let out = start.inward * -1.0;
    let lead_out = start.point + out * RAIL_LEAD;
    let lead_in = end.point - end.inward * RAIL_LEAD;
    let short_out = start.point + out * SHORT_LEAD;
    let short_in = end.point - end.inward * SHORT_LEAD;
    let mut grid = Grid::new(lead_out, lead_in, obstacles);
    grid.add_lead_lines(short_out, short_in);
    grid.add_port_lines(start);
    grid.add_port_lines(end);
    let search = Search {
        start: *start,
        end: *end,
        lead_out,
        lead_in,
        short_out,
        short_in,
        out: heading_of(out)?,
        arrive: heading_of(end.inward)?,
    };
    let points = grid.search(&search);
    let points = points?;
    let (first, last) = (*points.first()?, *points.last()?);
    let mut corners = vec![start.slid(start.point, start.lateral_of(first))];
    corners.extend(points);
    corners.push(end.slid(end.point, end.lateral_of(last)));
    Some(simplified(&without_micro_jogs(simplified(&corners))))
}

/// Steps across shorter than this (tiles) are dropped: the ports' slots may miss each other by a hair, and a
/// step of a few thousandths of a tile draws as a kink (two bends) on a straight rail (T-421).
const MICRO_JOG: f64 = 0.03;

/// `corners` with every step across shorter than [`MICRO_JOG`] between two parallel legs removed: the rest of the
/// rail (down to the end port, which slides across its face by as much) moves over onto the first leg's line.
fn without_micro_jogs(mut corners: Vec<PlanPoint>) -> Vec<PlanPoint> {
    let mut index = 1;
    while index + 2 < corners.len() {
        let (before, step, after) = (
            corners[index - 1].to(corners[index]),
            corners[index].to(corners[index + 1]),
            corners[index + 1].to(corners[index + 2]),
        );
        let parallel = (before.x * after.y - before.y * after.x).abs() <= SAME_LINE * before.length().max(1.0)
            && before.dot(after) > 0.0;
        if parallel && step.length() < MICRO_JOG {
            for point in &mut corners[index + 1..] {
                *point = *point - step;
            }
            corners.remove(index + 1);
        } else {
            index += 1;
        }
    }
    corners
}

/// Two ports that face each other along one axis with a lateral coordinate both can slide to: a single
/// straight rail (no bend at all), even when the nodes are closer than two leads. It sits in the middle of
/// the shared range, or on the preferred lane of the start when that lies inside it. `None` when they do
/// not face each other or a padded node stands in between.
fn facing_straight(start: &RailPort, end: &RailPort, obstacles: &ElbowObstacles<'_>) -> Option<Vec<PlanPoint>> {
    let out = start.inward * -1.0;
    if out.dot(end.inward) < 0.99 {
        return None;
    }
    let low = start.lateral.0.max(end.lateral.0);
    let high = start.lateral.1.min(end.lateral.1);
    if low > high + SAME_LINE {
        return None;
    }
    let preferred = start.lateral_of(start.point);
    let lateral = if preferred >= low && preferred <= high { preferred } else { (low + high) / 2.0 };
    let (from, to) = (start.slid(start.point, lateral), end.slid(end.point, lateral));
    if from.to(to).dot(out) <= RAIL_LEAD {
        return None;
    }
    let length = from.distance(to);
    let steps = (length / STRAIGHT_SAMPLE_STEP).ceil().max(1.0) as usize;
    let padded: Vec<PlanRect> = obstacles.nodes.iter().map(|rect| rect.inflate(RAIL_CLEARANCE)).collect();
    let blocked = (0..=steps).any(|step| {
        let along = length * step as f64 / steps as f64;
        if along < RAIL_CLEARANCE + STRAIGHT_SAMPLE_STEP || along > length - RAIL_CLEARANCE - STRAIGHT_SAMPLE_STEP {
            return false;
        }
        let point = from + out * along;
        padded.iter().any(|rect| point.x > rect.x0 && point.x < rect.x1 && point.y > rect.y0 && point.y < rect.y1)
    });
    (!blocked).then(|| vec![from, to])
}

/// Step (tiles) of the samples testing a straight rail against the nodes.
const STRAIGHT_SAMPLE_STEP: f64 = 0.1;

/// The elbow without avoidance (only the ports' rules: square leads, no turning back), for when the
/// search among the obstacles finds nothing.
pub fn plain_elbow(start: &RailPort, end: &RailPort) -> Vec<PlanPoint> {
    let free = ElbowObstacles { nodes: &[], heights: &[], rails: &[], own: [usize::MAX; 2] };
    let fixed = |port: &RailPort| {
        let lateral = port.lateral_of(port.point);
        RailPort { lateral: (lateral, lateral), ..*port }
    };
    elbow_corners(&fixed(start), &fixed(end), &free).unwrap_or_else(|| vec![start.point, end.point])
}

/// The two ends of one search.
struct Search {
    start: RailPort,
    end: RailPort,
    lead_out: PlanPoint,
    lead_in: PlanPoint,
    /// Where a shortened lead ([`SHORT_LEAD`]) out of the start and into the end turns.
    short_out: PlanPoint,
    short_in: PlanPoint,
    out: usize,
    arrive: usize,
}

/// Index of the heading of a unit axis vector.
fn heading_of(direction: PlanVector) -> Option<usize> {
    HEADINGS.iter().position(|heading| heading.dot(direction) > 0.99)
}

/// Drops repeated points and the middle of straight runs.
fn simplified(points: &[PlanPoint]) -> Vec<PlanPoint> {
    let mut result: Vec<PlanPoint> = Vec::with_capacity(points.len());
    for &point in points {
        if result.last().is_some_and(|last| last.distance(point) <= SAME_LINE) {
            continue;
        }
        while let [.., before, corner] = result[..] {
            let (incoming, outgoing) = (before.to(corner), corner.to(point));
            let cross = incoming.x * outgoing.y - incoming.y * outgoing.x;
            if cross.abs() > SAME_LINE * incoming.length().max(1.0) || incoming.dot(outgoing) < 0.0 {
                break;
            }
            result.pop();
        }
        result.push(point);
    }
    result
}

/// The non-uniform grid of one search.
struct Grid<'a> {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Padded footprints the legs must stay out of (those reaching the search window).
    blocked: Vec<PlanRect>,
    /// Footprints and heights of the nodes whose hidden floor reaches the window.
    hiders: Vec<(PlanRect, f64)>,
    /// Footprints and heights of the nodes the rail does not connect, grown by the graze margin: running
    /// there reads as passing under the node (T-407).
    grazers: Vec<(PlanRect, f64)>,
    /// Segments of the drawn rails inside the window, with whether their rail shares a node.
    segments: Vec<(PlanPoint, PlanPoint, bool)>,
    _obstacles: std::marker::PhantomData<&'a ()>,
}

impl Grid<'_> {
    fn new(lead_out: PlanPoint, lead_in: PlanPoint, obstacles: &ElbowObstacles<'_>) -> Self {
        let window = PlanRect::new(lead_out.x, lead_out.y, lead_in.x, lead_in.y).inflate(SEARCH_MARGIN);
        let meets_window = |rect: &PlanRect| {
            rect.x0 <= window.x1 && rect.x1 >= window.x0 && rect.y0 <= window.y1 && rect.y1 >= window.y0
        };
        // A lead point already inside a padded footprint (nodes closer than two clearances) must still get
        // out: that footprint's padding is not enforced for this search, but its body still is (a rail never
        // runs through a node, not even its own, T-407).
        let holds = |rect: &PlanRect, point: PlanPoint| {
            point.x > rect.x0 && point.x < rect.x1 && point.y > rect.y0 && point.y < rect.y1
        };
        let blocked: Vec<PlanRect> = obstacles
            .nodes
            .iter()
            .map(|rect| {
                let padded = rect.inflate(RAIL_CLEARANCE);
                if holds(&padded, lead_out) || holds(&padded, lead_in) { *rect } else { padded }
            })
            .filter(|rect| meets_window(rect) && !holds(rect, lead_out) && !holds(rect, lead_in))
            .collect();
        let hiders: Vec<(PlanRect, f64)> = obstacles
            .nodes
            .iter()
            .zip(obstacles.heights)
            .filter(|(rect, height)| {
                let hidden = PlanRect::new(rect.x0 - **height, rect.y0 - **height, rect.x1, rect.y1);
                meets_window(&hidden)
            })
            .map(|(rect, height)| (*rect, *height))
            .collect();
        let grazers: Vec<(PlanRect, f64)> = obstacles
            .nodes
            .iter()
            .zip(obstacles.heights)
            .enumerate()
            .filter(|(node, _)| !obstacles.own.contains(node))
            .map(|(_, (rect, height))| (rect.inflate(GRAZE_MARGIN), height + GRAZE_MARGIN))
            .filter(|(rect, height)| meets_window(&PlanRect::new(rect.x0 - height, rect.y0 - height, rect.x1, rect.y1)))
            .collect();
        let segments: Vec<(PlanPoint, PlanPoint, bool)> = obstacles
            .rails
            .iter()
            .flat_map(|rail| rail.corners.windows(2).map(move |pair| (pair[0], pair[1], rail.shares_node)))
            .filter(|(a, b, _)| meets_window(&PlanRect::new(a.x, a.y, b.x, b.y).inflate(OVERLAP_DISTANCE)))
            .collect();
        let mut xs = vec![lead_out.x, lead_in.x, window.x0, window.x1];
        let mut ys = vec![lead_out.y, lead_in.y, window.y0, window.y1];
        for rect in &blocked {
            xs.extend([rect.x0, rect.x1]);
            ys.extend([rect.y0, rect.y1]);
        }
        // Lanes beside the legs already drawn, so a parallel rail has a line of its own, and lines just past
        // their ends, so a rail can go round the end of another instead of crossing it.
        for (a, b, _) in &segments {
            if (a.x - b.x).abs() <= SAME_LINE {
                xs.extend([a.x - LANE_SPACING, a.x + LANE_SPACING]);
                ys.extend([a.y.min(b.y) - LANE_SPACING, a.y.max(b.y) + LANE_SPACING]);
            } else if (a.y - b.y).abs() <= SAME_LINE {
                ys.extend([a.y - LANE_SPACING, a.y + LANE_SPACING]);
                xs.extend([a.x.min(b.x) - LANE_SPACING, a.x.max(b.x) + LANE_SPACING]);
            }
        }
        let inside = |value: f64, low: f64, high: f64| value >= low - SAME_LINE && value <= high + SAME_LINE;
        xs.retain(|&x| inside(x, window.x0, window.x1));
        ys.retain(|&y| inside(y, window.y0, window.y1));
        Self {
            xs: with_channels(xs),
            ys: with_channels(ys),
            blocked,
            hiders,
            grazers,
            segments,
            _obstacles: std::marker::PhantomData,
        }
    }

    /// Lines through the turning points of the shortened leads, so the search can bend there.
    fn add_lead_lines(&mut self, short_out: PlanPoint, short_in: PlanPoint) {
        for (lines, values) in [(&mut self.xs, [short_out.x, short_in.x]), (&mut self.ys, [short_out.y, short_in.y])] {
            lines.extend(values);
            lines.sort_by(f64::total_cmp);
            lines.dedup_by(|a, b| (*a - *b).abs() <= SAME_LINE);
        }
    }

    /// Lines where `port` may land across its face when no grid line falls inside its slide range: both ends
    /// of the range and its preferred spot clamped into it. Without them a range narrower than the gap between
    /// two grid lines (a crowded face, a single slot) offers the search no lead cell at all, and the rail fell
    /// back to the elbow that ignores every node (T-413: a scrape rail drawn straight through the pod in its
    /// way).
    fn add_port_lines(&mut self, port: &RailPort) {
        let target = if port.inward.x.abs() > port.inward.y.abs() { &mut self.ys } else { &mut self.xs };
        if target.iter().any(|line| port.allows(*line)) {
            return;
        }
        let preferred = port.lateral_of(port.point).clamp(port.lateral.0, port.lateral.1);
        target.extend([port.lateral.0, port.lateral.1, preferred]);
        target.sort_by(f64::total_cmp);
        target.dedup_by(|a, b| (*a - *b).abs() <= SAME_LINE);
    }

    fn point(&self, (column, row): (usize, usize)) -> PlanPoint {
        PlanPoint::new(self.xs[column], self.ys[row])
    }

    fn locate(&self, point: PlanPoint) -> Option<(usize, usize)> {
        let column = self.xs.iter().position(|x| (x - point.x).abs() <= SAME_LINE)?;
        let row = self.ys.iter().position(|y| (y - point.y).abs() <= SAME_LINE)?;
        Some((column, row))
    }

    /// The grid point one step from `cell` along `heading`, if any.
    fn step(&self, (column, row): (usize, usize), heading: usize) -> Option<(usize, usize)> {
        match heading {
            0 => (column + 1 < self.xs.len()).then_some((column + 1, row)),
            1 => (row + 1 < self.ys.len()).then_some((column, row + 1)),
            2 => column.checked_sub(1).map(|column| (column, row)),
            _ => row.checked_sub(1).map(|row| (column, row)),
        }
    }

    /// True when the leg between two neighbouring grid points runs inside a padded footprint. Footprint
    /// borders are grid lines, so a leg is either wholly inside or outside: its middle tells.
    fn leg_blocked(&self, from: PlanPoint, to: PlanPoint) -> bool {
        let middle = PlanPoint::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        self.blocked.iter().any(|rect| {
            middle.x > rect.x0 + SAME_LINE
                && middle.x < rect.x1 - SAME_LINE
                && middle.y > rect.y0 + SAME_LINE
                && middle.y < rect.y1 - SAME_LINE
        })
    }

    /// Price of the stretch of the leg `a`–`b` that some node's body hides on screen.
    fn hidden_cost(&self, a: PlanPoint, b: PlanPoint) -> u64 {
        let length = a.distance(b);
        let samples = (length * HIDDEN_SAMPLES_PER_TILE).ceil().max(1.0);
        let (mut hidden, mut grazing) = (0usize, 0usize);
        for index in 0..samples as usize {
            let t = (index as f64 + 0.5) / samples;
            let point = PlanPoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            if self.hiders.iter().any(|(footprint, height)| hidden_by(point, footprint, *height)) {
                hidden += 1;
            } else if self.grazers.iter().any(|(footprint, height)| hidden_by(point, footprint, *height)) {
                grazing += 1;
            }
        }
        milli(length / samples * (hidden as f64 * HIDDEN_PENALTY_PER_TILE + grazing as f64 * GRAZE_PENALTY_PER_TILE))
    }

    /// Multi-source, multi-target A* over `(point, heading)`: it starts on any grid point of the lead line
    /// of `start` its port may slide to and ends on any of `end`'s. Returns the bend points from the first
    /// lead point to the last, both included.
    fn search(&self, search: &Search) -> Option<Vec<PlanPoint>> {
        let columns = self.xs.len();
        // Heading 4 is the arrived state of a cell on the end's lead line.
        let index = |(column, row): (usize, usize), heading: usize| (row * columns + column) * STATE_HEADINGS + heading;
        let states = columns * self.ys.len() * STATE_HEADINGS;
        let mut best = vec![u64::MAX; states];
        let mut parent = vec![usize::MAX; states];
        // Manhattan distance to the nearest point of the end's lead line, plus one bend when that point is
        // not straight ahead (a lower bound: admissible, so the search stays exact).
        let heuristic = |cell: (usize, usize), heading: usize| {
            let point = self.point(cell);
            let lateral = search.end.lateral_of(point).clamp(search.end.lateral.0, search.end.lateral.1);
            let target = search.end.slid(search.lead_in, lateral);
            let offset = point.to(target);
            let ahead = HEADINGS[heading];
            let straight =
                (offset.x * ahead.y - offset.y * ahead.x).abs() <= SAME_LINE && offset.dot(ahead) >= -SAME_LINE;
            milli(offset.x.abs() + offset.y.abs()) + if straight { 0 } else { BEND_PENALTY }
        };
        let mut open = BinaryHeap::new();
        let mut counter: u64 = 0;
        for (cell, lead) in self.lead_cells_both(&search.start, search.lead_out, search.short_out) {
            let slide = (search.start.lateral_of(self.point(cell)) - search.start.lateral_of(search.start.point)).abs();
            let cost = milli(slide * SLIDE_PENALTY_PER_TILE) + lead;
            let state = index(cell, search.out);
            if cost < best[state] {
                best[state] = cost;
                counter += 1;
                open.push(Reverse((cost + heuristic(cell, search.out), counter, cell, search.out)));
            }
        }
        let goals = self.lead_cells_both(&search.end, search.lead_in, search.short_in);
        let mut legs = vec![UNPRICED; columns * self.ys.len() * 2];
        let mut expansions = 0usize;
        while let Some(Reverse((_, _, cell, heading))) = open.pop() {
            let here = index(cell, heading);
            if heading == ARRIVED {
                return Some(self.trace(&parent, here, columns));
            }
            expansions += 1;
            if expansions > MAX_EXPANSIONS {
                return None;
            }
            let cost = best[here];
            let goal = goals.iter().find(|(goal, _)| *goal == cell).map(|(_, lead)| *lead);
            if let Some(lead) = goal.filter(|_| heading != (search.arrive + 2) % 4) {
                // Arriving here: square to the face, or with one more bend at the lead point.
                let slide = (search.end.lateral_of(self.point(cell)) - search.end.lateral_of(search.end.point)).abs();
                let mut arrival = milli(slide * SLIDE_PENALTY_PER_TILE) + lead;
                if heading != search.arrive {
                    arrival += BEND_PENALTY;
                }
                let there = index(cell, ARRIVED);
                let next_cost = cost.saturating_add(arrival);
                if next_cost < best[there] {
                    best[there] = next_cost;
                    parent[there] = here;
                    counter += 1;
                    open.push(Reverse((next_cost, counter, cell, ARRIVED)));
                }
            }
            for next_heading in 0..4 {
                // Never back the way it came: no spikes, no hairpins on the spot.
                if next_heading == (heading + 2) % 4 {
                    continue;
                }
                let Some(next) = self.step(cell, next_heading) else { continue };
                let Some(leg) = self.leg_cost(&mut legs, cell, next) else { continue };
                let mut step_cost = leg;
                if next_heading != heading {
                    step_cost += BEND_PENALTY;
                }
                let there = index(next, next_heading);
                let next_cost = cost.saturating_add(step_cost);
                if next_cost >= best[there] {
                    continue;
                }
                best[there] = next_cost;
                parent[there] = here;
                counter += 1;
                open.push(Reverse((next_cost + heuristic(next, next_heading), counter, next, next_heading)));
            }
        }
        None
    }

    /// Price of the leg between two neighbouring grid points (`None` when it is blocked), worked out once
    /// per search: a leg is met from both ends and under every heading.
    fn leg_cost(&self, legs: &mut [u64], a: (usize, usize), b: (usize, usize)) -> Option<u64> {
        let (low, high) = if (a.1, a.0) <= (b.1, b.0) { (a, b) } else { (b, a) };
        let vertical = usize::from(low.0 == high.0);
        let slot = (low.1 * self.xs.len() + low.0) * 2 + vertical;
        if legs[slot] == UNPRICED {
            let (from, to) = (self.point(low), self.point(high));
            legs[slot] = if self.leg_blocked(from, to) {
                BLOCKED
            } else {
                milli(from.distance(to)) + rail_cost(from, to, &self.segments) + self.hidden_cost(from, to)
            };
        }
        (legs[slot] != BLOCKED).then_some(legs[slot])
    }

    /// Grid points where the lead of `port` may end and the rail turn: on the full lead line through `lead`
    /// for free, or on any grid line between it and the shortened one through `short`, at up to
    /// [`SHORT_LEAD_PENALTY`] (the shorter, the dearer), each where its port may slide to, with its price. A
    /// lead a little shorter lets the rail turn straight onto the lane of the other port instead of stepping
    /// over to it (T-421).
    fn lead_cells_both(&self, port: &RailPort, lead: PlanPoint, short: PlanPoint) -> Vec<((usize, usize), u64)> {
        let along_x = port.inward.x.abs() > port.inward.y.abs();
        let (lines, full, shortest) = if along_x { (&self.xs, lead.x, short.x) } else { (&self.ys, lead.y, short.y) };
        let (low, high) = (full.min(shortest), full.max(shortest));
        let span = (full - shortest).abs().max(SAME_LINE);
        let mut cells = Vec::new();
        for &line in lines.iter().filter(|&&line| line >= low - SAME_LINE && line <= high + SAME_LINE) {
            let shortening = ((line - full).abs() / span).min(1.0);
            let price = (SHORT_LEAD_PENALTY as f64 * shortening).round() as u64;
            let at = if along_x { PlanPoint::new(line, lead.y) } else { PlanPoint::new(lead.x, line) };
            for cell in self.lead_cells(port, at) {
                if cells.iter().all(|(other, _)| *other != cell) {
                    cells.push((cell, price));
                }
            }
        }
        cells
    }

    /// Grid points on the lead line of `port` (through `lead`) its port may slide to.
    fn lead_cells(&self, port: &RailPort, lead: PlanPoint) -> Vec<(usize, usize)> {
        let Some((column, row)) = self.locate(lead) else { return Vec::new() };
        if port.inward.x.abs() > port.inward.y.abs() {
            (0..self.ys.len()).filter(|&row| port.allows(self.ys[row])).map(|row| (column, row)).collect()
        } else {
            (0..self.xs.len()).filter(|&column| port.allows(self.xs[column])).map(|column| (column, row)).collect()
        }
    }

    fn trace(&self, parent: &[usize], mut state: usize, columns: usize) -> Vec<PlanPoint> {
        let point_of = |state: usize| {
            let cell = state / STATE_HEADINGS;
            self.point((cell % columns, cell / columns))
        };
        let mut points = vec![point_of(state)];
        while parent[state] != usize::MAX {
            state = parent[state];
            points.push(point_of(state));
        }
        points.reverse();
        points.dedup_by(|a, b| a.distance(*b) <= SAME_LINE);
        points
    }
}

/// A leg not priced yet, and a blocked one.
const UNPRICED: u64 = u64::MAX;
const BLOCKED: u64 = u64::MAX - 1;
/// Search states per grid point: the 4 headings and [`ARRIVED`].
const STATE_HEADINGS: usize = 5;
/// The state of a path that has reached the end's lead line and stops there.
const ARRIVED: usize = 4;

/// Sorted, deduplicated grid lines plus the channel half way between each pair of neighbours (the middle
/// of the floor between two nodes is where a rail looks best).
fn with_channels(mut lines: Vec<f64>) -> Vec<f64> {
    lines.sort_by(f64::total_cmp);
    lines.dedup_by(|a, b| (*a - *b).abs() <= SAME_LINE);
    let channels: Vec<f64> = lines.windows(2).map(|pair| (pair[0] + pair[1]) / 2.0).collect();
    lines.extend(channels);
    lines.sort_by(f64::total_cmp);
    lines.dedup_by(|a, b| (*a - *b).abs() <= SAME_LINE);
    lines
}

fn milli(tiles: f64) -> u64 {
    (tiles * MILLI).round() as u64
}

/// Price of the leg `a`–`b` against the segments of the rails already drawn: crossings and stretches on
/// top of one.
fn rail_cost(a: PlanPoint, b: PlanPoint, segments: &[(PlanPoint, PlanPoint, bool)]) -> u64 {
    let reach = PlanRect::new(a.x, a.y, b.x, b.y).inflate(OVERLAP_DISTANCE);
    let mut cost = 0;
    for &(c, d, shares_node) in segments {
        if c.x.max(d.x) < reach.x0 || c.x.min(d.x) > reach.x1 || c.y.max(d.y) < reach.y0 || c.y.min(d.y) > reach.y1 {
            continue;
        }
        if segments_cross(a, b, c, d) {
            cost += if shares_node { SHARED_CROSSING_PENALTY } else { CROSSING_PENALTY };
        }
        let overlap = overlap_length(a, b, c, d);
        if overlap > 0.0 {
            cost += milli(overlap * OVERLAP_PENALTY_PER_TILE);
        }
    }
    cost
}

/// True when the leg `a0`–`a1` crosses the segment `b0`–`b1`: inside the segment, and inside the leg or at
/// its start (half-open), so a crossing exactly on a grid point is counted once, by the leg leaving it.
fn segments_cross(a0: PlanPoint, a1: PlanPoint, b0: PlanPoint, b1: PlanPoint) -> bool {
    let (along_a, along_b) = (a0.to(a1), b0.to(b1));
    let denominator = along_a.x * along_b.y - along_a.y * along_b.x;
    if denominator.abs() <= f64::EPSILON {
        return false;
    }
    let offset = a0.to(b0);
    let t = (offset.x * along_b.y - offset.y * along_b.x) / denominator;
    let u = (offset.x * along_a.y - offset.y * along_a.x) / denominator;
    const INSIDE: f64 = 1e-9;
    t > -INSIDE && t < 1.0 - INSIDE && u > INSIDE && u < 1.0 - INSIDE
}

/// Length (tiles) over which two rails, given by their corners, run parallel closer than one lane.
pub fn glued_length(a: &[PlanPoint], b: &[PlanPoint]) -> f64 {
    a.windows(2)
        .flat_map(|first| b.windows(2).map(move |second| overlap_length(first[0], first[1], second[0], second[1])))
        .sum()
}

/// Length (tiles) over which two parallel axis segments lie on top of each other.
fn overlap_length(a0: PlanPoint, a1: PlanPoint, b0: PlanPoint, b1: PlanPoint) -> f64 {
    let vertical = |p: PlanPoint, q: PlanPoint| (p.x - q.x).abs() <= SAME_LINE;
    let horizontal = |p: PlanPoint, q: PlanPoint| (p.y - q.y).abs() <= SAME_LINE;
    let shared = |low_a: f64, high_a: f64, low_b: f64, high_b: f64| (high_a.min(high_b) - low_a.max(low_b)).max(0.0);
    if vertical(a0, a1) && vertical(b0, b1) && (a0.x - b0.x).abs() < OVERLAP_DISTANCE {
        shared(a0.y.min(a1.y), a0.y.max(a1.y), b0.y.min(b1.y), b0.y.max(b1.y))
    } else if horizontal(a0, a1) && horizontal(b0, b1) && (a0.y - b0.y).abs() < OVERLAP_DISTANCE {
        shared(a0.x.min(a1.x), a0.x.max(a1.x), b0.x.min(b1.x), b0.x.max(b1.x))
    } else {
        0.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn end(point: (f64, f64), inward: (f64, f64)) -> RailPort {
        let point = PlanPoint::new(point.0, point.1);
        let inward = PlanVector::new(inward.0, inward.1);
        let lateral = if inward.x.abs() > inward.y.abs() { point.y } else { point.x };
        RailPort { point, inward, lateral: (lateral, lateral) }
    }

    fn bends(corners: &[PlanPoint]) -> usize {
        corners.len().saturating_sub(2)
    }

    fn axis_legs(corners: &[PlanPoint]) -> bool {
        corners.windows(2).all(|pair| (pair[0].x - pair[1].x).abs() < 1e-9 || (pair[0].y - pair[1].y).abs() < 1e-9)
    }

    #[test]
    fn aligned_ports_get_a_straight_rail() {
        let obstacles = ElbowObstacles { nodes: &[], heights: &[], rails: &[], own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.5, 0.0), (-1.0, 0.0)), &end((5.0, 0.0), (1.0, 0.0)), &obstacles).unwrap();
        assert_eq!(corners.len(), 2, "{corners:?}");
    }

    #[test]
    fn perpendicular_faces_get_one_bend() {
        let obstacles = ElbowObstacles { nodes: &[], heights: &[], rails: &[], own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.5, 0.0), (-1.0, 0.0)), &end((4.0, 3.5), (0.0, 1.0)), &obstacles).unwrap();
        assert!(axis_legs(&corners), "{corners:?}");
        assert_eq!(bends(&corners), 1, "{corners:?}");
    }

    #[test]
    fn a_node_in_the_way_is_passed_with_clearance() {
        let blocker = PlanRect::new(2.0, -0.5, 3.0, 0.5);
        let obstacles = ElbowObstacles { nodes: &[blocker], heights: &[0.2], rails: &[], own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.5, 0.0), (-1.0, 0.0)), &end((5.0, 0.0), (1.0, 0.0)), &obstacles).unwrap();
        assert!(axis_legs(&corners), "{corners:?}");
        let padded = blocker.inflate(RAIL_CLEARANCE - 1e-3);
        for pair in corners.windows(2) {
            for step in 0..=20 {
                let t = f64::from(step) / 20.0;
                let point =
                    PlanPoint::new(pair[0].x + (pair[1].x - pair[0].x) * t, pair[0].y + (pair[1].y - pair[0].y) * t);
                assert!(!padded.contains(point), "{corners:?}");
            }
        }
        assert!(bends(&corners) <= 4, "{corners:?}");
    }

    #[test]
    fn a_rail_of_the_same_node_is_not_crossed() {
        let other = [PlanPoint::new(2.5, -2.0), PlanPoint::new(2.5, 2.0)];
        let rails = [DrawnRail { corners: &other, shares_node: true }];
        let obstacles = ElbowObstacles { nodes: &[], heights: &[], rails: &rails, own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.5, 0.0), (-1.0, 0.0)), &end((4.0, 3.5), (0.0, 1.0)), &obstacles).unwrap();
        let crossings = corners.windows(2).filter(|pair| segments_cross(pair[0], pair[1], other[0], other[1])).count();
        assert_eq!(crossings, 0, "{corners:?}");
    }

    #[test]
    fn the_floor_behind_a_tall_body_is_hidden() {
        let footprint = PlanRect::new(0.0, 0.0, 1.0, 1.0);
        assert!(hidden_by(PlanPoint::new(-0.5, -0.5), &footprint, 1.0));
        assert!(!hidden_by(PlanPoint::new(-0.5, -0.5), &footprint, 0.2));
        assert!(!hidden_by(PlanPoint::new(1.5, 1.5), &footprint, 3.0), "in front");
    }

    #[test]
    fn a_port_a_little_off_the_lead_is_reached_by_a_shorter_lead_not_a_step() {
        // The target lane lies 0.3 tiles out of the start face: the full lead (0.4) would overshoot it and step
        // back with two more bends; a shorter lead turns straight onto it (T-421).
        let obstacles = ElbowObstacles { nodes: &[], heights: &[], rails: &[], own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.0, 0.0), (0.0, -1.0)), &end((3.0, 0.3), (1.0, 0.0)), &obstacles).unwrap();
        assert!(axis_legs(&corners), "{corners:?}");
        assert_eq!(bends(&corners), 1, "{corners:?}");
        let lead = corners[0].distance(corners[1]);
        assert!((SHORT_LEAD - 1e-9..RAIL_LEAD).contains(&lead), "lead {lead}: {corners:?}");
    }

    #[test]
    fn the_full_lead_is_kept_when_nothing_calls_for_a_shorter_one() {
        let obstacles = ElbowObstacles { nodes: &[], heights: &[], rails: &[], own: [usize::MAX; 2] };
        let corners = elbow_corners(&end((0.0, 0.0), (0.0, -1.0)), &end((3.0, 2.0), (1.0, 0.0)), &obstacles).unwrap();
        assert_eq!(bends(&corners), 1, "{corners:?}");
        assert!(corners[0].distance(corners[1]) >= RAIL_LEAD - 1e-9, "{corners:?}");
    }

    #[test]
    fn a_hairline_step_between_two_ports_is_straightened() {
        let corners = vec![
            PlanPoint::new(9.164, 4.92),
            PlanPoint::new(9.164, 5.52),
            PlanPoint::new(9.158, 5.52),
            PlanPoint::new(9.158, 6.0),
        ];
        let straight = simplified(&without_micro_jogs(corners));
        assert_eq!(straight.len(), 2, "{straight:?}");
        assert!((straight[1].x - 9.164).abs() < 1e-9 && (straight[1].y - 6.0).abs() < 1e-9, "{straight:?}");
        let real = vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 2.0),
            PlanPoint::new(0.5, 2.0),
            PlanPoint::new(0.5, 4.0),
        ];
        assert_eq!(without_micro_jogs(real.clone()), real, "a real step stays");
    }

    #[test]
    fn overlapping_legs_are_measured() {
        let length = overlap_length(
            PlanPoint::new(0.0, 1.0),
            PlanPoint::new(3.0, 1.0),
            PlanPoint::new(2.0, 1.0),
            PlanPoint::new(5.0, 1.0),
        );
        assert!((length - 1.0).abs() < 1e-9);
    }
}
