//! A tiny dense quadratic program: minimize `Σ wᵢ (tᵢ − aᵢ)² + Σ (t_p − t_q − c)²` subject to separations
//! `t_after − t_before ≥ gap`. Solved by an exterior penalty with an active set (semi-smooth Newton): each step
//! solves the linear system of the objective plus a stiff spring on every violated separation, until the set of
//! violated separations stops changing; the stiffness then grows. Exact enough for the few dozen top-level
//! blocks of a diagram, and deterministic.

/// Penalty stiffnesses, relative to one link.
const STIFFNESS: [f64; 3] = [1e3, 1e6, 1e9];
/// Active-set steps per stiffness.
const MAX_ACTIVE_SET_STEPS: usize = 40;
/// A separation counts as violated past this slack (tiles).
const VIOLATION_SLACK: f64 = 1e-12;
/// Pivots smaller than this make the system singular (cannot happen with positive anchor weights).
const SINGULAR_PIVOT: f64 = 1e-300;

/// `t_after − t_before ≥ gap`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Separation {
    /// Variable that comes first.
    pub before: usize,
    /// Variable that comes after.
    pub after: usize,
    /// Smallest difference.
    pub gap: f64,
}

/// The quadratic objective, as its normal equations `H t = r`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct QuadraticProgram {
    hessian: Vec<Vec<f64>>,
    rhs: Vec<f64>,
}

impl QuadraticProgram {
    /// A program over `size` variables with no terms.
    pub fn new(size: usize) -> Self {
        Self { hessian: vec![vec![0.0; size]; size], rhs: vec![0.0; size] }
    }

    /// Adds `weight · (t_i − target)²`.
    pub fn add_anchor(&mut self, variable: usize, target: f64, weight: f64) {
        self.hessian[variable][variable] += weight;
        self.rhs[variable] += weight * target;
    }

    /// Adds `weight · (t_p − t_q − offset)²`.
    pub fn add_difference(&mut self, p: usize, q: usize, offset: f64, weight: f64) {
        if p == q {
            return;
        }
        self.hessian[p][p] += weight;
        self.hessian[q][q] += weight;
        self.hessian[p][q] -= weight;
        self.hessian[q][p] -= weight;
        self.rhs[p] += weight * offset;
        self.rhs[q] -= weight * offset;
    }

    /// Minimizer subject to `separations` (up to the final stiffness; callers legalize the rest).
    pub fn solve(&self, separations: &[Separation]) -> Vec<f64> {
        let mut active: Vec<bool> = vec![false; separations.len()];
        let mut values = self.solve_with(separations, &active, 0.0);
        for stiffness in STIFFNESS {
            for _ in 0..MAX_ACTIVE_SET_STEPS {
                let violated: Vec<bool> = separations
                    .iter()
                    .zip(&active)
                    .map(|(separation, &was)| {
                        let slack = values[separation.after] - values[separation.before] - separation.gap;
                        slack < -VIOLATION_SLACK || (was && slack <= 0.0)
                    })
                    .collect();
                values = self.solve_with(separations, &violated, stiffness);
                if violated == active {
                    break;
                }
                active = violated;
            }
        }
        values
    }

    fn solve_with(&self, separations: &[Separation], active: &[bool], stiffness: f64) -> Vec<f64> {
        let mut program = self.clone();
        for (separation, _) in separations.iter().zip(active).filter(|(_, active)| **active) {
            program.add_difference(separation.after, separation.before, separation.gap, stiffness);
        }
        gaussian_elimination(program.hessian, program.rhs)
    }
}

/// Solves `matrix · x = rhs` with partial pivoting (zero where the system is singular).
fn gaussian_elimination(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Vec<f64> {
    let size = rhs.len();
    for column in 0..size {
        let pivot = (column..size)
            .fold(column, |best, row| if matrix[row][column].abs() > matrix[best][column].abs() { row } else { best });
        if matrix[pivot][column].abs() < SINGULAR_PIVOT {
            continue;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let (done, below) = matrix.split_at_mut(column + 1);
        let pivot_row = &done[column];
        for (offset, row) in below.iter_mut().enumerate() {
            let factor = row[column] / pivot_row[column];
            if factor == 0.0 {
                continue;
            }
            for (value, pivot_value) in row[column..].iter_mut().zip(&pivot_row[column..]) {
                *value -= factor * pivot_value;
            }
            rhs[column + 1 + offset] -= factor * rhs[column];
        }
    }
    let mut solution = vec![0.0; size];
    for row in (0..size).rev() {
        let known: f64 = (row + 1..size).map(|inner| matrix[row][inner] * solution[inner]).sum();
        solution[row] =
            if matrix[row][row].abs() < SINGULAR_PIVOT { 0.0 } else { (rhs[row] - known) / matrix[row][row] };
    }
    solution
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconstrained_links_meet_between_their_anchors() {
        let mut program = QuadraticProgram::new(2);
        program.add_anchor(0, 0.0, 1.0);
        program.add_anchor(1, 4.0, 1.0);
        program.add_difference(0, 1, 0.0, 1.0);
        let solution = program.solve(&[]);
        assert!((solution[0] - 4.0 / 3.0).abs() < 1e-9 && (solution[1] - 8.0 / 3.0).abs() < 1e-9, "{solution:?}");
    }

    #[test]
    fn separations_hold_and_inactive_ones_do_not_pull() {
        let mut program = QuadraticProgram::new(3);
        for (variable, anchor) in [0.0, 1.0, 10.0].into_iter().enumerate() {
            program.add_anchor(variable, anchor, 1.0);
        }
        let separations = [Separation { before: 0, after: 1, gap: 3.0 }, Separation { before: 1, after: 2, gap: 1.0 }];
        let solution = program.solve(&separations);
        assert!((solution[1] - solution[0] - 3.0).abs() < 1e-6, "{solution:?}");
        assert!((solution[2] - 10.0).abs() < 1e-6, "the slack separation leaves the third alone: {solution:?}");
    }
}
