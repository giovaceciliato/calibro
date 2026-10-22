//! # calibro-kernel
//!
//! Thin adapter between Calibro's document model and the B-rep kernel
//! ([truck](https://github.com/ricosjp/truck), pure Rust, Apache-2.0).
//!
//! The rest of the application only talks to this crate, never to truck
//! directly. That keeps the door open for a second backend (for example
//! Open CASCADE through bindings on desktop) for operations truck does not
//! yet offer, such as fillets, chamfers and shells.
//!
//! Three representations are kept separate, as required by the spec:
//! 1. [`Solid`] — the authoritative exact B-rep geometry.
//! 2. [`BodyMesh`] — a tessellated display mesh derived from it.
//! 3. `face_planes` / face ids / edge polylines — picking and interaction data.

use calibro_sketch::{Profile, Segment};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use truck_meshalgo::prelude::*;
use truck_modeling::{builder, Rad};

pub use truck_modeling::{Point3, Solid, Vector3};

pub type Wire = truck_modeling::Wire;
pub type Face = truck_modeling::Face;
pub type Vertex = truck_modeling::Vertex;

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum KernelError {
    #[error("the sketch has no closed profile to use")]
    NoProfile,
    #[error("could not build a planar face from the profile: {0}")]
    Face(String),
    #[error("the boolean failed: the kernel could not intersect these shapes (usually faces lying exactly on top of each other — move or resize one body slightly)")]
    BooleanFailed,
    #[error("the boolean operation produced an empty result")]
    EmptyResult,
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("the geometry kernel could not handle this case ({0}); try changing the dimensions slightly")]
    Internal(String),
}

/// Runs a kernel operation, converting a panic inside the kernel into an error
/// so one bad operation cannot crash the application.
///
/// Note: on WebAssembly panics abort instead of unwinding, so this guard only
/// protects native builds; release builds of truck skip the internal debug
/// assertions that cause most of these panics, and results are validated.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "internal error".into());
            Err(KernelError::Internal(format!("{what}: {msg}")))
        }
    }
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// A 2D coordinate frame in 3D space used for sketches.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

impl Plane {
    /// Top plane (XY), normal +Z. Calibro is Z-up.
    pub const XY: Plane = Plane {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    /// Front plane (XZ), normal -Y (facing the viewer in front view).
    pub const XZ: Plane = Plane {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    /// Right plane (YZ), normal +X.
    pub const YZ: Plane = Plane {
        origin: [0.0; 3],
        x_axis: [0.0, 1.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };

    pub fn normal(&self) -> [f64; 3] {
        normalize(cross(self.x_axis, self.y_axis))
    }

    pub fn to_world(&self, p: [f64; 2]) -> [f64; 3] {
        [
            self.origin[0] + self.x_axis[0] * p[0] + self.y_axis[0] * p[1],
            self.origin[1] + self.x_axis[1] * p[0] + self.y_axis[1] * p[1],
            self.origin[2] + self.x_axis[2] * p[0] + self.y_axis[2] * p[1],
        ]
    }

    pub fn to_local(&self, p: [f64; 3]) -> [f64; 2] {
        let d = sub3(p, self.origin);
        [dot3(d, self.x_axis), dot3(d, self.y_axis)]
    }

    /// Builds an orthonormal plane from an origin and normal.
    pub fn from_origin_normal(origin: [f64; 3], normal: [f64; 3]) -> Plane {
        let n = normalize(normal);
        let helper = if n[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let x = normalize(cross(helper, n));
        let y = cross(n, x);
        // keep x horizontal-ish for vertical faces: use world Z as "up" when possible
        Plane {
            origin,
            x_axis: x,
            y_axis: y,
        }
    }

    pub fn offset(&self, d: f64) -> Plane {
        let n = self.normal();
        Plane {
            origin: [
                self.origin[0] + n[0] * d,
                self.origin[1] + n[1] * d,
                self.origin[2] + n[2] * d,
            ],
            ..*self
        }
    }
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn normalize(a: [f64; 3]) -> [f64; 3] {
    let l = dot3(a, a).sqrt().max(1e-300);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn p3(a: [f64; 3]) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}
fn v3(a: [f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

// ---------------------------------------------------------------------------
// Profiles -> faces
// ---------------------------------------------------------------------------

fn loop_to_wire(plane: &Plane, segments: &[Segment]) -> Result<Wire> {
    if let [Segment::Circle { center, radius }] = segments {
        // two half arcs
        let a = builder::vertex(p3(plane.to_world([center[0] + radius, center[1]])));
        let b = builder::vertex(p3(plane.to_world([center[0] - radius, center[1]])));
        let top = p3(plane.to_world([center[0], center[1] + radius]));
        let bottom = p3(plane.to_world([center[0], center[1] - radius]));
        let e0 = builder::circle_arc(&a, &b, top);
        let e1 = builder::circle_arc(&b, &a, bottom);
        return Ok(vec![e0, e1].into());
    }
    let n = segments.len();
    if n == 0 {
        return Err(KernelError::NoProfile);
    }
    let start_point = |s: &Segment| match *s {
        Segment::Line { from, .. } | Segment::Arc { from, .. } => from,
        Segment::Circle { .. } => unreachable!(),
    };
    let verts: Vec<Vertex> = segments
        .iter()
        .map(|s| builder::vertex(p3(plane.to_world(start_point(s)))))
        .collect();
    let mut edges = Vec::with_capacity(n);
    for (i, s) in segments.iter().enumerate() {
        let (v0, v1) = (&verts[i], &verts[(i + 1) % n]);
        let e = match s {
            Segment::Line { .. } => builder::line(v0, v1),
            Segment::Arc { .. } => builder::circle_arc(v0, v1, p3(plane.to_world(s.midpoint()))),
            Segment::Circle { .. } => {
                return Err(KernelError::Face("circle inside a segment loop".into()))
            }
        };
        edges.push(e);
    }
    Ok(edges.into())
}

/// Builds a planar face (outer boundary + holes) on `plane` from a sketch profile.
pub fn profile_to_face(plane: &Plane, profile: &Profile) -> Result<Face> {
    let mut wires = vec![loop_to_wire(plane, &profile.outer.segments)?];
    for h in &profile.holes {
        let w = loop_to_wire(plane, &h.segments)?;
        // Holes must run clockwise. Segment loops already carry their direction;
        // a full circle is always built counter-clockwise, so flip it.
        if matches!(h.segments.as_slice(), [Segment::Circle { .. }]) {
            wires.push(w.inverse());
        } else {
            wires.push(w);
        }
    }
    builder::try_attach_plane(&wires).map_err(|e| KernelError::Face(e.to_string()))
}

/// Triangulated fill of a sketch region (for highlighting regions in the viewport).
pub fn profile_fill(plane: &Plane, profile: &Profile, tol: f64) -> Result<Vec<[f32; 3]>> {
    let face = profile_to_face(plane, profile)?;
    let shell: truck_modeling::Shell = vec![face].into();
    let poly = shell.triangulation(tol).to_polygon();
    let p = poly.positions();
    let mut out = Vec::new();
    for t in poly.faces().triangle_iter() {
        for v in t.iter() {
            let q = p[v.pos];
            out.push([q.x as f32, q.y as f32, q.z as f32]);
        }
    }
    Ok(out)
}

/// Ensures the solid's boundary points outward (positive volume).
fn orient_outward(mut solid: Solid) -> Solid {
    if signed_volume(&solid) < 0.0 {
        solid.not();
    }
    solid
}

/// Extrudes each profile by `distance` along the plane normal (negative = opposite
/// direction). With `symmetric`, the solid extends `distance/2` each side.
pub fn extrude(
    plane: &Plane,
    profiles: &[Profile],
    distance: f64,
    symmetric: bool,
) -> Result<Vec<Solid>> {
    guarded("extrude", || {
        extrude_unguarded(plane, profiles, distance, symmetric)
    })
}

fn extrude_unguarded(
    plane: &Plane,
    profiles: &[Profile],
    distance: f64,
    symmetric: bool,
) -> Result<Vec<Solid>> {
    if profiles.is_empty() {
        return Err(KernelError::NoProfile);
    }
    if distance.abs() < 1e-9 {
        return Err(KernelError::InvalidParameter(
            "extrude distance must not be zero".into(),
        ));
    }
    let n = plane.normal();
    let (start, len) = if symmetric {
        (plane.offset(-distance.abs() * 0.5), distance.abs())
    } else {
        (*plane, distance)
    };
    profiles
        .iter()
        .map(|p| {
            let face = profile_to_face(&start, p)?;
            let solid = builder::tsweep(&face, v3([n[0] * len, n[1] * len, n[2] * len]));
            Ok(orient_outward(solid))
        })
        .collect()
}

/// Overlap used when retrying a join/cut whose faces would be exactly coplanar
/// (truck's boolean engine cannot intersect coplanar faces). Larger than
/// [`BOOLEAN_TOLERANCE`] so the faces are no longer treated as touching.
pub const COPLANAR_OVERLAP: f64 = 0.2;

/// Like [`extrude`], but the solid also extends `back` units behind the sketch
/// plane (opposite to the extrusion direction). Used to make a tool body
/// overlap the body it is joined to / cut from instead of touching it.
pub fn extrude_with_overlap(
    plane: &Plane,
    profiles: &[Profile],
    distance: f64,
    symmetric: bool,
    back: f64,
) -> Result<Vec<Solid>> {
    if symmetric {
        return extrude(plane, profiles, distance.abs() + 2.0 * back, true);
    }
    let s = distance.signum();
    extrude(
        &plane.offset(-s * back),
        profiles,
        distance + s * back,
        false,
    )
}

/// Revolves each profile around an axis lying in the sketch plane.
/// `axis_origin` and `axis_dir` are 2D sketch coordinates.
pub fn revolve(
    plane: &Plane,
    profiles: &[Profile],
    axis_origin: [f64; 2],
    axis_dir: [f64; 2],
    angle_deg: f64,
) -> Result<Vec<Solid>> {
    guarded("revolve", || {
        revolve_unguarded(plane, profiles, axis_origin, axis_dir, angle_deg)
    })
}

fn revolve_unguarded(
    plane: &Plane,
    profiles: &[Profile],
    axis_origin: [f64; 2],
    axis_dir: [f64; 2],
    angle_deg: f64,
) -> Result<Vec<Solid>> {
    if profiles.is_empty() {
        return Err(KernelError::NoProfile);
    }
    if angle_deg.abs() < 1e-6 {
        return Err(KernelError::InvalidParameter(
            "revolve angle must not be zero".into(),
        ));
    }
    let o = plane.to_world(axis_origin);
    let tip = plane.to_world([axis_origin[0] + axis_dir[0], axis_origin[1] + axis_dir[1]]);
    let axis = normalize(sub3(tip, o));
    // The profile must not cross the axis.
    let d2 = {
        let l = (axis_dir[0].powi(2) + axis_dir[1].powi(2)).sqrt();
        [axis_dir[0] / l, axis_dir[1] / l]
    };
    for p in profiles {
        let side = p
            .outer
            .polyline()
            .iter()
            .map(|q| d2[0] * (q[1] - axis_origin[1]) - d2[1] * (q[0] - axis_origin[0]))
            .collect::<Vec<_>>();
        let pos = side.iter().any(|s| *s > 1e-7);
        let neg = side.iter().any(|s| *s < -1e-7);
        if pos && neg {
            return Err(KernelError::InvalidParameter(
                "the profile crosses the revolve axis".into(),
            ));
        }
    }
    let angle = angle_deg.clamp(-360.0, 360.0).to_radians();
    profiles
        .iter()
        .map(|p| {
            let face = profile_to_face(plane, p)?;
            let solid = builder::rsweep(&face, p3(o), v3(axis), Rad(angle));
            Ok(orient_outward(solid))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

pub fn make_box(min: [f64; 3], size: [f64; 3]) -> Result<Solid> {
    if size.iter().any(|s| *s <= 0.0) {
        return Err(KernelError::InvalidParameter(
            "box size must be positive".into(),
        ));
    }
    let v = builder::vertex(p3(min));
    let e = builder::tsweep(&v, Vector3::new(size[0], 0.0, 0.0));
    let f = builder::tsweep(&e, Vector3::new(0.0, size[1], 0.0));
    Ok(orient_outward(builder::tsweep(
        &f,
        Vector3::new(0.0, 0.0, size[2]),
    )))
}

pub fn make_cylinder(base_center: [f64; 3], radius: f64, height: f64) -> Result<Solid> {
    if radius <= 0.0 || height <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "cylinder radius and height must be positive".into(),
        ));
    }
    let plane = Plane {
        origin: base_center,
        ..Plane::XY
    };
    let profile = Profile {
        outer: calibro_sketch::Loop {
            segments: vec![Segment::Circle {
                center: [0.0, 0.0],
                radius,
            }],
        },
        holes: vec![],
    };
    Ok(extrude(&plane, &[profile], height, false)?.remove(0))
}

pub fn make_sphere(center: [f64; 3], radius: f64) -> Result<Solid> {
    if radius <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "sphere radius must be positive".into(),
        ));
    }
    let c = p3(center);
    let top = builder::vertex(c + Vector3::new(0.0, 0.0, radius));
    let bottom = builder::vertex(c - Vector3::new(0.0, 0.0, radius));
    let arc = builder::circle_arc(&top, &bottom, c + Vector3::new(radius, 0.0, 0.0));
    let wire: Wire = vec![arc].into();
    let shell = builder::cone(&wire, Vector3::unit_z(), Rad(std::f64::consts::TAU));
    Ok(orient_outward(Solid::new(vec![shell])))
}

// ---------------------------------------------------------------------------
// Booleans & transforms
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOp {
    Union,
    Subtract,
    Intersect,
}

/// Tolerance passed to truck's boolean engine (model units, mm).
pub const BOOLEAN_TOLERANCE: f64 = 0.05;

pub fn boolean(a: &Solid, b: &Solid, op: BooleanOp) -> Result<Solid> {
    guarded("boolean", || boolean_unguarded(a, b, op))
}

fn boolean_unguarded(a: &Solid, b: &Solid, op: BooleanOp) -> Result<Solid> {
    let res = match op {
        BooleanOp::Union => truck_shapeops::or(a, b, BOOLEAN_TOLERANCE),
        BooleanOp::Intersect => truck_shapeops::and(a, b, BOOLEAN_TOLERANCE),
        BooleanOp::Subtract => {
            let mut nb = b.clone();
            nb.not();
            truck_shapeops::and(a, &nb, BOOLEAN_TOLERANCE)
        }
    }
    .ok_or(KernelError::BooleanFailed)?;
    if res.boundaries().is_empty() {
        return Err(KernelError::EmptyResult);
    }
    if !is_valid_solid(&res) {
        return Err(KernelError::BooleanFailed);
    }
    Ok(res)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translate: [f64; 3],
    /// Rotation axis through `pivot`.
    pub rotate_axis: [f64; 3],
    pub rotate_deg: f64,
    pub pivot: [f64; 3],
    pub scale: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translate: [0.0; 3],
            rotate_axis: [0.0, 0.0, 1.0],
            rotate_deg: 0.0,
            pivot: [0.0; 3],
            scale: 1.0,
        }
    }
}

pub fn transform(solid: &Solid, t: &Transform) -> Result<Solid> {
    if t.scale <= 0.0 {
        return Err(KernelError::InvalidParameter(
            "scale must be positive".into(),
        ));
    }
    let mut s = solid.clone();
    if (t.scale - 1.0).abs() > 1e-12 {
        s = builder::scaled(&s, p3(t.pivot), Vector3::new(t.scale, t.scale, t.scale));
    }
    if t.rotate_deg.abs() > 1e-12 {
        s = builder::rotated(
            &s,
            p3(t.pivot),
            v3(normalize(t.rotate_axis)),
            Rad(t.rotate_deg.to_radians()),
        );
    }
    if dot3(t.translate, t.translate) > 0.0 {
        s = builder::translated(&s, v3(t.translate));
    }
    Ok(s)
}

/// Mirror image of a solid across a plane.
pub fn mirror(solid: &Solid, plane: &Plane) -> Result<Solid> {
    use truck_modeling::Matrix4;
    let n = plane.normal();
    let o = plane.origin;
    // Householder reflection: x' = x - 2 n (n.(x - o))
    let d = dot3(n, o);
    let m = Matrix4::new(
        1.0 - 2.0 * n[0] * n[0],
        -2.0 * n[0] * n[1],
        -2.0 * n[0] * n[2],
        0.0,
        -2.0 * n[1] * n[0],
        1.0 - 2.0 * n[1] * n[1],
        -2.0 * n[1] * n[2],
        0.0,
        -2.0 * n[2] * n[0],
        -2.0 * n[2] * n[1],
        1.0 - 2.0 * n[2] * n[2],
        0.0,
        2.0 * d * n[0],
        2.0 * d * n[1],
        2.0 * d * n[2],
        1.0,
    );
    Ok(orient_outward(builder::transformed(solid, m)))
}

// ---------------------------------------------------------------------------
// Tessellation & analysis
// ---------------------------------------------------------------------------

/// Display / export mesh of one body. Triangles reference `positions`;
/// `face_ids[i]` is the B-rep face that produced vertex `i` (for picking).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub face_ids: Vec<u32>,
    pub indices: Vec<u32>,
    /// Boundary edges of B-rep faces as polylines (for edge display/selection).
    pub edges: Vec<Vec<[f32; 3]>>,
    /// For each B-rep face: its plane if planar (enables sketching on faces).
    pub face_planes: Vec<Option<Plane>>,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
}

impl BodyMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Enclosed volume (divergence theorem). Positive for outward-facing closed meshes.
    pub fn volume(&self) -> f64 {
        let mut v = 0.0;
        for t in self.indices.chunks_exact(3) {
            let a = self.positions[t[0] as usize].map(|x| x as f64);
            let b = self.positions[t[1] as usize].map(|x| x as f64);
            let c = self.positions[t[2] as usize].map(|x| x as f64);
            v += dot3(a, cross(b, c)) / 6.0;
        }
        v
    }

    pub fn surface_area(&self) -> f64 {
        let mut s = 0.0;
        for t in self.indices.chunks_exact(3) {
            let a = self.positions[t[0] as usize].map(|x| x as f64);
            let b = self.positions[t[1] as usize].map(|x| x as f64);
            let c = self.positions[t[2] as usize].map(|x| x as f64);
            let n = cross(sub3(b, a), sub3(c, a));
            s += dot3(n, n).sqrt() * 0.5;
        }
        s
    }
}

/// Chord tolerance suitable for a solid of the given size.
pub fn default_tolerance(solid: &Solid) -> f64 {
    let bb = solid_bbox(solid);
    let d = sub3(bb.1, bb.0);
    (dot3(d, d).sqrt() * 0.001).clamp(0.005, 0.5)
}

fn solid_bbox(solid: &Solid) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for shell in solid.boundaries() {
        for v in shell.vertex_iter() {
            let p = v.point();
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    if !min[0].is_finite() {
        return ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

fn signed_volume(solid: &Solid) -> f64 {
    let tol = default_tolerance(solid) * 4.0;
    tessellate(solid, tol).volume()
}

/// Tessellates a solid into a display/export mesh with chord tolerance `tol`.
/// Returns an empty mesh if the kernel fails.
pub fn tessellate(solid: &Solid, tol: f64) -> BodyMesh {
    guarded("tessellation", || Ok(tessellate_unguarded(solid, tol))).unwrap_or_default()
}

fn tessellate_unguarded(solid: &Solid, tol: f64) -> BodyMesh {
    let meshed = solid.triangulation(tol);
    let mut out = BodyMesh::default();
    // face planes from the exact geometry
    for face in solid.face_iter() {
        let plane = match face.surface() {
            truck_modeling::Surface::Plane(pl) => {
                let o = pl.subs(0.0, 0.0);
                let mut n = pl.normal();
                if !face.orientation() {
                    n = -n;
                }
                Some(Plane::from_origin_normal([o.x, o.y, o.z], [n.x, n.y, n.z]))
            }
            _ => None,
        };
        out.face_planes.push(plane);
    }
    for (fid, face) in meshed.face_iter().enumerate() {
        let Some(mut poly) = face.surface() else {
            continue;
        };
        if !face.orientation() {
            poly.invert();
        }
        let positions = poly.positions();
        let normals = poly.normals();
        for tri in poly.faces().triangle_iter() {
            let a = positions[tri[0].pos];
            let b = positions[tri[1].pos];
            let c = positions[tri[2].pos];
            let flat = {
                let n = (b - a).cross(c - a);
                let l = n.magnitude();
                if l < 1e-300 {
                    continue;
                }
                n / l
            };
            for v in tri.iter() {
                let p = positions[v.pos];
                let n = v
                    .nor
                    .and_then(|i| normals.get(i))
                    .filter(|n| n.magnitude() > 0.5 && n.dot(flat) > 0.0)
                    .unwrap_or(flat);
                out.indices.push(out.positions.len() as u32);
                out.positions.push([p.x as f32, p.y as f32, p.z as f32]);
                out.normals.push([n.x as f32, n.y as f32, n.z as f32]);
                out.face_ids.push(fid as u32);
            }
        }
    }
    weld(&mut out);
    let mut seen = HashSet::new();
    for edge in meshed.edge_iter() {
        if !seen.insert(edge.id()) {
            continue;
        }
        let curve = edge.curve();
        out.edges.push(
            curve
                .iter()
                .map(|p| [p.x as f32, p.y as f32, p.z as f32])
                .collect(),
        );
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &out.positions {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    if out.positions.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    out.bbox_min = min;
    out.bbox_max = max;
    out
}

/// Merges identical vertices (same position, normal and face) to shrink buffers.
fn weld(m: &mut BodyMesh) {
    use std::collections::HashMap;
    let key = |p: [f32; 3], n: [f32; 3], f: u32| -> [u32; 7] {
        [
            p[0].to_bits(),
            p[1].to_bits(),
            p[2].to_bits(),
            n[0].to_bits(),
            n[1].to_bits(),
            n[2].to_bits(),
            f,
        ]
    };
    let mut map: HashMap<[u32; 7], u32> = HashMap::new();
    let (mut pos, mut nor, mut fid) = (Vec::new(), Vec::new(), Vec::new());
    let mut remap = Vec::with_capacity(m.positions.len());
    for i in 0..m.positions.len() {
        let k = key(m.positions[i], m.normals[i], m.face_ids[i]);
        let idx = *map.entry(k).or_insert_with(|| {
            pos.push(m.positions[i]);
            nor.push(m.normals[i]);
            fid.push(m.face_ids[i]);
            (pos.len() - 1) as u32
        });
        remap.push(idx);
    }
    for i in m.indices.iter_mut() {
        *i = remap[*i as usize];
    }
    m.positions = pos;
    m.normals = nor;
    m.face_ids = fid;
}

/// Basic validity check of a solid: every shell is closed and oriented.
pub fn is_valid_solid(solid: &Solid) -> bool {
    use truck_topology::shell::ShellCondition;
    !solid.boundaries().is_empty()
        && solid
            .boundaries()
            .iter()
            .all(|s| s.shell_condition() == ShellCondition::Closed)
}

// ---------------------------------------------------------------------------
// Serialization of exact geometry (used for imported or cached solids)
// ---------------------------------------------------------------------------

pub fn solid_to_json(solid: &Solid) -> Result<String> {
    serde_json::to_string(&solid.compress()).map_err(|e| KernelError::Serialization(e.to_string()))
}

pub fn solid_from_json(s: &str) -> Result<Solid> {
    let c: truck_topology::compress::CompressedSolid<
        Point3,
        truck_modeling::Curve,
        truck_modeling::Surface,
    > = serde_json::from_str(s).map_err(|e| KernelError::Serialization(e.to_string()))?;
    Solid::extract(c).map_err(|e| KernelError::Serialization(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calibro_sketch::{find_profiles, Sketch};

    fn rect_profile(w: f64, h: f64) -> Profile {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [w, h]);
        find_profiles(&s).remove(0)
    }

    #[test]
    fn extrude_rectangle_volume() {
        let solids = extrude(&Plane::XY, &[rect_profile(20.0, 10.0)], 5.0, false).unwrap();
        let m = tessellate(&solids[0], 0.01);
        assert!((m.volume() - 1000.0).abs() < 1e-3, "volume {}", m.volume());
        assert!(is_valid_solid(&solids[0]));
        assert_eq!(m.face_planes.len(), 6);
        assert!(m.face_planes.iter().all(|p| p.is_some()));
    }

    #[test]
    fn negative_extrude_is_outward() {
        let solids = extrude(&Plane::XZ, &[rect_profile(10.0, 10.0)], -4.0, false).unwrap();
        let m = tessellate(&solids[0], 0.01);
        assert!((m.volume() - 400.0).abs() < 1e-3, "volume {}", m.volume());
    }

    #[test]
    fn symmetric_extrude_centered() {
        let solids = extrude(&Plane::XY, &[rect_profile(10.0, 10.0)], 6.0, true).unwrap();
        let m = tessellate(&solids[0], 0.01);
        assert!((m.bbox_min[2] + 3.0).abs() < 1e-5 && (m.bbox_max[2] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn extrude_with_hole() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [40.0, 40.0]);
        s.add_circle([20.0, 20.0], 5.0);
        let ring = find_profiles(&s).remove(0);
        let solids = extrude(&Plane::XY, &[ring], 10.0, false).unwrap();
        let m = tessellate(&solids[0], 0.01);
        let expect = (1600.0 - std::f64::consts::PI * 25.0) * 10.0;
        assert!(
            (m.volume() - expect).abs() / expect < 2e-3,
            "volume {} vs {}",
            m.volume(),
            expect
        );
    }

    #[test]
    fn extrude_slot_with_arcs() {
        // stadium: 20 x 10 with semicircular ends
        let mut s = Sketch::new();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(20.0, 0.0);
        let c = s.add_point(20.0, 10.0);
        let d = s.add_point(0.0, 10.0);
        let c1 = s.add_point(20.0, 5.0);
        let c2 = s.add_point(0.0, 5.0);
        s.add_line(a, b);
        s.add_arc(c1, b, c);
        s.add_line(c, d);
        s.add_arc(c2, d, a);
        let p = find_profiles(&s);
        let solids = extrude(&Plane::XY, &p, 4.0, false).unwrap();
        let v = tessellate(&solids[0], 0.005).volume();
        let exp = (200.0 + std::f64::consts::PI * 25.0) * 4.0;
        assert!((v - exp).abs() / exp < 2e-3, "{v} vs {exp}");
        assert!(is_valid_solid(&solids[0]));
    }

    #[test]
    fn cylinder_and_sphere_volumes() {
        let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let vc = tessellate(&c, 0.005).volume();
        let exp = std::f64::consts::PI * 250.0;
        assert!((vc - exp).abs() / exp < 5e-3, "cyl {vc}");
        let s = make_sphere([0.0; 3], 5.0).unwrap();
        let vs = tessellate(&s, 0.005).volume();
        let exps = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        assert!((vs - exps).abs() / exps < 1e-2, "sphere {vs}");
    }

    #[test]
    fn revolve_rectangle_makes_tube() {
        // rectangle x in [5,10], y in [0,10] revolved around the sketch Y axis
        let mut s = Sketch::new();
        s.add_rectangle([5.0, 0.0], [10.0, 10.0]);
        let p = find_profiles(&s);
        let solids = revolve(&Plane::XZ, &p, [0.0, 0.0], [0.0, 1.0], 360.0).unwrap();
        let v = tessellate(&solids[0], 0.005).volume();
        let exp = std::f64::consts::PI * (100.0 - 25.0) * 10.0;
        assert!((v - exp).abs() / exp < 5e-3, "revolve {v} vs {exp}");
    }

    #[test]
    fn revolve_rejects_axis_crossing() {
        let p = rect_profile(10.0, 10.0);
        assert!(revolve(&Plane::XZ, &[p], [5.0, 0.0], [0.0, 1.0], 360.0).is_err());
    }

    #[test]
    fn box_subtract_cylinder() {
        let b = make_box([0.0; 3], [20.0, 20.0, 10.0]).unwrap();
        let c = make_cylinder([10.0, 10.0, -1.0], 4.0, 12.0).unwrap();
        let r = boolean(&b, &c, BooleanOp::Subtract).unwrap();
        let v = tessellate(&r, 0.01).volume();
        let exp = 4000.0 - std::f64::consts::PI * 16.0 * 10.0;
        assert!((v - exp).abs() / exp < 5e-3, "subtract {v} vs {exp}");
    }

    #[test]
    fn box_union_and_intersect() {
        let a = make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let b = make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
        let u = tessellate(&boolean(&a, &b, BooleanOp::Union).unwrap(), 0.01).volume();
        let i = tessellate(&boolean(&a, &b, BooleanOp::Intersect).unwrap(), 0.01).volume();
        assert!((u - 1875.0).abs() < 1e-2, "union {u}");
        assert!((i - 125.0).abs() < 1e-2, "intersect {i}");
    }

    #[test]
    fn transform_moves_and_rotates() {
        let b = make_box([0.0; 3], [10.0, 2.0, 2.0]).unwrap();
        let t = Transform {
            translate: [0.0, 0.0, 5.0],
            rotate_axis: [0.0, 0.0, 1.0],
            rotate_deg: 90.0,
            ..Default::default()
        };
        let m = tessellate(&transform(&b, &t).unwrap(), 0.01);
        assert!((m.bbox_max[1] - 10.0).abs() < 1e-4, "{:?}", m.bbox_max);
        assert!((m.bbox_min[2] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn coplanar_join_needs_overlap() {
        // A boss sketched on the top face of a plate: exact coplanar union.
        let plate = make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap();
        let mut s = Sketch::new();
        s.add_circle([20.0, 15.0], 5.0);
        let p = find_profiles(&s);
        let top = Plane {
            origin: [0.0, 0.0, 10.0],
            ..Plane::XY
        };
        let boss = extrude_with_overlap(&top, &p, 8.0, false, COPLANAR_OVERLAP)
            .unwrap()
            .remove(0);
        let u = boolean(&plate, &boss, BooleanOp::Union).unwrap();
        let v = tessellate(&u, 0.005).volume();
        let exp = 12000.0 + std::f64::consts::PI * 25.0 * 8.0;
        assert!((v - exp).abs() / exp < 2e-3, "{v} vs {exp}");
        // Cutting a pocket downward from the same face.
        let pocket = extrude_with_overlap(&top, &p, -4.0, false, COPLANAR_OVERLAP)
            .unwrap()
            .remove(0);
        let c = boolean(&plate, &pocket, BooleanOp::Subtract).unwrap();
        let v = tessellate(&c, 0.005).volume();
        let exp = 12000.0 - std::f64::consts::PI * 25.0 * 4.0;
        assert!((v - exp).abs() / exp < 2e-3, "{v} vs {exp}");
    }

    #[test]
    fn coplanar_boolean_never_panics() {
        // Faces exactly coplanar with the target: truck may refuse, but must not crash us.
        let plate = make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap();
        let corner = make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        for op in [BooleanOp::Subtract, BooleanOp::Union, BooleanOp::Intersect] {
            match boolean(&plate, &corner, op) {
                Ok(s) => assert!(is_valid_solid(&s)),
                Err(e) => eprintln!("{op:?}: {e}"),
            }
        }
    }

    #[test]
    fn mirror_box() {
        let b = make_box([2.0, 0.0, 0.0], [3.0, 4.0, 5.0]).unwrap();
        let m = tessellate(&mirror(&b, &Plane::YZ).unwrap(), 0.01);
        assert!((m.volume() - 60.0).abs() < 1e-6, "{}", m.volume());
        assert!((m.bbox_min[0] + 5.0).abs() < 1e-5 && (m.bbox_max[0] + 2.0).abs() < 1e-5);
    }

    #[test]
    fn json_round_trip() {
        let b = make_box([0.0; 3], [3.0, 4.0, 5.0]).unwrap();
        let s = solid_to_json(&b).unwrap();
        let b2 = solid_from_json(&s).unwrap();
        assert!((tessellate(&b2, 0.01).volume() - 60.0).abs() < 1e-6);
    }
}
