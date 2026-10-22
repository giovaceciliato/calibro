//! Geometric constraint solver.
//!
//! Every constraint becomes one or more residual equations `r_i(x) = 0` over the
//! unknowns `x` (point coordinates and circle radii). The solver minimises
//! `|r(x)|^2` with damped Gauss–Newton (Levenberg–Marquardt). Because the
//! damping keeps steps small, under-constrained sketches move as little as
//! possible from where the user drew them — the behaviour people expect when
//! they drag geometry or change a dimension.
//!
//! After solving, the rank of the Jacobian gives the remaining degrees of
//! freedom, which drives the "fully / under / over defined" sketch status.

use crate::model::{Constraint, CurveId, CurveKind, PointId, Sketch, SketchError};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum SolveStatus {
    /// No remaining degrees of freedom, all constraints satisfied.
    FullyConstrained,
    /// Constraints satisfied; `dof` degrees of freedom remain.
    UnderConstrained { dof: usize },
    /// Constraints satisfied but some are redundant (duplicated information).
    Redundant { dof: usize },
    /// Constraints contradict each other; geometry was left at the best compromise.
    Conflicting { residual: f64 },
}

#[derive(Clone, Debug)]
pub struct SolveReport {
    pub status: SolveStatus,
    pub iterations: usize,
    pub residual: f64,
}

#[derive(Clone, Copy, Debug)]
enum Var {
    X(PointId),
    Y(PointId),
    R(CurveId),
}

struct System {
    vars: Vec<Var>,
    point_index: BTreeMap<PointId, usize>,
    radius_index: BTreeMap<CurveId, usize>,
}

const TOL: f64 = 1e-9;

impl System {
    fn build(sketch: &Sketch) -> Self {
        let mut vars = Vec::new();
        let mut point_index = BTreeMap::new();
        let mut radius_index = BTreeMap::new();
        for id in sketch.points.keys() {
            point_index.insert(*id, vars.len());
            vars.push(Var::X(*id));
            vars.push(Var::Y(*id));
        }
        for (id, c) in &sketch.curves {
            if let CurveKind::Circle { .. } = c.kind {
                radius_index.insert(*id, vars.len());
                vars.push(Var::R(*id));
            }
        }
        Self {
            vars,
            point_index,
            radius_index,
        }
    }

    fn read(&self, sketch: &Sketch) -> Vec<f64> {
        self.vars
            .iter()
            .map(|v| match *v {
                Var::X(p) => sketch.points[&p].x,
                Var::Y(p) => sketch.points[&p].y,
                Var::R(c) => match sketch.curves[&c].kind {
                    CurveKind::Circle { radius, .. } => radius,
                    _ => 0.0,
                },
            })
            .collect()
    }

    fn write(&self, sketch: &mut Sketch, x: &[f64]) {
        for (v, val) in self.vars.iter().zip(x) {
            match *v {
                Var::X(p) => sketch.points.get_mut(&p).unwrap().x = *val,
                Var::Y(p) => sketch.points.get_mut(&p).unwrap().y = *val,
                Var::R(c) => {
                    if let CurveKind::Circle { radius, .. } =
                        &mut sketch.curves.get_mut(&c).unwrap().kind
                    {
                        *radius = *val;
                    }
                }
            }
        }
    }
}

/// Read-only view of the unknowns while evaluating residuals.
struct Eval<'a> {
    sys: &'a System,
    sketch: &'a Sketch,
    x: &'a [f64],
}

#[derive(Clone, Copy)]
enum Shape {
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
}

impl Eval<'_> {
    fn p(&self, id: PointId) -> [f64; 2] {
        let i = self.sys.point_index[&id];
        [self.x[i], self.x[i + 1]]
    }

    fn shape(&self, id: CurveId) -> Shape {
        match self.sketch.curves[&id].kind {
            CurveKind::Line { a, b } => Shape::Line(self.p(a), self.p(b)),
            CurveKind::Circle { center, .. } => {
                Shape::Circle(self.p(center), self.x[self.sys.radius_index[&id]])
            }
            CurveKind::Arc { center, start, .. } => {
                let c = self.p(center);
                let s = self.p(start);
                Shape::Circle(c, dist(c, s))
            }
        }
    }

    fn line(&self, id: CurveId) -> Option<([f64; 2], [f64; 2])> {
        match self.shape(id) {
            Shape::Line(a, b) => Some((a, b)),
            _ => None,
        }
    }

    fn circle(&self, id: CurveId) -> Option<([f64; 2], f64)> {
        match self.shape(id) {
            Shape::Circle(c, r) => Some((c, r)),
            _ => None,
        }
    }

    fn residuals(&self, out: &mut Vec<f64>) {
        out.clear();
        // Implicit equations owned by entities: arcs keep both ends on the circle.
        for c in self.sketch.curves.values() {
            if let CurveKind::Arc { center, start, end } = c.kind {
                let cc = self.p(center);
                out.push(dist(cc, self.p(end)) - dist(cc, self.p(start)));
            }
        }
        for c in self.sketch.constraints.values() {
            self.constraint_residuals(c, out);
        }
    }

    fn constraint_residuals(&self, c: &Constraint, out: &mut Vec<f64>) {
        match *c {
            Constraint::Coincident { a, b } => {
                let (pa, pb) = (self.p(a), self.p(b));
                out.push(pa[0] - pb[0]);
                out.push(pa[1] - pb[1]);
            }
            Constraint::Fixed { point, x, y } => {
                let p = self.p(point);
                out.push(p[0] - x);
                out.push(p[1] - y);
            }
            Constraint::PointOnCurve { point, curve } => {
                let p = self.p(point);
                match self.shape(curve) {
                    Shape::Line(a, b) => out.push(signed_line_dist(p, a, b)),
                    Shape::Circle(cc, r) => out.push(dist(p, cc) - r),
                }
            }
            Constraint::Horizontal { line } => {
                if let Some((a, b)) = self.line(line) {
                    out.push(b[1] - a[1]);
                }
            }
            Constraint::Vertical { line } => {
                if let Some((a, b)) = self.line(line) {
                    out.push(b[0] - a[0]);
                }
            }
            Constraint::Parallel { a, b } => {
                if let (Some(l1), Some(l2)) = (self.line(a), self.line(b)) {
                    let (d1, d2) = (unit(sub(l1.1, l1.0)), unit(sub(l2.1, l2.0)));
                    out.push(cross(d1, d2));
                }
            }
            Constraint::Perpendicular { a, b } => {
                if let (Some(l1), Some(l2)) = (self.line(a), self.line(b)) {
                    let (d1, d2) = (unit(sub(l1.1, l1.0)), unit(sub(l2.1, l2.0)));
                    out.push(dot(d1, d2));
                }
            }
            Constraint::Tangent { a, b } => match (self.shape(a), self.shape(b)) {
                (Shape::Line(p, q), Shape::Circle(c, r))
                | (Shape::Circle(c, r), Shape::Line(p, q)) => {
                    out.push(signed_line_dist(c, p, q).abs() - r);
                }
                (Shape::Circle(c1, r1), Shape::Circle(c2, r2)) => {
                    // External tangency unless one circle contains the other's center.
                    let d = dist(c1, c2);
                    if d < r1.max(r2) {
                        out.push(d - (r1 - r2).abs());
                    } else {
                        out.push(d - (r1 + r2));
                    }
                }
                _ => {}
            },
            Constraint::Equal { a, b } => match (self.shape(a), self.shape(b)) {
                (Shape::Line(p1, q1), Shape::Line(p2, q2)) => out.push(dist(p1, q1) - dist(p2, q2)),
                (Shape::Circle(_, r1), Shape::Circle(_, r2)) => out.push(r1 - r2),
                _ => {}
            },
            Constraint::Concentric { a, b } => {
                if let (Some((c1, _)), Some((c2, _))) = (self.circle(a), self.circle(b)) {
                    out.push(c1[0] - c2[0]);
                    out.push(c1[1] - c2[1]);
                }
            }
            Constraint::Midpoint { point, line } => {
                if let Some((a, b)) = self.line(line) {
                    let p = self.p(point);
                    out.push(p[0] - 0.5 * (a[0] + b[0]));
                    out.push(p[1] - 0.5 * (a[1] + b[1]));
                }
            }
            Constraint::Symmetric { a, b, axis } => {
                if let Some((p, q)) = self.line(axis) {
                    let (pa, pb) = (self.p(a), self.p(b));
                    let d = unit(sub(q, p));
                    // segment a-b perpendicular to axis, midpoint on axis
                    out.push(dot(sub(pb, pa), d));
                    let m = [(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5];
                    out.push(signed_line_dist(m, p, q));
                }
            }
            Constraint::Distance { a, b, value } => out.push(dist(self.p(a), self.p(b)) - value),
            Constraint::HorizontalDistance { a, b, value } => {
                out.push((self.p(b)[0] - self.p(a)[0]).abs() - value)
            }
            Constraint::VerticalDistance { a, b, value } => {
                out.push((self.p(b)[1] - self.p(a)[1]).abs() - value)
            }
            Constraint::Length { line, value } => {
                if let Some((a, b)) = self.line(line) {
                    out.push(dist(a, b) - value);
                }
            }
            Constraint::Angle { a, b, degrees } => {
                if let (Some(l1), Some(l2)) = (self.line(a), self.line(b)) {
                    let (d1, d2) = (unit(sub(l1.1, l1.0)), unit(sub(l2.1, l2.0)));
                    let t = degrees.to_radians();
                    // sin(phi - t) where phi is the angle from d1 to d2
                    out.push(cross(d1, d2) * t.cos() - dot(d1, d2) * t.sin());
                }
            }
            Constraint::Radius { curve, value } => {
                if let Some((_, r)) = self.circle(curve) {
                    out.push(r - value);
                }
            }
            Constraint::Diameter { curve, value } => {
                if let Some((_, r)) = self.circle(curve) {
                    out.push(2.0 * r - value);
                }
            }
        }
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}
fn unit(a: [f64; 2]) -> [f64; 2] {
    let l = (a[0] * a[0] + a[1] * a[1]).sqrt().max(1e-12);
    [a[0] / l, a[1] / l]
}
fn signed_line_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    cross(unit(sub(b, a)), sub(p, a))
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Numerical Jacobian (central differences), row-major m x n.
fn jacobian(sys: &System, sketch: &Sketch, x: &[f64], m: usize) -> Vec<f64> {
    let n = x.len();
    let mut jac = vec![0.0; m * n];
    let mut xp = x.to_vec();
    let mut rp = Vec::with_capacity(m);
    let mut rm = Vec::with_capacity(m);
    for j in 0..n {
        let h = 1e-7 * x[j].abs().max(1.0);
        let orig = xp[j];
        xp[j] = orig + h;
        Eval {
            sys,
            sketch,
            x: &xp,
        }
        .residuals(&mut rp);
        xp[j] = orig - h;
        Eval {
            sys,
            sketch,
            x: &xp,
        }
        .residuals(&mut rm);
        xp[j] = orig;
        for i in 0..m {
            jac[i * n + j] = (rp[i] - rm[i]) / (2.0 * h);
        }
    }
    jac
}

/// Solves the dense symmetric positive system A x = b in place (Gaussian elimination
/// with partial pivoting). Returns None if singular.
fn solve_linear(a: &mut [f64], b: &mut [f64], n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv =
            (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))?;
        if a[piv * n + col].abs() < 1e-300 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(piv * n + k, col * n + k);
            }
            b.swap(piv, col);
        }
        let d = a[col * n + col];
        for row in col + 1..n {
            let f = a[row * n + col] / d;
            if f != 0.0 {
                for k in col..n {
                    a[row * n + k] -= f * a[col * n + k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = b[row];
        for k in row + 1..n {
            s -= a[row * n + k] * x[k];
        }
        x[row] = s / a[row * n + row];
    }
    Some(x)
}

/// Numerical rank of an m x n matrix.
fn rank(mut a: Vec<f64>, m: usize, n: usize) -> usize {
    let scale = a.iter().fold(0.0f64, |s, v| s.max(v.abs())).max(1.0);
    let tol = 1e-8 * scale;
    let mut r = 0;
    for col in 0..n {
        if r == m {
            break;
        }
        let piv = (r..m).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()));
        let Some(piv) = piv else { break };
        if a[piv * n + col].abs() <= tol {
            continue;
        }
        for k in 0..n {
            a.swap(piv * n + k, r * n + k);
        }
        for row in r + 1..m {
            let f = a[row * n + col] / a[r * n + col];
            if f != 0.0 {
                for k in col..n {
                    a[row * n + k] -= f * a[r * n + k];
                }
            }
        }
        r += 1;
    }
    r
}

/// Solves the sketch in place.
pub fn solve(sketch: &mut Sketch) -> Result<SolveReport, SketchError> {
    sketch.validate()?;
    let sys = System::build(sketch);
    let mut x = sys.read(sketch);
    let n = x.len();
    let mut r = Vec::new();
    Eval {
        sys: &sys,
        sketch,
        x: &x,
    }
    .residuals(&mut r);
    let m = r.len();
    if n == 0 {
        return Ok(SolveReport {
            status: SolveStatus::FullyConstrained,
            iterations: 0,
            residual: 0.0,
        });
    }

    let mut cost = norm(&r);
    let mut lambda = 1e-3;
    let mut iterations = 0;
    if m > 0 {
        while iterations < 200 && cost > TOL {
            iterations += 1;
            let jac = jacobian(&sys, sketch, &x, m);
            // normal equations: (J^T J + lambda I) dx = -J^T r
            let mut jtj = vec![0.0; n * n];
            let mut jtr = vec![0.0; n];
            for i in 0..m {
                let row = &jac[i * n..(i + 1) * n];
                for a in 0..n {
                    if row[a] == 0.0 {
                        continue;
                    }
                    jtr[a] -= row[a] * r[i];
                    for b in 0..n {
                        jtj[a * n + b] += row[a] * row[b];
                    }
                }
            }
            let mut improved = false;
            for _ in 0..12 {
                let mut a = jtj.clone();
                for k in 0..n {
                    a[k * n + k] += lambda;
                }
                let mut b = jtr.clone();
                let Some(dx) = solve_linear(&mut a, &mut b, n) else {
                    lambda *= 10.0;
                    continue;
                };
                let xn: Vec<f64> = x.iter().zip(&dx).map(|(a, b)| a + b).collect();
                let mut rn = Vec::with_capacity(m);
                Eval {
                    sys: &sys,
                    sketch,
                    x: &xn,
                }
                .residuals(&mut rn);
                let cn = norm(&rn);
                if cn.is_finite() && cn < cost {
                    x = xn;
                    r = rn;
                    cost = cn;
                    lambda = (lambda * 0.3).max(1e-12);
                    improved = true;
                    break;
                }
                lambda *= 10.0;
            }
            if !improved {
                break;
            }
        }
    }
    sys.write(sketch, &x);

    let status = if m == 0 {
        SolveStatus::UnderConstrained { dof: n }
    } else if cost > 1e-6 {
        SolveStatus::Conflicting { residual: cost }
    } else {
        let jac = jacobian(&sys, sketch, &x, m);
        let rk = rank(jac, m, n);
        let dof = n - rk;
        if rk < m {
            SolveStatus::Redundant { dof }
        } else if dof == 0 {
            SolveStatus::FullyConstrained
        } else {
            SolveStatus::UnderConstrained { dof }
        }
    };
    Ok(SolveReport {
        status,
        iterations,
        residual: cost,
    })
}

/// Moves a point towards `target` while keeping all constraints satisfied
/// (interactive dragging). The point is first moved, then the sketch re-solved;
/// because Levenberg–Marquardt takes minimum-norm steps, unconstrained motion is kept.
pub fn drag_point(
    sketch: &mut Sketch,
    point: PointId,
    target: [f64; 2],
) -> Result<SolveReport, SketchError> {
    let backup = sketch.clone();
    if let Some(p) = sketch.points.get_mut(&point) {
        p.x = target[0];
        p.y = target[1];
    } else {
        return Err(SketchError::MissingPoint(point));
    }
    let report = solve(sketch)?;
    if matches!(report.status, SolveStatus::Conflicting { .. }) {
        *sketch = backup;
        return solve(sketch);
    }
    Ok(report)
}
