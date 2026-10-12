//! Local repair of the greedy label placement (T-370).
//!
//! The greedy trials only change *which label claims a spot first*. Around a hub that is not always enough:
//! a pill and a node label can each have a free spot that the other one is sitting on. After the best trial,
//! every label still in a residual overlap is moved alone, then together with one neighbouring label, to the
//! candidate pair that lowers the weighted cost the most. Candidates, prices and preferences are the greedy
//! ones (same lists, same fixed costs, misread leaders stay out unless the label already has one), so a
//! repaired label lands on a spot the greedy could have picked.

use crate::geometry::{Rect, ScreenPoint};

use super::super::COLLISION_MARGIN_PX;
use super::super::collision::{Candidate, LEADER_MARGIN_PX, Obstacle};
use super::attribution::{self, MISATTRIBUTION_CHOICE_WEIGHT, NEARER_SLACK_SHARE};
use super::{LEADER_HALF_WIDTH_PX, LEADER_MISREAD_TOLERANCE_PX, LeaderCheck};

/// Rounds of single and pair moves (each round only runs when the previous one improved).
const MAX_REPAIR_ROUNDS: usize = 4;

/// Smallest cost drop accepted as an improvement (avoids cycling on float noise).
const MIN_IMPROVEMENT: f64 = 1e-6;

/// A label the repair may move: its priced candidates and the one it currently uses.
#[derive(Debug, Clone)]
pub(super) struct Movable<'c> {
    /// Candidates in greedy preference order, with their fixed costs.
    pub(super) candidates: &'c [Candidate<()>],
    /// Index of the current candidate.
    pub(super) chosen: usize,
    /// Node labels: the leader foot and the node index (for the leader misread check).
    pub(super) leader: Option<LeaderFoot>,
    /// True when the greedy result reported this label in a residual overlap: only those are repaired
    /// (a label that only pays a choice penalty is where the greedy wanted it).
    pub(super) in_overlap: bool,
    /// Pills: gap (px) between each candidate and the own edge, to tell when a node label nearby would
    /// take the pill over (T-394). Empty for node labels.
    pub(super) own_gaps: &'c [f64],
}

/// Where a node label's leader starts.
#[derive(Debug, Clone, Copy)]
pub(super) struct LeaderFoot {
    /// Node index (into the scene nodes).
    pub(super) node: usize,
    /// Leader start (top of the node).
    pub(super) anchor: ScreenPoint,
}

/// The boxes a label occupies at one candidate: its box and, for node labels, its leader line.
#[derive(Debug, Clone, Copy)]
struct Shapes {
    label: Rect,
    leader: Option<Rect>,
    /// Pills: gap to the own edge.
    own_gap: Option<f64>,
}

impl Movable<'_> {
    fn shapes(&self, at: usize) -> Shapes {
        let label = self.candidates[at].rect;
        let leader = self.leader.map(|foot| leader_rect(foot.anchor, label));
        Shapes { label, leader, own_gap: self.own_gaps.get(at).copied() }
    }

    fn current(&self) -> Shapes {
        self.shapes(self.chosen)
    }
}

/// Leader box from `anchor` up to the bottom of `label` (same box the greedy places as an obstacle).
pub(super) fn leader_rect(anchor: ScreenPoint, label: Rect) -> Rect {
    Rect::from_points([anchor, ScreenPoint::new(anchor.x, label.y1)]).inflate(LEADER_HALF_WIDTH_PX)
}

/// Overlap between two placed labels: box against box and each box against the other's leader (leaders
/// never collide with each other, as in the greedy), plus the choice cost of either one reading as the
/// other's (T-394).
fn interaction(a: Shapes, b: Shapes) -> f64 {
    let mut area = a.label.overlap_area(&b.label, COLLISION_MARGIN_PX);
    if let Some(leader) = b.leader {
        area += a.label.overlap_area(&leader, LEADER_MARGIN_PX);
    }
    if let Some(leader) = a.leader {
        area += b.label.overlap_area(&leader, LEADER_MARGIN_PX);
    }
    area + MISATTRIBUTION_CHOICE_WEIGHT * (misread_area(a, b) + misread_area(b, a))
}

/// Area of the label that reads wrong because of `other`: `other`'s label when the leader of `label` runs
/// behind it, or the pill `label` when the node label `other` is nearer to it than its own edge.
fn misread_area(label: Shapes, other: Shapes) -> f64 {
    if let Some(leader) = label.leader {
        let x = leader.center().x;
        let (foot, end) = (ScreenPoint::new(x, leader.y1), ScreenPoint::new(x, leader.y0));
        return if attribution::leader_runs_behind(foot, end, &other.label) { other.label.area() } else { 0.0 };
    }
    match label.own_gap {
        Some(own_gap) if other.leader.is_some() => {
            let nearer = own_gap - NEARER_SLACK_SHARE * label.label.height();
            if nearer > 0.0 && attribution::gap_between(&label.label, &other.label) < nearer {
                label.label.area()
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

/// Repairs `movables` in place; `leaders` vets the leader of node label candidates.
pub(super) fn repair(movables: &mut [Movable<'_>], leaders: &LeaderCheck<'_>) {
    let mut costs = CostTable::new(movables.len());
    for _ in 0..MAX_REPAIR_ROUNDS {
        let mut improved = false;
        for index in conflicted(movables, &mut costs) {
            if move_alone(movables, index, leaders, &mut costs) {
                improved = true;
                costs = CostTable::new(movables.len());
            }
        }
        for index in conflicted(movables, &mut costs) {
            for neighbour in neighbours(movables, index) {
                if move_pair(movables, index, neighbour, leaders, &mut costs) {
                    improved = true;
                    costs = CostTable::new(movables.len());
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Cost of every candidate of a label against all the others where they are now, computed on demand and
/// dropped whenever a label moves (the same labels are looked at many times).
struct CostTable {
    rows: Vec<Option<Vec<f64>>>,
}

impl CostTable {
    fn new(count: usize) -> Self {
        Self { rows: vec![None; count] }
    }

    fn row(&mut self, movables: &[Movable<'_>], index: usize) -> &[f64] {
        self.rows[index].get_or_insert_with(|| {
            // Labels out of reach of every candidate add nothing: only the neighbours are measured.
            let near = neighbours(movables, index);
            (0..movables[index].candidates.len())
                .map(|at| {
                    let shapes = movables[index].shapes(at);
                    let others: f64 = near.iter().map(|&other| interaction(shapes, movables[other].current())).sum();
                    movables[index].candidates[at].fixed_cost + others
                })
                .collect()
        })
    }
}

/// Labels reported in a residual overlap whose current spot still costs anything.
fn conflicted(movables: &[Movable<'_>], costs: &mut CostTable) -> Vec<usize> {
    (0..movables.len())
        .filter(|&index| movables[index].in_overlap && costs.row(movables, index)[movables[index].chosen] > 0.0)
        .collect()
}

/// Labels whose current boxes touch the area any candidate of `index` could take.
fn neighbours(movables: &[Movable<'_>], index: usize) -> Vec<usize> {
    let reach = movables[index]
        .candidates
        .iter()
        .fold(Rect::EMPTY, |bounds, candidate| bounds.union(candidate.rect))
        .inflate(2.0 * COLLISION_MARGIN_PX);
    (0..movables.len())
        .filter(|&other| other != index)
        .filter(|&other| {
            let shapes = movables[other].current();
            reach.intersects(&shapes.label) || shapes.leader.is_some_and(|leader| reach.intersects(&leader))
        })
        .collect()
}

/// True when the candidate's leader reads well, or the label already has a misread leader (then any
/// candidate is as acceptable as the current one). Pills always pass.
fn leader_reads_well(movables: &[Movable<'_>], index: usize, at: usize, leaders: &LeaderCheck<'_>) -> bool {
    let movable = &movables[index];
    let Some(foot) = movable.leader else { return true };
    let placed: Vec<Obstacle> = movables
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .flat_map(|(_, other)| {
            let shapes = other.current();
            [Some(shapes.label), shapes.leader].into_iter().flatten()
        })
        .map(|rect| Obstacle { rect, item: placeholder_item() })
        .collect();
    let misread = |candidate: usize| {
        let end = ScreenPoint::new(foot.anchor.x, movable.candidates[candidate].rect.y1);
        leaders.fixed_misread_length(foot.node, foot.anchor, end)
            + LeaderCheck::across_labels(foot.anchor, end, &placed)
            > LEADER_MISREAD_TOLERANCE_PX
    };
    !misread(at) || misread(movable.chosen)
}

/// Obstacles built for the leader check only need a box; their role must not be a node label's leader
/// (those are skipped when thin), so a pill role is used.
fn placeholder_item() -> super::ItemRef {
    super::ItemRef { role: crate::canvas::ItemRole::EdgeLabel, id: String::new() }
}

/// Moves label `index` to its cheapest candidate (earliest on ties) when that lowers its cost.
fn move_alone(movables: &mut [Movable<'_>], index: usize, leaders: &LeaderCheck<'_>, costs: &mut CostTable) -> bool {
    let chosen = movables[index].chosen;
    let row = costs.row(movables, index);
    let mut best = (row[chosen], chosen);
    for (at, &cost) in row.iter().enumerate() {
        if cost < best.0 - MIN_IMPROVEMENT && leader_reads_well(movables, index, at, leaders) {
            best = (cost, at);
        }
    }
    if best.1 == chosen {
        return false;
    }
    movables[index].chosen = best.1;
    true
}

/// Moves labels `a` and `b` together to the pair of candidates with the lowest joint cost, when that is
/// lower than the current one.
fn move_pair(
    movables: &mut [Movable<'_>],
    a: usize,
    b: usize,
    leaders: &LeaderCheck<'_>,
    costs: &mut CostTable,
) -> bool {
    let joint = |at_a: usize, at_b: usize| interaction(movables[a].shapes(at_a), movables[b].shapes(at_b));
    let (chosen_a, chosen_b) = (movables[a].chosen, movables[b].chosen);
    let (now_a, now_b) = (movables[a].current(), movables[b].current());
    // Cost of one label without the other one: the full cost minus their current interaction.
    let without_a = costs.row(movables, b)[chosen_b] - interaction(now_b, now_a);
    let without_b = costs.row(movables, a)[chosen_a] - interaction(now_a, now_b);
    let current = without_b + without_a + joint(chosen_a, chosen_b);
    // Only candidates cheaper than the current pair on their own can be part of a better pair.
    let mut ranked = |index: usize, other: Shapes| -> Vec<(f64, usize)> {
        let row = costs.row(movables, index);
        let mut ranked: Vec<(f64, usize)> = row
            .iter()
            .enumerate()
            .map(|(at, cost)| (cost - interaction(movables[index].shapes(at), other), at))
            .filter(|(cost, _)| *cost < current - MIN_IMPROVEMENT)
            .collect();
        ranked.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        ranked
    };
    let (ranked_a, ranked_b) = (ranked(a, now_b), ranked(b, now_a));
    let Some(cheapest_b) = ranked_b.first().map(|(cost, _)| *cost) else { return false };
    let mut best = (current, chosen_a, chosen_b);
    for &(cost_a, at_a) in &ranked_a {
        if cost_a + cheapest_b >= best.0 - MIN_IMPROVEMENT {
            break;
        }
        let mut a_reads_well = None;
        for &(cost_b, at_b) in &ranked_b {
            if cost_a + cost_b >= best.0 - MIN_IMPROVEMENT {
                break;
            }
            let total = cost_a + cost_b + joint(at_a, at_b);
            if total >= best.0 - MIN_IMPROVEMENT {
                continue;
            }
            // Leader checks are the expensive part: only for a pair that would win.
            if !*a_reads_well.get_or_insert_with(|| leader_reads_well(movables, a, at_a, leaders)) {
                break;
            }
            if leader_reads_well(movables, b, at_b, leaders) {
                best = (total, at_a, at_b);
            }
        }
    }
    if (best.1, best.2) == (chosen_a, chosen_b) {
        return false;
    }
    movables[a].chosen = best.1;
    movables[b].chosen = best.2;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(rects: &[Rect]) -> Vec<Candidate<()>> {
        rects.iter().map(|rect| Candidate::new(*rect, ())).collect()
    }

    fn no_leaders() -> LeaderCheck<'static> {
        LeaderCheck { covers: Vec::new(), edges: Vec::new(), fixed: Vec::new() }
    }

    #[test]
    fn a_label_moves_alone_to_a_free_spot() {
        let first = candidates(&[Rect::new(0.0, 0.0, 10.0, 10.0)]);
        let second = candidates(&[Rect::new(2.0, 2.0, 12.0, 12.0), Rect::new(40.0, 0.0, 50.0, 10.0)]);
        let mut movables = vec![
            Movable { candidates: &first, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
            Movable { candidates: &second, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
        ];
        repair(&mut movables, &no_leaders());
        assert_eq!(movables[1].chosen, 1);
        assert!(conflicted(&movables, &mut CostTable::new(movables.len())).is_empty());
    }

    #[test]
    fn two_labels_move_together_when_neither_can_move_alone() {
        // `a` at P overlaps `b` at R a little. Alone, `a` to Q or `b` to S overlaps more; together (Q, S) is free.
        let (p, q) = (Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(20.0, 0.0, 30.0, 10.0));
        let (r, s) = (Rect::new(8.0, 0.0, 25.0, 10.0), Rect::new(-5.0, 0.0, 5.0, 10.0));
        let (a, b) = (candidates(&[p, q]), candidates(&[r, s]));
        let mut movables = vec![
            Movable { candidates: &a, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
            Movable { candidates: &b, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
        ];
        let mut costs = CostTable::new(movables.len());
        assert!(!move_alone(&mut movables, 0, &no_leaders(), &mut costs));
        assert!(!move_alone(&mut movables, 1, &no_leaders(), &mut costs));
        repair(&mut movables, &no_leaders());
        assert_eq!((movables[0].chosen, movables[1].chosen), (1, 1));
        assert!(conflicted(&movables, &mut CostTable::new(movables.len())).is_empty());
    }

    #[test]
    fn a_fixed_penalty_is_never_traded_for_a_worse_overlap() {
        let mut penalised = candidates(&[Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(2.0, 2.0, 12.0, 12.0)]);
        penalised[0].fixed_cost = 5.0;
        let other = candidates(&[Rect::new(4.0, 4.0, 14.0, 14.0)]);
        let mut movables = vec![
            Movable { candidates: &penalised, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
            Movable { candidates: &other, chosen: 0, leader: None, in_overlap: true, own_gaps: &[] },
        ];
        let before = CostTable::new(2).row(&movables, 0)[0];
        repair(&mut movables, &no_leaders());
        assert!(CostTable::new(2).row(&movables, 0)[movables[0].chosen] <= before);
    }
}
