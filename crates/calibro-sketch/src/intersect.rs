//! Curve–curve intersections in a sketch, used to split crossing curves into
//! separate edges for region detection, and by the Trim tool.

use crate::model::{CurveId, CurveKind, Sketch};
use crate::profile::{arc_angles, Segment};
use std::f64::consts::TAU;

/// Geometric tolerance for intersections (sketch units = mm).
pub const TOL: f64 = 1e-7;

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
    dot(d, d).sqrt()
}

/// The segment of a (non-construction or construction) curve with the current
/// point positions. Arcs are counter-clockwise from start to end.
pub fn curve_segment(sketch: &Sketch, id: CurveId) -> Option<Segment> {
    let c = sketch.curves.get(&id)?;
    let p = |pid| sketch.points.get(&pid).map(|p| p.xy());
    Some(match c.kind {
        CurveKind::Line { a, b } => Segment::Line {
            from: p(a)?,
            to: p(b)?,
        },
        CurveKind::Circle { center, radius } => Segment::Circle {
            center: p(center)?,
            radius,
        },
        CurveKind::Arc { center, start, end } => {
            let (cc, s) = (p(center)?, p(start)?);
            Segment::Arc {
                center: cc,
                radius: dist(cc, s),
                from: s,
                to: p(end)?,
                ccw: true,
            }
        }
    })
}

/// Parameter of point `p` (assumed on the curve's carrier) along `seg`: 0..1 for lines
/// and arcs (None if outside), 0..1 (fraction of a turn from +X) for circles.
pub fn param_of(seg: &Segment, p: [f64; 2]) -> Option<f64> {
    match *seg {
        Segment::Line { from, to } => {
            let d = sub(to, from);
            let l2 = dot(d, d);
            if l2 < 1e-24 {
                return None;
            }
            let t = dot(sub(p, from), d) / l2;
            let tol = TOL / l2.sqrt();
            (-tol..=1.0 + tol).contains(&t).then_some(t.clamp(0.0, 1.0))
        }
        Segment::Arc {
            center,
            radius,
            from,
            to,
            ccw,
        } => {
            let (a0, sweep) = arc_angles(center, from, to, ccw);
            let a = (p[1] - center[1]).atan2(p[0] - center[0]);
            let mut d = (a - a0) * sweep.signum();
            d = d.rem_euclid(TAU);
            let tol = TOL / radius.max(1e-12);
            if d > sweep.abs() + tol && d < TAU - tol {
                return None;
            }
            if d >= TAU - tol {
                d = 0.0;
            }
            Some((d / sweep.abs()).clamp(0.0, 1.0))
        }
        Segment::Circle { center, .. } => {
            let a = (p[1] - center[1]).atan2(p[0] - center[0]);
            Some(a.rem_euclid(TAU) / TAU)
        }
    }
}

/// Point at parameter `t` (see [`param_of`]).
pub fn point_at(seg: &Segment, t: f64) -> [f64; 2] {
    match *seg {
        Segment::Line { from, to } => [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        ],
        Segment::Arc {
            center,
            radius,
            from,
            to,
            ccw,
        } => {
            let (a0, sweep) = arc_angles(center, from, to, ccw);
            let a = a0 + sweep * t;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        }
        Segment::Circle { center, radius } => {
            let a = TAU * t;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        }
    }
}

fn circle_of(seg: &Segment) -> Option<([f64; 2], f64)> {
    match *seg {
        Segment::Arc { center, radius, .. } | Segment::Circle { center, radius } => {
            Some((center, radius))
        }
        Segment::Line { .. } => None,
    }
}

/// Points where two segments meet (crossing or touching), restricted to both segments.
pub fn intersections(a: &Segment, b: &Segment) -> Vec<[f64; 2]> {
    let candidates: Vec<[f64; 2]> = match (circle_of(a), circle_of(b)) {
        (None, None) => {
            let (Segment::Line { from: p, to: p2 }, Segment::Line { from: q, to: q2 }) = (a, b)
            else {
                unreachable!()
            };
            let (r, s) = (sub(*p2, *p), sub(*q2, *q));
            let den = cross(r, s);
            if den.abs() < 1e-14 * (dot(r, r).sqrt() * dot(s, s).sqrt()).max(1e-30) {
                vec![] // parallel (overlaps are not split)
            } else {
                let t = cross(sub(*q, *p), s) / den;
                vec![[p[0] + r[0] * t, p[1] + r[1] * t]]
            }
        }
        (Some((c, rad)), None) | (None, Some((c, rad))) => {
            let line = if circle_of(a).is_none() { a } else { b };
            let Segment::Line { from, to } = *line else {
                unreachable!()
            };
            let d = sub(to, from);
            let f = sub(from, c);
            let (qa, qb, qc) = (dot(d, d), 2.0 * dot(f, d), dot(f, f) - rad * rad);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa < 1e-24 || disc < -1e-12 * qa * rad.max(1.0) {
                vec![]
            } else {
                let sq = disc.max(0.0).sqrt();
                let mut v = vec![(-qb - sq) / (2.0 * qa)];
                if sq > 1e-12 {
                    v.push((-qb + sq) / (2.0 * qa));
                }
                v.into_iter()
                    .map(|t| [from[0] + d[0] * t, from[1] + d[1] * t])
                    .collect()
            }
        }
        (Some((c1, r1)), Some((c2, r2))) => {
            let d = dist(c1, c2);
            if d < 1e-12 || d > r1 + r2 + TOL || d < (r1 - r2).abs() - TOL {
                vec![]
            } else {
                let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = [(c2[0] - c1[0]) / d, (c2[1] - c1[1]) / d];
                let m = [c1[0] + u[0] * x, c1[1] + u[1] * x];
                let mut v = vec![[m[0] - u[1] * h, m[1] + u[0] * h]];
                if h > 1e-9 {
                    v.push([m[0] + u[1] * h, m[1] - u[0] * h]);
                }
                v
            }
        }
    };
    candidates
        .into_iter()
        .filter(|p| param_of(a, *p).is_some() && param_of(b, *p).is_some())
        .collect()
}

/// Sorted interior split parameters of `seg` caused by `others` (crossings, touches and
/// other curves' end points lying on it). End points of `seg` itself are excluded.
pub fn split_params(seg: &Segment, others: &[Segment]) -> Vec<f64> {
    let len_scale = match *seg {
        Segment::Line { from, to } => dist(from, to),
        Segment::Arc { radius, .. } | Segment::Circle { radius, .. } => radius * TAU,
    }
    .max(1e-12);
    let eps = 1e-6 / len_scale;
    let closed = matches!(seg, Segment::Circle { .. });
    let mut ts: Vec<f64> = Vec::new();
    let mut add = |t: f64| {
        if !closed && (t <= eps || t >= 1.0 - eps) {
            return;
        }
        if !ts
            .iter()
            .any(|u| (u - t).abs() < eps || (closed && (u - t).abs() > 1.0 - eps))
        {
            ts.push(t);
        }
    };
    for o in others {
        for p in intersections(seg, o) {
            if let Some(t) = param_of(seg, p) {
                add(t);
            }
        }
        // end points of the other curve lying on this one (T junctions)
        let ends: Vec<[f64; 2]> = match *o {
            Segment::Line { from, to } | Segment::Arc { from, to, .. } => vec![from, to],
            Segment::Circle { .. } => vec![],
        };
        for e in ends {
            if let Some(t) = param_of(seg, e) {
                if dist(point_at(seg, t), e) < 1e-6 {
                    add(t);
                }
            }
        }
    }
    ts.sort_by(|a, b| a.total_cmp(b));
    ts
}

/// Sub-segment of `seg` between parameters `t0 < t1` (for circles, wraps past 1.0
/// when `t1 < t0`).
pub fn sub_segment(seg: &Segment, t0: f64, t1: f64) -> Segment {
    match *seg {
        Segment::Line { .. } => Segment::Line {
            from: point_at(seg, t0),
            to: point_at(seg, t1),
        },
        Segment::Arc {
            center,
            radius,
            ccw,
            ..
        } => Segment::Arc {
            center,
            radius,
            from: point_at(seg, t0),
            to: point_at(seg, t1),
            ccw,
        },
        Segment::Circle { center, radius } => Segment::Arc {
            center,
            radius,
            from: point_at(seg, t0),
            to: point_at(seg, t1),
            ccw: true,
        },
    }
}

/// Distance from `p` to the segment (approximate for arcs/circles: exact radial distance
/// when the angle is inside the arc, else distance to the nearer end).
pub fn distance_to(seg: &Segment, p: [f64; 2]) -> f64 {
    match *seg {
        Segment::Line { from, to } => {
            let d = sub(to, from);
            let l2 = dot(d, d).max(1e-30);
            let t = (dot(sub(p, from), d) / l2).clamp(0.0, 1.0);
            dist(p, [from[0] + d[0] * t, from[1] + d[1] * t])
        }
        Segment::Circle { center, radius } => (dist(p, center) - radius).abs(),
        Segment::Arc {
            center,
            radius,
            from,
            to,
            ..
        } => {
            let r = dist(p, center).max(1e-12);
            let on = [
                center[0] + (p[0] - center[0]) / r * radius,
                center[1] + (p[1] - center[1]) / r * radius,
            ];
            if param_of(seg, on).is_some() {
                (r - radius).abs()
            } else {
                dist(p, from).min(dist(p, to))
            }
        }
    }
}

/// Trims the piece of curve `id` that contains the point nearest to `click`: the curve
/// is cut where other (non-construction) curves cross or touch it, and the clicked piece
/// is removed. Lines and arcs keep their remaining pieces (new end points are added),
/// circles become an arc. A curve with no crossings is deleted entirely.
/// Returns the ids of the curves that replace it.
pub fn trim_curve(
    sketch: &mut Sketch,
    id: CurveId,
    click: [f64; 2],
) -> Result<Vec<CurveId>, String> {
    let seg = curve_segment(sketch, id).ok_or("that curve no longer exists")?;
    let construction = sketch.curves[&id].construction;
    let others: Vec<Segment> = sketch
        .curves
        .iter()
        .filter(|(cid, c)| **cid != id && !c.construction)
        .filter_map(|(cid, _)| curve_segment(sketch, *cid))
        .collect();
    let ts = split_params(&seg, &others);
    if ts.is_empty() {
        sketch.remove_curve(id);
        return Ok(vec![]);
    }
    // parameter of the clicked spot
    let tc = {
        let mut best = (f64::INFINITY, 0.0);
        for i in 0..=400 {
            let t = i as f64 / 400.0;
            let d = dist(point_at(&seg, t), click);
            if d < best.0 {
                best = (d, t);
            }
        }
        best.1
    };
    // pieces to keep, as parameter intervals
    let keep: Vec<(f64, f64)> = match seg {
        Segment::Circle { .. } => {
            // a circle cut at n points has n arcs; remove the one containing tc
            let n = ts.len();
            if n < 2 {
                // a single touching point: removing "the piece" removes everything
                sketch.remove_curve(id);
                return Ok(vec![]);
            }
            let k = (0..n)
                .find(|&k| {
                    let (a, b) = (ts[k], ts[(k + 1) % n]);
                    if a < b {
                        (a..=b).contains(&tc)
                    } else {
                        tc >= a || tc <= b
                    }
                })
                .unwrap_or(0);
            // keep the complementary arc from ts[k+1] around to ts[k]
            vec![(ts[(k + 1) % n], ts[k])]
        }
        _ => {
            let mut cuts = vec![0.0];
            cuts.extend(&ts);
            cuts.push(1.0);
            let k = (0..cuts.len() - 1)
                .find(|&k| tc >= cuts[k] && tc <= cuts[k + 1])
                .unwrap_or(0);
            let mut v = Vec::new();
            if k > 0 {
                v.push((0.0, cuts[k]));
            }
            if k + 1 < cuts.len() - 1 {
                v.push((cuts[k + 1], 1.0));
            }
            v
        }
    };
    // existing end points are reused for pieces that start/end at them
    let (pa, pb, center) = match sketch.curves[&id].kind {
        CurveKind::Line { a, b } => (Some(a), Some(b), None),
        CurveKind::Arc { center, start, end } => (Some(start), Some(end), Some(center)),
        CurveKind::Circle { center, .. } => (None, None, Some(center)),
    };
    let center_xy = center.map(|c| sketch.points[&c].xy());
    let mut new_ids = Vec::new();
    let make_point = |sketch: &mut Sketch, t: f64| -> u32 {
        if t <= 0.0 {
            if let Some(p) = pa {
                return p;
            }
        }
        if t >= 1.0 {
            if let Some(p) = pb {
                return p;
            }
        }
        let p = point_at(&seg, t);
        sketch.add_point(p[0], p[1])
    };
    let mut pieces = Vec::new();
    for (t0, t1) in keep {
        let a = make_point(sketch, t0);
        let b = make_point(sketch, t1);
        pieces.push((a, b));
    }
    // remove the old curve but keep points that the pieces reuse
    sketch.curves.remove(&id);
    sketch
        .constraints
        .retain(|_, c| !c.referenced_curves().contains(&id));
    for (a, b) in pieces {
        let nid = match (seg, center_xy) {
            (Segment::Line { .. }, _) => sketch.add_line(a, b),
            (_, Some(c)) => {
                let cp = match center {
                    Some(cid) if sketch.points.contains_key(&cid) => cid,
                    _ => sketch.add_point(c[0], c[1]),
                };
                sketch.add_arc(cp, a, b)
            }
            _ => continue,
        };
        if let Some(c) = sketch.curves.get_mut(&nid) {
            c.construction = construction;
        }
        new_ids.push(nid);
    }
    // drop points that nothing uses any more (e.g. the end on the trimmed side)
    for p in [pa, pb, center].into_iter().flatten() {
        sketch.remove_point_if_unused(p);
    }
    Ok(new_ids)
}
