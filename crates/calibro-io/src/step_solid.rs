//! STEP import as editable solids.
//!
//! `truck-stepio` reads STEP topology into shells whose geometry uses STEP's own
//! curve/surface types. This module converts that geometry to the types the modelling
//! kernel works with (`truck_modeling::{Curve, Surface}`), so imported parts become real
//! solids: they can be cut, joined, filleted and exported to STEP again.
//!
//! | STEP geometry | Converted to | Exact? |
//! |---|---|---|
//! | line, B-spline, NURBS curve | same | yes |
//! | circle / ellipse (trimmed) | rational quadratic NURBS | yes |
//! | polyline, parabola, hyperbola, curve on surface | degree-1 B-spline through 64+ samples | approximation |
//! | plane, cylinder, cone, surface of revolution, B-spline, NURBS surface | same | yes |
//! | sphere, torus | surface of revolution of a NURBS arc | yes |
//! | surface of linear extrusion | NURBS surface | yes for exact curves |
//!
//! Shells that cannot be converted (or are not closed) are reported so the caller can
//! fall back to a reference mesh.

use truck_modeling::{
    BSplineCurve, BSplineSurface, Curve, InnerSpace, Invertible, KnotVec, Line, NurbsCurve,
    NurbsSurface, ParametricCurve, ParametricSurface3D, Point3, Processor, RevolutedCurve,
    SearchParameter, Surface, Transformed, Vector3, Vector4,
};
use truck_stepio::r#in::alias::{self as sa, Curve3D, ElementarySurface, SweptCurve};
use truck_topology::compress::{CompressedEdge, CompressedFace, CompressedShell};

type StepShell = CompressedShell<Point3, Curve3D, sa::Surface>;

/// NURBS arc of the unit circle in the XY plane, counter-clockwise from `a0` by `sweep`.
fn unit_arc(a0: f64, sweep: f64) -> NurbsCurve<Vector4> {
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let d = sweep / n as f64;
    let w = (d * 0.5).cos();
    let mut pts = Vec::with_capacity(2 * n + 1);
    let mut knots = vec![0.0, 0.0, 0.0];
    for i in 0..n {
        let t0 = a0 + d * i as f64;
        let tm = t0 + d * 0.5;
        if i == 0 {
            pts.push(Vector4::new(t0.cos(), t0.sin(), 0.0, 1.0));
        }
        pts.push(Vector4::new(tm.cos(), tm.sin(), 0.0, w));
        let t1 = t0 + d;
        pts.push(Vector4::new(t1.cos(), t1.sin(), 0.0, 1.0));
        if i + 1 < n {
            let k = (i + 1) as f64 / n as f64;
            knots.push(k);
            knots.push(k);
        }
    }
    knots.extend([1.0, 1.0, 1.0]);
    NurbsCurve::new(BSplineCurve::new(KnotVec::from(knots), pts))
}

/// Arc of a circle with centre `c`, in the plane spanned by `x` and `y` (unit vectors).
fn arc(c: Point3, x: Vector3, y: Vector3, r: f64, a0: f64, sweep: f64) -> NurbsCurve<Vector4> {
    let m = truck_modeling::Matrix4::from_cols(
        (x * r).extend(0.0),
        (y * r).extend(0.0),
        x.cross(y).extend(0.0),
        c.to_homogeneous(),
    );
    let mut a = unit_arc(a0, sweep);
    a.transform_by(m);
    a
}

/// Exact line or circular arc through `pts` (samples along a curve), if they fit one.
fn fit_line_or_arc(pts: &[Point3]) -> Option<Curve> {
    let (a, b) = (pts[0], *pts.last()?);
    let size = pts
        .iter()
        .map(|p| (*p - a).magnitude())
        .fold(0.0, f64::max)
        .max(1e-9);
    let tol = size * 1e-7;
    // straight line
    let d = b - a;
    if d.magnitude() > tol {
        let u = d.normalize();
        if pts.iter().all(|p| {
            let q = *p - a;
            (q - u * q.dot(u)).magnitude() < tol
        }) {
            return Some(Curve::Line(Line(a, b)));
        }
    }
    // circular arc through first, middle and last sample
    let m = pts[pts.len() / 2];
    let (u, v) = (m - a, b - a);
    let n = u.cross(v);
    if n.magnitude() < tol * size {
        return None;
    }
    // circumcentre of a, m, b
    let c =
        a + (n.cross(u) * v.magnitude2() + v.cross(n) * u.magnitude2()) / (2.0 * n.magnitude2());
    let r = (a - c).magnitude();
    let nn = n.normalize();
    if !pts.iter().all(|p| {
        ((*p - c).magnitude() - r).abs() < tol.max(r * 1e-7)
            && (*p - c).dot(nn).abs() < tol.max(r * 1e-7)
    }) {
        return None;
    }
    let x = (a - c) / r;
    let y = nn.cross(x);
    let ang = |p: Point3| {
        let q = p - c;
        q.dot(y).atan2(q.dot(x)).rem_euclid(std::f64::consts::TAU)
    };
    // the normal was taken from (m - a) x (b - a), so going counter-clockwise around it
    // from a passes m before b; a closed curve (a == b) needs the full turn
    let mut sweep = ang(b);
    if sweep < 1e-9 {
        sweep = std::f64::consts::TAU;
    }
    Some(Curve::NurbsCurve(arc(c, x, y, r, 0.0, sweep)))
}

/// Any bounded curve: exact line/arc when it is one, else a degree-1 B-spline through
/// 64 samples.
fn sampled<C: ParametricCurve<Point = Point3> + truck_modeling::BoundedCurve>(c: &C) -> Curve {
    let (t0, t1) = c.range_tuple();
    let n = 64;
    let pts: Vec<Point3> = (0..=n)
        .map(|i| c.subs(t0 + (t1 - t0) * i as f64 / n as f64))
        .collect();
    if let Some(exact) = fit_line_or_arc(&pts) {
        return exact;
    }
    Curve::BSplineCurve(BSplineCurve::new(KnotVec::uniform_knot(1, n), pts))
}

fn convert_curve(c: &Curve3D) -> Curve {
    match c {
        Curve3D::Line(l) => Curve::Line(*l),
        Curve3D::BSplineCurve(b) => Curve::BSplineCurve(b.clone()),
        Curve3D::NurbsCurve(n) => Curve::NurbsCurve(n.clone()),
        Curve3D::Conic(sa::Conic3D::Ellipse(p)) => {
            let (t0, t1) = truck_modeling::BoundedCurve::range_tuple(p.entity());
            let mut a = unit_arc(t0, t1 - t0);
            a.transform_by(*p.transform());
            if !p.orientation() {
                a.invert();
            }
            Curve::NurbsCurve(a)
        }
        other => sampled(other),
    }
}

/// Converts an edge's curve so that it runs exactly from vertex `pa` to vertex `pb`.
/// STEP edges often reference a longer (or periodic) base curve and leave the trimming to
/// the vertices; in that case the piece between the vertices is extracted.
fn convert_edge_curve(c: &Curve3D, pa: Point3, pb: Point3) -> Curve {
    use truck_modeling::BoundedCurve;
    let conv = convert_curve(c);
    let scale = (pb - pa).magnitude().max(1.0);
    let tol = 1e-6 * scale;
    let ok = |p: Point3, q: Point3| (p - q).magnitude() < tol; // false for NaN
    let (f, b) = (conv.front(), conv.back());
    if ok(f, pa) && ok(b, pb) {
        return conv;
    }
    if ok(f, pb) && ok(b, pa) {
        let mut r = conv;
        r.invert();
        return r;
    }
    // sample the original curve and keep the stretch between the two vertices
    let (t0, t1) = c.range_tuple();
    let n = 720;
    let samples: Vec<(f64, Point3)> = (0..=n)
        .map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / n as f64;
            (t, c.subs(t))
        })
        .filter(|(_, p)| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        .collect();
    if samples.len() < 2 {
        return Curve::Line(Line(pa, pb));
    }
    let nearest = |q: Point3| {
        samples
            .iter()
            .enumerate()
            .min_by(|x, y| {
                (x.1 .1 - q)
                    .magnitude()
                    .total_cmp(&(y.1 .1 - q).magnitude())
            })
            .map(|x| x.0)
            .unwrap()
    };
    let (ia, ib) = (nearest(pa), nearest(pb));
    let closed = (samples[0].1 - samples[samples.len() - 1].1).magnitude() < tol;
    let mut idx: Vec<usize> = if ia < ib {
        (ia..=ib).collect()
    } else if closed {
        // wrap around the end of a closed curve (the last sample equals the first)
        (ia..samples.len() - 1).chain(0..=ib).collect()
    } else {
        (ib..=ia).rev().collect()
    };
    if idx.len() < 2 {
        idx = vec![ia, ib];
    }
    let mut pts: Vec<Point3> = idx.iter().map(|&i| samples[i].1).collect();
    pts[0] = pa;
    let last = pts.len() - 1;
    pts[last] = pb;
    if let Some(exact) = fit_line_or_arc(&pts) {
        if ok(exact.front(), pa) && ok(exact.back(), pb) {
            return exact;
        }
    }
    // thin out to at most ~64 segments
    let step = (pts.len() / 64).max(1);
    let mut thin: Vec<Point3> = pts.iter().step_by(step).copied().collect();
    if *thin.last().unwrap() != pb {
        thin.push(pb);
    }
    let k = thin.len() - 1;
    Curve::BSplineCurve(BSplineCurve::new(KnotVec::uniform_knot(1, k), thin))
}

fn to_nurbs(c: &Curve) -> Option<NurbsCurve<Vector4>> {
    Some(match c {
        Curve::Line(l) => {
            NurbsCurve::from(BSplineCurve::new(KnotVec::bezier_knot(1), vec![l.0, l.1]))
        }
        Curve::BSplineCurve(b) => NurbsCurve::from(b.clone()),
        Curve::NurbsCurve(n) => n.clone(),
        Curve::IntersectionCurve(_) => return None,
    })
}

/// Re-creates a placed surface of revolution: the STEP placement (rigid transform and
/// orientation) is applied to the profile, origin and axis directly.
fn rebuild<E>(
    p: &Processor<E, truck_modeling::Matrix4>,
    curve: Curve,
    origin: Point3,
    axis: Vector3,
) -> Processor<RevolutedCurve<Curve>, truck_modeling::Matrix4> {
    let m = *p.transform();
    let mut c = curve;
    c.transform_by(m);
    let o = Point3::from_homogeneous(m * origin.to_homogeneous());
    let a = (m * axis.extend(0.0)).truncate().normalize();
    let mut q = Processor::new(RevolutedCurve::by_revolution(c, o, a));
    if !p.orientation() {
        q.invert();
    }
    q
}

/// Converts a STEP surface. Returns the surface and whether its normal must be checked
/// against the original (re-parameterised surfaces).
fn convert_surface(s: &sa::Surface) -> Result<(Surface, bool), String> {
    Ok(match s {
        sa::Surface::ElementarySurface(e) => match &**e {
            ElementarySurface::Plane(p) => (Surface::Plane(*p), false),
            ElementarySurface::CylindricalSurface(p) | ElementarySurface::ConicalSurface(p) => {
                let rc = p.entity();
                let line: Line<Point3> = *rc.entity_curve();
                (
                    Surface::RevolutedCurve(rebuild(p, Curve::Line(line), rc.origin(), rc.axis())),
                    false,
                )
            }
            ElementarySurface::Sphere(p) => {
                let sph = p.entity().0;
                let (c, r) = (sph.center(), sph.radius());
                // half circle from the south to the north pole in the XZ plane
                let half = arc(
                    c,
                    Vector3::unit_x(),
                    Vector3::unit_z(),
                    r,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                );
                (
                    Surface::RevolutedCurve(rebuild(
                        p,
                        Curve::NurbsCurve(half),
                        c,
                        Vector3::unit_z(),
                    )),
                    true,
                )
            }
            ElementarySurface::ToroidalSurface(p) => {
                let t = p.entity();
                let c = t.center();
                let ring = arc(
                    c + Vector3::unit_x() * t.large_radius(),
                    Vector3::unit_x(),
                    Vector3::unit_z(),
                    t.small_radius(),
                    0.0,
                    std::f64::consts::TAU,
                );
                (
                    Surface::RevolutedCurve(rebuild(
                        p,
                        Curve::NurbsCurve(ring),
                        c,
                        Vector3::unit_z(),
                    )),
                    true,
                )
            }
        },
        sa::Surface::SweptCurve(sw) => match &**sw {
            SweptCurve::ExtrudedCurve(ec) => {
                let base = to_nurbs(&convert_curve(ec.entity_curve()))
                    .ok_or("unsupported extrusion profile")?;
                let v = ec.extruding_vector();
                let b = base.non_rationalized();
                let ctrl: Vec<Vec<Vector4>> = b
                    .control_points()
                    .iter()
                    .map(|p| vec![*p, *p + Vector4::new(v.x * p.w, v.y * p.w, v.z * p.w, 0.0)])
                    .collect();
                let knots = (b.knot_vec().clone(), KnotVec::bezier_knot(1));
                (
                    Surface::NurbsSurface(NurbsSurface::new(BSplineSurface::new(knots, ctrl))),
                    true,
                )
            }
            SweptCurve::RevolutedCurve(p) => {
                let rc = p.entity();
                let curve = convert_curve(rc.entity_curve());
                (
                    Surface::RevolutedCurve(rebuild(p, curve, rc.origin(), rc.axis())),
                    false,
                )
            }
        },
        sa::Surface::BSplineSurface(b) => (Surface::BSplineSurface((**b).clone()), false),
        sa::Surface::NurbsSurface(n) => (Surface::NurbsSurface((**n).clone()), false),
    })
}

/// Makes the converted surface's normal point the same way as the original's, compared
/// at `p` (a point of the face boundary).
fn align_normal(orig: &sa::Surface, conv: &mut Surface, p: Point3) {
    let Some((u, v)) = orig.search_parameter(p, None, 100) else {
        return;
    };
    let n0 = orig.normal(u, v);
    if let Some((cu, cv)) = conv.search_parameter(p, None, 100) {
        if conv.normal(cu, cv).dot(n0) < 0.0 {
            conv.invert();
        }
    }
}

/// Converts one STEP shell to a kernel shell.
fn convert_shell(cs: &StepShell) -> Result<CompressedShell<Point3, Curve, Surface>, String> {
    use truck_modeling::{BoundedCurve, Cut};
    let mut vertices = cs.vertices.clone();
    let mut edges: Vec<CompressedEdge<Curve>> = cs
        .edges
        .iter()
        .map(|e| CompressedEdge {
            vertices: e.vertices,
            curve: convert_edge_curve(
                &e.curve,
                cs.vertices[e.vertices.0],
                cs.vertices[e.vertices.1],
            ),
        })
        .collect();
    // The kernel's topology cannot hold an edge that starts and ends at the same vertex
    // (a full circle): split such edges in two at their middle.
    let mut split: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..edges.len() {
        let (a, b) = edges[i].vertices;
        if a != b {
            continue;
        }
        let (t0, t1) = edges[i].curve.range_tuple();
        let tm = 0.5 * (t0 + t1);
        let mid = edges[i].curve.subs(tm);
        vertices.push(mid);
        let vm = vertices.len() - 1;
        let mut first = edges[i].curve.clone();
        let second = first.cut(tm);
        edges[i] = CompressedEdge {
            vertices: (a, vm),
            curve: first,
        };
        edges.push(CompressedEdge {
            vertices: (vm, a),
            curve: second,
        });
        split.insert(i, edges.len() - 1);
    }
    let mut faces = Vec::with_capacity(cs.faces.len());
    for f in &cs.faces {
        let (mut s, check) = convert_surface(&f.surface)?;
        if check {
            let first = f.boundaries.first().and_then(|b| b.first());
            if let Some(e) = first {
                let v = cs.edges[e.index].vertices.0;
                align_normal(&f.surface, &mut s, cs.vertices[v]);
            }
        }
        let boundaries = f
            .boundaries
            .iter()
            .map(|wire| {
                let mut w = Vec::with_capacity(wire.len());
                for e in wire {
                    match split.get(&e.index) {
                        Some(&j) if e.orientation => {
                            w.push(*e);
                            w.push((j, true).into());
                        }
                        Some(&j) => {
                            w.push((j, false).into());
                            w.push(*e);
                        }
                        None => w.push(*e),
                    }
                }
                w
            })
            .collect();
        faces.push(CompressedFace {
            boundaries,
            orientation: f.orientation,
            surface: s,
        });
    }
    let mut shell = CompressedShell {
        vertices,
        edges,
        faces,
    };
    split_seam_faces(&mut shell)?;
    rotate_periodic_starts(&mut shell);
    Ok(shell)
}

/// The kernel meshes a face of a surface of revolution in its (angle, profile) parameter
/// space, which breaks when the face crosses the angle where the parameter wraps. Each
/// such face gets its own copy of the surface, turned so that the wrap lies in the
/// largest angular gap the face does not cover.
fn rotate_periodic_starts(sh: &mut CShell) {
    use truck_modeling::{BoundedCurve, Matrix4, Rad};
    for fi in 0..sh.faces.len() {
        let Surface::RevolutedCurve(p) = &sh.faces[fi].surface else {
            continue;
        };
        let rc = p.entity();
        let (o, a) = (rc.origin(), rc.axis().normalize());
        let profile = rc.entity_curve().clone();
        let (t0, t1) = profile.range_tuple();
        // reference direction (angle 0) = where the profile lies
        let mut e1 = None;
        for i in 0..=8 {
            let q = profile.subs(t0 + (t1 - t0) * i as f64 / 8.0) - o;
            let r = q - a * q.dot(a);
            if r.magnitude() > 1e-9 {
                e1 = Some(r.normalize());
                break;
            }
        }
        let Some(e1) = e1 else { continue };
        let e2 = a.cross(e1);
        let mut angles = Vec::new();
        for w in &sh.faces[fi].boundaries {
            for e in w {
                let c = &sh.edges[e.index].curve;
                let (c0, c1) = c.range_tuple();
                for i in 0..=16 {
                    let q = c.subs(c0 + (c1 - c0) * i as f64 / 16.0) - o;
                    let r = q - a * q.dot(a);
                    if r.magnitude() > 1e-9 {
                        angles.push(r.dot(e2).atan2(r.dot(e1)).rem_euclid(std::f64::consts::TAU));
                    }
                }
            }
        }
        if angles.len() < 2 {
            continue;
        }
        angles.sort_by(|x, y| x.total_cmp(y));
        let tau = std::f64::consts::TAU;
        let mut best = (
            angles[0] + tau - angles[angles.len() - 1],
            angles[angles.len() - 1],
        );
        for w in angles.windows(2) {
            if w[1] - w[0] > best.0 {
                best = (w[1] - w[0], w[0]);
            }
        }
        if best.0 < 1e-6 {
            continue; // covers the full turn: nothing to do
        }
        let mid = best.1 + best.0 * 0.5;
        // already wrapping inside the gap?
        let gap_contains_zero = {
            let start = best.1;
            let end = best.1 + best.0;
            (start..=end).contains(&tau) || (start..=end).contains(&0.0)
        };
        if gap_contains_zero {
            continue;
        }
        let m = Matrix4::from_translation(o.to_homogeneous().truncate())
            * Matrix4::from_axis_angle(a, Rad(mid))
            * Matrix4::from_translation(-o.to_homogeneous().truncate());
        let mut c = profile;
        c.transform_by(m);
        let mut q = Processor::new(RevolutedCurve::by_revolution(c, o, a));
        if !p.orientation() {
            q.invert();
        }
        sh.faces[fi].surface = Surface::RevolutedCurve(q);
    }
}

type CShell = CompressedShell<Point3, Curve, Surface>;
type EIdx = truck_topology::compress::CompressedEdgeIndex;

/// Start vertex of an oriented edge.
fn front(sh: &CShell, e: &EIdx) -> usize {
    let (a, b) = sh.edges[e.index].vertices;
    if e.orientation {
        a
    } else {
        b
    }
}

/// Faces of periodic surfaces (cylinders, cones, tori...) are bounded in STEP by a wire
/// that runs along a "seam" edge twice. The kernel only accepts simple wires, so such a
/// face is cut in two along a second edge on the opposite side of the surface:
///
/// ```text
///  wire  = [seam P→Q, loop A at Q, seam Q→P, loop B at P]
///  face1 = [seam P→Q, A1 Q→Ma, new Ma→Mb, B2 Mb→P]
///  face2 = [A2 Ma→Q, seam Q→P, B1 P→Mb, new Mb→Ma]
/// ```
fn split_seam_faces(sh: &mut CShell) -> Result<(), String> {
    let mut new_faces = Vec::new();
    let faces = std::mem::take(&mut sh.faces);
    for f in faces {
        let seam = f.boundaries.iter().enumerate().find_map(|(wi, w)| {
            let mut seen = std::collections::HashMap::new();
            for (k, e) in w.iter().enumerate() {
                if let Some(first) = seen.insert(e.index, k) {
                    return Some((wi, first, k));
                }
            }
            None
        });
        let Some((wi, k1, k2)) = seam else {
            new_faces.push(f);
            continue;
        };
        if f.boundaries.len() != 1 {
            return Err("a face with a seam and holes is not supported yet".into());
        }
        let w = &f.boundaries[wi];
        let n = w.len();
        let s1 = w[k1];
        let s2 = w[k2];
        let loop_a: Vec<EIdx> = (k1 + 1..k2).map(|k| w[k]).collect();
        let loop_b: Vec<EIdx> = (k2 + 1..k1 + n).map(|k| w[k % n]).collect();
        if loop_a.len() < 2 || loop_b.len() < 2 {
            return Err(
                "a seam face without boundary loops (e.g. a full sphere) is not supported yet"
                    .into(),
            );
        }
        let (a1, a2) = loop_a.split_at(loop_a.len() / 2);
        let (b1, b2) = loop_b.split_at(loop_b.len() / 2);
        let ma = front(sh, &a2[0]);
        let mb = front(sh, &b2[0]);
        // new edge Ma -> Mb on the surface, along a straight line in parameter space
        let (pa, pb) = (sh.vertices[ma], sh.vertices[mb]);
        let ua = f
            .surface
            .search_parameter(pa, None, 100)
            .ok_or("could not place a seam split point")?;
        let ub = f
            .surface
            .search_parameter(pb, None, 100)
            .ok_or("could not place a seam split point")?;
        let samples = 32;
        let mut pts: Vec<Point3> = (0..=samples)
            .map(|i| {
                let t = i as f64 / samples as f64;
                truck_modeling::ParametricSurface::subs(
                    &f.surface,
                    ua.0 + (ub.0 - ua.0) * t,
                    ua.1 + (ub.1 - ua.1) * t,
                )
            })
            .collect();
        pts[0] = pa;
        pts[samples] = pb;
        let straight = pts.iter().all(|p| {
            let d = pb - pa;
            let l2 = d.magnitude2().max(1e-30);
            let q = *p - pa;
            (q - d * (q.dot(d) / l2)).magnitude() < 1e-6 * l2.sqrt().max(1.0)
        });
        let curve = if straight {
            Curve::Line(Line(pa, pb))
        } else {
            Curve::BSplineCurve(BSplineCurve::new(KnotVec::uniform_knot(1, samples), pts))
        };
        sh.edges.push(CompressedEdge {
            vertices: (ma, mb),
            curve,
        });
        let ne = sh.edges.len() - 1;
        let mut w1 = vec![s1];
        w1.extend_from_slice(a1);
        w1.push((ne, true).into());
        w1.extend_from_slice(b2);
        let mut w2: Vec<EIdx> = a2.to_vec();
        w2.push(s2);
        w2.extend_from_slice(b1);
        w2.push((ne, false).into());
        for w in [w1, w2] {
            new_faces.push(CompressedFace {
                boundaries: vec![w],
                orientation: f.orientation,
                surface: f.surface.clone(),
            });
        }
    }
    sh.faces = new_faces;
    Ok(())
}

/// A shell made of one sphere or torus face (how most CAD systems write a full sphere
/// or torus) is rebuilt with the kernel's own primitives, which avoid the pole and seam
/// singularities.
fn whole_quadric(cs: &StepShell) -> Option<calibro_kernel::Result<calibro_kernel::Solid>> {
    if cs.faces.len() != 1 {
        return None;
    }
    let sa::Surface::ElementarySurface(e) = &cs.faces[0].surface else {
        return None;
    };
    let tr =
        |m: truck_modeling::Matrix4, p: Point3| Point3::from_homogeneous(m * p.to_homogeneous());
    let dir = |m: truck_modeling::Matrix4, v: Vector3| (m * v.extend(0.0)).truncate();
    match &**e {
        ElementarySurface::Sphere(p) => {
            let m = *p.transform();
            let s = p.entity().0;
            let c = tr(m, s.center());
            let r = s.radius() * dir(m, Vector3::unit_x()).magnitude();
            Some(calibro_kernel::make_sphere([c.x, c.y, c.z], r))
        }
        ElementarySurface::ToroidalSurface(p) => {
            let m = *p.transform();
            let t = p.entity();
            let k = dir(m, Vector3::unit_x()).magnitude();
            let c = tr(m, t.center());
            let a = dir(m, Vector3::unit_z()).normalize();
            let x = dir(m, Vector3::unit_x()).normalize();
            Some(calibro_kernel::make_torus(
                [c.x, c.y, c.z],
                [a.x, a.y, a.z],
                [x.x, x.y, x.z],
                t.large_radius() * k,
                t.small_radius() * k,
            ))
        }
        _ => None,
    }
}

/// Result of reading a STEP file as solids.
pub struct StepSolids {
    pub solids: Vec<calibro_kernel::Solid>,
    /// Shells that could not be turned into solids, with the reason.
    pub skipped: Vec<String>,
}

/// Reads every closed shell of a STEP file as an editable solid.
pub fn import_step_solids(text: &str) -> Result<StepSolids, crate::IoError> {
    use truck_stepio::r#in::Table;
    let table = Table::from_step(text)
        .ok_or_else(|| crate::IoError::Parse("could not parse the STEP file".into()))?;
    let mut out = StepSolids {
        solids: vec![],
        skipped: vec![],
    };
    for holder in table.shell.values() {
        let res = calibro_kernel::guarded("STEP import", || {
            let cs = table
                .to_compressed_shell(holder)
                .map_err(|e| calibro_kernel::KernelError::InvalidParameter(e.to_string()))?;
            if let Some(solid) = whole_quadric(&cs) {
                return solid;
            }
            let conv = convert_shell(&cs).map_err(calibro_kernel::KernelError::InvalidParameter)?;
            let shell = truck_modeling::Shell::extract(conv)
                .map_err(|e| calibro_kernel::KernelError::InvalidParameter(e.to_string()))?;
            let solid = truck_modeling::Solid::try_new(vec![shell]).map_err(|e| {
                calibro_kernel::KernelError::InvalidParameter(format!("not a closed solid ({e})"))
            })?;
            calibro_kernel::validate_imported(solid)
        });
        match res {
            Ok(s) => out.solids.push(s),
            Err(e) => out.skipped.push(e.to_string()),
        }
    }
    Ok(out)
}

/// Diagnostics for STEP conversion problems (used while developing the importer).
#[doc(hidden)]
pub fn debug_report(text: &str) -> String {
    use std::fmt::Write;
    use truck_modeling::{BoundedCurve, ParametricSurface};
    let mut out = String::new();
    let Some(table) = truck_stepio::r#in::Table::from_step(text) else {
        return "parse error".into();
    };
    for holder in table.shell.values() {
        let Ok(cs) = table.to_compressed_shell(holder) else {
            out.push_str("shell conversion failed\n");
            continue;
        };
        let sh = match convert_shell(&cs) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(out, "convert error {e}");
                continue;
            }
        };
        let _ = writeln!(
            out,
            "{} vertices {} edges {} faces",
            sh.vertices.len(),
            sh.edges.len(),
            sh.faces.len()
        );
        for (i, e) in cs.edges.iter().enumerate() {
            let k = match &e.curve {
                Curve3D::Line(_) => "line".to_string(),
                Curve3D::Polyline(_) => "polyline".into(),
                Curve3D::Conic(sa::Conic3D::Ellipse(p)) => format!(
                    "ellipse range {:?}",
                    truck_modeling::BoundedCurve::range_tuple(p.entity())
                ),
                Curve3D::Conic(_) => "conic".into(),
                Curve3D::BSplineCurve(_) => "bspline".into(),
                Curve3D::PCurve(pc) => {
                    let (t0, t1) = truck_modeling::BoundedCurve::range_tuple(pc);
                    let c2 = &**pc.curve();
                    let r2 = truck_modeling::BoundedCurve::range_tuple(c2);
                    format!("pcurve range ({t0}, {t1}) front {:?} mid {:?} back {:?} 2d {:?} range2d {:?} uv0 {:?} uvm {:?}", truck_modeling::ParametricCurve::subs(pc, t0), truck_modeling::ParametricCurve::subs(pc, 0.5*(t0+t1)), truck_modeling::ParametricCurve::subs(pc, t1), c2, r2, truck_modeling::ParametricCurve::subs(c2, t0), truck_modeling::ParametricCurve::subs(c2, 0.5*(t0+t1)))
                }
                Curve3D::NurbsCurve(_) => "nurbs".into(),
            };
            let _ = writeln!(out, "step edge {i} {:?} {k}", e.vertices);
        }
        for (i, e) in sh.edges.iter().enumerate() {
            let (a, b) = e.vertices;
            let (f, k) = (e.curve.front(), e.curve.back());
            let da = (f - sh.vertices[a]).magnitude();
            let db = (k - sh.vertices[b]).magnitude();
            if da > 1e-6 || db > 1e-6 {
                let _ = writeln!(
                    out,
                    "edge {i} {a}->{b} endpoint mismatch {da:.3e} {db:.3e} kind {:?}",
                    std::mem::discriminant(&e.curve)
                );
            }
        }
        for (fi, f) in sh.faces.iter().enumerate() {
            let kind = match &f.surface {
                Surface::Plane(_) => "plane",
                Surface::BSplineSurface(_) => "bspline",
                Surface::NurbsSurface(_) => "nurbs",
                Surface::RevolutedCurve(_) => "revolved",
            };
            let mut worst: f64 = 0.0;
            for w in &f.boundaries {
                for e in w {
                    let p = sh.vertices[front(&sh, e)];
                    match f.surface.search_parameter(p, None, 100) {
                        Some((u, v)) => worst = worst.max((f.surface.subs(u, v) - p).magnitude()),
                        None => worst = f64::INFINITY,
                    }
                }
            }
            let _ = writeln!(
                out,
                "face {fi} {kind} wires {:?} worst vertex distance {worst:.3e}",
                f.boundaries.iter().map(|w| w.len()).collect::<Vec<_>>()
            );
            if std::env::var("CALIBRO_FACE")
                .ok()
                .and_then(|x| x.parse::<usize>().ok())
                == Some(fi)
            {
                let _ = writeln!(
                    out,
                    "  orientation {} surface {:?}",
                    f.orientation,
                    match &f.surface {
                        Surface::Plane(p) => format!("{:?} n={:?}", p.origin(), p.normal()),
                        _ => String::new(),
                    }
                );
                for w in &f.boundaries {
                    let _ = writeln!(out, "  wire");
                    for e in w {
                        let c = &sh.edges[e.index].curve;
                        let (t0, t1) = c.range_tuple();
                        let k = match c {
                            Curve::Line(_) => "line",
                            Curve::BSplineCurve(_) => "bspline",
                            Curve::NurbsCurve(_) => "nurbs",
                            Curve::IntersectionCurve(_) => "isect",
                        };
                        let _ = writeln!(
                            out,
                            "    e{} {} {:?} {k} {:?} mid {:?} -> {:?}",
                            e.index,
                            e.orientation,
                            sh.edges[e.index].vertices,
                            c.front(),
                            c.subs(0.5 * (t0 + t1)),
                            c.back()
                        );
                    }
                }
            }
            let skip: Vec<usize> = std::env::var("CALIBRO_SKIP")
                .unwrap_or_default()
                .split(',')
                .filter_map(|x| x.parse().ok())
                .collect();
            if std::env::var("CALIBRO_DEBUG_TESS").is_ok() && !skip.contains(&fi) {
                use truck_meshalgo::prelude::*;
                let mut ff = f.clone();
                if let Ok(k) = std::env::var("CALIBRO_WIRE") {
                    let k: usize = k.parse().unwrap();
                    if ff.boundaries.len() > k {
                        ff.boundaries = vec![ff.boundaries[k].clone()];
                    }
                }
                let single = CompressedShell {
                    vertices: sh.vertices.clone(),
                    edges: sh.edges.clone(),
                    faces: vec![ff],
                };
                eprintln!("tessellating face {fi} ({kind})...");
                if let Ok(one) = truck_modeling::Shell::extract(single) {
                    let t = std::time::Instant::now();
                    let m = one.triangulation(0.05);
                    let n = m
                        .face_iter()
                        .filter_map(|f| f.surface())
                        .map(|p| p.faces().len())
                        .sum::<usize>();
                    eprintln!("   face {fi}: {n} tris in {:?}", t.elapsed());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use truck_modeling::{BoundedCurve, ParameterDivision1D};

    #[test]
    fn nurbs_arcs_are_circles() {
        for sweep in [0.3, 1.5, 3.0, 6.2, std::f64::consts::TAU] {
            let a = arc(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::unit_x(),
                Vector3::unit_z(),
                5.0,
                0.4,
                sweep,
            );
            let (t0, t1) = a.range_tuple();
            for i in 0..=20 {
                let p = a.subs(t0 + (t1 - t0) * i as f64 / 20.0);
                let d = (p - Point3::new(1.0, 2.0, 3.0)).magnitude();
                assert!((d - 5.0).abs() < 1e-9, "sweep {sweep}: {d}");
                assert!((p.y - 2.0).abs() < 1e-9);
            }
            let (_, pts) = a.parameter_division(a.range_tuple(), 0.01);
            assert!(pts.len() > 4, "sweep {sweep}: only {} points", pts.len());
        }
    }
}
