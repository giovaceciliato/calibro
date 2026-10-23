//! Application state, frame loop, file operations, shortcuts and dialogs.

use crate::camera::{self, CameraView};
use crate::files::{Files, PickedFile, Purpose};
use crate::renderer::ViewportRenderer;
use calibro_core::kernel::Plane;
use calibro_core::sketch::{ConstraintId, CurveId, CurveKind, PointId};
use calibro_core::*;
use glam::{DVec3, Vec2};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Line,
    Rectangle,
    Circle,
    Arc,
    Polygon,
    Measure,
}

impl Tool {
    pub fn is_sketch_tool(self) -> bool {
        matches!(
            self,
            Tool::Line | Tool::Rectangle | Tool::Circle | Tool::Arc | Tool::Polygon
        )
    }
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click to select · double-click selects a whole body · Ctrl/Shift adds · right-drag orbit · middle/Shift+right-drag pan · wheel zoom",
            Tool::Line => "Line: click points to draw connected lines · click the first point to close · Esc or right-click to stop",
            Tool::Rectangle => "Rectangle: click two opposite corners",
            Tool::Circle => "Circle: click centre then circumference, or drag from centre · set an exact radius in Properties · Esc cancels",
            Tool::Arc => "Arc: click the centre, the start point, then the end (counter-clockwise)",
            Tool::Polygon => "Polygon: click the centre, then a corner",
            Tool::Measure => "Measure: click two points on the model · Esc to reset",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    ShadedEdges,
    Shaded,
    Wireframe,
    XRay,
}

#[derive(Default, Clone, Debug)]
pub struct Selection {
    pub bodies: Vec<BodyId>,
    pub face: Option<(BodyId, u32)>,
    /// Selected B-rep edges (Shift/Ctrl+click adds more): (body, edge index in its mesh).
    pub edges: Vec<(BodyId, usize)>,
    /// Sketch regions: (sketch feature, interior seed point)
    pub regions: Vec<(FeatureId, [f64; 2])>,
    pub feature: Option<FeatureId>,
    pub sk_points: Vec<PointId>,
    pub sk_curves: Vec<CurveId>,
    pub sk_constraint: Option<ConstraintId>,
}

impl Selection {
    pub fn clear_geometry(&mut self) {
        self.bodies.clear();
        self.face = None;
        self.edges.clear();
        self.regions.clear();
    }
    pub fn clear_sketch(&mut self) {
        self.sk_points.clear();
        self.sk_curves.clear();
        self.sk_constraint = None;
    }
}

pub struct SketchSession {
    pub feature: FeatureId,
    /// Clicked positions for the current tool, with the point they snapped to.
    pub clicks: Vec<([f64; 2], Option<PointId>)>,
    pub chain_start: Option<PointId>,
    pub cursor: Option<[f64; 2]>,
    pub snapped: Option<PointId>,
    pub dragging: Option<PointId>,
    pub polygon_sides: u32,
    pub circle_dragging: bool,
    pub circle_radius: f64,
    pub grid_snap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleKind {
    ExtrudeDistance(FeatureId),
    Translate(FeatureId, usize),
    EdgeAmount(FeatureId),
}

pub struct HandleDrag {
    pub kind: HandleKind,
    pub start_value: f64,
    pub start_px: Vec2,
    pub dir_screen: Vec2,
    pub px_per_unit: f32,
    pub last_px: Vec2,
    pub raw_value: f64,
    pub last_attempt: f64,
    pub error: Option<String>,
}

pub enum Pending {
    New,
    Open,
    Quit,
    OpenPicked(PickedFile),
}

#[derive(Clone, Copy)]
pub struct Section {
    pub enabled: bool,
    pub axis: usize,
    pub offset: f64,
    pub flip: bool,
}

#[derive(Default, Clone, Copy)]
pub struct Measure {
    pub a: Option<DVec3>,
    pub b: Option<DVec3>,
}

/// Cached triangulated sketch-region fills: (content hash, triangles per region).
pub type RegionFills = (u64, Vec<Vec<[f32; 3]>>);

pub struct CalibroApp {
    pub ed: Editor,
    pub renderer: Option<ViewportRenderer>,
    pub files: Files,
    #[cfg(not(target_arch = "wasm32"))]
    pub path: Option<std::path::PathBuf>,
    pub file_name: String,
    pub tool: Tool,
    pub sel: Selection,
    pub hover_face: Option<(BodyId, u32)>,
    pub hover_region: Option<(FeatureId, usize)>,
    pub sketch: Option<SketchSession>,
    pub display: DisplayMode,
    pub section: Section,
    pub measure: Measure,
    pub handle: Option<HandleDrag>,
    pub message: Option<(String, bool, f64)>,
    pub pending: Option<Pending>,
    pub allow_close: bool,
    pub view: Option<CameraView>,
    pub fill_cache: HashMap<FeatureId, RegionFills>,
    pub recovery: Option<Document>,
    pub show_shortcuts: bool,
    pub show_about: bool,
    pub show_grid: bool,
    /// Side panels can be hidden to give the viewport room on small screens.
    pub show_tools: bool,
    pub show_inspector: bool,
    /// Set once the first frame has chosen panel visibility from the screen width.
    pub layout_initialised: bool,
    /// Previous width, used to hide panels when the window crosses compact thresholds.
    pub layout_width: f32,
    /// Last window title sent, so it is only updated when it changes.
    pub last_title: String,
    pub picking_plane: bool,
    pub viewport_rect: egui::Rect,
    pub time: f64,
    pub last_click: (f64, Vec2),
}

const RECOVERY_KEY: &str = "calibro_recovery_v1";

impl CalibroApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let renderer = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| ViewportRenderer::new(&rs.device));
        if renderer.is_none() {
            log::error!("wgpu is not available: the 3D viewport cannot be shown");
        }
        crate::style::apply(&cc.egui_ctx);
        // Offer to restore work that was not saved when the app last closed.
        let recovery = cc
            .storage
            .and_then(|s| s.get_string(RECOVERY_KEY))
            .and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).ok()?;
                if v.get("dirty")?.as_bool()? {
                    serde_json::from_value::<Document>(v.get("doc")?.clone()).ok()
                } else {
                    None
                }
            });
        let mut app = Self {
            ed: Editor::new(Self::starter_document()),
            renderer,
            files: Files::default(),
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
            file_name: "Untitled.calibro".into(),
            tool: Tool::Select,
            sel: Selection::default(),
            hover_face: None,
            hover_region: None,
            sketch: None,
            display: DisplayMode::ShadedEdges,
            section: Section {
                enabled: false,
                axis: 0,
                offset: 0.0,
                flip: false,
            },
            measure: Measure::default(),
            handle: None,
            message: None,
            pending: None,
            allow_close: false,
            view: None,
            fill_cache: HashMap::new(),
            recovery,
            show_shortcuts: false,
            show_about: false,
            show_grid: true,
            show_tools: true,
            show_inspector: true,
            layout_initialised: false,
            layout_width: 0.0,
            last_title: String::new(),
            picking_plane: false,
            viewport_rect: egui::Rect::NOTHING,
            time: 0.0,
            last_click: (-1.0, Vec2::ZERO),
        };
        app.ed.mark_saved();
        // Desktop: open a project or import a model passed on the command line
        // (this is also what "Open with Calibro" / double-clicking a file does).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(arg) = std::env::args().nth(1) {
            let path = std::path::PathBuf::from(arg);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let f = PickedFile {
                        name: path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        path: Some(path.clone()),
                        bytes,
                        purpose: Purpose::Import,
                    };
                    app.recovery = None;
                    app.import_picked(f);
                    app.ed.mark_saved();
                    app.zoom_to_fit();
                }
                Err(e) => app.error(format!("Cannot open {}: {e}", path.display())),
            }
        }
        app
    }

    /// The document a new session opens with: empty, ready to sketch.
    pub fn starter_document() -> Document {
        let mut d = Document::new();
        d.metadata.title = "Untitled".into();
        d
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), false, self.time));
    }
    pub fn error(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        log::warn!("{m}");
        self.message = Some((m, true, self.time));
    }

    // ------------------------------------------------------------------ files

    pub fn request(&mut self, action: Pending) {
        let needs_confirm = self.ed.is_dirty() && !matches!(action, Pending::OpenPicked(_));
        if needs_confirm {
            self.pending = Some(action);
        } else {
            self.run_pending(action);
        }
    }

    fn run_pending(&mut self, action: Pending) {
        match action {
            Pending::New => self.new_project(),
            Pending::Open => self.files.pick(
                Purpose::OpenProject,
                &[("Calibro project", &[calibro_project::EXTENSION])],
            ),
            Pending::Quit => {
                self.allow_close = true;
                self.pending = None;
            }
            Pending::OpenPicked(f) => self.open_picked(f),
        }
    }

    pub fn new_project(&mut self) {
        self.ed.replace_document(Self::starter_document());
        self.ed.mark_saved();
        self.reset_session();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.path = None;
        }
        self.file_name = "Untitled.calibro".into();
        self.info("New project");
    }

    fn reset_session(&mut self) {
        self.sel = Selection::default();
        self.sketch = None;
        self.tool = Tool::Select;
        self.measure = Measure::default();
        self.handle = None;
        self.fill_cache.clear();
    }

    fn open_picked(&mut self, f: PickedFile) {
        match calibro_project::load_from_bytes(&f.bytes) {
            Ok(doc) => {
                self.ed.replace_document(doc);
                self.ed.mark_saved();
                self.reset_session();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.path = f.path.clone();
                }
                self.file_name = f.name.clone();
                let failed = self.ed.model().failed_features().len();
                if failed > 0 {
                    self.error(format!("Opened {} — {failed} feature(s) failed to rebuild (shown in red in History)", f.name));
                } else {
                    self.info(format!("Opened {}", f.name));
                }
            }
            Err(e) => self.error(format!("Could not open {}: {e}", f.name)),
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let doc = self.ed.doc().clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = match (&self.path, save_as) {
                (Some(p), false) => Some(p.clone()),
                _ => crate::files::save_dialog(
                    &self.file_name,
                    "Calibro project",
                    calibro_project::EXTENSION,
                ),
            };
            let Some(path) = path else { return false };
            match calibro_project::save_to_path(&doc, &path) {
                Ok(()) => {
                    self.ed.mark_saved();
                    self.file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.path = Some(path.clone());
                    self.info(format!("Saved {}", path.display()));
                    true
                }
                Err(e) => {
                    self.error(format!(
                        "Save failed: {e}. Your previous file was not changed."
                    ));
                    false
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = save_as;
            match calibro_project::save_to_bytes(&doc) {
                Ok(bytes) => match crate::files::download(&self.file_name, &bytes) {
                    Ok(()) => {
                        self.ed.mark_saved();
                        self.info(format!("Downloaded {}", self.file_name));
                        true
                    }
                    Err(e) => {
                        self.error(format!("Download failed: {e}"));
                        false
                    }
                },
                Err(e) => {
                    self.error(format!("Save failed: {e}"));
                    false
                }
            }
        }
    }

    pub fn import_picked(&mut self, f: PickedFile) {
        let lower = f.name.to_lowercase();
        let result: Result<MeshResource, String> = if lower.ends_with(".stl") {
            calibro_io::import_stl(&f.bytes).map_err(|e| e.to_string())
        } else if lower.ends_with(".obj") {
            std::str::from_utf8(&f.bytes)
                .map_err(|e| e.to_string())
                .and_then(|t| calibro_io::import_obj(t).map_err(|e| e.to_string()))
        } else if lower.ends_with(".step") || lower.ends_with(".stp") {
            std::str::from_utf8(&f.bytes)
                .map_err(|e| e.to_string())
                .and_then(|t| calibro_io::import_step(t).map_err(|e| e.to_string()))
        } else if lower.ends_with(".calibro") {
            self.request(Pending::OpenPicked(f));
            return;
        } else {
            Err("unsupported file type (use STEP, STL, OBJ or .calibro)".into())
        };
        match result {
            Ok(mesh) => {
                let tris = mesh.indices.len() / 3;
                let name = f.name.clone();
                self.ed.apply("Import", |d| {
                    let key = format!("import_{}_{}.cmsh", d.features.len() + 1, sanitize(&name));
                    d.resources.insert(key.clone(), mesh.to_bytes());
                    let id =
                        d.add_feature(name.clone(), FeatureKind::ImportedMesh { resource: key });
                    d.body_props_mut(BodyId {
                        feature: id,
                        index: 0,
                    })
                    .name = name.clone();
                });
                self.info(format!(
                    "Imported {} ({tris} triangles) as a reference mesh",
                    f.name
                ));
                self.zoom_to_fit();
            }
            Err(e) => self.error(format!("Import of {} failed: {e}", f.name)),
        }
    }

    pub fn export(&mut self, format: &str, selection_only: bool) {
        let sel = selection_only.then(|| self.sel.bodies.clone());
        let doc = self.ed.doc().clone();
        let bodies = calibro_io::collect_bodies(&doc, self.ed.model(), sel.as_deref());
        let stem = self.file_name.trim_end_matches(".calibro").to_string();
        let res: Result<(Vec<u8>, &str, &str), String> = (|| {
            Ok(match format {
                "stl" => (
                    calibro_io::export_stl_binary(&bodies).map_err(|e| e.to_string())?,
                    "STL mesh",
                    "stl",
                ),
                "stl-ascii" => (
                    calibro_io::export_stl_ascii(&bodies)
                        .map_err(|e| e.to_string())?
                        .into_bytes(),
                    "STL mesh",
                    "stl",
                ),
                "obj" => {
                    // OBJ + MTL in one go: materials are written to a sibling file.
                    let mtl_name = format!("{stem}.mtl");
                    let (obj, mtl) =
                        calibro_io::export_obj(&bodies, &mtl_name).map_err(|e| e.to_string())?;
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if let Some(path) =
                            crate::files::save_dialog(&format!("{stem}.obj"), "OBJ mesh", "obj")
                        {
                            std::fs::write(&path, obj).map_err(|e| e.to_string())?;
                            std::fs::write(path.with_extension("mtl"), mtl)
                                .map_err(|e| e.to_string())?;
                            return Err(format!("__done__Exported {} (+ .mtl)", path.display()));
                        }
                        return Err("__cancel__".into());
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        crate::files::download(&mtl_name, mtl.as_bytes())?;
                        (obj.into_bytes(), "OBJ mesh", "obj")
                    }
                }
                "glb" => (
                    calibro_io::export_glb(&bodies).map_err(|e| e.to_string())?,
                    "glTF binary",
                    "glb",
                ),
                "step" => (
                    calibro_io::export_step(&bodies, &format!("{stem}.step"))
                        .map_err(|e| e.to_string())?
                        .into_bytes(),
                    "STEP",
                    "step",
                ),
                _ => return Err("unknown format".into()),
            })
        })();
        match res {
            Ok((bytes, filter, ext)) => {
                match crate::files::save_bytes(&format!("{stem}.{ext}"), filter, ext, &bytes) {
                    Ok(Some(where_)) => self.info(format!("Exported {where_}")),
                    Ok(None) => {}
                    Err(e) => self.error(format!("Export failed: {e}")),
                }
            }
            Err(e) if e.starts_with("__done__") => {
                self.info(e.trim_start_matches("__done__").to_string())
            }
            Err(e) if e == "__cancel__" => {}
            Err(e) => self.error(format!("Export failed: {e}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_image(&mut self, frame: &eframe::Frame) {
        let (Some(r), Some(rs)) = (&self.renderer, frame.wgpu_render_state()) else {
            return;
        };
        let Some((w, h, px)) = r.read_pixels(&rs.device, &rs.queue) else {
            self.error("Could not read the viewport image");
            return;
        };
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            match enc
                .write_header()
                .and_then(|mut wr| wr.write_image_data(&px))
            {
                Ok(()) => {}
                Err(e) => {
                    self.error(format!("PNG encoding failed: {e}"));
                    return;
                }
            }
        }
        let stem = self.file_name.trim_end_matches(".calibro").to_string();
        match crate::files::save_bytes(&format!("{stem}.png"), "PNG image", "png", &png_bytes) {
            Ok(Some(p)) => self.info(format!("Saved image {p} ({w}×{h})")),
            Ok(None) => {}
            Err(e) => self.error(e),
        }
    }

    // ------------------------------------------------------------------ view

    pub fn zoom_to_fit(&mut self) {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for b in self.ed.model().bodies.values() {
            if !self
                .ed
                .doc()
                .body_props(b.id)
                .map(|p| p.visible)
                .unwrap_or(true)
            {
                continue;
            }
            min = min.min(DVec3::from(b.mesh.bbox_min.map(|x| x as f64)));
            max = max.max(DVec3::from(b.mesh.bbox_max.map(|x| x as f64)));
        }
        for f in &self.ed.doc().features {
            if let FeatureKind::Sketch { plane, sketch, .. } = &f.kind {
                if f.hidden {
                    continue;
                }
                if let Some((a, b)) = sketch.bounds() {
                    for p in [a, b, [a[0], b[1]], [b[0], a[1]]] {
                        let w = DVec3::from(plane.to_world(p));
                        min = min.min(w);
                        max = max.max(w);
                    }
                }
            }
        }
        if !min.x.is_finite() {
            min = DVec3::splat(-50.0);
            max = DVec3::splat(50.0);
        }
        self.ed
            .apply_silent(|d| camera::fit(&mut d.camera, min, max));
    }

    pub fn set_view(&mut self, name: &str) {
        self.ed
            .apply_silent(|d| camera::set_view(&mut d.camera, name));
    }

    // ------------------------------------------------------------------ sketch session

    pub fn start_sketch(&mut self, plane: Plane, on_body: Option<BodyId>) {
        let id = self.ed.apply("New sketch", |d| {
            d.add_feature(
                "",
                FeatureKind::Sketch {
                    plane,
                    sketch: calibro_core::sketch::Sketch::new(),
                    on_body,
                },
            )
        });
        self.enter_sketch(id);
        self.tool = Tool::Line;
        self.info("Sketch started — draw a closed shape, then press Finish (or Esc twice)");
    }

    /// Starts a sketch on a flat face of a body and turns the camera to look straight at
    /// it (Shapr3D-style: select a face, press Space).
    pub fn start_sketch_on_face(&mut self, body: BodyId, face: u32) {
        let Some(mesh) = self.ed.model().bodies.get(&body).map(|b| b.mesh.clone()) else {
            return;
        };
        let Some(Some(pl)) = mesh.face_planes.get(face as usize).copied() else {
            self.error("That face is curved — pick a flat face or a standard plane");
            return;
        };
        // keep sketch coordinates aligned with the world origin
        let n = glam::DVec3::from(pl.normal());
        let o = n * n.dot(glam::DVec3::from(pl.origin));
        let plane = Plane::from_origin_normal(o.to_array(), n.to_array());
        // bounding box of the face, to frame it
        let mut lo = glam::DVec3::splat(f64::INFINITY);
        let mut hi = glam::DVec3::splat(f64::NEG_INFINITY);
        for t in mesh.indices.chunks_exact(3) {
            if mesh.face_ids[t[0] as usize] != face {
                continue;
            }
            for &i in t {
                let p = glam::DVec3::from(mesh.positions[i as usize].map(|x| x as f64));
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        self.start_sketch(plane, Some(body));
        if lo.x.is_finite() {
            self.ed.apply_silent(|d| {
                camera::fit(&mut d.camera, lo, hi);
                let target = d.camera.target;
                camera::look_at_plane(&mut d.camera, plane.normal(), target);
            });
        }
    }

    pub fn enter_sketch(&mut self, id: FeatureId) {
        let Some(FeatureKind::Sketch { plane, .. }) =
            self.ed.doc().feature(id).map(|f| f.kind.clone())
        else {
            return;
        };
        self.ed.apply_silent(|d| {
            let target = d.camera.target;
            camera::look_at_plane(&mut d.camera, plane.normal(), target);
            d.camera.orthographic = true;
        });
        // make sure it is visible while editing
        if self.ed.doc().feature(id).map(|f| f.hidden).unwrap_or(false) {
            self.ed
                .apply("Show sketch", |d| d.feature_mut(id).unwrap().hidden = false);
        }
        self.sketch = Some(SketchSession {
            feature: id,
            clicks: Vec::new(),
            chain_start: None,
            cursor: None,
            snapped: None,
            dragging: None,
            polygon_sides: 6,
            circle_dragging: false,
            circle_radius: 10.0,
            grid_snap: true,
        });
        self.sel = Selection {
            feature: Some(id),
            ..Default::default()
        };
        self.picking_plane = false;
    }

    /// Tool changes must not reinterpret a previous tool's unfinished points.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if let Some(s) = &mut self.sketch {
            s.clicks.clear();
            s.chain_start = None;
            s.cursor = None;
            s.snapped = None;
            s.dragging = None;
            s.circle_dragging = false;
        }
        if tool == Tool::Measure {
            self.measure = Measure::default();
        }
    }

    /// All circle gestures and exact-radius entry share a single undoable command.
    pub fn create_sketch_circle(
        &mut self,
        center: [f64; 2],
        snapped: Option<PointId>,
        radius: f64,
    ) -> bool {
        let Some(fid) = self.sketch.as_ref().map(|s| s.feature) else {
            return false;
        };
        if !matches!(
            self.ed.doc().feature(fid).map(|f| &f.kind),
            Some(FeatureKind::Sketch { .. })
        ) {
            return false;
        }
        if !radius.is_finite() || radius <= 1e-9 || !center.iter().all(|v| v.is_finite()) {
            self.error("Circle radius must be greater than zero");
            return false;
        }
        self.ed.apply("Draw circle", |d| {
            let (_, sk) = d.sketch_mut(fid).unwrap();
            let existing = snapped.filter(|id| sk.points.contains_key(id));
            let curve = sk.add_circle(center, radius);
            if let Some(existing) = existing {
                if let CurveKind::Circle {
                    center: temporary, ..
                } = sk.curves[&curve].kind
                {
                    sk.curves.get_mut(&curve).unwrap().kind = CurveKind::Circle {
                        center: existing,
                        radius,
                    };
                    sk.points.remove(&temporary);
                }
            }
            let _ = calibro_core::sketch::solve(sk);
        });
        if let Some(s) = &mut self.sketch {
            s.clicks.clear();
            s.chain_start = None;
            s.circle_dragging = false;
            s.circle_radius = radius;
        }
        true
    }

    pub fn finish_sketch(&mut self) {
        if let Some(s) = self.sketch.take() {
            self.ed.apply_silent(|d| d.camera.orthographic = false);
            self.tool = Tool::Select;
            self.sel = Selection::default();
            let regions = self
                .ed
                .model()
                .profiles
                .get(&s.feature)
                .map(|p| p.len())
                .unwrap_or(0);
            if regions > 0 {
                self.info(format!("Sketch finished: {regions} closed region(s). Click a region, then Extrude (E) or Revolve."));
            } else {
                self.info("Sketch finished (no closed region yet)");
            }
        }
    }

    // ------------------------------------------------------------------ modelling commands

    pub fn selected_regions_for_one_sketch(&self) -> Option<(FeatureId, Vec<[f64; 2]>)> {
        let first = self.sel.regions.first()?.0;
        let seeds: Vec<[f64; 2]> = self
            .sel
            .regions
            .iter()
            .filter(|r| r.0 == first)
            .map(|r| r.1)
            .collect();
        Some((first, seeds))
    }

    pub fn cmd_extrude(&mut self) {
        let Some((sketch, seeds)) = self.selected_regions_for_one_sketch() else {
            self.error("Select one or more sketch regions first (click inside a closed shape)");
            return;
        };
        let on_body = match self.ed.doc().feature(sketch).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { on_body, .. }) => *on_body,
            _ => None,
        };
        let body = match on_body {
            Some(b) if self.ed.model().bodies.contains_key(&b) => BodyMode::Join { target: b },
            _ => BodyMode::NewBody,
        };
        let id = self.ed.apply("Extrude", |d| {
            let id = d.add_feature(
                "",
                FeatureKind::Extrude {
                    sketch,
                    profiles: ProfileSelection::Seeds { points: seeds },
                    distance: 10.0,
                    symmetric: false,
                    body,
                },
            );
            if let Some(f) = d.feature_mut(sketch) {
                f.hidden = true;
            }
            id
        });
        self.after_feature_created(id);
    }

    pub fn cmd_revolve(&mut self) {
        let Some((sketch, seeds)) = self.selected_regions_for_one_sketch() else {
            self.error("Select a sketch region to revolve");
            return;
        };
        let id = self.ed.apply("Revolve", |d| {
            let id = d.add_feature(
                "",
                FeatureKind::Revolve {
                    sketch,
                    profiles: ProfileSelection::Seeds { points: seeds },
                    axis: RevolveAxis::SketchY,
                    angle_deg: 360.0,
                    body: BodyMode::NewBody,
                },
            );
            if let Some(f) = d.feature_mut(sketch) {
                f.hidden = true;
            }
            id
        });
        self.after_feature_created(id);
    }

    pub fn cmd_primitive(&mut self, p: Primitive) {
        let id = self.ed.apply("Add primitive", |d| {
            d.add_feature(
                "",
                FeatureKind::Primitive {
                    primitive: p,
                    body: BodyMode::NewBody,
                },
            )
        });
        self.after_feature_created(id);
    }

    pub fn cmd_boolean(&mut self, op: calibro_core::kernel::BooleanOp) {
        if self.sel.bodies.len() < 2 {
            self.error("Select two or more bodies: the first one is kept, the others are the tools (Ctrl/Shift+click to add)");
            return;
        }
        let target = self.sel.bodies[0];
        let tools = self.sel.bodies[1..].to_vec();
        let id = self.ed.apply("Boolean", |d| {
            d.add_feature(
                "",
                FeatureKind::Boolean {
                    op,
                    target,
                    tools,
                    keep_tools: false,
                },
            )
        });
        self.sel.clear_geometry();
        self.sel.bodies.push(target);
        self.after_feature_created(id);
    }

    pub fn cmd_on_selected_bodies(
        &mut self,
        label: &str,
        make: impl FnOnce(Vec<BodyId>) -> FeatureKind,
    ) {
        if self.sel.bodies.is_empty() {
            self.error("Select a body first (double-click selects a whole body)");
            return;
        }
        let kind = make(self.sel.bodies.clone());
        let id = self.ed.apply(label, |d| d.add_feature("", kind));
        self.after_feature_created(id);
    }

    /// Straight selected edges of one body, as end-point pairs (for fillet/chamfer).
    pub fn selected_straight_edges(&mut self) -> Option<(BodyId, Vec<[[f64; 3]; 2]>)> {
        let Some(&(body, _)) = self.sel.edges.first() else {
            self.error(
                "Select one or more straight edges first (click an edge; Shift+click adds more)",
            );
            return None;
        };
        let mut out = Vec::new();
        for &(b, e) in &self.sel.edges {
            if b != body {
                self.error("All selected edges must belong to the same body");
                return None;
            }
            let Some(pl) = self
                .ed
                .model()
                .bodies
                .get(&b)
                .and_then(|m| m.mesh.edges.get(e))
            else {
                continue;
            };
            let (Some(a), Some(z)) = (pl.first(), pl.last()) else {
                continue;
            };
            let a = glam::DVec3::from(a.map(|x| x as f64));
            let z = glam::DVec3::from(z.map(|x| x as f64));
            let dir = (z - a).normalize_or_zero();
            let straight = dir != glam::DVec3::ZERO
                && pl.iter().all(|p| {
                    let p = glam::DVec3::from(p.map(|x| x as f64)) - a;
                    (p - dir * p.dot(dir)).length() < 1e-3 * (z - a).length().max(1.0)
                });
            if !straight {
                self.error("Only straight edges can be rounded or chamfered for now");
                return None;
            }
            out.push([a.to_array(), z.to_array()]);
        }
        if out.is_empty() {
            self.error("The selected edges no longer exist");
            return None;
        }
        Some((body, out))
    }

    pub fn cmd_fillet(&mut self, chamfer: bool) {
        let Some((body, edges)) = self.selected_straight_edges() else {
            return;
        };
        let (label, kind) = if chamfer {
            (
                "Chamfer",
                FeatureKind::Chamfer {
                    body,
                    edges,
                    distance: 1.0,
                },
            )
        } else {
            (
                "Fillet",
                FeatureKind::Fillet {
                    body,
                    edges,
                    radius: 1.0,
                },
            )
        };
        let id = self.ed.apply(label, |d| d.add_feature("", kind));
        self.sel.edges.clear();
        self.after_feature_created(id);
    }

    /// Preview edge amounts transactionally: keep the last valid solid on limits/failures.
    pub fn set_edge_amount(&mut self, fid: FeatureId, value: f64) -> Result<(), String> {
        if !value.is_finite() || value <= 0.0 {
            return Err("The size must be positive".into());
        }
        let name = match self.ed.doc().feature(fid).map(|f| &f.kind) {
            Some(FeatureKind::Fillet { .. }) => "Fillet",
            Some(FeatureKind::Chamfer { .. }) => "Chamfer",
            _ => return Err("Select a fillet or chamfer feature".into()),
        };
        let previously_failed = self.ed.model().failed_features();
        self.ed.apply_coalesced_checked(
            &format!("Edit {name}"),
            fid,
            |doc| match &mut doc.feature_mut(fid).unwrap().kind {
                FeatureKind::Fillet { radius, .. } => *radius = value,
                FeatureKind::Chamfer { distance, .. } => *distance = value,
                _ => unreachable!(),
            },
            |model| {
                match model.status.get(&fid) {
                    Some(FeatureStatus::Ok | FeatureStatus::Warning(_)) => {}
                    Some(FeatureStatus::Failed(reason)) => return Err(reason.clone()),
                    _ => return Err("The feature is suppressed or rolled back".into()),
                }
                for (id, status) in &model.status {
                    if !previously_failed.contains(id) {
                        if let FeatureStatus::Failed(reason) = status {
                            return Err(format!("A later feature would fail: {reason}"));
                        }
                    }
                }
                Ok(())
            },
        )
    }

    pub fn cmd_shell(&mut self) {
        let (body, open_faces) = match self.sel.face {
            Some((b, f)) => {
                let plane = self
                    .ed
                    .model()
                    .bodies
                    .get(&b)
                    .and_then(|m| m.mesh.face_planes.get(f as usize).copied().flatten());
                match plane {
                    Some(p) => (b, vec![p]),
                    None => {
                        self.error("Shell openings must be flat faces");
                        return;
                    }
                }
            }
            None => match self.sel.bodies.first() {
                Some(&b) => (b, Vec::new()),
                None => {
                    self.error(
                        "Select the face to open (or a body for a closed hollow), then Shell",
                    );
                    return;
                }
            },
        };
        let id = self.ed.apply("Shell", |d| {
            d.add_feature(
                "",
                FeatureKind::Shell {
                    body,
                    open_faces,
                    thickness: 2.0,
                },
            )
        });
        self.sel.face = None;
        self.after_feature_created(id);
    }

    /// Sketches (other than `except`) whose curves form an open path usable by Sweep,
    /// latest first.
    pub fn path_sketches(&self, except: FeatureId) -> Vec<FeatureId> {
        let doc = self.ed.doc();
        doc.features
            .iter()
            .rev()
            .filter(|f| f.id != except && matches!(f.kind, FeatureKind::Sketch { .. }))
            .filter(|f| calibro_core::recompute::sketch_path(doc, f.id).is_ok())
            .map(|f| f.id)
            .collect()
    }

    pub fn cmd_sweep(&mut self) {
        let Some((sketch, seeds)) = self.selected_regions_for_one_sketch() else {
            self.error("Select the profile region(s) to sweep, then Sweep (the path is another sketch with an open line/arc chain)");
            return;
        };
        let Some(&path) = self.path_sketches(sketch).first() else {
            self.error(
                "Draw the path first: a separate sketch with connected lines/arcs (an open chain)",
            );
            return;
        };
        let id = self.ed.apply("Sweep", |d| {
            let id = d.add_feature(
                "",
                FeatureKind::Sweep {
                    sketch,
                    profiles: ProfileSelection::Seeds { points: seeds },
                    path,
                    body: BodyMode::NewBody,
                },
            );
            for s in [sketch, path] {
                if let Some(f) = d.feature_mut(s) {
                    f.hidden = true;
                }
            }
            id
        });
        self.after_feature_created(id);
    }

    pub fn cmd_loft(&mut self) {
        let r = self.sel.regions.clone();
        if r.len() != 2 || r[0].0 == r[1].0 {
            self.error(
                "Select two regions in two different sketches (Shift+click the second), then Loft",
            );
            return;
        }
        let id = self.ed.apply("Loft", |d| {
            let id = d.add_feature(
                "",
                FeatureKind::Loft {
                    from_sketch: r[0].0,
                    from_seed: r[0].1,
                    to_sketch: r[1].0,
                    to_seed: r[1].1,
                    body: BodyMode::NewBody,
                },
            );
            for s in [r[0].0, r[1].0] {
                if let Some(f) = d.feature_mut(s) {
                    f.hidden = true;
                }
            }
            id
        });
        self.after_feature_created(id);
    }

    pub fn after_feature_created(&mut self, id: FeatureId) {
        self.sel.feature = Some(id);
        self.sel.regions.clear();
        if let Some(FeatureStatus::Failed(e)) = self.ed.model().status.get(&id) {
            let e = e.clone();
            self.error(format!("The new feature failed: {e}"));
        }
    }

    pub fn delete_selection(&mut self) {
        if let Some(s) = &self.sketch {
            let fid = s.feature;
            let curves = self.sel.sk_curves.clone();
            let points = self.sel.sk_points.clone();
            let cons = self.sel.sk_constraint;
            if curves.is_empty() && points.is_empty() && cons.is_none() {
                return;
            }
            self.ed.apply("Delete sketch entities", |d| {
                if let Some((_, sk)) = d.sketch_mut(fid) {
                    if let Some(c) = cons {
                        sk.remove_constraint(c);
                    }
                    for c in curves {
                        sk.remove_curve(c);
                    }
                    for p in points {
                        sk.remove_point_if_unused(p);
                        // a lone point used only by constraints
                        sk.constraints.retain(|_, c| {
                            !c.referenced_points().contains(&p) || sk.points.contains_key(&p)
                        });
                    }
                    let _ = calibro_core::sketch::solve(sk);
                }
            });
            self.sel.clear_sketch();
            return;
        }
        if let Some(fid) = self.sel.feature {
            let name = self
                .ed
                .doc()
                .feature(fid)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            let gone = self.ed.apply("Delete feature", |d| d.delete_feature(fid));
            self.sel = Selection::default();
            self.info(format!(
                "Deleted {name}{}",
                if gone.len() > 1 {
                    format!(" and {} dependent feature(s)", gone.len() - 1)
                } else {
                    String::new()
                }
            ));
            return;
        }
        if !self.sel.bodies.is_empty() {
            // Deleting a body = deleting the feature that created it.
            let feats: Vec<FeatureId> = self.sel.bodies.iter().map(|b| b.feature).collect();
            self.ed.apply("Delete body", |d| {
                for f in feats {
                    d.delete_feature(f);
                }
            });
            self.sel = Selection::default();
        }
    }

    // ------------------------------------------------------------------ shortcuts

    fn shortcuts(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let typing = ctx.text_edit_focused();
        let consume = |m: Modifiers, k: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
        };
        if consume(cmd_shift, Key::Z) || consume(cmd, Key::Y) {
            if self.ed.redo() {
                self.info("Redo");
            }
        } else if consume(cmd, Key::Z) {
            if let Some(l) = self.ed.undo_label().map(|s| s.to_string()) {
                self.ed.undo();
                self.info(format!("Undo: {l}"));
            }
        }
        if consume(cmd_shift, Key::S) {
            self.save(true);
        } else if consume(cmd, Key::S) {
            self.save(false);
        }
        if consume(cmd, Key::O) {
            self.request(Pending::Open);
        }
        if consume(cmd, Key::N) {
            self.request(Pending::New);
        }
        if consume(cmd, Key::I) {
            self.files.pick(Purpose::Import, IMPORT_FILTERS);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if consume(cmd_shift, Key::P) {
            self.save_image(frame);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
        if typing {
            return;
        }
        let plain = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if plain(Key::Escape) {
            self.escape();
        }
        if plain(Key::Delete) || plain(Key::Backspace) {
            self.delete_selection();
        }
        if plain(Key::F) {
            self.zoom_to_fit();
        }
        if plain(Key::Num1) {
            self.set_view("Front");
        }
        if plain(Key::Num3) {
            self.set_view("Right");
        }
        if plain(Key::Num7) {
            self.set_view("Top");
        }
        if plain(Key::Num0) {
            self.set_view("Iso");
        }
        if plain(Key::Num5) {
            self.ed
                .apply_silent(|d| d.camera.orthographic = !d.camera.orthographic);
        }
        if plain(Key::X) {
            self.display = if self.display == DisplayMode::XRay {
                DisplayMode::ShadedEdges
            } else {
                DisplayMode::XRay
            };
        }
        if plain(Key::M) {
            self.set_tool(Tool::Measure);
        }
        if self.sketch.is_some() {
            if plain(Key::L) {
                self.set_tool(Tool::Line);
            }
            if plain(Key::R) {
                self.set_tool(Tool::Rectangle);
            }
            if plain(Key::C) {
                self.set_tool(Tool::Circle);
            }
            if plain(Key::A) {
                self.set_tool(Tool::Arc);
            }
            if plain(Key::P) {
                self.set_tool(Tool::Polygon);
            }
            if plain(Key::S) {
                self.set_tool(Tool::Select);
            }
            if plain(Key::Enter) {
                self.finish_sketch();
            }
        } else {
            if plain(Key::E) {
                self.cmd_extrude();
            }
            if plain(Key::Space) {
                match self.sel.face {
                    Some((b, f)) => self.start_sketch_on_face(b, f),
                    None => self.info("Select a flat face first, then press Space to sketch on it"),
                }
            }
            if plain(Key::S) {
                self.picking_plane = true;
                self.info("Pick a plane: choose Top/Front/Right or click a flat face");
            }
        }
    }

    pub fn escape(&mut self) {
        if let Some(handle) = self.handle.take() {
            if matches!(handle.kind, HandleKind::EdgeAmount(_)) {
                self.ed.cancel_gesture();
                self.info("Edge adjustment cancelled");
            } else {
                self.ed.end_gesture();
            }
            return;
        }
        if self.picking_plane {
            self.picking_plane = false;
            return;
        }
        if let Some(s) = &mut self.sketch {
            if !s.clicks.is_empty() {
                s.clicks.clear();
                s.chain_start = None;
                s.circle_dragging = false;
                return;
            }
            if self.tool != Tool::Select {
                self.tool = Tool::Select;
                return;
            }
            if !self.sel.sk_curves.is_empty() || !self.sel.sk_points.is_empty() {
                self.sel.clear_sketch();
                return;
            }
            self.finish_sketch();
            return;
        }
        if self.tool == Tool::Measure {
            if self.measure.a.is_some() {
                self.measure = Measure::default();
            } else {
                self.tool = Tool::Select;
            }
            return;
        }
        self.sel = Selection::default();
    }

    // ------------------------------------------------------------------ dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self
            .pending
            .as_ref()
            .map(|p| !matches!(p, Pending::OpenPicked(_)))
            .unwrap_or(false)
        {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Save changes?");
                ui.add_space(4.0);
                ui.label(format!(
                    "“{}” has unsaved changes. They will be lost if you don't save them.",
                    self.file_name
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            });
            match choice {
                Some(0) => {
                    if self.save(false) {
                        if let Some(p) = self.pending.take() {
                            self.run_pending(p);
                        }
                    }
                }
                Some(1) => {
                    if let Some(p) = self.pending.take() {
                        self.run_pending(p);
                    }
                }
                Some(2) => self.pending = None,
                _ => {}
            }
        }
        if self.recovery.is_some() {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("recovery")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Restore unsaved work?");
                ui.label(
                    "Calibro closed last time with unsaved changes. A recovery copy was kept.",
                );
                ui.small("(Imported meshes are not part of the recovery copy.)");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(c) = choice {
                if let Some(doc) = self.recovery.take() {
                    if c {
                        self.ed.replace_document(doc);
                        self.info("Recovered unsaved work — remember to save it");
                    }
                }
            }
        }
        if self.show_shortcuts {
            egui::Window::new("Keyboard shortcuts")
                .open(&mut self.show_shortcuts)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("keys").striped(true).show(ui, |ui| {
                        for (k, v) in SHORTCUTS {
                            ui.monospace(*k);
                            ui.label(*v);
                            ui.end_row();
                        }
                    });
                });
        }
        if self.show_about {
            egui::Window::new("About Calibro")
                .open(&mut self.show_about)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading(format!("Calibro {}", env!("CARGO_PKG_VERSION")));
                    ui.label(
                        "An independent, open-source precision 3D CAD application written in Rust.",
                    );
                    ui.label("Geometry kernel: truck (B-rep). UI: egui. Rendering: wgpu.");
                    ui.small("Calibro is not affiliated with or endorsed by Shapr3D.");
                });
        }
    }
}

pub const IMPORT_FILTERS: &[(&str, &[&str])] = &[
    ("All supported", &["step", "stp", "stl", "obj", "calibro"]),
    ("STEP", &["step", "stp"]),
    ("STL", &["stl"]),
    ("OBJ", &["obj"]),
];

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+N / O / S", "New / Open / Save"),
    ("Ctrl+Shift+S", "Save as"),
    ("Ctrl+I", "Import STEP/STL/OBJ"),
    ("Ctrl+Z / Ctrl+Shift+Z", "Undo / Redo"),
    ("Ctrl+Shift+P", "Save viewport image (desktop)"),
    ("S", "New sketch (pick plane) · in a sketch: Select tool"),
    ("L R C A P", "Sketch: Line, Rectangle, Circle, Arc, Polygon"),
    ("Enter", "Finish sketch"),
    ("E", "Extrude selected regions"),
    (
        "Space",
        "Sketch on the selected flat face (camera turns to face it)",
    ),
    ("M", "Measure"),
    ("Delete", "Delete selection"),
    ("Esc", "Cancel / step back"),
    ("F", "Zoom to fit"),
    ("1 / 3 / 7 / 0", "Front / Right / Top / Isometric view"),
    ("5", "Toggle perspective / orthographic"),
    ("X", "Toggle X-ray"),
    ("Right-drag", "Orbit"),
    ("Middle-drag, Shift+right-drag", "Pan"),
    ("Wheel / pinch", "Zoom at cursor"),
];

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

impl eframe::App for CalibroApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.time = ctx.input(|i| i.time);
        while let Some(f) = self.files.poll() {
            match f.purpose {
                Purpose::OpenProject => self.request(Pending::OpenPicked(f)),
                Purpose::Import => self.import_picked(f),
            }
        }
        // Unsaved-changes guard when closing the window (desktop).
        if ctx.input(|i| i.viewport().close_requested()) && self.ed.is_dirty() && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        if self.allow_close && self.pending.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.handle.is_some()
            && self.sketch.is_none()
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        {
            self.handle = None;
        }
        if ctx.input(|i| {
            (i.pointer.any_released() && self.handle.is_none()) || i.key_pressed(egui::Key::Enter)
        }) {
            self.ed.end_gesture();
        }
        let title = format!(
            "{}{} — Calibro",
            if self.ed.is_dirty() { "● " } else { "" },
            self.file_name
        );
        if title != self.last_title {
            #[cfg(target_arch = "wasm32")]
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                doc.set_title(&title);
            }
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        // Small screens (phones, narrow browser windows): start with the side
        // panels hidden so the viewport stays usable. They can be toggled any time.
        let w = ctx.content_rect().width();
        if w > 0.0 {
            if !self.layout_initialised {
                self.show_inspector = w >= 900.0;
                self.show_tools = w >= 620.0;
                self.layout_initialised = true;
            } else {
                if self.layout_width >= 900.0 && w < 900.0 {
                    self.show_inspector = false;
                }
                if self.layout_width >= 620.0 && w < 620.0 {
                    self.show_tools = false;
                }
            }
            self.layout_width = w;
        }

        self.shortcuts(&ctx, frame);
        self.menu_bar(ui, frame);
        self.status_bar(ui);
        // Choosing a sketch plane happens in the inspector, so make sure it is visible.
        if self.picking_plane && self.sketch.is_none() {
            self.show_inspector = true;
        }
        if self.show_tools {
            self.left_panel(ui);
        }
        if self.show_inspector {
            self.right_panel(ui);
        }
        self.viewport_panel(ui, frame);
        self.dialogs(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let mut doc = self.ed.doc().clone();
        doc.resources.clear();
        let v = serde_json::json!({ "dirty": self.ed.is_dirty(), "doc": doc });
        storage.set_string(RECOVERY_KEY, v.to_string());
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(20)
    }
}
