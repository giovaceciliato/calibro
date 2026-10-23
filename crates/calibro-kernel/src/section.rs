//! Section caps: where a section plane cuts a closed mesh, the cut outline is
//! computed on the CPU and filled, so a sectioned body looks solid instead of hollow.

use super::BodyMesh;
use std::collections::HashMap;

/// Closed outlines where the plane `n·p = d` cuts the mesh, as 3D polylines (first point
/// not repeated). Open chains (from non-watertight meshes) are dropped.
pub fn section_loops(mesh: &BodyMesh, n: [f64; 3], d: f64) -> Vec<Vec<[f64; 3]>> {
    let size = (0..3)
        .map(|k| (mesh.bbox_max[k] - mesh.bbox_min[k]) as f64)
        .fold(0.0, f64::max)
        .max(1e-6);
    // Snap the plane a hair off any vertex lying exactly on it, so every crossing is
    // a clean edge crossing.
    let eps = size * 1e-7;
    let dist = |p: [f32; 3]| {
        let v = n[0] * p[0] as f64 + n[1] * p[1] as f64 + n[2] * p[2] as f64 - d;
        if v.abs() < eps {
            eps
        } else {
            v
        }
    };
    // quantised key for matching crossing points between neighbouring triangles
    let q = size * 1e-6;
    let key = |p: [f64; 3]| p.map(|x| (x / q).round() as i64);
    let cross = |a: [f32; 3], b: [f32; 3], da: f64, db: f64| -> [f64; 3] {
        let t = da / (da - db);
        [0, 1, 2].map(|k| a[k] as f64 + (b[k] as f64 - a[k] as f64) * t)
    };
    // directed segments: from -> to, oriented consistently using the triangle winding
    let mut next: HashMap<[i64; 3], ([f64; 3], [i64; 3])> = HashMap::new();
    for t in mesh.indices.chunks_exact(3) {
        let p = [0, 1, 2].map(|k| mesh.positions[t[k] as usize]);
        let dv = p.map(dist);
        let mut pts = Vec::with_capacity(2);
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            if (dv[i] > 0.0) != (dv[j] > 0.0) {
                pts.push((cross(p[i], p[j], dv[i], dv[j]), dv[i] > 0.0));
            }
        }
        if pts.len() != 2 {
            continue;
        }
        // the edge going from + to - is where the loop enters; orient so loops are CCW
        // when seen from the + side
        let (a, b) = if pts[0].1 {
            (pts[1].0, pts[0].0)
        } else {
            (pts[0].0, pts[1].0)
        };
        let (ka, kb) = (key(a), key(b));
        if ka != kb {
            next.insert(ka, (a, kb));
        }
    }
    let mut loops = Vec::new();
    let mut used = std::collections::HashSet::new();
    let starts: Vec<[i64; 3]> = next.keys().copied().collect();
    for s in starts {
        if used.contains(&s) {
            continue;
        }
        let mut lp = Vec::new();
        let mut k = s;
        let closed = loop {
            if !used.insert(k) {
                break k == s;
            }
            let Some(&(p, nk)) = next.get(&k) else {
                break false;
            };
            lp.push(p);
            k = nk;
        };
        if closed && lp.len() >= 3 {
            loops.push(lp);
        }
    }
    loops
}

/// Triangles (flat list of corners) filling the section of `mesh` by the plane
/// `n·p = d`. Holes are respected (even-odd nesting).
pub fn section_cap(mesh: &BodyMesh, n: [f64; 3], d: f64) -> Vec<[f32; 3]> {
    let loops = section_loops(mesh, n, d);
    if loops.is_empty() {
        return Vec::new();
    }
    // 2D basis on the plane
    let a = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let crs = |x: [f64; 3], y: [f64; 3]| {
        [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ]
    };
    let u = {
        let k = dot(a, n);
        let v = [a[0] - n[0] * k, a[1] - n[1] * k, a[2] - n[2] * k];
        let l = dot(v, v).sqrt();
        v.map(|x| x / l)
    };
    let v = crs(n, u);
    let flat: Vec<Vec<[f64; 2]>> = loops
        .iter()
        .map(|l| l.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect())
        .collect();
    let area = |l: &[[f64; 2]]| {
        let mut s = 0.0;
        for i in 0..l.len() {
            let (p, q) = (l[i], l[(i + 1) % l.len()]);
            s += p[0] * q[1] - q[0] * p[1];
        }
        s * 0.5
    };
    let inside = |pt: [f64; 2], l: &[[f64; 2]]| {
        let mut c = false;
        for i in 0..l.len() {
            let (p, q) = (l[i], l[(i + 1) % l.len()]);
            if (p[1] > pt[1]) != (q[1] > pt[1])
                && pt[0] < (q[0] - p[0]) * (pt[1] - p[1]) / (q[1] - p[1]) + p[0]
            {
                c = !c;
            }
        }
        c
    };
    // nesting depth of each loop -> even = outer boundary, odd = hole
    let depth: Vec<usize> = (0..flat.len())
        .map(|i| {
            (0..flat.len())
                .filter(|&j| j != i && inside(flat[i][0], &flat[j]))
                .count()
        })
        .collect();
    let mut out = Vec::new();
    for i in (0..flat.len()).filter(|&i| depth[i].is_multiple_of(2)) {
        let holes: Vec<usize> = (0..flat.len())
            .filter(|&j| depth[j] == depth[i] + 1 && inside(flat[j][0], &flat[i]))
            .collect();
        let mut coords: Vec<f64> = Vec::new();
        let mut pts3: Vec<[f64; 3]> = Vec::new();
        let mut hole_idx = Vec::new();
        let push = |li: usize, ccw: bool, coords: &mut Vec<f64>, pts3: &mut Vec<[f64; 3]>| {
            let mut idx: Vec<usize> = (0..flat[li].len()).collect();
            if (area(&flat[li]) > 0.0) != ccw {
                idx.reverse();
            }
            for k in idx {
                coords.extend_from_slice(&flat[li][k]);
                pts3.push(loops[li][k]);
            }
        };
        push(i, true, &mut coords, &mut pts3);
        for h in holes {
            hole_idx.push(pts3.len());
            push(h, false, &mut coords, &mut pts3);
        }
        let Ok(tris) = earcutr::earcut(&coords, &hole_idx, 2) else {
            continue;
        };
        for t in tris.chunks_exact(3) {
            // make the triangle face along +n
            let [p0, p1, p2] = [t[0], t[1], t[2]].map(|k| pts3[k]);
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let order = if dot(crs(e1, e2), n) >= 0.0 {
                [p0, p1, p2]
            } else {
                [p0, p2, p1]
            };
            out.extend(order.map(|p| p.map(|x| x as f32)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn area(tris: &[[f32; 3]]) -> f64 {
        tris.chunks_exact(3)
            .map(|t| {
                let a = t[0].map(|x| x as f64);
                let b = t[1].map(|x| x as f64);
                let c = t[2].map(|x| x as f64);
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5
            })
            .sum()
    }

    #[test]
    fn cap_of_box_with_hole() {
        let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
        let c = make_cylinder([20.0, 15.0, -1.0], 5.0, 22.0).unwrap();
        let s = boolean(&b, &c, BooleanOp::Subtract).unwrap();
        let m = tessellate(&s, 0.01);
        // horizontal cut: rectangle minus circle
        let cap = section_cap(&m, [0.0, 0.0, 1.0], 10.0);
        let exp = 1200.0 - std::f64::consts::PI * 25.0;
        assert!(
            (area(&cap) - exp).abs() / exp < 2e-3,
            "{} vs {exp}",
            area(&cap)
        );
        assert_eq!(section_loops(&m, [0.0, 0.0, 1.0], 10.0).len(), 2);
        // vertical cut through the hole: two separate rectangles 15x20 each side of the hole
        let cap2 = section_cap(&m, [1.0, 0.0, 0.0], 20.0);
        assert!((area(&cap2) - 20.0 * 20.0).abs() < 0.5, "{}", area(&cap2));
        // plane through a face plane / outside the body -> nothing or the face
        assert!(section_cap(&m, [0.0, 0.0, 1.0], 50.0).is_empty());
    }
}
