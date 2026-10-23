//! Modify operations built on top of truck's primitives: fillet, chamfer,
//! shell, sweep and loft.
//!
//! truck 0.6 has no native fillet/chamfer/shell/sweep-along-path, so these are
//! constructed here from sweeps, homotopies and booleans:
//!
//! * **Fillet / chamfer** — straight edges between two flat faces. A tool prism
//!   whose cross-section is the material to remove (corner minus quarter-round,
//!   or the corner triangle) is swept along the edge and subtracted. truck's
//!   booleans cannot intersect tangent or coplanar surfaces, so the round's centre
//!   is moved 0.5% of the radius toward the edge (it meets the faces at a few
//!   degrees instead of tangentially) and the chamfer tool reaches `NUDGE` below
//!   each face. Both deviations are far below visible size.
//! * **Shell** — convex bodies with flat faces: the inner cavity is the
//!   intersection of the face half-spaces moved inward by the thickness (removed
//!   faces are pushed far outward so the cavity opens there); it is subtracted.
//! * **Sweep** — a profile along a smooth path of lines and arcs: the profile's
//!   loops are swept segment by segment, re-using the end wires so the walls are
//!   one connected shell, then capped.
//! * **Loft** — between two closed profiles: ruled surfaces between matching
//!   segments (exact when both outlines have the same number of segments,
//!   otherwise both are resampled into matching polygons), then capped.

use super::*;
use truck_modeling::{Edge, Shell};

/// How far (relative to the size of the feature) tools reach past a face to keep
/// boolean intersections transversal.
const NUDGE: f64 = 0.002;

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn len(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}
fn pt(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// Outward normal of a planar face, or None for curved faces.
fn face_plane(face: &Face) -> Option<([f64; 3], f64)> {
    match face.surface() {
        truck_modeling::Surface::Plane(pl) => {
            let mut n = pl.normal();
            if !face.orientation() {
                n = -n;
            }
            let o = pl.subs(0.0, 0.0);
            let n = [n.x, n.y, n.z];
            Some((n, dot3(n, [o.x, o.y, o.z])))
        }
        _ => None,
    }
}

/// Smallest width of a flat face: extent of its vertices across its longest edge.
fn face_thickness(face: &Face) -> f64 {
    let pts: Vec<[f64; 3]> = face
        .boundaries()
        .iter()
        .flat_map(|w| w.vertex_iter().map(|v| pt(v.point())).collect::<Vec<_>>())
        .collect();
    let mut best = (0.0, [1.0, 0.0, 0.0], [0.0; 3]);
    for w in face.boundaries() {
        for e in w.edge_iter() {
            let (p, q) = (pt(e.front().point()), pt(e.back().point()));
            let l = len(sub3(q, p));
            if l > best.0 {
                best = (l, mul(sub3(q, p), 1.0 / l), p);
            }
        }
    }
    let (_, t, o) = best;
    pts.iter()
        .map(|x| {
            let r = sub3(*x, o);
            len(sub3(r, mul(t, dot3(r, t))))
        })
        .fold(0.0, f64::max)
}

/// A straight edge shared by two flat faces.
struct PlanarEdge {
    a: [f64; 3],
    b: [f64; 3],
    n1: [f64; 3],
    n2: [f64; 3],
}

fn solid_size(solid: &Solid) -> f64 {
    let (lo, hi) = solid_bbox(solid);
    len(sub3(hi, lo)).max(1e-6)
}

fn find_planar_edge(solid: &Solid, a: [f64; 3], b: [f64; 3]) -> Result<PlanarEdge> {
    let tol = solid_size(solid) * 1e-6 + 1e-7;
    // Booleans may split an edge into several collinear pieces, so an edge "matches" when
    // both of its end points lie on the segment a-b.
    let ab = sub3(b, a);
    let l2 = dot3(ab, ab).max(1e-30);
    let on_segment = |p: [f64; 3]| {
        let t = dot3(sub3(p, a), ab) / l2;
        let foot = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
        (-1e-9..=1.0 + 1e-9).contains(&t) && len(sub3(p, foot)) < tol
    };
    let mut normals: Vec<[f64; 3]> = Vec::new();
    let mut curved = false;
    for face in solid.face_iter() {
        let touches = face.boundaries().iter().any(|w| {
            w.edge_iter().any(|e| {
                let (p, q) = (pt(e.front().point()), pt(e.back().point()));
                len(sub3(p, q)) > tol && on_segment(p) && on_segment(q)
            })
        });
        if touches {
            match face_plane(face) {
                Some((n, _)) => {
                    if !normals.iter().any(|m| dot3(*m, n) > 1.0 - 1e-9) {
                        normals.push(n)
                    }
                }
                None => curved = true,
            }
        }
    }
    if curved {
        return Err(KernelError::InvalidParameter(
            "this edge touches a curved face; only edges between two flat faces are supported for now".into(),
        ));
    }
    if normals.len() != 2 {
        return Err(KernelError::InvalidParameter(
            "the selected edge no longer exists on this body (an earlier change moved it)".into(),
        ));
    }
    Ok(PlanarEdge {
        a,
        b,
        n1: normals[0],
        n2: normals[1],
    })
}

/// Directions along each face, perpendicular to the edge, pointing into the face.
fn in_face_dirs(e: &PlanarEdge) -> Result<([f64; 3], [f64; 3], f64)> {
    let t = normalize(sub3(e.b, e.a));
    let pick = |n: [f64; 3], other: [f64; 3]| {
        let d = normalize(cross(n, t));
        // moving along this face away from the edge goes "below" the other face
        if dot3(d, other) < 0.0 {
            d
        } else {
            mul(d, -1.0)
        }
    };
    let d1 = pick(e.n1, e.n2);
    let d2 = pick(e.n2, e.n1);
    if dot3(d1, e.n2) > -1e-9 || dot3(d2, e.n1) > -1e-9 {
        return Err(KernelError::InvalidParameter(
            "only outside (convex) edges can be filleted or chamfered for now".into(),
        ));
    }
    let alpha = dot3(d1, d2).clamp(-1.0, 1.0).acos();
    Ok((d1, d2, alpha))
}

fn wire_from_points(points: &[[f64; 3]], arc_last_through: Option<[f64; 3]>) -> Wire {
    let verts: Vec<Vertex> = points.iter().map(|p| builder::vertex(p3(*p))).collect();
    let n = verts.len();
    let mut edges = Vec::with_capacity(n);
    for i in 0..n {
        let (v0, v1) = (&verts[i], &verts[(i + 1) % n]);
        let e = match (i == n - 1, arc_last_through) {
            (true, Some(m)) => builder::circle_arc(v0, v1, p3(m)),
            _ => builder::line(v0, v1),
        };
        edges.push(e);
    }
    edges.into()
}

/// Sweeps a closed cross-section (lying in the plane through `a` perpendicular to
/// the edge) along the edge, extended by `margin` past both ends.
fn edge_tool(
    e: &PlanarEdge,
    section: &[[f64; 3]],
    arc_through: Option<[f64; 3]>,
    margin: f64,
) -> Result<Solid> {
    let t = normalize(sub3(e.b, e.a));
    let shift = mul(t, -margin);
    let pts: Vec<[f64; 3]> = section.iter().map(|p| add(*p, shift)).collect();
    let wire = wire_from_points(&pts, arc_through.map(|m| add(m, shift)));
    let face =
        builder::try_attach_plane(&[wire]).map_err(|err| KernelError::Face(err.to_string()))?;
    let l = len(sub3(e.b, e.a)) + 2.0 * margin;
    Ok(orient_outward(builder::tsweep(&face, v3(mul(t, l)))))
}

/// Rounds straight edges between flat faces with radius `r`.
pub fn fillet(solid: &Solid, edges: &[[[f64; 3]; 2]], r: f64) -> Result<Solid> {
    if r <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "the fillet radius must be positive".into(),
        ));
    }
    reject_shared_corners(edges)?;
    guarded("fillet", || {
        let mut tools = Vec::new();
        for [a, b] in edges {
            let e = find_planar_edge(solid, *a, *b)?;
            let (d1, d2, alpha) = in_face_dirs(&e)?;
            let bis = normalize(add(d1, d2));
            let half = alpha * 0.5;
            let tangent = r / half.tan();
            if tangent * 2.0 > len(sub3(e.b, e.a)).max(1e-9) * 50.0 {
                return Err(KernelError::InvalidParameter(
                    "the fillet radius is too large".into(),
                ));
            }
            // Centre moved a hair toward the edge: the rounded surface then crosses
            // each face at a small angle instead of touching it tangentially, which
            // truck's boolean cannot intersect. Deviation from a perfect fillet: ~0.5% of r.
            let delta = (0.005 * r).max(0.002);
            let c = add(e.a, mul(bis, r / half.sin() - delta));
            let q1 = add(c, mul(e.n1, r));
            let q2 = add(c, mul(e.n2, r));
            let m = sub3(c, mul(bis, r));
            let o = sub3(e.a, mul(bis, tangent.max(r)));
            tools.push(edge_tool(
                &e,
                &[q1, o, q2],
                Some(m),
                tangent.max(r) * 2.0 + 1.0,
            )?);
        }
        subtract_all(solid, &tools)
    })
}

/// Bevels straight edges between flat faces by distance `d` along each face.
pub fn chamfer(solid: &Solid, edges: &[[[f64; 3]; 2]], d: f64) -> Result<Solid> {
    if d <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "the chamfer distance must be positive".into(),
        ));
    }
    guarded("chamfer", || {
        let mut tools = Vec::new();
        for [a, b] in edges {
            let e = find_planar_edge(solid, *a, *b)?;
            let (d1, d2, _) = in_face_dirs(&e)?;
            let bis = normalize(add(d1, d2));
            let nudge = NUDGE * d;
            let p1 = sub3(add(e.a, mul(d1, d)), mul(e.n1, nudge));
            let p2 = sub3(add(e.a, mul(d2, d)), mul(e.n2, nudge));
            let o = sub3(e.a, mul(bis, d));
            tools.push(edge_tool(&e, &[p1, o, p2], None, d * 2.0 + 1.0)?);
        }
        subtract_all(solid, &tools)
    })
}

/// Two rounds of equal radius meeting at a corner intersect along a singular
/// curve that truck cannot compute; refuse clearly instead of failing obscurely.
fn reject_shared_corners(edges: &[[[f64; 3]; 2]]) -> Result<()> {
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            for p in edges[i] {
                for q in edges[j] {
                    if len(sub3(p, q)) < 1e-6 {
                        return Err(KernelError::InvalidParameter(
                            "filleting edges that meet at the same corner is not supported yet; fillet edges that don't touch"
                                .into(),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Subtracts tools one by one. Fine tolerances first: the thin slivers next to
/// fillet and chamfer tools need precise intersection curves (measured: 0.05
/// fails on several edge orders, 0.01 succeeds on all of them).
fn subtract_all(solid: &Solid, tools: &[Solid]) -> Result<Solid> {
    let mut acc = solid.clone();
    for t in tools {
        acc = boolean_ladder(
            &acc,
            t,
            BooleanOp::Subtract,
            &[0.01, 0.002, BOOLEAN_TOLERANCE],
        )?;
    }
    Ok(acc)
}

/// Hollows a convex body with flat faces, leaving walls of `thickness`. Faces whose
/// planes match `open_faces` (point + normal) are removed (the cavity opens there).
pub fn shell(solid: &Solid, open_faces: &[Plane], thickness: f64) -> Result<Solid> {
    if thickness <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "the shell thickness must be positive".into(),
        ));
    }
    guarded("shell", || {
        let size = solid_size(solid);
        let tol = size * 1e-7 + 1e-9;
        let mut planes: Vec<([f64; 3], f64)> = Vec::new();
        for face in solid.face_iter() {
            let Some(p) = face_plane(face) else {
                return Err(KernelError::InvalidParameter(
                    "Shell currently supports bodies with flat faces only (boxes, prisms, extruded polygons)".into(),
                ));
            };
            // Hair-thin sliver faces (left by chamfer/fillet tools, see NUDGE) are not
            // real walls: skip them, the neighbouring faces define the shape.
            if face_thickness(face) < size * 1e-3 {
                continue;
            }
            if !planes
                .iter()
                .any(|q| dot3(q.0, p.0) > 1.0 - 1e-9 && (q.1 - p.1).abs() < tol * 10.0)
            {
                planes.push(p);
            }
        }
        // convexity: every vertex inside every face plane (tolerance 0.1% of the body
        // size, which also covers the slivers skipped above).
        for shell in solid.boundaries() {
            for v in shell.vertex_iter() {
                let x = pt(v.point());
                if planes.iter().any(|(n, d)| dot3(*n, x) > d + size * 1e-3) {
                    return Err(KernelError::InvalidParameter(
                        "Shell currently supports convex bodies only (no pockets or notches)"
                            .into(),
                    ));
                }
            }
        }
        let mut opened = 0;
        let inner: Vec<([f64; 3], f64)> = planes
            .iter()
            .map(|(n, d)| {
                let open = open_faces.iter().any(|f| {
                    dot3(f.normal(), *n) > 1.0 - 1e-6
                        && (dot3(*n, f.origin) - d).abs() < size * 1e-6
                });
                if open {
                    opened += 1;
                    (*n, d + size)
                } else {
                    (*n, d - thickness)
                }
            })
            .collect();
        if opened != open_faces.len() {
            return Err(KernelError::InvalidParameter(
                "a face chosen to remove no longer exists on this body".into(),
            ));
        }
        let cavity = convex_polytope(&inner, size)?;
        boolean_ladder(
            solid,
            &cavity,
            BooleanOp::Subtract,
            &[0.01, BOOLEAN_TOLERANCE, 0.002],
        )
    })
}

/// Builds the convex solid { x : n_i . x <= d_i } (must be bounded).
fn convex_polytope(planes: &[([f64; 3], f64)], size: f64) -> Result<Solid> {
    let eps = size * 1e-7 + 1e-9;
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let k = planes.len();
    for i in 0..k {
        for j in i + 1..k {
            for l in j + 1..k {
                let (a, b, c) = (planes[i], planes[j], planes[l]);
                let det = dot3(a.0, cross(b.0, c.0));
                if det.abs() < 1e-12 {
                    continue;
                }
                let x = mul(
                    add(
                        add(mul(cross(b.0, c.0), a.1), mul(cross(c.0, a.0), b.1)),
                        mul(cross(a.0, b.0), c.1),
                    ),
                    1.0 / det,
                );
                if planes.iter().all(|(n, d)| dot3(*n, x) <= d + eps * 10.0)
                    && !verts.iter().any(|v| len(sub3(*v, x)) < eps * 100.0)
                {
                    verts.push(x);
                }
            }
        }
    }
    if verts.len() < 4 {
        return Err(KernelError::InvalidParameter(
            "the walls are too thick for this body".into(),
        ));
    }
    let tverts: Vec<Vertex> = verts.iter().map(|v| builder::vertex(p3(*v))).collect();
    let mut edge_map: std::collections::HashMap<(usize, usize), Edge> =
        std::collections::HashMap::new();
    let mut faces = Vec::new();
    for (n, d) in planes {
        let mut on: Vec<usize> = (0..verts.len())
            .filter(|&i| (dot3(*n, verts[i]) - d).abs() < eps * 100.0)
            .collect();
        if on.len() < 3 {
            continue;
        }
        let centre = mul(
            on.iter().fold([0.0; 3], |s, &i| add(s, verts[i])),
            1.0 / on.len() as f64,
        );
        let u = normalize(sub3(verts[on[0]], centre));
        let w = cross(*n, u);
        on.sort_by(|&i, &j| {
            let ang = |q: usize| {
                let r = sub3(verts[q], centre);
                dot3(r, w).atan2(dot3(r, u))
            };
            ang(i).total_cmp(&ang(j))
        });
        let mut edges = Vec::new();
        for s in 0..on.len() {
            let (i, j) = (on[s], on[(s + 1) % on.len()]);
            let key = (i.min(j), i.max(j));
            let e = edge_map
                .entry(key)
                .or_insert_with(|| builder::line(&tverts[key.0], &tverts[key.1]))
                .clone();
            edges.push(if i < j { e } else { e.inverse() });
        }
        let wire: Wire = edges.into();
        faces.push(
            builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Face(e.to_string()))?,
        );
    }
    let shell: Shell = faces.into();
    let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Face(e.to_string()))?;
    Ok(orient_outward(solid))
}

/// One segment of a sweep path in 3D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    Line {
        from: [f64; 3],
        to: [f64; 3],
    },
    /// Rotation by `angle` (radians, right-handed about `axis`) around `center`.
    Arc {
        from: [f64; 3],
        to: [f64; 3],
        center: [f64; 3],
        axis: [f64; 3],
        angle: f64,
    },
}

impl PathSegment {
    fn from(&self) -> [f64; 3] {
        match *self {
            PathSegment::Line { from, .. } | PathSegment::Arc { from, .. } => from,
        }
    }
    fn start_tangent(&self) -> [f64; 3] {
        match *self {
            PathSegment::Line { from, to } => normalize(sub3(to, from)),
            PathSegment::Arc {
                from,
                center,
                axis,
                angle,
                ..
            } => normalize(mul(cross(axis, sub3(from, center)), angle.signum())),
        }
    }
    fn end_tangent(&self) -> [f64; 3] {
        match *self {
            PathSegment::Line { from, to } => normalize(sub3(to, from)),
            PathSegment::Arc {
                to,
                center,
                axis,
                angle,
                ..
            } => normalize(mul(cross(axis, sub3(to, center)), angle.signum())),
        }
    }
    fn translated(&self, o: [f64; 3]) -> PathSegment {
        match *self {
            PathSegment::Line { from, to } => PathSegment::Line {
                from: add(from, o),
                to: add(to, o),
            },
            PathSegment::Arc {
                from,
                to,
                center,
                axis,
                angle,
            } => PathSegment::Arc {
                from: add(from, o),
                to: add(to, o),
                center: add(center, o),
                axis,
                angle,
            },
        }
    }
}

/// Sweeps a profile along a smooth path. The path is moved so that it starts at
/// `anchor` (a point inside the profile), so the profile follows the path's shape.
pub fn sweep(
    plane: &Plane,
    profile: &Profile,
    path: &[PathSegment],
    anchor: [f64; 2],
) -> Result<Solid> {
    if path.is_empty() {
        return Err(KernelError::InvalidParameter("the path is empty".into()));
    }
    for w in path.windows(2) {
        if dot3(w[0].end_tangent(), w[1].start_tangent()) < 0.9998 {
            return Err(KernelError::InvalidParameter(
                "the path has a sharp corner; join lines with arcs so it is smooth".into(),
            ));
        }
    }
    let n = plane.normal();
    if dot3(n, path[0].start_tangent()).abs() < 0.05 {
        return Err(KernelError::InvalidParameter(
            "the profile lies along the path; sketch it on a plane that crosses the path start"
                .into(),
        ));
    }
    guarded("sweep", || {
        let offset = sub3(plane.to_world(anchor), path[0].from());
        let path: Vec<PathSegment> = path.iter().map(|s| s.translated(offset)).collect();
        let mut loops = vec![loop_to_wire(plane, &profile.outer.segments)?];
        for h in &profile.holes {
            let w = loop_to_wire(plane, &h.segments)?;
            loops.push(
                if matches!(h.segments.as_slice(), [Segment::Circle { .. }]) {
                    w.inverse()
                } else {
                    w
                },
            );
        }
        let start_cap =
            builder::try_attach_plane(&loops).map_err(|e| KernelError::Face(e.to_string()))?;
        let mut walls: Vec<Face> = Vec::new();
        let mut current = loops.clone();
        for seg in &path {
            let mut next = Vec::new();
            for w in &current {
                let swept: Shell = match *seg {
                    PathSegment::Line { from, to } => builder::tsweep(w, v3(sub3(to, from))),
                    PathSegment::Arc {
                        center,
                        axis,
                        angle,
                        ..
                    } => builder::rsweep(w, p3(center), v3(normalize(axis)), Rad(angle)),
                };
                let end = swept
                    .extract_boundaries()
                    .into_iter()
                    .find(|b| {
                        !b.edge_iter()
                            .any(|e| w.edge_iter().any(|f| f.id() == e.id()))
                    })
                    .ok_or_else(|| KernelError::Face("could not follow the path".into()))?;
                // keep the walls consistently oriented across segments
                let mut trial: Shell = walls.iter().cloned().collect();
                trial.extend(swept.face_iter().cloned());
                if walls.is_empty()
                    || matches!(
                        trial.shell_condition(),
                        truck_topology::shell::ShellCondition::Oriented
                            | truck_topology::shell::ShellCondition::Closed
                    )
                {
                    walls.extend(swept.face_iter().cloned());
                } else {
                    walls.extend(swept.face_iter().map(|f| f.inverse()));
                }
                next.push(end);
            }
            current = next;
        }
        // end cap: the end wires were produced in sweep orientation; try both ways.
        let end_cap =
            builder::try_attach_plane(&current).map_err(|e| KernelError::Face(e.to_string()))?;
        close_shell(walls, vec![start_cap, end_cap])
    })
}

/// Assembles walls and caps into a closed, outward-oriented solid, flipping caps
/// as needed.
fn close_shell(walls: Vec<Face>, caps: Vec<Face>) -> Result<Solid> {
    use truck_topology::shell::ShellCondition;
    let n = caps.len();
    for mask in 0..(1u32 << n) {
        let mut faces = walls.clone();
        for (i, c) in caps.iter().enumerate() {
            faces.push(if mask & (1 << i) != 0 {
                c.inverse()
            } else {
                c.clone()
            });
        }
        let shell: Shell = faces.into();
        if shell.shell_condition() == ShellCondition::Closed {
            let solid =
                Solid::try_new(vec![shell]).map_err(|e| KernelError::Face(e.to_string()))?;
            return Ok(orient_outward(solid));
        }
    }
    // walls may be inward: flip all walls and retry
    let walls: Vec<Face> = walls.iter().map(|f| f.inverse()).collect();
    for mask in 0..(1u32 << n) {
        let mut faces = walls.clone();
        for (i, c) in caps.iter().enumerate() {
            faces.push(if mask & (1 << i) != 0 {
                c.inverse()
            } else {
                c.clone()
            });
        }
        let shell: Shell = faces.into();
        if shell.shell_condition() == truck_topology::shell::ShellCondition::Closed {
            let solid =
                Solid::try_new(vec![shell]).map_err(|e| KernelError::Face(e.to_string()))?;
            return Ok(orient_outward(solid));
        }
    }
    Err(KernelError::Face("the result is not a closed solid".into()))
}

/// Number of points used when two loft outlines have different segment counts.
const LOFT_SAMPLES: usize = 48;

/// Lofts between two closed profiles (outer outlines only).
pub fn loft(plane_a: &Plane, a: &Profile, plane_b: &Plane, b: &Profile) -> Result<Solid> {
    if !a.holes.is_empty() || !b.holes.is_empty() {
        return Err(KernelError::InvalidParameter(
            "loft between regions with holes is not supported yet".into(),
        ));
    }
    guarded("loft", || {
        // Exact loft when both outlines are made of the same number of segments
        // (and neither is a full circle, which has no natural start point).
        let simple = |p: &Profile| !matches!(p.outer.segments.as_slice(), [Segment::Circle { .. }]);
        if a.outer.segments.len() == b.outer.segments.len() && simple(a) && simple(b) {
            let w0 = loop_to_wire(plane_a, &a.outer.segments)?;
            let w1 = loop_to_wire(plane_b, &b.outer.segments)?;
            let (w1, ok) = align_wire(&w0, w1);
            if ok {
                if let Ok(s) = loft_wires(w0, w1) {
                    return Ok(s);
                }
            }
        }
        // General case: matching polygons with LOFT_SAMPLES points.
        let pa: Vec<[f64; 3]> = resample(&a.outer.polyline(), LOFT_SAMPLES)
            .into_iter()
            .map(|p| plane_a.to_world(p))
            .collect();
        let mut pb: Vec<[f64; 3]> = resample(&b.outer.polyline(), LOFT_SAMPLES)
            .into_iter()
            .map(|p| plane_b.to_world(p))
            .collect();
        if dot3(newell(&pa), newell(&pb)) < 0.0 {
            pb.reverse();
        }
        // rotate b so corresponding points are closest (minimal twist)
        let best = (0..pb.len())
            .min_by(|&i, &j| twist(&pa, &pb, i).total_cmp(&twist(&pa, &pb, j)))
            .unwrap_or(0);
        pb.rotate_left(best);
        let w0 = wire_from_points(&pa, None);
        let w1 = wire_from_points(&pb, None);
        loft_wires(w0, w1)
    })
}

fn loft_wires(w0: Wire, w1: Wire) -> Result<Solid> {
    let walls =
        builder::try_wire_homotopy(&w0, &w1).map_err(|e| KernelError::Face(e.to_string()))?;
    let c0 = builder::try_attach_plane(&[w0]).map_err(|e| KernelError::Face(e.to_string()))?;
    let c1 = builder::try_attach_plane(&[w1]).map_err(|e| KernelError::Face(e.to_string()))?;
    let solid = close_shell(walls.face_iter().cloned().collect(), vec![c0, c1])?;
    if !is_valid_solid(&solid) {
        return Err(KernelError::Face("the loft is not a closed solid".into()));
    }
    Ok(solid)
}

/// Re-orders `w1`'s edges to start at the vertex closest to `w0`'s start and to run
/// in the same rotational direction. Returns false if that is not possible.
fn align_wire(w0: &Wire, w1: Wire) -> (Wire, bool) {
    let p0: Vec<[f64; 3]> = w0.vertex_iter().map(|v| pt(v.point())).collect();
    let mut edges: Vec<Edge> = w1.edge_iter().cloned().collect();
    let p1: Vec<[f64; 3]> = w1.vertex_iter().map(|v| pt(v.point())).collect();
    if p0.len() != p1.len() || p0.len() < 3 {
        return (w1, false);
    }
    if dot3(newell(&p0), newell(&p1)) < 0.0 {
        edges = edges.iter().rev().map(|e| e.inverse()).collect();
    }
    let pts: Vec<[f64; 3]> = edges.iter().map(|e| pt(e.front().point())).collect();
    let best = (0..pts.len())
        .min_by(|&i, &j| twist(&p0, &pts, i).total_cmp(&twist(&p0, &pts, j)))
        .unwrap_or(0);
    edges.rotate_left(best);
    (edges.into(), true)
}

fn twist(a: &[[f64; 3]], b: &[[f64; 3]], shift: usize) -> f64 {
    let n = b.len();
    a.iter()
        .enumerate()
        .map(|(i, p)| {
            let q = b[(i + shift) % n];
            dot3(sub3(*p, q), sub3(*p, q))
        })
        .sum()
}

fn newell(p: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// Resamples a closed polyline into `n` points evenly spaced by arc length.
fn resample(poly: &[[f64; 2]], n: usize) -> Vec<[f64; 2]> {
    let m = poly.len();
    let seg = |i: usize| {
        let (a, b) = (poly[i], poly[(i + 1) % m]);
        ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
    };
    let total: f64 = (0..m).map(seg).sum();
    let mut out = Vec::with_capacity(n);
    let (mut i, mut acc) = (0usize, 0.0f64);
    for k in 0..n {
        let target = total * k as f64 / n as f64;
        while i < m - 1 && acc + seg(i) < target {
            acc += seg(i);
            i += 1;
        }
        let l = seg(i).max(1e-12);
        let t = ((target - acc) / l).clamp(0.0, 1.0);
        let (a, b) = (poly[i], poly[(i + 1) % m]);
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use calibro_sketch::{find_profiles, Sketch};
    use std::f64::consts::PI;

    fn vol(s: &Solid) -> f64 {
        tessellate(s, 0.002).volume()
    }

    #[test]
    fn fillet_box_edge() {
        let b = make_box([0.0; 3], [20.0, 10.0, 10.0]).unwrap();
        let r = 2.0;
        let f = fillet(&b, &[[[0.0, 0.0, 10.0], [20.0, 0.0, 10.0]]], r).unwrap();
        assert!(is_valid_solid(&f));
        let removed = (r * r - PI * r * r / 4.0) * 20.0;
        let v = vol(&f);
        assert!(
            (v - (2000.0 - removed)).abs() < 1.0,
            "{v} vs {}",
            2000.0 - removed
        );
        // several radii, and two edges meeting at a corner
        for r in [0.5, 1.0, 3.0, 4.5] {
            let f = fillet(&b, &[[[0.0, 0.0, 10.0], [20.0, 0.0, 10.0]]], r).unwrap();
            assert!(is_valid_solid(&f), "r={r}");
        }
        // edges meeting at a corner: refused with a clear message
        assert!(fillet(
            &b,
            &[
                [[0.0, 0.0, 10.0], [20.0, 0.0, 10.0]],
                [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]]
            ],
            1.5
        )
        .is_err());
        // the four vertical edges of the box (no shared corners)
        let verticals = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]
            .map(|[x, y]| [[x, y, 0.0], [x, y, 10.0]]);
        let four = fillet(&b, &verticals, 2.0).unwrap();
        assert!(is_valid_solid(&four));
        let exp = 2000.0 - 4.0 * (4.0 - PI) * 10.0;
        assert!((vol(&four) - exp).abs() < 2.0, "{} vs {exp}", vol(&four));
    }

    #[test]
    fn chamfer_two_edges_and_reject_bad_edges() {
        let b = make_box([0.0; 3], [20.0, 10.0, 10.0]).unwrap();
        let c = chamfer(
            &b,
            &[
                [[0.0, 0.0, 10.0], [20.0, 0.0, 10.0]],
                [[0.0, 10.0, 10.0], [20.0, 10.0, 10.0]],
            ],
            1.0,
        )
        .unwrap();
        let v = vol(&c);
        assert!((v - (2000.0 - 2.0 * 0.5 * 20.0)).abs() < 0.2, "{v}");
        // an edge that doesn't exist
        assert!(chamfer(&b, &[[[1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]], 1.0).is_err());
        // an edge touching a curved face
        let cyl = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let m = tessellate(&cyl, 0.01);
        let e = &m.edges[0];
        let (a, z) = (e[0].map(|x| x as f64), e[e.len() - 1].map(|x| x as f64));
        assert!(fillet(&cyl, &[[a, z]], 1.0).is_err());
    }

    #[test]
    fn shell_open_box() {
        let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
        let top = Plane {
            origin: [0.0, 0.0, 20.0],
            ..Plane::XY
        };
        let s = shell(&b, &[top], 2.0).unwrap();
        let v = vol(&s);
        let exp = 24000.0 - 36.0 * 26.0 * 18.0;
        assert!((v - exp).abs() < 0.5, "{v} vs {exp}");
        // a chamfered box is still convex (its hair-thin tool slivers are ignored)
        let c = chamfer(&b, &[[[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]]], 1.0).unwrap();
        let s2 = shell(&c, &[top], 2.0).unwrap();
        assert!(vol(&s2) < vol(&c) && vol(&s2) > exp - 50.0, "{}", vol(&s2));
        // non-convex / curved bodies are refused with a message
        let cyl = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        assert!(shell(&cyl, &[], 1.0).is_err());
    }

    #[test]
    fn sweep_along_line_and_arc() {
        // 4x4 square profile on the XZ plane swept along +Y 20, then a quarter arc.
        let mut s = Sketch::new();
        s.add_rectangle([-2.0, -2.0], [2.0, 2.0]);
        let prof = find_profiles(&s).remove(0);
        let plane = Plane::XZ; // normal -Y; tangent along +Y is fine
        let path = vec![
            PathSegment::Line {
                from: [0.0, 0.0, 0.0],
                to: [0.0, 20.0, 0.0],
            },
            PathSegment::Arc {
                from: [0.0, 20.0, 0.0],
                to: [10.0, 30.0, 0.0],
                center: [10.0, 20.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                angle: -PI / 2.0,
            },
        ];
        let sol = sweep(&plane, &prof, &path, [0.0, 0.0]).unwrap();
        assert!(is_valid_solid(&sol));
        let exp = 16.0 * 20.0 + 16.0 * (PI / 2.0 * 10.0);
        let v = vol(&sol);
        assert!((v - exp).abs() / exp < 5e-3, "{v} vs {exp}");
        // sharp corner refused
        let bad = vec![
            PathSegment::Line {
                from: [0.0; 3],
                to: [0.0, 10.0, 0.0],
            },
            PathSegment::Line {
                from: [0.0, 10.0, 0.0],
                to: [10.0, 10.0, 0.0],
            },
        ];
        assert!(sweep(&plane, &prof, &bad, [0.0, 0.0]).is_err());
    }

    #[test]
    fn loft_square_to_square_and_circle() {
        let mut s1 = Sketch::new();
        s1.add_rectangle([-10.0, -10.0], [10.0, 10.0]);
        let mut s2 = Sketch::new();
        s2.add_rectangle([-5.0, -5.0], [5.0, 5.0]);
        let a = find_profiles(&s1).remove(0);
        let b = find_profiles(&s2).remove(0);
        let top = Plane {
            origin: [0.0, 0.0, 12.0],
            ..Plane::XY
        };
        let l = loft(&Plane::XY, &a, &top, &b).unwrap();
        // frustum of a square pyramid: h/3 (A1 + A2 + sqrt(A1 A2))
        let exp = 12.0 / 3.0 * (400.0 + 100.0 + 200.0);
        let v = vol(&l);
        assert!((v - exp).abs() / exp < 1e-3, "{v} vs {exp}");
        let mut s3 = Sketch::new();
        s3.add_circle([0.0, 0.0], 6.0);
        let c = find_profiles(&s3).remove(0);
        let l2 = loft(&Plane::XY, &a, &top, &c).unwrap();
        assert!(is_valid_solid(&l2));
        let v2 = vol(&l2);
        assert!(v2 > 12.0 * 113.0 && v2 < 12.0 * 400.0, "{v2}");
    }
}
