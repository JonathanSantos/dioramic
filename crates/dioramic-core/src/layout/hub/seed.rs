//! Constructive seed of the top level (cluster growth, as in VLSI constructive placement and the "place the
//! core first" step of HOLA): the busiest item goes in the middle cell, then, one by one, the item most connected
//! to what is already placed takes the free neighbouring cell with the shortest wiring, the fewest edges
//! pointing back against the flow and the squarest bounding box.

use std::collections::BTreeSet;

/// A cell of the seed grid, in item units (`x` along the flow).
pub(super) type SeedCell = (i32, i32);

/// Weight of an edge pointing against the flow, per cell it runs backwards, relative to one cell of wire. Edges
/// that run across the flow are free: that is what lets a long chain fold into a block.
const FLOW_PENALTY: f64 = 1.0;
/// Weights of the longer side of the bounding box, which keeps the seed square instead of a long line: the hub
/// layout tries each (a loose and a tight seed) and keeps the cheaper drawing.
pub(super) const SPREAD_VARIANTS: [f64; 2] = [0.4, 1.2];

/// What the seed needs to know about the top-level items.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SeedInput {
    /// `weights[a][b]`: edges between items `a` and `b` (symmetric, zero diagonal).
    pub weights: Vec<Vec<f64>>,
    /// Mean flow slot of each item (`None` for an empty zone).
    pub slots: Vec<Option<f64>>,
    /// Weight of the longer side of the bounding box (see [`SPREAD_VARIANTS`]).
    pub spread: f64,
}

/// Cell of every item, indexed like the input.
pub(super) fn seed_cells(input: &SeedInput) -> Vec<SeedCell> {
    let count = input.weights.len();
    let totals: Vec<f64> = input.weights.iter().map(|row| row.iter().sum()).collect();
    let mut cells: Vec<Option<SeedCell>> = vec![None; count];
    let mut occupied: BTreeSet<SeedCell> = BTreeSet::new();
    for _ in 0..count {
        let Some(item) = next_item(input, &totals, &cells) else { break };
        let cell = if occupied.is_empty() { (0, 0) } else { best_cell(input, item, &cells, &occupied) };
        cells[item] = Some(cell);
        occupied.insert(cell);
    }
    cells.into_iter().map(|cell| cell.unwrap_or_default()).collect()
}

/// The unplaced item most connected to the placed ones; ties → busiest overall → declared first.
fn next_item(input: &SeedInput, totals: &[f64], cells: &[Option<SeedCell>]) -> Option<usize> {
    let attachment = |item: usize| -> f64 {
        cells.iter().enumerate().filter(|(_, cell)| cell.is_some()).map(|(other, _)| input.weights[item][other]).sum()
    };
    (0..cells.len())
        .filter(|&item| cells[item].is_none())
        .fold(None, |best: Option<(usize, f64, f64)>, item| {
            let key = (attachment(item), totals[item]);
            match best {
                Some((_, attached, total)) if (attached, total) >= key => best,
                _ => Some((item, key.0, key.1)),
            }
        })
        .map(|(item, _, _)| item)
}

fn best_cell(input: &SeedInput, item: usize, cells: &[Option<SeedCell>], occupied: &BTreeSet<SeedCell>) -> SeedCell {
    let candidates: BTreeSet<SeedCell> = occupied
        .iter()
        .flat_map(|&(x, y)| [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])
        .filter(|cell| !occupied.contains(cell))
        .collect();
    let mut best: Option<(f64, i32, SeedCell)> = None;
    for cell in candidates {
        let cost = cell_cost(input, item, cell, cells, occupied);
        let distance = cell.0.abs() + cell.1.abs();
        let better = match best {
            None => true,
            Some((best_cost, best_distance, _)) => {
                cost < best_cost - f64::EPSILON
                    || ((cost - best_cost).abs() <= f64::EPSILON && distance < best_distance)
            }
        };
        if better {
            best = Some((cost, distance, cell));
        }
    }
    best.map_or((0, 0), |(_, _, cell)| cell)
}

fn cell_cost(
    input: &SeedInput,
    item: usize,
    cell: SeedCell,
    cells: &[Option<SeedCell>],
    occupied: &BTreeSet<SeedCell>,
) -> f64 {
    let mut cost = 0.0;
    for (other, placed) in cells.iter().enumerate() {
        let (Some(placed), weight) = (placed, input.weights[item][other]) else { continue };
        if weight <= 0.0 {
            continue;
        }
        let (dx, dy) = (f64::from(cell.0 - placed.0), f64::from(cell.1 - placed.1));
        cost += weight * dx.hypot(dy);
        if let (Some(mine), Some(theirs)) = (input.slots[item], input.slots[other])
            && (mine - theirs).abs() > f64::EPSILON
        {
            let downstream = (mine - theirs).signum();
            cost += FLOW_PENALTY * weight * (-downstream * dx).max(0.0);
        }
    }
    let (mut x0, mut x1, mut y0, mut y1) = (cell.0, cell.0, cell.1, cell.1);
    for &(x, y) in occupied {
        (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
    }
    cost + input.spread * f64::from((x1 - x0).max(y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(leaves: usize) -> SeedInput {
        let count = leaves + 1;
        let mut weights = vec![vec![0.0; count]; count];
        for row in weights.iter_mut().skip(1) {
            row[0] = 1.0;
        }
        for weight in weights[0].iter_mut().skip(1) {
            *weight = 1.0;
        }
        SeedInput { weights, slots: vec![None; count], spread: SPREAD_VARIANTS[0] }
    }

    #[test]
    fn the_hub_takes_the_middle_and_leaves_surround_it() {
        let cells = seed_cells(&star(4));
        assert_eq!(cells[0], (0, 0));
        let mut leaves: Vec<SeedCell> = cells[1..].to_vec();
        leaves.sort_unstable();
        assert_eq!(leaves, vec![(-1, 0), (0, -1), (0, 1), (1, 0)]);
    }

    #[test]
    fn edges_never_point_back_against_the_flow() {
        let mut input = star(2);
        input.slots = vec![Some(1.0), Some(0.0), Some(2.0)];
        let cells = seed_cells(&input);
        assert!(cells[1].0 <= cells[0].0, "the source never goes after the hub: {cells:?}");
        assert!(cells[2].0 >= cells[0].0, "the sink never goes before the hub: {cells:?}");
        assert!(cells[2].0 > cells[1].0, "the sink goes after the source: {cells:?}");
    }

    #[test]
    fn seed_is_deterministic_and_never_stacks_items() {
        let input = star(7);
        let cells = seed_cells(&input);
        assert_eq!(cells, seed_cells(&input));
        let unique: BTreeSet<SeedCell> = cells.iter().copied().collect();
        assert_eq!(unique.len(), cells.len());
    }
}
