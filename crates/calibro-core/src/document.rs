//! The editable document: an ordered feature history plus appearance,
//! viewport and metadata. This is exactly what the native `.calibro` file
//! stores; geometry is *recomputed* from it, so saved projects stay editable.

use calibro_kernel::{BooleanOp, Plane, Transform};
use calibro_sketch::{CurveId, Sketch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type FeatureId = u64;

/// Identifies a body by the feature that created it and an index within
/// that feature's outputs. Stable across recomputes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BodyId {
    pub feature: FeatureId,
    pub index: u32,
}

impl std::fmt::Display for BodyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.feature, self.index)
    }
}

/// How a new solid combines with existing geometry (like Shapr3D's extrude
/// behaviour of adding to or cutting from a body).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum BodyMode {
    NewBody,
    Join { target: BodyId },
    Cut { target: BodyId },
    Intersect { target: BodyId },
}

impl BodyMode {
    pub fn target(&self) -> Option<BodyId> {
        match *self {
            BodyMode::NewBody => None,
            BodyMode::Join { target }
            | BodyMode::Cut { target }
            | BodyMode::Intersect { target } => Some(target),
        }
    }
}

/// Which closed regions of a sketch a feature uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by")]
pub enum ProfileSelection {
    /// Every outermost region (regions that are not holes of others).
    All,
    /// Regions containing these interior seed points (sketch coordinates).
    /// Seeds survive edits better than region indices.
    Seeds { points: Vec<[f64; 2]> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "axis")]
pub enum RevolveAxis {
    SketchX,
    SketchY,
    Line { curve: CurveId },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape")]
pub enum Primitive {
    Box {
        min: [f64; 3],
        size: [f64; 3],
    },
    Cylinder {
        base: [f64; 3],
        radius: f64,
        height: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
}

/// Triangle mesh stored as a document resource (imported STL/STEP reference geometry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshResource {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl MeshResource {
    /// Little-endian binary: "CMSH", u32 version, u32 vertex count, u32 index count, data.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(16 + self.positions.len() * 12 + self.indices.len() * 4);
        b.extend_from_slice(b"CMSH");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(self.positions.len() as u32).to_le_bytes());
        b.extend_from_slice(&(self.indices.len() as u32).to_le_bytes());
        for p in &self.positions {
            for c in p {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in &self.indices {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, String> {
        if b.len() < 16 || &b[0..4] != b"CMSH" {
            return Err("not a Calibro mesh resource".into());
        }
        let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let nv = u(8) as usize;
        let ni = u(12) as usize;
        let need = 16 + nv * 12 + ni * 4;
        if b.len() < need {
            return Err("mesh resource is truncated".into());
        }
        let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let positions = (0..nv)
            .map(|i| {
                let o = 16 + i * 12;
                [f(o), f(o + 4), f(o + 8)]
            })
            .collect();
        let base = 16 + nv * 12;
        let indices: Vec<u32> = (0..ni).map(|i| u(base + i * 4)).collect();
        if indices.iter().any(|&i| i as usize >= nv) {
            return Err("mesh resource has out-of-range indices".into());
        }
        Ok(Self { positions, indices })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FeatureKind {
    Sketch {
        plane: Plane,
        sketch: Sketch,
        /// Body whose face the sketch was placed on (used for smart defaults).
        #[serde(default)]
        on_body: Option<BodyId>,
    },
    Extrude {
        sketch: FeatureId,
        profiles: ProfileSelection,
        distance: f64,
        #[serde(default)]
        symmetric: bool,
        body: BodyMode,
    },
    Revolve {
        sketch: FeatureId,
        profiles: ProfileSelection,
        axis: RevolveAxis,
        angle_deg: f64,
        body: BodyMode,
    },
    Primitive {
        primitive: Primitive,
        body: BodyMode,
    },
    Boolean {
        op: BooleanOp,
        target: BodyId,
        tools: Vec<BodyId>,
        #[serde(default)]
        keep_tools: bool,
    },
    Transform {
        bodies: Vec<BodyId>,
        transform: Transform,
        /// Create transformed copies instead of moving the originals.
        #[serde(default)]
        copy: bool,
    },
    LinearPattern {
        body: BodyId,
        direction: [f64; 3],
        count: u32,
        spacing: f64,
    },
    CircularPattern {
        body: BodyId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        count: u32,
        total_angle_deg: f64,
    },
    Mirror {
        body: BodyId,
        plane: Plane,
    },
    /// Exact B-rep geometry stored in the project (e.g. imported or baked).
    ImportedSolid {
        resource: String,
    },
    /// Reference-only triangle mesh (STL, tessellated STEP). Cannot be used in booleans.
    ImportedMesh {
        resource: String,
    },
    /// Rounds straight edges between flat faces. Edges are stored by their end
    /// points so they can be found again after earlier features change.
    Fillet {
        body: BodyId,
        edges: Vec<[[f64; 3]; 2]>,
        radius: f64,
    },
    /// Bevels straight edges between flat faces by `distance` along each face.
    Chamfer {
        body: BodyId,
        edges: Vec<[[f64; 3]; 2]>,
        distance: f64,
    },
    /// Hollows a body, leaving walls of `thickness`; `open_faces` are removed.
    Shell {
        body: BodyId,
        open_faces: Vec<Plane>,
        thickness: f64,
    },
    /// Sweeps sketch regions along the curves of another sketch (a smooth chain
    /// of lines and arcs).
    Sweep {
        sketch: FeatureId,
        profiles: ProfileSelection,
        path: FeatureId,
        body: BodyMode,
    },
    /// Lofts between one region of each of two sketches.
    Loft {
        from_sketch: FeatureId,
        from_seed: [f64; 2],
        to_sketch: FeatureId,
        to_seed: [f64; 2],
        body: BodyMode,
    },
}

impl FeatureKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::Primitive { primitive, .. } => match primitive {
                Primitive::Box { .. } => "Box",
                Primitive::Cylinder { .. } => "Cylinder",
                Primitive::Sphere { .. } => "Sphere",
            },
            FeatureKind::Boolean { op, .. } => match op {
                BooleanOp::Union => "Union",
                BooleanOp::Subtract => "Subtract",
                BooleanOp::Intersect => "Intersect",
            },
            FeatureKind::Transform { .. } => "Transform",
            FeatureKind::LinearPattern { .. } => "Linear pattern",
            FeatureKind::CircularPattern { .. } => "Circular pattern",
            FeatureKind::Mirror { .. } => "Mirror",
            FeatureKind::ImportedSolid { .. } => "Imported solid",
            FeatureKind::ImportedMesh { .. } => "Imported mesh",
            FeatureKind::Fillet { .. } => "Fillet",
            FeatureKind::Chamfer { .. } => "Chamfer",
            FeatureKind::Shell { .. } => "Shell",
            FeatureKind::Sweep { .. } => "Sweep",
            FeatureKind::Loft { .. } => "Loft",
        }
    }

    /// Features this one depends on directly (for the dependency graph).
    pub fn feature_dependencies(&self) -> Vec<FeatureId> {
        let mut deps = Vec::new();
        let mut body = |b: &BodyId| deps.push(b.feature);
        match self {
            FeatureKind::Extrude { body: m, .. }
            | FeatureKind::Revolve { body: m, .. }
            | FeatureKind::Sweep { body: m, .. }
            | FeatureKind::Loft { body: m, .. }
            | FeatureKind::Primitive { body: m, .. } => {
                if let Some(t) = m.target() {
                    body(&t);
                }
            }
            FeatureKind::Boolean { target, tools, .. } => {
                body(target);
                tools.iter().for_each(&mut body);
            }
            FeatureKind::Transform { bodies, .. } => bodies.iter().for_each(&mut body),
            FeatureKind::LinearPattern { body: b, .. }
            | FeatureKind::CircularPattern { body: b, .. }
            | FeatureKind::Mirror { body: b, .. }
            | FeatureKind::Fillet { body: b, .. }
            | FeatureKind::Chamfer { body: b, .. }
            | FeatureKind::Shell { body: b, .. } => body(b),
            _ => {}
        }
        match self {
            FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => {
                deps.push(*sketch)
            }
            FeatureKind::Sweep { sketch, path, .. } => {
                deps.push(*sketch);
                deps.push(*path);
            }
            FeatureKind::Loft {
                from_sketch,
                to_sketch,
                ..
            } => {
                deps.push(*from_sketch);
                deps.push(*to_sketch);
            }
            _ => {}
        }
        deps.sort_unstable();
        deps.dedup();
        deps
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub kind: FeatureKind,
    #[serde(default)]
    pub suppressed: bool,
    /// Hidden in the viewport (used for sketches).
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Base colour as sRGB (what colour pickers show), alpha = opacity.
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Matte grey".into(),
            color: [0.72, 0.74, 0.77, 1.0],
            metallic: 0.0,
            roughness: 0.55,
        }
    }
}

/// Built-in material presets.
pub fn material_presets() -> Vec<Material> {
    let m = |name: &str, c: [f32; 3], metallic: f32, roughness: f32| Material {
        name: name.into(),
        color: [c[0], c[1], c[2], 1.0],
        metallic,
        roughness,
    };
    vec![
        Material::default(),
        m("White plastic", [0.92, 0.92, 0.9], 0.0, 0.35),
        m("Black plastic", [0.06, 0.06, 0.07], 0.0, 0.4),
        m("Aluminium", [0.91, 0.92, 0.92], 1.0, 0.3),
        m("Steel", [0.56, 0.57, 0.58], 1.0, 0.25),
        m("Brass", [0.91, 0.78, 0.42], 1.0, 0.25),
        m("Copper", [0.95, 0.64, 0.54], 1.0, 0.3),
        m("Oak", [0.62, 0.42, 0.24], 0.0, 0.7),
        m("Concrete", [0.6, 0.6, 0.58], 0.0, 0.9),
        m("Red", [0.8, 0.12, 0.1], 0.0, 0.45),
        m("Blue", [0.1, 0.3, 0.8], 0.0, 0.45),
        Material {
            name: "Glass".into(),
            color: [0.8, 0.9, 0.95, 0.3],
            metallic: 0.0,
            roughness: 0.05,
        },
    ]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyProps {
    pub name: String,
    pub material: Material,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    pub target: [f64; 3],
    pub distance: f64,
    /// Rotation around world Z, radians.
    pub yaw: f64,
    /// Elevation above the XY plane, radians.
    pub pitch: f64,
    pub orthographic: bool,
    pub fov_deg: f64,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            target: [0.0; 3],
            distance: 160.0,
            yaw: -0.9,
            pitch: 0.55,
            orthographic: false,
            fov_deg: 35.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Units {
    #[default]
    Millimetres,
    Centimetres,
    Metres,
    Inches,
}

impl Units {
    pub fn suffix(&self) -> &'static str {
        match self {
            Units::Millimetres => "mm",
            Units::Centimetres => "cm",
            Units::Metres => "m",
            Units::Inches => "in",
        }
    }
    /// Size of one unit in millimetres (model space is always millimetres).
    pub fn in_mm(&self) -> f64 {
        match self {
            Units::Millimetres => 1.0,
            Units::Centimetres => 10.0,
            Units::Metres => 1000.0,
            Units::Inches => 25.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyPropsEntry {
    pub body: BodyId,
    pub props: BodyProps,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub metadata: Metadata,
    #[serde(default)]
    pub units: Units,
    pub features: Vec<Feature>,
    /// Appearance and names of bodies.
    #[serde(default)]
    pub bodies: Vec<BodyPropsEntry>,
    /// Evaluate only the first N features ("rollback bar"); None = all.
    #[serde(default)]
    pub rollback: Option<usize>,
    #[serde(default)]
    pub camera: CameraState,
    next_id: u64,
    /// Binary resources (meshes, exact solids); stored as separate entries in the file.
    #[serde(skip)]
    pub resources: BTreeMap<String, Vec<u8>>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            metadata: Metadata {
                title: "Untitled".into(),
                ..Default::default()
            },
            units: Units::Millimetres,
            features: Vec::new(),
            bodies: Vec::new(),
            rollback: None,
            camera: CameraState::default(),
            next_id: 0,
            resources: BTreeMap::new(),
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_id(&mut self) -> FeatureId {
        self.next_id += 1;
        self.next_id
    }

    /// Ensures `next_id` is past every existing id (after loading old files).
    pub fn fix_ids(&mut self) {
        let max = self.features.iter().map(|f| f.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(max);
    }

    /// Adds a feature at the end of the history (or right after the rollback
    /// marker if the history is rolled back). Returns its id.
    pub fn add_feature(&mut self, name: impl Into<String>, kind: FeatureKind) -> FeatureId {
        let id = self.alloc_id();
        let mut name = name.into();
        if name.is_empty() {
            let n = self
                .features
                .iter()
                .filter(|f| f.kind.type_name() == kind.type_name())
                .count();
            name = format!("{} {}", kind.type_name(), n + 1);
        }
        let f = Feature {
            id,
            name,
            kind,
            suppressed: false,
            hidden: false,
        };
        match self.rollback {
            Some(r) if r < self.features.len() => {
                self.features.insert(r, f);
                self.rollback = Some(r + 1);
            }
            _ => self.features.push(f),
        }
        id
    }

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn feature_mut(&mut self, id: FeatureId) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }

    pub fn feature_index(&self, id: FeatureId) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    pub fn sketch_mut(&mut self, id: FeatureId) -> Option<(&mut Plane, &mut Sketch)> {
        match &mut self.feature_mut(id)?.kind {
            FeatureKind::Sketch { plane, sketch, .. } => Some((plane, sketch)),
            _ => None,
        }
    }

    /// Features that (transitively) depend on `id`.
    pub fn dependents(&self, id: FeatureId) -> Vec<FeatureId> {
        let mut out = vec![id];
        for f in &self.features {
            if f.kind
                .feature_dependencies()
                .iter()
                .any(|d| out.contains(d))
            {
                out.push(f.id);
            }
        }
        out.remove(0);
        out
    }

    /// Deletes a feature and everything that depends on it.
    pub fn delete_feature(&mut self, id: FeatureId) -> Vec<FeatureId> {
        let mut gone = self.dependents(id);
        gone.push(id);
        self.features.retain(|f| !gone.contains(&f.id));
        self.bodies.retain(|b| !gone.contains(&b.body.feature));
        if let Some(r) = self.rollback {
            self.rollback = Some(r.min(self.features.len()));
        }
        gone
    }

    pub fn body_props(&self, b: BodyId) -> Option<&BodyProps> {
        self.bodies.iter().find(|e| e.body == b).map(|e| &e.props)
    }

    /// Shapr3D-style automatic join/cut for an extrusion drawn on a body's face:
    /// pulling outward adds material (Join), pushing into the body removes it (Cut).
    /// Applied only when the direction actually flips (the sign of `distance`
    /// changes compared with `old_distance`), so an explicit user choice made in
    /// the inspector is kept. Returns true if the mode was changed.
    pub fn auto_extrude_mode(&mut self, fid: FeatureId, old_distance: f64) -> bool {
        let Some(FeatureKind::Extrude {
            sketch,
            distance,
            symmetric,
            body,
            ..
        }) = self.feature(fid).map(|f| f.kind.clone())
        else {
            return false;
        };
        if symmetric || (distance > 0.0) == (old_distance > 0.0) {
            return false;
        }
        let on_body = match self.feature(sketch).map(|f| &f.kind) {
            Some(FeatureKind::Sketch {
                on_body: Some(b), ..
            }) => *b,
            _ => return false,
        };
        let new_mode = match body {
            BodyMode::Join { target } | BodyMode::Cut { target } if target == on_body => {
                if distance > 0.0 {
                    BodyMode::Join { target }
                } else {
                    BodyMode::Cut { target }
                }
            }
            _ => return false,
        };
        if new_mode == body {
            return false;
        }
        if let Some(FeatureKind::Extrude { body, .. }) = self.feature_mut(fid).map(|f| &mut f.kind)
        {
            *body = new_mode;
        }
        true
    }

    pub fn body_props_mut(&mut self, b: BodyId) -> &mut BodyProps {
        if let Some(i) = self.bodies.iter().position(|e| e.body == b) {
            return &mut self.bodies[i].props;
        }
        let name = format!("Body {}", self.bodies.len() + 1);
        self.bodies.push(BodyPropsEntry {
            body: b,
            props: BodyProps {
                name,
                material: Material::default(),
                visible: true,
            },
        });
        &mut self.bodies.last_mut().unwrap().props
    }
}
