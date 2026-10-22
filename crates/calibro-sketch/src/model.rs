//! Sketch data model: points, curves and constraints on a 2D sketch plane.
//!
//! Coordinates are in sketch-plane units (millimetres). Every entity has a
//! stable numeric id so that constraints, dimensions and downstream features
//! can refer to it across edits and save/load cycles.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type PointId = u32;
pub type CurveId = u32;
pub type ConstraintId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn xy(&self) -> [f64; 2] {
        [self.x, self.y]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CurveKind {
    /// Straight segment between two points.
    Line { a: PointId, b: PointId },
    /// Full circle.
    Circle { center: PointId, radius: f64 },
    /// Circular arc, counter-clockwise from `start` to `end` around `center`.
    /// The solver keeps `|end-center| == |start-center|`.
    Arc {
        center: PointId,
        start: PointId,
        end: PointId,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub kind: CurveKind,
    /// Construction geometry guides the sketch but never forms profiles.
    #[serde(default)]
    pub construction: bool,
}

/// Geometric and dimensional constraints. Dimensions are constraints with a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Constraint {
    Coincident {
        a: PointId,
        b: PointId,
    },
    PointOnCurve {
        point: PointId,
        curve: CurveId,
    },
    Horizontal {
        line: CurveId,
    },
    Vertical {
        line: CurveId,
    },
    Parallel {
        a: CurveId,
        b: CurveId,
    },
    Perpendicular {
        a: CurveId,
        b: CurveId,
    },
    /// Line–circle/arc or circle/arc–circle/arc tangency.
    Tangent {
        a: CurveId,
        b: CurveId,
    },
    /// Equal length (lines) or equal radius (circles/arcs).
    Equal {
        a: CurveId,
        b: CurveId,
    },
    Concentric {
        a: CurveId,
        b: CurveId,
    },
    Midpoint {
        point: PointId,
        line: CurveId,
    },
    /// `a` and `b` are mirror images across `axis` (a line).
    Symmetric {
        a: PointId,
        b: PointId,
        axis: CurveId,
    },
    /// Locks a point at an absolute position.
    Fixed {
        point: PointId,
        x: f64,
        y: f64,
    },
    // ---- dimensions ----
    Distance {
        a: PointId,
        b: PointId,
        value: f64,
    },
    HorizontalDistance {
        a: PointId,
        b: PointId,
        value: f64,
    },
    VerticalDistance {
        a: PointId,
        b: PointId,
        value: f64,
    },
    Length {
        line: CurveId,
        value: f64,
    },
    /// Angle in degrees measured from line `a` to line `b`.
    Angle {
        a: CurveId,
        b: CurveId,
        degrees: f64,
    },
    Radius {
        curve: CurveId,
        value: f64,
    },
    Diameter {
        curve: CurveId,
        value: f64,
    },
}

impl Constraint {
    pub fn label(&self) -> &'static str {
        match self {
            Constraint::Coincident { .. } => "Coincident",
            Constraint::PointOnCurve { .. } => "Point on curve",
            Constraint::Horizontal { .. } => "Horizontal",
            Constraint::Vertical { .. } => "Vertical",
            Constraint::Parallel { .. } => "Parallel",
            Constraint::Perpendicular { .. } => "Perpendicular",
            Constraint::Tangent { .. } => "Tangent",
            Constraint::Equal { .. } => "Equal",
            Constraint::Concentric { .. } => "Concentric",
            Constraint::Midpoint { .. } => "Midpoint",
            Constraint::Symmetric { .. } => "Symmetric",
            Constraint::Fixed { .. } => "Fixed",
            Constraint::Distance { .. } => "Distance",
            Constraint::HorizontalDistance { .. } => "Horizontal distance",
            Constraint::VerticalDistance { .. } => "Vertical distance",
            Constraint::Length { .. } => "Length",
            Constraint::Angle { .. } => "Angle",
            Constraint::Radius { .. } => "Radius",
            Constraint::Diameter { .. } => "Diameter",
        }
    }

    /// Mutable access to the numeric value of a dimensional constraint.
    pub fn value_mut(&mut self) -> Option<&mut f64> {
        match self {
            Constraint::Distance { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::Length { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. } => Some(value),
            Constraint::Angle { degrees, .. } => Some(degrees),
            _ => None,
        }
    }

    pub fn is_dimension(&self) -> bool {
        matches!(
            self,
            Constraint::Distance { .. }
                | Constraint::HorizontalDistance { .. }
                | Constraint::VerticalDistance { .. }
                | Constraint::Length { .. }
                | Constraint::Angle { .. }
                | Constraint::Radius { .. }
                | Constraint::Diameter { .. }
        )
    }

    pub fn referenced_points(&self) -> Vec<PointId> {
        match *self {
            Constraint::Coincident { a, b }
            | Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => vec![a, b],
            Constraint::PointOnCurve { point, .. }
            | Constraint::Midpoint { point, .. }
            | Constraint::Fixed { point, .. } => vec![point],
            Constraint::Symmetric { a, b, .. } => vec![a, b],
            _ => vec![],
        }
    }

    pub fn referenced_curves(&self) -> Vec<CurveId> {
        match *self {
            Constraint::PointOnCurve { curve, .. } => vec![curve],
            Constraint::Horizontal { line } | Constraint::Vertical { line } => vec![line],
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Tangent { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::Angle { a, b, .. } => vec![a, b],
            Constraint::Midpoint { line, .. } => vec![line],
            Constraint::Symmetric { axis, .. } => vec![axis],
            Constraint::Length { line, .. } => vec![line],
            Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => vec![curve],
            _ => vec![],
        }
    }
}

/// Serializes `BTreeMap<u32, T>` as a list of `{ "id": .., ...value }` entries.
/// (JSON object keys are strings; integer keys don't survive serde's
/// internally-tagged enums, so an explicit list is more robust.)
mod id_map {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Entry<T> {
        id: u32,
        #[serde(flatten)]
        value: T,
    }

    #[derive(Serialize)]
    struct EntryRef<'a, T> {
        id: u32,
        #[serde(flatten)]
        value: &'a T,
    }

    pub fn serialize<S: Serializer, T: Serialize>(
        m: &BTreeMap<u32, T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter().map(|(id, value)| EntryRef { id: *id, value }))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<BTreeMap<u32, T>, D::Error> {
        let v: Vec<Entry<T>> = Vec::deserialize(d)?;
        Ok(v.into_iter().map(|e| (e.id, e.value)).collect())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    #[serde(with = "id_map")]
    pub points: BTreeMap<PointId, Point>,
    #[serde(with = "id_map")]
    pub curves: BTreeMap<CurveId, Curve>,
    #[serde(with = "id_map")]
    pub constraints: BTreeMap<ConstraintId, Constraint>,
    next_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SketchError {
    MissingPoint(PointId),
    MissingCurve(CurveId),
    WrongCurveKind {
        curve: CurveId,
        expected: &'static str,
    },
}

impl std::fmt::Display for SketchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SketchError::MissingPoint(p) => write!(f, "point {p} does not exist"),
            SketchError::MissingCurve(c) => write!(f, "curve {c} does not exist"),
            SketchError::WrongCurveKind { curve, expected } => {
                write!(f, "curve {curve} must be a {expected}")
            }
        }
    }
}

impl std::error::Error for SketchError {}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    fn alloc(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn add_point(&mut self, x: f64, y: f64) -> PointId {
        let id = self.alloc();
        self.points.insert(id, Point::new(x, y));
        id
    }

    pub fn add_line(&mut self, a: PointId, b: PointId) -> CurveId {
        let id = self.alloc();
        self.curves.insert(
            id,
            Curve {
                kind: CurveKind::Line { a, b },
                construction: false,
            },
        );
        id
    }

    /// Adds a line with two new endpoints.
    pub fn add_line_xy(&mut self, a: [f64; 2], b: [f64; 2]) -> CurveId {
        let pa = self.add_point(a[0], a[1]);
        let pb = self.add_point(b[0], b[1]);
        self.add_line(pa, pb)
    }

    pub fn add_circle(&mut self, center: [f64; 2], radius: f64) -> CurveId {
        let c = self.add_point(center[0], center[1]);
        let id = self.alloc();
        self.curves.insert(
            id,
            Curve {
                kind: CurveKind::Circle { center: c, radius },
                construction: false,
            },
        );
        id
    }

    pub fn add_arc(&mut self, center: PointId, start: PointId, end: PointId) -> CurveId {
        let id = self.alloc();
        self.curves.insert(
            id,
            Curve {
                kind: CurveKind::Arc { center, start, end },
                construction: false,
            },
        );
        id
    }

    /// Adds an axis-aligned rectangle made of four connected lines with
    /// horizontal/vertical constraints, like the rectangle tool in most CAD apps.
    /// Returns the four line ids (bottom, right, top, left).
    pub fn add_rectangle(&mut self, p0: [f64; 2], p1: [f64; 2]) -> [CurveId; 4] {
        let (x0, x1) = (p0[0].min(p1[0]), p0[0].max(p1[0]));
        let (y0, y1) = (p0[1].min(p1[1]), p0[1].max(p1[1]));
        let a = self.add_point(x0, y0);
        let b = self.add_point(x1, y0);
        let c = self.add_point(x1, y1);
        let d = self.add_point(x0, y1);
        let l0 = self.add_line(a, b);
        let l1 = self.add_line(b, c);
        let l2 = self.add_line(c, d);
        let l3 = self.add_line(d, a);
        self.add_constraint(Constraint::Horizontal { line: l0 });
        self.add_constraint(Constraint::Horizontal { line: l2 });
        self.add_constraint(Constraint::Vertical { line: l1 });
        self.add_constraint(Constraint::Vertical { line: l3 });
        [l0, l1, l2, l3]
    }

    /// Regular polygon inscribed in a circle of `radius` (lines + equal constraints).
    pub fn add_polygon(&mut self, center: [f64; 2], radius: f64, sides: usize) -> Vec<CurveId> {
        let n = sides.max(3);
        let start = std::f64::consts::FRAC_PI_2;
        let pts: Vec<PointId> = (0..n)
            .map(|i| {
                let t = start + i as f64 * std::f64::consts::TAU / n as f64;
                self.add_point(center[0] + radius * t.cos(), center[1] + radius * t.sin())
            })
            .collect();
        let lines: Vec<CurveId> = (0..n)
            .map(|i| self.add_line(pts[i], pts[(i + 1) % n]))
            .collect();
        for i in 1..n {
            self.add_constraint(Constraint::Equal {
                a: lines[0],
                b: lines[i],
            });
        }
        lines
    }

    pub fn add_constraint(&mut self, c: Constraint) -> ConstraintId {
        let id = self.alloc();
        self.constraints.insert(id, c);
        id
    }

    pub fn point(&self, id: PointId) -> Result<Point, SketchError> {
        self.points
            .get(&id)
            .copied()
            .ok_or(SketchError::MissingPoint(id))
    }

    pub fn curve(&self, id: CurveId) -> Result<&Curve, SketchError> {
        self.curves.get(&id).ok_or(SketchError::MissingCurve(id))
    }

    pub fn line_points(&self, id: CurveId) -> Result<(PointId, PointId), SketchError> {
        match self.curve(id)?.kind {
            CurveKind::Line { a, b } => Ok((a, b)),
            _ => Err(SketchError::WrongCurveKind {
                curve: id,
                expected: "line",
            }),
        }
    }

    /// Points that a curve owns (endpoints / centers).
    pub fn curve_points(&self, id: CurveId) -> Vec<PointId> {
        match self.curves.get(&id).map(|c| c.kind) {
            Some(CurveKind::Line { a, b }) => vec![a, b],
            Some(CurveKind::Circle { center, .. }) => vec![center],
            Some(CurveKind::Arc { center, start, end }) => vec![center, start, end],
            None => vec![],
        }
    }

    /// Removes a curve, the constraints that use it, and points nothing else uses.
    pub fn remove_curve(&mut self, id: CurveId) {
        let pts = self.curve_points(id);
        self.curves.remove(&id);
        self.constraints
            .retain(|_, c| !c.referenced_curves().contains(&id));
        for p in pts {
            self.remove_point_if_unused(p);
        }
    }

    pub fn remove_point_if_unused(&mut self, p: PointId) {
        let used = self
            .curves
            .keys()
            .any(|c| self.curve_points(*c).contains(&p));
        if !used {
            self.points.remove(&p);
            self.constraints
                .retain(|_, c| !c.referenced_points().contains(&p));
        }
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) {
        self.constraints.remove(&id);
    }

    /// Checks that every constraint refers to existing entities of the right kind.
    pub fn validate(&self) -> Result<(), SketchError> {
        for c in self.curves.values() {
            match c.kind {
                CurveKind::Line { a, b } => {
                    self.point(a)?;
                    self.point(b)?;
                }
                CurveKind::Circle { center, .. } => {
                    self.point(center)?;
                }
                CurveKind::Arc { center, start, end } => {
                    self.point(center)?;
                    self.point(start)?;
                    self.point(end)?;
                }
            }
        }
        for c in self.constraints.values() {
            for p in c.referenced_points() {
                self.point(p)?;
            }
            for cv in c.referenced_curves() {
                self.curve(cv)?;
            }
        }
        Ok(())
    }

    /// Radius of a circle or arc.
    pub fn radius_of(&self, id: CurveId) -> Option<f64> {
        match self.curves.get(&id)?.kind {
            CurveKind::Circle { radius, .. } => Some(radius),
            CurveKind::Arc { center, start, .. } => {
                let c = self.points.get(&center)?;
                let s = self.points.get(&start)?;
                Some(((s.x - c.x).powi(2) + (s.y - c.y).powi(2)).sqrt())
            }
            _ => None,
        }
    }

    /// Axis-aligned bounds of all geometry: (min, max).
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        let mut any = false;
        let mut grow = |x: f64, y: f64| {
            min[0] = min[0].min(x);
            min[1] = min[1].min(y);
            max[0] = max[0].max(x);
            max[1] = max[1].max(y);
        };
        for p in self.points.values() {
            grow(p.x, p.y);
            any = true;
        }
        for c in self.curves.values() {
            if let CurveKind::Circle { center, radius } = c.kind {
                if let Some(p) = self.points.get(&center) {
                    grow(p.x - radius, p.y - radius);
                    grow(p.x + radius, p.y + radius);
                }
            }
        }
        any.then_some((min, max))
    }
}
