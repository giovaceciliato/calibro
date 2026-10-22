//! Parametric recompute: walks the feature history in order, evaluates each
//! feature with the kernel, and records per-feature status. Results are cached
//! by a content hash of each feature and its inputs, so editing a late feature
//! does not recompute everything before it, while editing an early feature
//! naturally invalidates everything downstream.

use crate::document::*;
use calibro_kernel::{self as kernel, BodyMesh, BooleanOp, Solid};
use calibro_sketch::{find_profiles, CurveKind, Profile, SolveStatus};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum FeatureStatus {
    Ok,
    /// Evaluated, with a non-fatal warning.
    Warning(String),
    Failed(String),
    Suppressed,
    /// After the rollback marker; not evaluated.
    RolledBack,
}

impl FeatureStatus {
    pub fn is_failed(&self) -> bool {
        matches!(self, FeatureStatus::Failed(_))
    }
}

#[derive(Clone)]
pub enum BodyGeometry {
    /// Exact B-rep solid.
    Solid(Solid),
    /// Reference mesh only.
    Mesh,
}

#[derive(Clone)]
pub struct EvaluatedBody {
    pub id: BodyId,
    pub geometry: BodyGeometry,
    pub mesh: Arc<BodyMesh>,
    key: u64,
}

impl EvaluatedBody {
    pub fn solid(&self) -> Option<&Solid> {
        match &self.geometry {
            BodyGeometry::Solid(s) => Some(s),
            BodyGeometry::Mesh => None,
        }
    }
}

/// Result of evaluating a document.
#[derive(Clone, Default)]
pub struct Model {
    pub bodies: BTreeMap<BodyId, EvaluatedBody>,
    pub status: BTreeMap<FeatureId, FeatureStatus>,
    pub sketch_status: BTreeMap<FeatureId, SolveStatus>,
    pub profiles: BTreeMap<FeatureId, Vec<Profile>>,
}

impl Model {
    pub fn failed_features(&self) -> Vec<FeatureId> {
        self.status
            .iter()
            .filter(|(_, s)| s.is_failed())
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Holds caches between recomputes.
#[derive(Default)]
pub struct Engine {
    solid_cache: HashMap<u64, Vec<(u32, Solid)>>,
    mesh_cache: HashMap<u64, Arc<BodyMesh>>,
    pub last_cache_hits: usize,
    pub last_evaluations: usize,
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn mesh_from_resource(r: &MeshResource) -> BodyMesh {
    let mut m = BodyMesh::default();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for t in r.indices.chunks_exact(3) {
        let (a, b, c) = (
            r.positions[t[0] as usize],
            r.positions[t[1] as usize],
            r.positions[t[2] as usize],
        );
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-20);
        let n = [n[0] / l, n[1] / l, n[2] / l];
        for p in [a, b, c] {
            m.indices.push(m.positions.len() as u32);
            m.positions.push(p);
            m.normals.push(n);
            m.face_ids.push(0);
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    m.face_planes.push(None);
    if m.positions.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    m.bbox_min = min;
    m.bbox_max = max;
    m
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    fn mesh_for(&mut self, key: u64, solid: &Solid) -> Arc<BodyMesh> {
        self.mesh_cache
            .entry(key)
            .or_insert_with(|| {
                let tol = kernel::default_tolerance(solid);
                Arc::new(kernel::tessellate(solid, tol))
            })
            .clone()
    }

    /// Evaluates the document.
    pub fn recompute(&mut self, doc: &Document) -> Model {
        self.last_cache_hits = 0;
        self.last_evaluations = 0;
        let mut model = Model::default();
        // bodies currently alive: id -> (solid or mesh, key)
        let limit = doc
            .rollback
            .unwrap_or(doc.features.len())
            .min(doc.features.len());
        let mut used_solid_keys = Vec::new();
        let mut used_mesh_keys = Vec::new();

        for (i, f) in doc.features.iter().enumerate() {
            if i >= limit {
                model.status.insert(f.id, FeatureStatus::RolledBack);
                continue;
            }
            if f.suppressed {
                model.status.insert(f.id, FeatureStatus::Suppressed);
                continue;
            }
            let status = match self.eval_feature(
                doc,
                f,
                &mut model,
                &mut used_solid_keys,
                &mut used_mesh_keys,
            ) {
                Ok(s) => s,
                Err(e) => FeatureStatus::Failed(e),
            };
            model.status.insert(f.id, status);
        }
        // Drop cache entries that this evaluation did not touch (bounded memory).
        self.solid_cache.retain(|k, _| used_solid_keys.contains(k));
        self.mesh_cache.retain(|k, _| used_mesh_keys.contains(k));
        model
    }

    fn input_solid(model: &Model, id: BodyId) -> Result<(Solid, u64), String> {
        let b = model
            .bodies
            .get(&id)
            .ok_or_else(|| format!("body {id} no longer exists (its feature may have failed)"))?;
        match &b.geometry {
            BodyGeometry::Solid(s) => Ok((s.clone(), b.key)),
            BodyGeometry::Mesh => Err(format!(
                "body {id} is a reference mesh and cannot be modified"
            )),
        }
    }

    fn sketch_profiles(
        doc: &Document,
        model: &Model,
        sketch_id: FeatureId,
        sel: &ProfileSelection,
    ) -> Result<(calibro_kernel::Plane, Vec<Profile>), String> {
        let f = doc
            .feature(sketch_id)
            .ok_or_else(|| "the referenced sketch was deleted".to_string())?;
        let FeatureKind::Sketch { plane, .. } = &f.kind else {
            return Err("the referenced feature is not a sketch".into());
        };
        let all = model.profiles.get(&sketch_id).ok_or_else(|| {
            "the referenced sketch comes later in the history or is suppressed".to_string()
        })?;
        let chosen: Vec<Profile> = match sel {
            ProfileSelection::All => {
                // outermost regions only: skip regions that are holes in another region
                all.iter()
                    .filter(|p| {
                        let probe = p.interior_point();
                        !all.iter().any(|q| {
                            !std::ptr::eq(*p, q) && q.contains(probe) && q.area() > p.area()
                        })
                    })
                    .cloned()
                    .collect()
            }
            ProfileSelection::Seeds { points } => {
                let mut out = Vec::new();
                for s in points {
                    match calibro_sketch::profile_at(all, *s) {
                        Some(p) => out.push(p.clone()),
                        None => {
                            return Err("a selected region no longer exists in the sketch".into())
                        }
                    }
                }
                out
            }
        };
        if chosen.is_empty() {
            return Err("the sketch has no closed region".into());
        }
        Ok((*plane, chosen))
    }

    /// Applies new solids according to `mode`, inserting results into the model.
    #[allow(clippy::too_many_arguments)]
    fn apply_mode(
        &mut self,
        model: &mut Model,
        fid: FeatureId,
        key: u64,
        solids: Vec<Solid>,
        mode: BodyMode,
        used_solid_keys: &mut Vec<u64>,
        used_mesh_keys: &mut Vec<u64>,
    ) -> Result<(), String> {
        match mode {
            BodyMode::NewBody => {
                for (i, s) in solids.into_iter().enumerate() {
                    let k = hash_of(&(key, i));
                    let mesh = self.mesh_for(k, &s);
                    used_mesh_keys.push(k);
                    let id = BodyId {
                        feature: fid,
                        index: i as u32,
                    };
                    model.bodies.insert(
                        id,
                        EvaluatedBody {
                            id,
                            geometry: BodyGeometry::Solid(s),
                            mesh,
                            key: k,
                        },
                    );
                }
                Ok(())
            }
            BodyMode::Join { target }
            | BodyMode::Cut { target }
            | BodyMode::Intersect { target } => {
                let op = match mode {
                    BodyMode::Join { .. } => BooleanOp::Union,
                    BodyMode::Cut { .. } => BooleanOp::Subtract,
                    _ => BooleanOp::Intersect,
                };
                let (base, _) = Self::input_solid(model, target)?;
                let k = hash_of(&(key, "mode"));
                let acc = self
                    .cached(k, used_solid_keys, || {
                        let mut acc = base;
                        for s in &solids {
                            acc = kernel::boolean(&acc, s, op).map_err(|e| e.to_string())?;
                        }
                        Ok(vec![acc])
                    })?
                    .remove(0);
                let mesh = self.mesh_for(k, &acc);
                used_mesh_keys.push(k);
                model.bodies.insert(
                    target,
                    EvaluatedBody {
                        id: target,
                        geometry: BodyGeometry::Solid(acc),
                        mesh,
                        key: k,
                    },
                );
                Ok(())
            }
        }
    }

    fn eval_feature(
        &mut self,
        doc: &Document,
        f: &Feature,
        model: &mut Model,
        used_solid_keys: &mut Vec<u64>,
        used_mesh_keys: &mut Vec<u64>,
    ) -> Result<FeatureStatus, String> {
        // Feature key = hash(feature definition, keys of input bodies, sketch content).
        let def = serde_json::to_string(&f.kind).map_err(|e| e.to_string())?;
        let mut input_keys: Vec<u64> = Vec::new();
        for dep in f.kind.feature_dependencies() {
            for b in model.bodies.values().filter(|b| b.id.feature == dep) {
                input_keys.push(b.key);
            }
            if let Some(df) = doc.feature(dep) {
                if let FeatureKind::Sketch { .. } = df.kind {
                    input_keys.push(hash_of(
                        &serde_json::to_string(&df.kind).unwrap_or_default(),
                    ));
                }
            }
        }
        // Inputs that are modified in place (targets) must be captured before mutation.
        let key = hash_of(&(def.as_str(), &input_keys, f.id));

        match &f.kind {
            FeatureKind::Sketch { sketch, .. } => {
                let mut s = sketch.clone();
                let report = calibro_sketch::solve(&mut s).map_err(|e| e.to_string())?;
                let profiles = find_profiles(&s);
                model.profiles.insert(f.id, profiles);
                let status = report.status.clone();
                model.sketch_status.insert(f.id, report.status);
                Ok(match status {
                    SolveStatus::Conflicting { .. } => {
                        FeatureStatus::Warning("conflicting constraints".into())
                    }
                    _ => FeatureStatus::Ok,
                })
            }
            FeatureKind::Extrude {
                sketch,
                profiles,
                distance,
                symmetric,
                body,
            } => {
                let solids = self.cached(key, used_solid_keys, || {
                    let (plane, profs) = Self::sketch_profiles(doc, model, *sketch, profiles)?;
                    kernel::extrude(&plane, &profs, *distance, *symmetric)
                        .map_err(|e| e.to_string())
                })?;
                match self.apply_mode(
                    model,
                    f.id,
                    key,
                    solids,
                    *body,
                    used_solid_keys,
                    used_mesh_keys,
                ) {
                    Ok(()) => Ok(FeatureStatus::Ok),
                    Err(e) if *body == BodyMode::NewBody => Err(e),
                    Err(_) => {
                        // Typical cause: the sketch sits exactly on a face of the target,
                        // so the faces are coplanar. Retry with a tool that overlaps.
                        let k2 = hash_of(&(key, "overlap"));
                        let solids = self.cached(k2, used_solid_keys, || {
                            let (plane, profs) =
                                Self::sketch_profiles(doc, model, *sketch, profiles)?;
                            kernel::extrude_with_overlap(
                                &plane,
                                &profs,
                                *distance,
                                *symmetric,
                                kernel::COPLANAR_OVERLAP,
                            )
                            .map_err(|e| e.to_string())
                        })?;
                        self.apply_mode(
                            model,
                            f.id,
                            k2,
                            solids,
                            *body,
                            used_solid_keys,
                            used_mesh_keys,
                        )?;
                        Ok(FeatureStatus::Ok)
                    }
                }
            }
            FeatureKind::Revolve {
                sketch,
                profiles,
                axis,
                angle_deg,
                body,
            } => {
                let solids = self.cached(key, used_solid_keys, || {
                    let (plane, profs) = Self::sketch_profiles(doc, model, *sketch, profiles)?;
                    let (o, d) = match axis {
                        RevolveAxis::SketchX => ([0.0, 0.0], [1.0, 0.0]),
                        RevolveAxis::SketchY => ([0.0, 0.0], [0.0, 1.0]),
                        RevolveAxis::Line { curve } => {
                            let FeatureKind::Sketch { sketch: sk, .. } =
                                &doc.feature(*sketch).unwrap().kind
                            else {
                                unreachable!()
                            };
                            match sk.curves.get(curve).map(|c| c.kind) {
                                Some(CurveKind::Line { a, b }) => {
                                    let (pa, pb) = (sk.points[&a], sk.points[&b]);
                                    ([pa.x, pa.y], [pb.x - pa.x, pb.y - pa.y])
                                }
                                _ => return Err("the revolve axis line was deleted".into()),
                            }
                        }
                    };
                    kernel::revolve(&plane, &profs, o, d, *angle_deg).map_err(|e| e.to_string())
                })?;
                self.apply_mode(
                    model,
                    f.id,
                    key,
                    solids,
                    *body,
                    used_solid_keys,
                    used_mesh_keys,
                )?;
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::Primitive { primitive, body } => {
                let solids = self.cached(key, used_solid_keys, || {
                    let s = match *primitive {
                        Primitive::Box { min, size } => kernel::make_box(min, size),
                        Primitive::Cylinder {
                            base,
                            radius,
                            height,
                        } => kernel::make_cylinder(base, radius, height),
                        Primitive::Sphere { center, radius } => kernel::make_sphere(center, radius),
                    }
                    .map_err(|e| e.to_string())?;
                    Ok(vec![s])
                })?;
                self.apply_mode(
                    model,
                    f.id,
                    key,
                    solids,
                    *body,
                    used_solid_keys,
                    used_mesh_keys,
                )?;
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::Boolean {
                op,
                target,
                tools,
                keep_tools,
            } => {
                if tools.is_empty() {
                    return Err("choose at least one tool body".into());
                }
                if tools.contains(target) {
                    return Err("a body cannot be combined with itself".into());
                }
                let (t, _) = Self::input_solid(model, *target)?;
                let mut tool_solids = Vec::new();
                for b in tools {
                    tool_solids.push(Self::input_solid(model, *b)?.0);
                }
                let result = self.cached(key, used_solid_keys, || {
                    let mut acc = t.clone();
                    for s in &tool_solids {
                        acc = kernel::boolean(&acc, s, *op).map_err(|e| e.to_string())?;
                    }
                    Ok(vec![acc])
                })?;
                if !keep_tools {
                    for b in tools {
                        model.bodies.remove(b);
                    }
                }
                let k = hash_of(&(key, "bool"));
                let s = result.into_iter().next().unwrap();
                let mesh = self.mesh_for(k, &s);
                used_mesh_keys.push(k);
                model.bodies.insert(
                    *target,
                    EvaluatedBody {
                        id: *target,
                        geometry: BodyGeometry::Solid(s),
                        mesh,
                        key: k,
                    },
                );
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::Transform {
                bodies,
                transform,
                copy,
            } => {
                let mut inputs = Vec::new();
                for b in bodies {
                    inputs.push((*b, Self::input_solid(model, *b)?.0));
                }
                let outs = self.cached(key, used_solid_keys, || {
                    inputs
                        .iter()
                        .map(|(_, s)| kernel::transform(s, transform).map_err(|e| e.to_string()))
                        .collect()
                })?;
                for (i, ((orig, _), s)) in inputs.iter().zip(outs).enumerate() {
                    let id = if *copy {
                        BodyId {
                            feature: f.id,
                            index: i as u32,
                        }
                    } else {
                        *orig
                    };
                    let k = hash_of(&(key, i));
                    let mesh = self.mesh_for(k, &s);
                    used_mesh_keys.push(k);
                    model.bodies.insert(
                        id,
                        EvaluatedBody {
                            id,
                            geometry: BodyGeometry::Solid(s),
                            mesh,
                            key: k,
                        },
                    );
                }
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::LinearPattern {
                body,
                direction,
                count,
                spacing,
            } => {
                if *count < 2 {
                    return Err("a pattern needs at least 2 instances".into());
                }
                let (s, _) = Self::input_solid(model, *body)?;
                let d = kernel::normalize(*direction);
                let outs = self.cached(key, used_solid_keys, || {
                    (1..*count)
                        .map(|i| {
                            let t = calibro_kernel::Transform {
                                translate: [
                                    d[0] * spacing * i as f64,
                                    d[1] * spacing * i as f64,
                                    d[2] * spacing * i as f64,
                                ],
                                ..Default::default()
                            };
                            kernel::transform(&s, &t).map_err(|e| e.to_string())
                        })
                        .collect()
                })?;
                self.insert_new(model, f.id, key, outs, used_mesh_keys);
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::CircularPattern {
                body,
                axis_origin,
                axis_dir,
                count,
                total_angle_deg,
            } => {
                if *count < 2 {
                    return Err("a pattern needs at least 2 instances".into());
                }
                let (s, _) = Self::input_solid(model, *body)?;
                let full = (total_angle_deg.abs() - 360.0).abs() < 1e-9;
                let step = if full {
                    total_angle_deg / *count as f64
                } else {
                    total_angle_deg / (*count - 1) as f64
                };
                let outs = self.cached(key, used_solid_keys, || {
                    (1..*count)
                        .map(|i| {
                            let t = calibro_kernel::Transform {
                                rotate_axis: *axis_dir,
                                rotate_deg: step * i as f64,
                                pivot: *axis_origin,
                                ..Default::default()
                            };
                            kernel::transform(&s, &t).map_err(|e| e.to_string())
                        })
                        .collect()
                })?;
                self.insert_new(model, f.id, key, outs, used_mesh_keys);
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::Mirror { body, plane } => {
                let (s, _) = Self::input_solid(model, *body)?;
                let outs = self.cached(key, used_solid_keys, || {
                    Ok(vec![kernel::mirror(&s, plane).map_err(|e| e.to_string())?])
                })?;
                self.insert_new(model, f.id, key, outs, used_mesh_keys);
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::ImportedSolid { resource } => {
                let bytes = doc
                    .resources
                    .get(resource)
                    .ok_or_else(|| format!("missing resource '{resource}' in the project"))?;
                let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                let outs = self.cached(key, used_solid_keys, || {
                    Ok(vec![
                        kernel::solid_from_json(text).map_err(|e| e.to_string())?
                    ])
                })?;
                self.insert_new(model, f.id, key, outs, used_mesh_keys);
                Ok(FeatureStatus::Ok)
            }
            FeatureKind::ImportedMesh { resource } => {
                let bytes = doc
                    .resources
                    .get(resource)
                    .ok_or_else(|| format!("missing resource '{resource}' in the project"))?;
                let k = hash_of(&(key, bytes.len()));
                used_mesh_keys.push(k);
                let mesh = match self.mesh_cache.get(&k) {
                    Some(m) => m.clone(),
                    None => {
                        let r = MeshResource::from_bytes(bytes)?;
                        let m = Arc::new(mesh_from_resource(&r));
                        self.mesh_cache.insert(k, m.clone());
                        m
                    }
                };
                let id = BodyId {
                    feature: f.id,
                    index: 0,
                };
                model.bodies.insert(
                    id,
                    EvaluatedBody {
                        id,
                        geometry: BodyGeometry::Mesh,
                        mesh,
                        key: k,
                    },
                );
                Ok(FeatureStatus::Ok)
            }
        }
    }

    fn insert_new(
        &mut self,
        model: &mut Model,
        fid: FeatureId,
        key: u64,
        outs: Vec<Solid>,
        used_mesh_keys: &mut Vec<u64>,
    ) {
        for (i, s) in outs.into_iter().enumerate() {
            let id = BodyId {
                feature: fid,
                index: i as u32,
            };
            let k = hash_of(&(key, i));
            let mesh = self.mesh_for(k, &s);
            used_mesh_keys.push(k);
            model.bodies.insert(
                id,
                EvaluatedBody {
                    id,
                    geometry: BodyGeometry::Solid(s),
                    mesh,
                    key: k,
                },
            );
        }
    }

    fn cached(
        &mut self,
        key: u64,
        used: &mut Vec<u64>,
        compute: impl FnOnce() -> Result<Vec<Solid>, String>,
    ) -> Result<Vec<Solid>, String> {
        used.push(key);
        if let Some(v) = self.solid_cache.get(&key) {
            self.last_cache_hits += 1;
            return Ok(v.iter().map(|(_, s)| s.clone()).collect());
        }
        self.last_evaluations += 1;
        let v = compute()?;
        for s in &v {
            if !kernel::is_valid_solid(s) {
                return Err("the operation produced invalid (non-closed) geometry".into());
            }
        }
        self.solid_cache.insert(
            key,
            v.iter()
                .cloned()
                .enumerate()
                .map(|(i, s)| (i as u32, s))
                .collect(),
        );
        Ok(v)
    }
}
