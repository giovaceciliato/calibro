//! Closed-region (profile) detection.
//!
//! Lines and arcs form a planar graph; points joined by `Coincident`
//! constraints, or lying within a small tolerance, are merged into one vertex.
//! Dangling edges are pruned, then every bounded face of the planar graph is
//! found by walking half-edges ("always take the sharpest turn"). Full circles
//! are standalone loops. Loops that sit inside another loop become holes of
//! the smallest loop that contains them — so a circle inside a rectangle gives
//! two regions: the rectangle with a hole, and the disc. A line splitting a
//! rectangle gives two regions, as in Shapr3D.
//!
//! Limitation: curves that cross each other without sharing an endpoint are
//! not split at the crossing (no curve–curve intersection yet).

use crate::model::{Constraint, CurveKind, Sketch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// Arc traversed from `from` to `to` around `center`; `ccw` gives direction.
    Arc {
        center: [f64; 2],
        radius: f64,
        from: [f64; 2],
        to: [f64; 2],
        ccw: bool,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
}

impl Segment {
    /// Polyline approximation (including the start point, excluding the end).
    pub fn sample(&self, out: &mut Vec<[f64; 2]>) {
        match *self {
            Segment::Line { from, .. } => out.push(from),
            Segment::Arc {
                center,
                radius,
                from,
                to,
                ccw,
            } => {
                let (a0, sweep) = arc_angles(center, from, to, ccw);
                let n = ((sweep.abs() / TAU) * 64.0).ceil().max(2.0) as usize;
                for i in 0..n {
                    let t = a0 + sweep * i as f64 / n as f64;
                    out.push([center[0] + radius * t.cos(), center[1] + radius * t.sin()]);
                }
            }
            Segment::Circle { center, radius } => {
                for i in 0..64 {
                    let t = TAU * i as f64 / 64.0;
                    out.push([center[0] + radius * t.cos(), center[1] + radius * t.sin()]);
                }
            }
        }
    }

    pub fn reversed(&self) -> Segment {
        match *self {
            Segment::Line { from, to } => Segment::Line { from: to, to: from },
            Segment::Arc {
                center,
                radius,
                from,
                to,
                ccw,
            } => Segment::Arc {
                center,
                radius,
                from: to,
                to: from,
                ccw: !ccw,
            },
            Segment::Circle { .. } => *self,
        }
    }

    /// Midpoint along the segment (used to build arcs through three points).
    pub fn midpoint(&self) -> [f64; 2] {
        match *self {
            Segment::Line { from, to } => [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5],
            Segment::Arc {
                center,
                radius,
                from,
                to,
                ccw,
            } => {
                let (a0, sweep) = arc_angles(center, from, to, ccw);
                let t = a0 + sweep * 0.5;
                [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
            }
            Segment::Circle { center, radius } => [center[0] - radius, center[1]],
        }
    }
}

/// Start angle and signed sweep of an arc.
pub fn arc_angles(center: [f64; 2], from: [f64; 2], to: [f64; 2], ccw: bool) -> (f64, f64) {
    let a0 = (from[1] - center[1]).atan2(from[0] - center[0]);
    let a1 = (to[1] - center[1]).atan2(to[0] - center[0]);
    let mut sweep = a1 - a0;
    if ccw {
        while sweep <= 1e-12 {
            sweep += TAU;
        }
    } else {
        while sweep >= -1e-12 {
            sweep -= TAU;
        }
    }
    (a0, sweep)
}

/// A closed loop of segments, oriented so `area > 0` means counter-clockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub segments: Vec<Segment>,
}

impl Loop {
    pub fn polyline(&self) -> Vec<[f64; 2]> {
        let mut pts = Vec::new();
        for s in &self.segments {
            s.sample(&mut pts);
        }
        pts
    }

    pub fn signed_area(&self) -> f64 {
        polygon_area(&self.polyline())
    }

    pub fn reversed(&self) -> Loop {
        Loop {
            segments: self.segments.iter().rev().map(|s| s.reversed()).collect(),
        }
    }

    pub fn contains(&self, p: [f64; 2]) -> bool {
        point_in_polygon(p, &self.polyline())
    }
}

/// A closed region usable for extrude/revolve: outer boundary (CCW) and holes (CW).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl Profile {
    pub fn area(&self) -> f64 {
        self.outer.signed_area().abs()
            - self
                .holes
                .iter()
                .map(|h| h.signed_area().abs())
                .sum::<f64>()
    }

    /// True if `p` is inside the outer loop and outside every hole.
    pub fn contains(&self, p: [f64; 2]) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }

    /// A point guaranteed (best effort) to lie inside the region. Used as a stable
    /// "seed" so features can keep referring to the same region after edits.
    pub fn interior_point(&self) -> [f64; 2] {
        let poly = self.outer.polyline();
        let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &poly {
            min[0] = min[0].min(p[0]);
            min[1] = min[1].min(p[1]);
            max[0] = max[0].max(p[0]);
            max[1] = max[1].max(p[1]);
        }
        // Scan horizontal lines; pick the widest inside span's midpoint.
        let mut best: Option<([f64; 2], f64)> = None;
        for k in 1..16 {
            let y = min[1] + (max[1] - min[1]) * k as f64 / 16.0;
            let mut xs = Vec::new();
            let mut all: Vec<Vec<[f64; 2]>> = vec![poly.clone()];
            all.extend(self.holes.iter().map(|h| h.polyline()));
            for pl in &all {
                for i in 0..pl.len() {
                    let (a, b) = (pl[i], pl[(i + 1) % pl.len()]);
                    if (a[1] > y) != (b[1] > y) {
                        xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                    }
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    let w = x1 - x0;
                    let c = [(x0 + x1) * 0.5, y];
                    if best.map(|b| w > b.1).unwrap_or(true) && self.contains(c) {
                        best = Some((c, w));
                    }
                }
            }
        }
        best.map(|b| b.0).unwrap_or(poly[0])
    }
}

pub fn polygon_area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a * 0.5
}

pub fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

struct UnionFind(Vec<usize>);
impl UnionFind {
    fn find(&mut self, i: usize) -> usize {
        if self.0[i] != i {
            let r = self.find(self.0[i]);
            self.0[i] = r;
        }
        self.0[i]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

#[derive(Clone, Copy)]
struct GEdge {
    u: usize,
    v: usize,
    /// segment oriented u -> v
    seg: Segment,
}

/// Finds all closed regions in the sketch.
pub fn find_profiles(sketch: &Sketch) -> Vec<Profile> {
    const MERGE_TOL: f64 = 1e-6;
    let ids: Vec<u32> = sketch.points.keys().copied().collect();
    let index: BTreeMap<u32, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut uf = UnionFind((0..ids.len()).collect());
    for c in sketch.constraints.values() {
        if let Constraint::Coincident { a, b } = c {
            if let (Some(&i), Some(&j)) = (index.get(a), index.get(b)) {
                uf.union(i, j);
            }
        }
    }
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (p, q) = (sketch.points[&ids[i]], sketch.points[&ids[j]]);
            if (p.x - q.x).abs() < MERGE_TOL && (p.y - q.y).abs() < MERGE_TOL {
                uf.union(i, j);
            }
        }
    }
    let pos = |id: u32| sketch.points[&id].xy();

    let mut loops: Vec<Loop> = Vec::new();
    let mut edges: Vec<GEdge> = Vec::new();
    for c in sketch.curves.values() {
        if c.construction {
            continue;
        }
        match c.kind {
            CurveKind::Line { a, b } => {
                let (u, v) = (uf.find(index[&a]), uf.find(index[&b]));
                if u != v {
                    edges.push(GEdge {
                        u,
                        v,
                        seg: Segment::Line {
                            from: pos(a),
                            to: pos(b),
                        },
                    });
                }
            }
            CurveKind::Arc { center, start, end } => {
                let (u, v) = (uf.find(index[&start]), uf.find(index[&end]));
                let cc = pos(center);
                let r = {
                    let s = pos(start);
                    ((s[0] - cc[0]).powi(2) + (s[1] - cc[1]).powi(2)).sqrt()
                };
                let seg = Segment::Arc {
                    center: cc,
                    radius: r,
                    from: pos(start),
                    to: pos(end),
                    ccw: true,
                };
                if u != v {
                    edges.push(GEdge { u, v, seg });
                } else if r > 1e-9 {
                    loops.push(Loop {
                        segments: vec![Segment::Circle {
                            center: cc,
                            radius: r,
                        }],
                    });
                }
            }
            CurveKind::Circle { center, radius } => {
                if radius > 1e-9 {
                    loops.push(Loop {
                        segments: vec![Segment::Circle {
                            center: pos(center),
                            radius,
                        }],
                    });
                }
            }
        }
    }

    // Prune dangling edges repeatedly.
    loop {
        let mut degree: BTreeMap<usize, usize> = BTreeMap::new();
        for e in &edges {
            *degree.entry(e.u).or_default() += 1;
            *degree.entry(e.v).or_default() += 1;
        }
        let before = edges.len();
        edges.retain(|e| degree[&e.u] > 1 && degree[&e.v] > 1);
        if edges.len() == before {
            break;
        }
    }

    // Half-edge face traversal.
    // half-edge h: 2*i (u->v) and 2*i+1 (v->u)
    let he_count = edges.len() * 2;
    let he_seg = |h: usize| -> Segment {
        let e = &edges[h / 2];
        if h.is_multiple_of(2) {
            e.seg
        } else {
            e.seg.reversed()
        }
    };
    let he_from = |h: usize| {
        if h.is_multiple_of(2) {
            edges[h / 2].u
        } else {
            edges[h / 2].v
        }
    };
    let he_to = |h: usize| {
        if h.is_multiple_of(2) {
            edges[h / 2].v
        } else {
            edges[h / 2].u
        }
    };
    // outgoing tangent direction angle at start of a half-edge
    let out_angle = |h: usize| -> f64 {
        let d = start_tangent(&he_seg(h));
        d[1].atan2(d[0])
    };
    let in_angle_reversed = |h: usize| -> f64 {
        // direction pointing back along the half-edge from its end
        let d = start_tangent(&he_seg(h).reversed());
        d[1].atan2(d[0])
    };
    let mut outgoing: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for h in 0..he_count {
        outgoing.entry(he_from(h)).or_default().push(h);
    }
    let mut used = vec![false; he_count];
    for start in 0..he_count {
        if used[start] {
            continue;
        }
        let mut cycle = Vec::new();
        let mut h = start;
        let mut ok = true;
        while !used[h] {
            used[h] = true;
            cycle.push(h);
            let v = he_to(h);
            let back = in_angle_reversed(h);
            // Choose the outgoing half-edge that comes first when rotating clockwise
            // from `back` (= largest counter-clockwise angle). This keeps the face
            // on the left, so bounded faces are traced counter-clockwise.
            let next = outgoing[&v]
                .iter()
                .copied()
                .filter(|&o| o / 2 != h / 2 || outgoing[&v].len() == 1)
                .max_by(|&a, &b| {
                    let da = ccw_delta(back, out_angle(a));
                    let db = ccw_delta(back, out_angle(b));
                    da.total_cmp(&db)
                });
            match next {
                Some(n) => h = n,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || h != start || cycle.len() < 2 {
            continue;
        }
        let lp = Loop {
            segments: cycle.iter().map(|&h| he_seg(h)).collect(),
        };
        // Bounded faces come out counter-clockwise with this turning rule.
        if lp.signed_area() > 1e-12 {
            loops.push(lp);
        }
    }

    // Normalise all loops to CCW.
    let loops: Vec<Loop> = loops
        .into_iter()
        .map(|l| {
            if l.signed_area() < 0.0 {
                l.reversed()
            } else {
                l
            }
        })
        .collect();
    let areas: Vec<f64> = loops.iter().map(|l| l.signed_area()).collect();
    let samples: Vec<[f64; 2]> = loops.iter().map(sample_on_loop).collect();
    // contained_by[i] = all loops containing loop i
    let contains = |outer: usize, inner: usize| -> bool {
        outer != inner && areas[inner] < areas[outer] && loops[outer].contains(samples[inner])
    };
    let mut profiles = Vec::new();
    for i in 0..loops.len() {
        let inside: Vec<usize> = (0..loops.len()).filter(|&j| contains(i, j)).collect();
        // direct children: not contained by another loop that is also inside i
        let holes: Vec<Loop> = inside
            .iter()
            .copied()
            .filter(|&j| !inside.iter().any(|&k| k != j && contains(k, j)))
            .map(|j| loops[j].reversed())
            .collect();
        profiles.push(Profile {
            outer: loops[i].clone(),
            holes,
        });
    }
    // Stable ordering: larger first, then by position.
    profiles.sort_by(|a, b| b.area().total_cmp(&a.area()));
    profiles
}

/// A point on the loop boundary, nudged slightly inward isn't needed: we use the
/// midpoint of the first segment for containment tests between disjoint loops.
fn sample_on_loop(l: &Loop) -> [f64; 2] {
    l.segments[0].midpoint()
}

fn start_tangent(s: &Segment) -> [f64; 2] {
    match *s {
        Segment::Line { from, to } => [to[0] - from[0], to[1] - from[1]],
        Segment::Arc {
            center, from, ccw, ..
        } => {
            let r = [from[0] - center[0], from[1] - center[1]];
            if ccw {
                [-r[1], r[0]]
            } else {
                [r[1], -r[0]]
            }
        }
        Segment::Circle { .. } => [0.0, 1.0],
    }
}

/// Counter-clockwise angle from `a` to `b` in (0, 2pi].
fn ccw_delta(a: f64, b: f64) -> f64 {
    let mut d = b - a;
    while d <= 1e-12 {
        d += TAU;
    }
    while d > TAU {
        d -= TAU;
    }
    d
}

/// Resolves a region by an interior seed point (see [`Profile::interior_point`]).
/// Returns the smallest region containing the point.
pub fn profile_at(profiles: &[Profile], seed: [f64; 2]) -> Option<&Profile> {
    profiles
        .iter()
        .filter(|p| p.contains(seed))
        .min_by(|a, b| a.area().total_cmp(&b.area()))
}
