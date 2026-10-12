//! Greedy collision picking (prototype `pick` / `hit` / `cost`): the first candidate that touches nothing
//! wins; otherwise the least-overlapping one, whose residual area is added to the scene score.

use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics};

use crate::canvas::ItemRole;
use crate::geometry::Rect;

use super::{COLLISION_MARGIN_PX, CollisionReport, ItemRef, Overlap};

/// Something already placed that labels must avoid.
#[derive(Debug, Clone, PartialEq)]
pub struct Obstacle {
    /// Screen box.
    pub rect: Rect,
    /// What it is.
    pub item: ItemRef,
}

/// A candidate position.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate<T> {
    /// Screen box the label would occupy.
    pub rect: Rect,
    /// Finer boxes covering the label when its shape is not axis aligned (floor pills); empty = `rect`.
    pub parts: Vec<Rect>,
    /// Leader line box of a node label: it must not cross other labels or badges (T-381), but may cross
    /// strokes and its own node (leaders through other nodes are vetted separately).
    pub leader: Option<Rect>,
    /// Caller data (position, transform…).
    pub value: T,
    /// Weighted overlap with obstacles that never move, measured once ([`price_against`]); the obstacles
    /// passed to [`Collisions::pick`] are added to it.
    pub fixed_cost: f64,
}

/// Who is being placed (for reports and diagnostics).
pub struct Owner<'a> {
    /// Item reference.
    pub item: ItemRef,
    /// Spec path of the label (`nodes[3].label`).
    pub path: &'a str,
}

/// Accumulates the residual score, overlaps and `W_LABEL_OVERLAP` diagnostics.
#[derive(Debug, Clone, Default)]
pub struct Collisions {
    /// Score and overlaps.
    pub report: CollisionReport,
    /// Diagnostics.
    pub diagnostics: Diagnostics,
}

/// A uniform grid over static obstacles (nodes, edge strokes), so each candidate only looks at the
/// obstacles in the cells it covers.
#[derive(Debug, Clone)]
pub struct ObstacleIndex {
    items: Vec<Obstacle>,
    origin: (f64, f64),
    columns: usize,
    rows: usize,
    cells: Vec<Vec<CellEntry>>,
}

/// An item listed in a cell, with a copy of its box: a query tests the boxes of a cell without jumping
/// around the item list (most of what it meets does not touch it).
#[derive(Debug, Clone, Copy)]
struct CellEntry {
    rect: Rect,
    position: u32,
    /// First cell (column, row) of the item, so a query spanning several of its cells takes it once.
    first_column: u32,
    first_row: u32,
}

/// How much worse covering a zone label is than covering anything else (see [`candidate_cost`]).
const ZONE_LABEL_COVER_WEIGHT: f64 = 4.0;

/// How much worse covering a node body is than covering a label or a stroke: the icon is the diagram.
/// Badges (`v7`, `×3`) are part of the node and weigh the same.
const NODE_COVER_WEIGHT: f64 = 3.0;

/// Margin (px) for leader lines, see [`margin_against`].
pub const LEADER_MARGIN_PX: f64 = 0.0;

/// A thin leader line through a label is less bad than a label box on top of it: share of the weighted
/// overlap a leader crossing costs when choosing (the score keeps the full area).
const LEADER_CROSSING_WEIGHT: f64 = 0.5;

/// Grid cell size of [`ObstacleIndex`] (px).
const INDEX_CELL_PX: f64 = 64.0;

impl ObstacleIndex {
    /// Indexes `items`.
    pub fn new(items: Vec<Obstacle>) -> Self {
        let bounds = items.iter().fold(Rect::EMPTY, |bounds, item| bounds.union(item.rect));
        let (origin, columns, rows) = if bounds.is_empty() {
            ((0.0, 0.0), 1, 1)
        } else {
            let columns = (bounds.width() / INDEX_CELL_PX).floor() as usize + 1;
            let rows = (bounds.height() / INDEX_CELL_PX).floor() as usize + 1;
            ((bounds.x0, bounds.y0), columns, rows)
        };
        let mut index = Self { items: Vec::new(), origin, columns, rows, cells: vec![Vec::new(); columns * rows] };
        for (position, item) in items.iter().enumerate() {
            let (c0, c1, r0, r1) = index.cell_range(&item.rect);
            let entry =
                CellEntry { rect: item.rect, position: position as u32, first_column: c0 as u32, first_row: r0 as u32 };
            for row in r0..=r1 {
                for column in c0..=c1 {
                    index.cells[row * columns + column].push(entry);
                }
            }
        }
        index.items = items;
        index
    }

    fn cell_range(&self, rect: &Rect) -> (usize, usize, usize, usize) {
        let to_cell = |value: f64, origin: f64, count: usize| {
            (((value - origin) / INDEX_CELL_PX).floor().max(0.0) as usize).min(count - 1)
        };
        (
            to_cell(rect.x0, self.origin.0, self.columns),
            to_cell(rect.x1, self.origin.0, self.columns),
            to_cell(rect.y0, self.origin.1, self.rows),
            to_cell(rect.y1, self.origin.1, self.rows),
        )
    }

    /// Obstacles that touch `rect` (with [`COLLISION_MARGIN_PX`]), in index order, deduplicated. Only touching
    /// obstacles can add to a cost (everything else overlaps by 0), so skipping the rest of the cells'
    /// members changes no sum, only the work.
    fn near<'a>(&'a self, rect: &Rect, out: &mut Vec<&'a Obstacle>) {
        if rect.is_empty() || self.items.is_empty() {
            return;
        }
        let touches = |entry: &CellEntry| rect.overlap_area(&entry.rect, COLLISION_MARGIN_PX) > 0.0;
        let (c0, c1, r0, r1) = self.cell_range(&rect.inflate(COLLISION_MARGIN_PX));
        if c0 == c1 && r0 == r1 {
            // One cell (the usual case for a label): its list is already in index order without duplicates.
            let cell = &self.cells[r0 * self.columns + c0];
            out.extend(cell.iter().filter(|entry| touches(entry)).map(|entry| &self.items[entry.position as usize]));
            return;
        }
        // An item listed in several cells of the query is taken in the first of them only (where its own
        // range meets the query range), so no dedup is needed; the sort restores index order.
        let mut found: Vec<u32> = Vec::new();
        for row in r0..=r1 {
            for column in c0..=c1 {
                found.extend(
                    self.cells[row * self.columns + column]
                        .iter()
                        .filter(|entry| {
                            (entry.first_column as usize).max(c0) == column
                                && (entry.first_row as usize).max(r0) == row
                                && touches(entry)
                        })
                        .map(|entry| entry.position),
                );
            }
        }
        found.sort_unstable();
        out.extend(found.into_iter().map(|position| &self.items[position as usize]));
    }
}

/// A set of obstacles a label must avoid: a plain list (small, growing sets of placed labels) or an index.
#[derive(Debug, Clone, Copy)]
pub enum Layer<'a> {
    /// Linear list.
    Loose(&'a [Obstacle]),
    /// Grid index.
    Indexed(&'a ObstacleIndex),
}

/// Obstacles of `layers` that may touch `rect`, minus the ignored ones.
fn obstacles_near<'a>(layers: &[Layer<'a>], rect: &Rect, ignore: &impl Fn(&ItemRef) -> bool) -> Vec<&'a Obstacle> {
    let mut near: Vec<&'a Obstacle> = Vec::new();
    for layer in layers {
        match layer {
            Layer::Loose(items) => {
                near.extend(items.iter().filter(|item| rect.overlap_area(&item.rect, COLLISION_MARGIN_PX) > 0.0));
            }
            Layer::Indexed(index) => index.near(rect, &mut near),
        }
    }
    near.retain(|obstacle| !ignore(&obstacle.item));
    near
}

impl Collisions {
    /// Picks the first free candidate (or the least bad one), ignoring obstacles for which `ignore` is true.
    /// Returns `None` only when there are no candidates.
    pub fn pick<T: Clone>(
        &mut self,
        owner: &Owner<'_>,
        candidates: Vec<Candidate<T>>,
        layers: &[Layer<'_>],
        ignore: impl Fn(&ItemRef) -> bool,
    ) -> Option<Candidate<T>> {
        self.pick_priced(owner, candidates, layers, &[], ignore)
    }

    /// [`Collisions::pick`] for candidates already priced against `priced` ([`price_against`]): only
    /// `layers` are searched per candidate; both are reported for the chosen one.
    pub fn pick_priced<T: Clone>(
        &mut self,
        owner: &Owner<'_>,
        candidates: Vec<Candidate<T>>,
        layers: &[Layer<'_>],
        priced: &[Layer<'_>],
        ignore: impl Fn(&ItemRef) -> bool,
    ) -> Option<Candidate<T>> {
        self.pick_priced_with(owner, candidates.iter().map(|candidate| (candidate, 0.0)), layers, priced, ignore)
    }

    /// [`Collisions::pick_priced`] over `(candidate, extra choice cost)` pairs, in the order given: the
    /// extra cost joins the candidate's fixed cost, and a lazy iterator only works it out for the candidates
    /// the search reaches (it stops at the first free one).
    pub fn pick_priced_with<'c, T: Clone + 'c>(
        &mut self,
        owner: &Owner<'_>,
        candidates: impl IntoIterator<Item = (&'c Candidate<T>, f64)>,
        layers: &[Layer<'_>],
        priced: &[Layer<'_>],
        ignore: impl Fn(&ItemRef) -> bool,
    ) -> Option<Candidate<T>> {
        let mut best: Option<(f64, f64, &Candidate<T>)> = None;
        for (candidate, extra) in candidates {
            let fixed_cost = candidate.fixed_cost + extra;
            if best.as_ref().is_some_and(|(best_cost, _, _)| fixed_cost >= *best_cost) {
                continue;
            }
            let near = obstacles_near(layers, &candidate.reach(), &ignore);
            let cost = fixed_cost + candidate_cost(candidate, &near);
            if cost == 0.0 {
                return Some(Candidate { fixed_cost, ..candidate.clone() });
            }
            if best.as_ref().is_none_or(|(best_cost, _, _)| cost < *best_cost) {
                best = Some((cost, fixed_cost, candidate));
            }
        }
        let (_, fixed_cost, chosen) = best?;
        let near = obstacles_near(&[layers, priced].concat(), &chosen.reach(), &ignore);
        self.record(owner, chosen, &near);
        Some(Candidate { fixed_cost, ..chosen.clone() })
    }

    /// Records a label that touches nothing but reads as `other`'s (T-394): `penalty` (px²) joins the score
    /// as an overlap between the two, with a `W_LABEL_OVERLAP` explaining the misreading.
    pub fn record_misread(&mut self, path: &str, label: ItemRef, other: ItemRef, penalty: f64, message: String) {
        if penalty <= 0.0 {
            return;
        }
        self.report.score += penalty;
        self.report.overlaps.push(Overlap { a: label, b: other, area: penalty });
        self.diagnostics.push(
            Diagnostic::new(DiagnosticCode::LabelOverlap, path, message)
                .with_hint("shorten the label, try view.rotation: auto, or use density: spaced"),
        );
    }

    fn record<T>(&mut self, owner: &Owner<'_>, chosen: &Candidate<T>, obstacles: &[&Obstacle]) {
        // `(owner side, other item, area)`: the label box, or its leader line.
        let leader_item = ItemRef { role: ItemRole::Leader, id: owner.item.id.clone() };
        let mut by_item: Vec<(ItemRef, ItemRef, f64)> = Vec::new();
        let mut add = |side: &ItemRef, other: &ItemRef, area: f64| {
            if area <= 0.0 {
                return;
            }
            match by_item.iter_mut().find(|(own, item, _)| own == side && item == other) {
                Some((_, _, total)) => *total += area,
                None => by_item.push((side.clone(), other.clone(), area)),
            }
        };
        for obstacle in obstacles {
            add(&owner.item, &obstacle.item, box_overlap(chosen, obstacle));
        }
        for (item, area) in leader_crossings(chosen, obstacles) {
            add(&leader_item, item, area);
        }
        // A candidate can win on its choice cost alone (e.g. a pill that reads as another edge) without
        // touching anything: that is not an overlap, so it must not raise `W_LABEL_OVERLAP`.
        if by_item.is_empty() {
            return;
        }
        self.report.score += by_item.iter().map(|(_, _, area)| area).sum::<f64>();
        let names: Vec<String> = by_item
            .iter()
            .map(|(side, item, area)| {
                let via = if side.role == ItemRole::Leader { " (leader line)" } else { "" };
                format!("{} '{}'{via} ({:.0} px²)", item.role.as_str(), item.id, area)
            })
            .collect();
        for (side, item, area) in by_item {
            self.report.overlaps.push(Overlap { a: side, b: item, area });
        }
        self.diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LabelOverlap,
                owner.path,
                format!("{} '{}' overlaps {}", owner.item.role.as_str(), owner.item.id, names.join(", ")),
            )
            .with_hint("shorten the label, try view.rotation: auto, or use density: spaced"),
        );
    }
}

impl<T> Candidate<T> {
    /// Axis-aligned candidate.
    pub fn new(rect: Rect, value: T) -> Self {
        Self { rect, parts: Vec::new(), leader: None, value, fixed_cost: 0.0 }
    }

    /// Node label candidate with its leader line box.
    pub fn with_leader(mut self, leader: Rect) -> Self {
        self.leader = Some(leader);
        self
    }

    /// Everything the candidate occupies (label and leader), to look up nearby obstacles.
    pub fn reach(&self) -> Rect {
        self.leader.map_or(self.rect, |leader| self.rect.union(leader))
    }

    /// Boxes used for collision tests.
    pub fn boxes(&self) -> &[Rect] {
        if self.parts.is_empty() { std::slice::from_ref(&self.rect) } else { &self.parts }
    }
}

/// True when the candidate touches none of the obstacles (except ignored ones), counting its fixed cost.
pub fn is_free<T>(candidate: &Candidate<T>, layers: &[Layer<'_>], ignore: impl Fn(&ItemRef) -> bool) -> bool {
    if candidate.fixed_cost != 0.0 {
        return false;
    }
    // The cost is a sum of non-negative terms: it is 0 exactly when no obstacle adds anything, so the
    // first one that does settles it (no list, no sum).
    let reach = candidate.reach();
    let adds_cost = |obstacle: &Obstacle| {
        !ignore(&obstacle.item) && (box_overlap(candidate, obstacle) > 0.0 || leader_overlap(candidate, obstacle) > 0.0)
    };
    let mut near: Vec<&Obstacle> = Vec::new();
    for layer in layers {
        let blocked = match layer {
            Layer::Loose(items) => items.iter().any(|item| touches_reach(&reach, item) && adds_cost(item)),
            Layer::Indexed(index) => {
                near.clear();
                index.near(&reach, &mut near);
                near.iter().any(|obstacle| adds_cost(obstacle))
            }
        };
        if blocked {
            return false;
        }
    }
    true
}

/// Adds to each candidate's fixed cost its overlap with `layers` (obstacles that stay put while many
/// placements are tried), so [`Collisions::pick_priced`] only searches the obstacles that change.
pub fn price_against<T>(candidates: &mut [Candidate<T>], layers: &[Layer<'_>], ignore: impl Fn(&ItemRef) -> bool) {
    let costs = costs_against(candidates, layers, ignore);
    for (candidate, cost) in candidates.iter_mut().zip(costs) {
        candidate.fixed_cost += cost;
    }
}

/// [`cost_against`] for every candidate. Neighbouring candidates (the shifts of a label, the side spots
/// of a pill) are looked up together: one index query for a run of them whose joint reach stays small,
/// then each one filters that short list. The list keeps the layers' order and each candidate keeps
/// exactly the obstacles a query of its own would return, so every cost is the same to the bit.
pub fn costs_against<T>(
    candidates: &[Candidate<T>],
    layers: &[Layer<'_>],
    ignore: impl Fn(&ItemRef) -> bool,
) -> Vec<f64> {
    let mut costs = Vec::with_capacity(candidates.len());
    let mut own: Vec<&Obstacle> = Vec::new();
    let mut start = 0;
    while start < candidates.len() {
        let end = shared_query_end(candidates, start);
        let reach = candidates[start..end].iter().fold(Rect::EMPTY, |reach, candidate| reach.union(candidate.reach()));
        let shared = obstacles_near(layers, &reach, &ignore);
        for candidate in &candidates[start..end] {
            let reach = candidate.reach();
            own.clear();
            own.extend(shared.iter().copied().filter(|obstacle| touches_reach(&reach, obstacle)));
            costs.push(candidate_cost(candidate, &own));
        }
        start = end;
    }
    costs
}

/// Most candidates looked up by one shared query in [`costs_against`].
const SHARED_QUERY_MAX: usize = 64;
/// A shared query may cover at most this many times the reach of its first candidate.
const SHARED_QUERY_GROWTH: f64 = 16.0;

/// End (exclusive) of the run of candidates from `start` looked up together.
fn shared_query_end<T>(candidates: &[Candidate<T>], start: usize) -> usize {
    let mut reach = candidates[start].reach();
    let limit = reach.area() * SHARED_QUERY_GROWTH;
    let mut end = start + 1;
    while end < candidates.len() && end - start < SHARED_QUERY_MAX {
        let grown = reach.union(candidates[end].reach());
        if grown.area() > limit {
            break;
        }
        reach = grown;
        end += 1;
    }
    end
}

/// True when `obstacle` touches `reach` (the test every layer applies before costing).
fn touches_reach(reach: &Rect, obstacle: &Obstacle) -> bool {
    reach.overlap_area(&obstacle.rect, COLLISION_MARGIN_PX) > 0.0
}

/// Weighted overlap of a candidate: what it would cover, with zone labels (they name the layers and are
/// reserved first) and node bodies counting more than labels and strokes.
fn candidate_cost<T>(candidate: &Candidate<T>, obstacles: &[&Obstacle]) -> f64 {
    let boxes: f64 =
        obstacles.iter().map(|obstacle| box_overlap(candidate, obstacle) * obstacle_weight(&obstacle.item)).sum();
    let leader: f64 = leader_crossings(candidate, obstacles)
        .iter()
        .map(|(item, area)| area * obstacle_weight(item) * LEADER_CROSSING_WEIGHT)
        .sum();
    boxes + leader
}

/// Overlap (px², with margin) of the candidate's leader line with each item it must not cross. An item
/// made of several boxes (a floor pill cut into slices) counts once, by its largest crossing: the line
/// crosses the item once, however many of its slices' bounding boxes overlap.
fn leader_crossings<'o, T>(candidate: &Candidate<T>, obstacles: &[&'o Obstacle]) -> Vec<(&'o ItemRef, f64)> {
    let mut crossings: Vec<(&ItemRef, f64)> = Vec::new();
    for obstacle in obstacles {
        let area = leader_overlap(candidate, obstacle);
        if area <= 0.0 {
            continue;
        }
        match crossings.iter_mut().find(|(item, _)| **item == obstacle.item) {
            Some((_, largest)) => *largest = largest.max(area),
            None => crossings.push((&obstacle.item, area)),
        }
    }
    crossings
}

/// Overlap (px², with margin) of the candidate's label boxes with an obstacle.
fn box_overlap<T>(candidate: &Candidate<T>, obstacle: &Obstacle) -> f64 {
    let margin = margin_against(&obstacle.item);
    if candidate.rect.overlap_area(&obstacle.rect, margin) <= 0.0 {
        return 0.0;
    }
    candidate.boxes().iter().map(|rect| rect.overlap_area(&obstacle.rect, margin)).sum()
}

/// Overlap (px²) of the candidate's leader line with an obstacle it must not cross.
fn leader_overlap<T>(candidate: &Candidate<T>, obstacle: &Obstacle) -> f64 {
    match candidate.leader {
        Some(leader) if blocks_leaders(&obstacle.item) => leader.overlap_area(&obstacle.rect, LEADER_MARGIN_PX),
        _ => 0.0,
    }
}

/// Margin under which two boxes count as touching: a leader line is only hit where it is drawn (a box a
/// pixel away from a thin line does not read as crossing it), every other box as soon as it touches.
fn margin_against(item: &ItemRef) -> f64 {
    if item.role == ItemRole::Leader { LEADER_MARGIN_PX } else { COLLISION_MARGIN_PX }
}

/// What a leader line must not cross: pills and zone labels (text it would strike through) and badges.
/// Strokes and other leaders are fine (they cross at an angle or run parallel); nodes and other node
/// labels are handled by the leader misread check in label placement, which only orders the candidates:
/// stacked labels in a dense cluster (the bank's teller area) have no spot that avoids every leader.
fn blocks_leaders(item: &ItemRef) -> bool {
    matches!(item.role, ItemRole::EdgeLabel | ItemRole::ZoneLabel | ItemRole::Badge)
}

/// Weight of covering an obstacle in [`candidate_cost`].
fn obstacle_weight(item: &ItemRef) -> f64 {
    match item.role {
        ItemRole::ZoneLabel => ZONE_LABEL_COVER_WEIGHT,
        ItemRole::Node | ItemRole::Badge => NODE_COVER_WEIGHT,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: ItemRole, id: &str) -> ItemRef {
        ItemRef { role, id: id.to_owned() }
    }

    fn cost_against<T>(candidate: &Candidate<T>, layers: &[Layer<'_>], ignore: impl Fn(&ItemRef) -> bool) -> f64 {
        costs_against(std::slice::from_ref(candidate), layers, ignore)[0]
    }

    #[test]
    fn shared_queries_price_like_one_query_per_candidate() {
        let obstacles: Vec<Obstacle> = (0..60)
            .map(|index| {
                let x = f64::from(index % 10) * 23.0;
                let y = f64::from(index / 10) * 17.0;
                let role = if index % 3 == 0 { ItemRole::Node } else { ItemRole::Edge };
                Obstacle { rect: Rect::new(x, y, x + 15.0, y + 4.0), item: item(role, &index.to_string()) }
            })
            .collect();
        let index = ObstacleIndex::new(obstacles);
        let layers = [Layer::Indexed(&index)];
        let candidates: Vec<Candidate<()>> = (0..40)
            .map(|step| {
                let x = f64::from(step) * 5.5;
                Candidate::new(Rect::new(x, 10.0 + f64::from(step % 4), x + 40.0, 30.0), ()).with_leader(Rect::new(
                    x + 19.0,
                    30.0,
                    x + 21.0,
                    70.0,
                ))
            })
            .collect();
        let ignore = |other: &ItemRef| other.id == "7";
        let shared = costs_against(&candidates, &layers, ignore);
        for (candidate, cost) in candidates.iter().zip(shared) {
            let alone = candidate_cost(candidate, &obstacles_near(&layers, &candidate.reach(), &ignore));
            assert_eq!(cost.to_bits(), alone.to_bits());
        }
    }

    #[test]
    fn index_finds_the_same_obstacles_as_a_scan() {
        let obstacles: Vec<Obstacle> = (0..40)
            .map(|index| {
                let x = f64::from(index % 8) * 50.0;
                let y = f64::from(index / 8) * 70.0;
                Obstacle { rect: Rect::new(x, y, x + 30.0, y + 12.0), item: item(ItemRole::Edge, &index.to_string()) }
            })
            .collect();
        let index = ObstacleIndex::new(obstacles.clone());
        let probe = Rect::new(95.0, 60.0, 260.0, 160.0);
        let ignore = |_: &ItemRef| false;
        let scanned = obstacles_near(&[Layer::Loose(&obstacles)], &probe, &ignore);
        let indexed = obstacles_near(&[Layer::Indexed(&index)], &probe, &ignore);
        let touching = |found: &[&Obstacle]| {
            found.iter().filter(|o| probe.overlap_area(&o.rect, COLLISION_MARGIN_PX) > 0.0).count()
        };
        assert!(touching(&scanned) > 0);
        assert_eq!(touching(&scanned), touching(&indexed));
    }

    #[test]
    fn first_free_candidate_wins_without_score() {
        let mut collisions = Collisions::default();
        let obstacles = [Obstacle { rect: Rect::new(0.0, 0.0, 10.0, 10.0), item: item(ItemRole::Node, "a") }];
        let owner = Owner { item: item(ItemRole::NodeLabel, "b"), path: "nodes[1].label" };
        let candidates = vec![
            Candidate::new(Rect::new(5.0, 5.0, 15.0, 15.0), 0),
            Candidate::new(Rect::new(20.0, 20.0, 30.0, 30.0), 1),
        ];
        let chosen = collisions.pick(&owner, candidates, &[Layer::Loose(&obstacles)], |_| false);
        assert_eq!(chosen.map(|candidate| candidate.value), Some(1));
        assert_eq!(collisions.report.score, 0.0);
        assert!(collisions.diagnostics.is_empty());
    }

    #[test]
    fn a_choice_cost_alone_is_not_reported_as_an_overlap() {
        let mut collisions = Collisions::default();
        let obstacles = [Obstacle { rect: Rect::new(0.0, 0.0, 10.0, 10.0), item: item(ItemRole::Node, "a") }];
        let owner = Owner { item: item(ItemRole::EdgeLabel, "x"), path: "edges[0].label" };
        let mut misread = Candidate::new(Rect::new(40.0, 40.0, 50.0, 50.0), ());
        misread.fixed_cost = 25.0;
        assert!(collisions.pick(&owner, vec![misread], &[Layer::Loose(&obstacles)], |_| false).is_some());
        assert_eq!(collisions.report.score, 0.0);
        assert!(collisions.report.overlaps.is_empty());
        assert!(collisions.diagnostics.is_empty());
    }

    #[test]
    fn leaders_cross_strokes_freely_but_not_pills_or_badges() {
        let obstacles = [
            Obstacle { rect: Rect::new(-20.0, 40.0, 20.0, 42.0), item: item(ItemRole::Edge, "e") },
            Obstacle { rect: Rect::new(-30.0, 60.0, 30.0, 80.0), item: item(ItemRole::EdgeLabel, "p") },
            Obstacle { rect: Rect::new(-8.0, 90.0, 8.0, 106.0), item: item(ItemRole::Badge, "n") },
        ];
        let label = Rect::new(40.0, 0.0, 100.0, 28.0);
        let leader = |x: f64| Rect::new(x - 1.0, 28.0, x + 1.0, 120.0);
        let layers = [Layer::Loose(&obstacles)];
        let clear = Candidate::new(label, ()).with_leader(leader(70.0));
        assert_eq!(cost_against(&clear, &layers, |_| false), 0.0);
        let only_stroke = Candidate::new(label, ()).with_leader(Rect::new(-1.0, 28.0, 1.0, 50.0));
        assert_eq!(cost_against(&only_stroke, &layers, |_| false), 0.0, "a leader may cross a stroke");
        let mut collisions = Collisions::default();
        let owner = Owner { item: item(ItemRole::NodeLabel, "n"), path: "nodes[0].label" };
        let crossing = Candidate::new(label, ()).with_leader(leader(0.0));
        assert!(cost_against(&crossing, &layers, |_| false) > 0.0);
        assert!(collisions.pick(&owner, vec![crossing], &layers, |_| false).is_some());
        let crossed: Vec<(&str, &str)> = collisions
            .report
            .overlaps
            .iter()
            .map(|overlap| (overlap.a.role.as_str(), overlap.b.role.as_str()))
            .collect();
        assert_eq!(crossed, [("leader", "edge-label"), ("leader", "badge")]);
        assert!(collisions.diagnostics.iter().all(|d| d.message.contains("(leader line)")));
    }

    #[test]
    fn a_misread_label_joins_the_score_with_its_reason() {
        let mut collisions = Collisions::default();
        let (pill, other) = (item(ItemRole::EdgeLabel, "a->b"), item(ItemRole::Edge, "c->d"));
        let message = "edge-label 'a->b' is nearer edge 'c->d' and reads as its label".to_owned();
        collisions.record_misread("edges[0].label", pill.clone(), other.clone(), 1200.0, message.clone());
        collisions.record_misread("edges[0].label", pill.clone(), other.clone(), 0.0, message.clone());
        assert_eq!(collisions.report.score, 1200.0);
        assert_eq!(collisions.report.overlaps, [Overlap { a: pill, b: other, area: 1200.0 }]);
        let [finding] = &collisions.diagnostics.iter().collect::<Vec<_>>()[..] else { panic!("one diagnostic") };
        assert_eq!((finding.code, finding.message.as_str()), (DiagnosticCode::LabelOverlap, message.as_str()));
    }

    #[test]
    fn residual_overlap_is_scored_and_reported_once_per_item() {
        let mut collisions = Collisions::default();
        let obstacles = [
            Obstacle { rect: Rect::new(0.0, 0.0, 10.0, 10.0), item: item(ItemRole::Edge, "e") },
            Obstacle { rect: Rect::new(10.0, 0.0, 20.0, 10.0), item: item(ItemRole::Edge, "e") },
        ];
        let owner = Owner { item: item(ItemRole::EdgeLabel, "x"), path: "edges[0].label" };
        let candidates = vec![Candidate::new(Rect::new(2.0, 2.0, 18.0, 8.0), ())];
        assert!(collisions.pick(&owner, candidates, &[Layer::Loose(&obstacles)], |_| false).is_some());
        assert!(collisions.report.score > 0.0);
        assert_eq!(collisions.report.overlaps.len(), 1);
        assert_eq!(collisions.diagnostics.len(), 1);
        assert!(collisions.diagnostics.iter().all(|d| d.code == DiagnosticCode::LabelOverlap));
        let index = ObstacleIndex::new(obstacles.to_vec());
        let ignored = collisions.pick(
            &owner,
            vec![Candidate::new(Rect::new(2.0, 2.0, 18.0, 8.0), ())],
            &[Layer::Indexed(&index)],
            |item| item.id == "e",
        );
        assert!(ignored.is_some());
        assert_eq!(collisions.report.overlaps.len(), 1);
    }
}
