//! CPU picking against display meshes (faces) and projected edges.
//! Picking uses the tessellation, while the B-rep stays the authority for
//! modelling; face ids map hits back to exact B-rep faces.

use crate::camera::{CameraView, Ray};
use calibro_core::BodyId;
use calibro_kernel::BodyMesh;
use glam::{DVec3, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct FaceHit {
    pub body: BodyId,
    pub face: u32,
    pub point: DVec3,
    /// Facing normal at the hit (used by tests and for future snapping).
    #[allow(dead_code)]
    pub normal: DVec3,
    pub t: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct EdgeHit {
    pub body: BodyId,
    pub edge: usize,
    pub point: DVec3,
    pub screen_dist: f32,
}

fn ray_box(ray: &Ray, min: [f32; 3], max: [f32; 3]) -> bool {
    let mut tmin = f64::NEG_INFINITY;
    let mut tmax = f64::INFINITY;
    for k in 0..3 {
        let o = ray.origin[k];
        let d = ray.dir[k];
        let (lo, hi) = (min[k] as f64 - 1e-3, max[k] as f64 + 1e-3);
        if d.abs() < 1e-15 {
            if o < lo || o > hi {
                return false;
            }
        } else {
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            tmin = tmin.max(a.min(b));
            tmax = tmax.min(a.max(b));
        }
    }
    tmax >= tmin.max(0.0)
}

/// Möller–Trumbore intersection; returns distance along the ray.
fn ray_tri(ray: &Ray, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

pub fn pick_face<'a>(
    ray: &Ray,
    bodies: impl Iterator<Item = (BodyId, &'a BodyMesh)>,
    clip: Option<(DVec3, f64)>,
) -> Option<FaceHit> {
    let mut best: Option<FaceHit> = None;
    for (id, m) in bodies {
        if !ray_box(ray, m.bbox_min, m.bbox_max) {
            continue;
        }
        for tri in m.indices.chunks_exact(3) {
            let p = |i: u32| DVec3::from(m.positions[i as usize].map(|x| x as f64));
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            if let Some(t) = ray_tri(ray, a, b, c) {
                if best.map(|h| t < h.t).unwrap_or(true) {
                    let point = ray.origin + ray.dir * t;
                    if let Some((n, d)) = clip {
                        if point.dot(n) > d {
                            continue;
                        }
                    }
                    let mut normal = (b - a).cross(c - a).normalize_or_zero();
                    if normal.dot(ray.dir) > 0.0 {
                        normal = -normal;
                    }
                    best = Some(FaceHit {
                        body: id,
                        face: m.face_ids[tri[0] as usize],
                        point,
                        normal,
                        t,
                    });
                }
            }
        }
    }
    best
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 > 0.0 {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a + ab * t - p).length(), t)
}

/// Finds the edge closest to `px` on screen, within `max_px`.
pub fn pick_edge<'a>(
    view: &CameraView,
    px: Vec2,
    max_px: f32,
    bodies: impl Iterator<Item = (BodyId, &'a BodyMesh)>,
) -> Option<EdgeHit> {
    let mut best: Option<EdgeHit> = None;
    for (id, m) in bodies {
        for (ei, pl) in m.edges.iter().enumerate() {
            for w in pl.windows(2) {
                let a3 = DVec3::from(w[0].map(|x| x as f64));
                let b3 = DVec3::from(w[1].map(|x| x as f64));
                let (Some(a), Some(b)) = (view.project(a3), view.project(b3)) else {
                    continue;
                };
                let (d, t) = dist_to_segment(px, a, b);
                if d <= max_px && best.map(|h| d < h.screen_dist).unwrap_or(true) {
                    best = Some(EdgeHit {
                        body: id,
                        edge: ei,
                        point: a3 + (b3 - a3) * t as f64,
                        screen_dist: d,
                    });
                }
            }
        }
    }
    best
}

/// Intersection of a ray with a plane (origin, normal).
pub fn ray_plane(ray: &Ray, origin: DVec3, normal: DVec3) -> Option<DVec3> {
    let d = ray.dir.dot(normal);
    if d.abs() < 1e-12 {
        return None;
    }
    let t = (origin - ray.origin).dot(normal) / d;
    (t > 0.0).then(|| ray.origin + ray.dir * t)
}

/// Length of an edge polyline.
pub fn polyline_length(pl: &[[f32; 3]]) -> f64 {
    pl.windows(2)
        .map(|w| {
            let a = DVec3::from(w[0].map(|x| x as f64));
            let b = DVec3::from(w[1].map(|x| x as f64));
            (b - a).length()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_box_top_face() {
        let b = calibro_kernel::make_box([0.0; 3], [10.0; 3]).unwrap();
        let m = calibro_kernel::tessellate(&b, 0.01);
        let ray = Ray {
            origin: DVec3::new(5.0, 5.0, 50.0),
            dir: DVec3::new(0.0, 0.0, -1.0),
        };
        let id = BodyId {
            feature: 1,
            index: 0,
        };
        let hit = pick_face(&ray, std::iter::once((id, &m)), None).unwrap();
        assert!((hit.point.z - 10.0).abs() < 1e-9);
        assert!((hit.normal.z - 1.0).abs() < 1e-9);
        let plane = m.face_planes[hit.face as usize].unwrap();
        assert!((plane.normal()[2] - 1.0).abs() < 1e-9, "{plane:?}");
        // with a section plane cutting z > 5 the top face is skipped and the bottom is hit
        let hit2 = pick_face(&ray, std::iter::once((id, &m)), Some((DVec3::Z, 5.0))).unwrap();
        assert!(hit2.point.z.abs() < 1e-9);
    }
}
