//! Edge paths between two ends (`docs/06-renderizacao.md#arestas`).
//!
//! - `curved`: cubic Bézier leaving along the exit direction and arriving tangent to the arrow head
//!   (prototype `edgeGeom`, control distance 0.42 × chord);
//! - `straight`: the same cubic with controls at 1/3 of the chord, a true segment when the ports sit on
//!   the line between the centers (only slid/spread ports bend its last third);
//! - `orthogonal`: A* on a plan grid avoiding footprints with a turn penalty; the last leg is along the
//!   arrow aim so the head stays tangent. Corners are rounded at emission time.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::geometry::{CubicBezier, PlanPoint, PlanRect, PlanVector, Projection};

use super::{EDGE_Z, EdgePath};

/// Control distance of curved edges as a fraction of the chord (prototype `0.42`).
const CURVE_TENSION: f64 = 0.42;
/// Control distance of straight edges (1/3 of the chord: a uniform-speed segment when collinear).
const STRAIGHT_TENSION: f64 = 1.0 / 3.0;
/// Shortest control arm (fraction of the normal one) for ends that fight the chord.
const MIN_REACH_FACTOR: f64 = 0.3;
/// Grid step of orthogonal routing (tiles).
const GRID_STEP: f64 = 0.5;
/// Clearance kept around footprints by orthogonal routes (tiles).
const ROUTE_CLEARANCE: f64 = 0.3;
/// Length of the straight lead-out / lead-in at each end of an orthogonal route (tiles).
const ROUTE_LEAD: f64 = 0.45;
/// Extra grid margin around the layout bounds (tiles).
const GRID_MARGIN: f64 = 3.0;
/// A* costs (integers keep the search deterministic).
const STEP_COST: u32 = 10;
const TURN_COST: u32 = 16;
/// Upper bound on explored states (protects against pathological specs).
const MAX_EXPANSIONS: usize = 200_000;

/// One end of an edge path, in plan coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathEnd {
    /// Where the stroke ends (head base, or the bare exit point).
    pub point: PlanPoint,
    /// Unit direction the path travels when it reaches this end, pointing into the node.
    pub inward: PlanVector,
}

/// Unit screen direction of a plan direction.
fn screen_direction(projection: &Projection, direction: PlanVector) -> crate::geometry::ScreenVector {
    projection.project_vector(direction).normalized()
}

/// Scale of the two control arms of a cubic (1 = the prototype's tension). Longer arms bow the curve
/// out along the end directions, which is how a curved edge swings around a node in its way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmScale {
    /// Arm leaving the start.
    pub leave: f64,
    /// Arm arriving at the end.
    pub arrive: f64,
}

impl ArmScale {
    /// The prototype's arms.
    pub const NORMAL: ArmScale = ArmScale { leave: 1.0, arrive: 1.0 };
}

/// Cubic from `start` to `end`: leaves `start` away from its node, arrives at `end` along `end.inward`.
pub fn cubic_path(projection: &Projection, start: &PathEnd, end: &PathEnd, straight: bool) -> EdgePath {
    scaled_cubic_path(projection, start, end, straight, ArmScale::NORMAL)
}

/// [`cubic_path`] with scaled control arms (tangency at both ends is unchanged).
pub fn scaled_cubic_path(
    projection: &Projection,
    start: &PathEnd,
    end: &PathEnd,
    straight: bool,
    arms: ArmScale,
) -> EdgePath {
    let p0 = projection.project(start.point, EDGE_Z);
    let p3 = projection.project(end.point, EDGE_Z);
    let tension = if straight { STRAIGHT_TENSION } else { CURVE_TENSION };
    let chord = p0.distance(p3);
    let leave = screen_direction(projection, start.inward * -1.0);
    let arrive = screen_direction(projection, end.inward);
    let along = if chord > f64::EPSILON { (p3 - p0) * (1.0 / chord) } else { arrive };
    // An end whose direction fights the chord (a port slid to a side corner) gets a shorter control arm,
    // so the curve turns close to the node instead of overshooting into a hook. Tangency is unchanged.
    let leave_reach = arms.leave * tension * chord * reach_factor(leave.x * along.x + leave.y * along.y);
    let arrive_reach = arms.arrive * tension * chord * reach_factor(arrive.x * along.x + arrive.y * along.y);
    EdgePath::Curve(CubicBezier::new(p0, p0 + leave * leave_reach, p3 - arrive * arrive_reach, p3))
}

/// Control arm scale from the cosine between an end direction and the chord: 1 when aligned,
/// [`MIN_REACH_FACTOR`] when perpendicular or opposed.
fn reach_factor(cosine: f64) -> f64 {
    MIN_REACH_FACTOR + (1.0 - MIN_REACH_FACTOR) * cosine.max(0.0)
}

/// Orthogonal route avoiding `obstacles` (footprint rects, already including the two end nodes).
/// Returns `None` when no grid path exists (the caller falls back to a curve).
#[cfg(test)]
pub fn orthogonal_path(
    projection: &Projection,
    start: &PathEnd,
    end: &PathEnd,
    obstacles: &[PlanRect],
    bounds: PlanRect,
) -> Option<EdgePath> {
    orthogonal_path_on(&RouteGrid::new(obstacles, bounds), projection, start, end)
}

/// The routing grid of a scene (footprints blocked), built once and shared by every orthogonal route.
pub struct RouteGrid(Grid);

impl RouteGrid {
    /// Grid over `bounds` (plus a margin) with `obstacles` blocked.
    pub fn new(obstacles: &[PlanRect], bounds: PlanRect) -> Self {
        Self(Grid::new(bounds.inflate(GRID_MARGIN), obstacles))
    }
}

/// [`orthogonal_path`] on a prebuilt grid.
pub fn orthogonal_path_on(
    grid: &RouteGrid,
    projection: &Projection,
    start: &PathEnd,
    end: &PathEnd,
) -> Option<EdgePath> {
    let corners = orthogonal_corners_on(grid, start, end)?;
    Some(EdgePath::Polyline(corners.iter().map(|point| projection.project(*point, EDGE_Z)).collect()))
}

/// Plan corners of the orthogonal route from `start` to `end` (both included), `None` without a path.
fn orthogonal_corners_on(grid: &RouteGrid, start: &PathEnd, end: &PathEnd) -> Option<Vec<PlanPoint>> {
    let grid = &grid.0;
    let lead_out = start.point - start.inward * ROUTE_LEAD;
    let lead_in = end.point - end.inward * ROUTE_LEAD;
    let from = grid.snap(lead_out);
    let to = grid.snap(lead_in);
    let cells = grid.search(from, to)?;
    let mut points = vec![start.point, lead_out];
    let first = grid.point(cells[0]);
    points.push(elbow(lead_out, first, start.inward * -1.0));
    points.extend(cells.iter().map(|&cell| grid.point(cell)));
    let last = grid.point(cells[cells.len() - 1]);
    points.push(elbow_before(last, lead_in, end.inward));
    points.push(lead_in);
    points.push(end.point);
    Some(simplify(&points))
}

/// Corner joining `from` (travelling along `heading`) to `to` with axis-aligned legs: keep the heading first.
fn elbow(from: PlanPoint, to: PlanPoint, heading: PlanVector) -> PlanPoint {
    if heading.x.abs() >= heading.y.abs() { PlanPoint::new(to.x, from.y) } else { PlanPoint::new(from.x, to.y) }
}

/// Corner joining `from` to `to` so the last leg runs along `heading`.
fn elbow_before(from: PlanPoint, to: PlanPoint, heading: PlanVector) -> PlanPoint {
    if heading.x.abs() >= heading.y.abs() { PlanPoint::new(from.x, to.y) } else { PlanPoint::new(to.x, from.y) }
}

/// Drops repeated points and middle points of collinear runs (keeps both ends).
fn simplify(points: &[PlanPoint]) -> Vec<PlanPoint> {
    const SAME: f64 = 1e-6;
    let mut deduped: Vec<PlanPoint> = Vec::with_capacity(points.len());
    for point in points {
        if deduped.last().is_none_or(|last| last.distance(*point) > SAME) {
            deduped.push(*point);
        }
    }
    let mut result: Vec<PlanPoint> = Vec::with_capacity(deduped.len());
    for point in deduped {
        while result.len() >= 2 {
            let a = result[result.len() - 2];
            let b = result[result.len() - 1];
            let first = a.to(b);
            let second = b.to(point);
            let cross = first.x * second.y - first.y * second.x;
            if cross.abs() > SAME || first.dot(second) < 0.0 {
                break;
            }
            result.pop();
        }
        result.push(point);
    }
    result
}

/// Grid cell index.
type Cell = (i32, i32);

/// Heading on the grid (0 = +x, 1 = +y, 2 = −x, 3 = −y; 4 = none yet).
const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
const NO_HEADING: u8 = 4;

/// Plan grid of orthogonal routing.
struct Grid {
    origin: PlanPoint,
    columns: i32,
    rows: i32,
    blocked: Vec<bool>,
}

impl Grid {
    fn new(area: PlanRect, obstacles: &[PlanRect]) -> Self {
        let origin =
            PlanPoint::new((area.x0 / GRID_STEP).floor() * GRID_STEP, (area.y0 / GRID_STEP).floor() * GRID_STEP);
        let columns = ((area.x1 - origin.x) / GRID_STEP).ceil() as i32 + 1;
        let rows = ((area.y1 - origin.y) / GRID_STEP).ceil() as i32 + 1;
        let mut grid = Self { origin, columns, rows, blocked: vec![false; (columns.max(0) * rows.max(0)) as usize] };
        // Mark only the cells under each obstacle instead of testing every cell against every obstacle.
        for rect in obstacles.iter().map(|rect| rect.inflate(ROUTE_CLEARANCE)) {
            let first = ((rect.x0 - origin.x) / GRID_STEP).ceil().max(0.0) as i32;
            let last = (((rect.x1 - origin.x) / GRID_STEP).floor() as i32).min(columns - 1);
            let top = ((rect.y0 - origin.y) / GRID_STEP).ceil().max(0.0) as i32;
            let bottom = (((rect.y1 - origin.y) / GRID_STEP).floor() as i32).min(rows - 1);
            for row in top..=bottom {
                for column in first..=last {
                    if rect.contains(grid.point((column, row))) {
                        grid.blocked[(row * columns + column) as usize] = true;
                    }
                }
            }
        }
        grid
    }

    fn point(&self, (column, row): Cell) -> PlanPoint {
        PlanPoint::new(self.origin.x + f64::from(column) * GRID_STEP, self.origin.y + f64::from(row) * GRID_STEP)
    }

    fn snap(&self, point: PlanPoint) -> Cell {
        let column = ((point.x - self.origin.x) / GRID_STEP).round() as i32;
        let row = ((point.y - self.origin.y) / GRID_STEP).round() as i32;
        (column.clamp(0, self.columns - 1), row.clamp(0, self.rows - 1))
    }

    fn is_free(&self, (column, row): Cell, from: Cell, to: Cell) -> bool {
        if column < 0 || row < 0 || column >= self.columns || row >= self.rows {
            return false;
        }
        (column, row) == from || (column, row) == to || !self.blocked[(row * self.columns + column) as usize]
    }

    /// A* with a turn penalty. Ties are broken by insertion order, so the result is deterministic.
    /// Costs and parents live in flat arrays indexed by [`Grid::state`] (unknown = `u32::MAX`).
    fn search(&self, from: Cell, to: Cell) -> Option<Vec<Cell>> {
        let heuristic = |(column, row): Cell| (column.abs_diff(to.0) + row.abs_diff(to.1)) * STEP_COST;
        let states = (self.columns.max(0) * self.rows.max(0)) as usize * HEADINGS;
        let mut best: Vec<u32> = vec![UNKNOWN; states];
        let mut parent: Vec<u32> = vec![UNKNOWN; states];
        let mut open = BinaryHeap::new();
        let mut counter: u64 = 0;
        best[self.state(from, NO_HEADING)] = 0;
        open.push(Reverse((heuristic(from), counter, from, NO_HEADING)));
        let mut expansions = 0usize;
        while let Some(Reverse((_, _, cell, heading))) = open.pop() {
            if cell == to {
                return Some(self.trace(&parent, self.state(cell, heading)));
            }
            expansions += 1;
            if expansions > MAX_EXPANSIONS {
                return None;
            }
            let here = self.state(cell, heading);
            let cost = best[here];
            for (index, (dx, dy)) in DIRECTIONS.iter().enumerate() {
                let next = (cell.0 + dx, cell.1 + dy);
                if !self.is_free(next, from, to) {
                    continue;
                }
                let next_heading = index as u8;
                let turn = if heading == NO_HEADING || heading == next_heading { 0 } else { TURN_COST };
                let next_cost = cost.saturating_add(STEP_COST + turn);
                let there = self.state(next, next_heading);
                if best[there] != UNKNOWN && best[there] <= next_cost {
                    continue;
                }
                best[there] = next_cost;
                parent[there] = here as u32;
                counter += 1;
                open.push(Reverse((next_cost + heuristic(next), counter, next, next_heading)));
            }
        }
        None
    }

    /// Flat index of a search state (a cell inside the grid and a heading).
    fn state(&self, (column, row): Cell, heading: u8) -> usize {
        (row * self.columns + column) as usize * HEADINGS + usize::from(heading)
    }

    fn trace(&self, parent: &[u32], mut state: usize) -> Vec<Cell> {
        let cell_of = |state: usize| {
            let index = (state / HEADINGS) as i32;
            (index % self.columns, index / self.columns)
        };
        let mut cells = vec![cell_of(state)];
        while parent[state] != UNKNOWN {
            state = parent[state] as usize;
            cells.push(cell_of(state));
        }
        cells.reverse();
        cells
    }
}

/// Search states per cell: the 4 headings and [`NO_HEADING`].
const HEADINGS: usize = 5;
/// Cost or parent not known yet in [`Grid::search`].
const UNKNOWN: u32 = u32::MAX;

#[cfg(test)]
mod tests {
    use super::*;

    fn projection() -> Projection {
        Projection::new(56.0)
    }

    #[test]
    fn curve_arrives_along_the_inward_direction() {
        let start = PathEnd { point: PlanPoint::new(0.5, 0.0), inward: PlanVector::new(-1.0, 0.0) };
        let end = PathEnd { point: PlanPoint::new(4.0, 2.0), inward: PlanVector::new(0.6, 0.8) };
        let EdgePath::Curve(curve) = cubic_path(&projection(), &start, &end, false) else {
            panic!("expected a curve");
        };
        let expected = projection().project_vector(end.inward);
        assert!(curve.end_direction().angle_between_degrees(expected).abs() < 1e-9);
        let leaving = curve.points[1] - curve.points[0];
        assert!(leaving.angle_between_degrees(projection().project_vector(PlanVector::new(1.0, 0.0))).abs() < 1e-9);
    }

    #[test]
    fn straight_edge_between_aligned_ports_is_a_segment() {
        let direction = PlanVector::new(1.0, 0.0);
        let start = PathEnd { point: PlanPoint::new(0.5, 0.0), inward: direction * -1.0 };
        let end = PathEnd { point: PlanPoint::new(3.0, 0.0), inward: direction };
        let EdgePath::Curve(curve) = cubic_path(&projection(), &start, &end, true) else {
            panic!("expected a curve");
        };
        let mid = curve.point_at(0.5);
        let chord_mid = curve.points[0].midpoint(curve.points[3]);
        assert!(mid.distance(chord_mid) < 1e-9);
    }

    #[test]
    fn orthogonal_route_avoids_a_blocking_footprint() {
        let start = PathEnd { point: PlanPoint::new(0.6, 0.0), inward: PlanVector::new(-1.0, 0.0) };
        let end = PathEnd { point: PlanPoint::new(5.4, 0.0), inward: PlanVector::new(1.0, 0.0) };
        let blocker = PlanRect::around(PlanPoint::new(3.0, 0.0), 0.5, 0.5);
        let obstacles = [PlanRect::around(PlanPoint::new(0.0, 0.0), 0.42, 0.42), blocker];
        let bounds = PlanRect::new(-1.0, -1.0, 7.0, 1.0);
        let Some(EdgePath::Polyline(points)) = orthogonal_path(&projection(), &start, &end, &obstacles, bounds) else {
            panic!("expected a polyline");
        };
        let plan: Vec<PlanPoint> = points.iter().map(|point| projection().unproject(*point, EDGE_Z)).collect();
        for pair in plan.windows(2) {
            let step = pair[0].to(pair[1]);
            assert!(step.x.abs() < 1e-6 || step.y.abs() < 1e-6, "legs are axis aligned: {pair:?}");
            for k in 0..=20 {
                let sample = pair[0] + step * (f64::from(k) / 20.0);
                assert!(!blocker.contains(sample), "route crosses the blocker at {sample:?}");
            }
        }
        let last = plan[plan.len() - 1];
        assert!(last.distance(end.point) < 1e-6);
    }

    #[test]
    fn simplify_merges_collinear_points() {
        let points = [
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(1.0, 0.0),
            PlanPoint::new(1.0, 0.0),
            PlanPoint::new(2.0, 0.0),
            PlanPoint::new(2.0, 1.0),
        ];
        assert_eq!(simplify(&points).len(), 3);
    }
}
