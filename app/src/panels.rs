//! Menu bar, tool palette, bodies & history lists, inspector and status bar.

use crate::app::*;
use crate::files::Purpose;
use calibro_core::kernel::{BooleanOp, Plane, Transform};
use calibro_core::sketch::{Constraint, CurveKind, SolveStatus};
use calibro_core::*;
use egui::{Color32, RichText};

pub const ACCENT: Color32 = Color32::from_rgb(255, 140, 26);
pub const OK_GREEN: Color32 = Color32::from_rgb(30, 142, 62);
pub const UNDER_BLUE: Color32 = Color32::from_rgb(42, 111, 219);
pub const ERR_RED: Color32 = Color32::from_rgb(210, 50, 40);

/// Drag value for a length in model millimetres, displayed in document units.
pub fn length_edit(ui: &mut egui::Ui, mm: &mut f64, units: Units) -> bool {
    let k = units.in_mm();
    let mut v = *mm / k;
    let r = ui.add(
        egui::DragValue::new(&mut v)
            .speed(0.1 / k.max(1.0))
            .max_decimals(4)
            .suffix(format!(" {}", units.suffix())),
    );
    if r.changed() {
        *mm = v * k;
        true
    } else {
        false
    }
}

pub fn angle_edit(ui: &mut egui::Ui, deg: &mut f64) -> bool {
    ui.add(
        egui::DragValue::new(deg)
            .speed(0.5)
            .max_decimals(3)
            .suffix("°"),
    )
    .changed()
}

fn vec3_edit(ui: &mut egui::Ui, v: &mut [f64; 3], units: Units) -> bool {
    let mut c = false;
    ui.horizontal(|ui| {
        for (i, l) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(*l);
            c |= length_edit(ui, &mut v[i], units);
        }
    });
    c
}

fn dir_combo(ui: &mut egui::Ui, id: &str, v: &mut [f64; 3]) -> bool {
    let current = match v {
        [x, y, z] if *x == 1.0 && *y == 0.0 && *z == 0.0 => "X axis",
        [x, y, z] if *x == 0.0 && *y == 1.0 && *z == 0.0 => "Y axis",
        [x, y, z] if *x == 0.0 && *y == 0.0 && *z == 1.0 => "Z axis",
        _ => "Custom",
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (name, d) in [
                ("X axis", [1.0, 0.0, 0.0]),
                ("Y axis", [0.0, 1.0, 0.0]),
                ("Z axis", [0.0, 0.0, 1.0]),
            ] {
                if ui.selectable_label(current == name, name).clicked() {
                    *v = d;
                    changed = true;
                }
            }
        });
    changed
}

fn plane_combo(ui: &mut egui::Ui, id: &str, p: &mut Plane) -> bool {
    let current = if *p == Plane::XY {
        "Top (XY)"
    } else if *p == Plane::XZ {
        "Front (XZ)"
    } else if *p == Plane::YZ {
        "Right (YZ)"
    } else {
        "Custom"
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (name, pl) in [
                ("Top (XY)", Plane::XY),
                ("Front (XZ)", Plane::XZ),
                ("Right (YZ)", Plane::YZ),
            ] {
                if ui.selectable_label(current == name, name).clicked() {
                    *p = pl;
                    changed = true;
                }
            }
        });
    changed
}

impl CalibroApp {
    pub fn body_name(&self, b: BodyId) -> String {
        self.ed
            .doc()
            .body_props(b)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| {
                let f = self
                    .ed
                    .doc()
                    .feature(b.feature)
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                if b.index == 0 {
                    f
                } else {
                    format!("{f} #{}", b.index + 1)
                }
            })
    }

    fn body_combo(&self, ui: &mut egui::Ui, id: &str, b: &mut BodyId) -> bool {
        let mut changed = false;
        egui::ComboBox::from_id_salt(id)
            .selected_text(self.body_name(*b))
            .show_ui(ui, |ui| {
                for k in self.ed.model().bodies.keys() {
                    if ui.selectable_label(k == b, self.body_name(*k)).clicked() {
                        *b = *k;
                        changed = true;
                    }
                }
            });
        changed
    }

    fn body_mode_ui(&self, ui: &mut egui::Ui, mode: &mut BodyMode, fid: FeatureId) -> bool {
        let mut changed = false;
        let names = ["New body", "Join", "Cut", "Intersect"];
        let cur = match mode {
            BodyMode::NewBody => 0,
            BodyMode::Join { .. } => 1,
            BodyMode::Cut { .. } => 2,
            BodyMode::Intersect { .. } => 3,
        };
        // a target must come from an earlier feature
        let default_target = self
            .ed
            .model()
            .bodies
            .keys()
            .find(|b| b.feature != fid)
            .copied();
        ui.horizontal(|ui| {
            ui.label("Result");
            egui::ComboBox::from_id_salt(("mode", fid))
                .selected_text(names[cur])
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        if ui.selectable_label(i == cur, *n).clicked() && i != cur {
                            let t = mode.target().or(default_target);
                            *mode = match (i, t) {
                                (0, _) => BodyMode::NewBody,
                                (1, Some(t)) => BodyMode::Join { target: t },
                                (2, Some(t)) => BodyMode::Cut { target: t },
                                (3, Some(t)) => BodyMode::Intersect { target: t },
                                _ => *mode,
                            };
                            changed = true;
                        }
                    }
                });
        });
        let mut target = mode.target();
        if let Some(t) = &mut target {
            ui.horizontal(|ui| {
                ui.label("Body");
                if self.body_combo(ui, &format!("target{fid}"), t) {
                    *mode = match *mode {
                        BodyMode::Join { .. } => BodyMode::Join { target: *t },
                        BodyMode::Cut { .. } => BodyMode::Cut { target: *t },
                        BodyMode::Intersect { .. } => BodyMode::Intersect { target: *t },
                        m => m,
                    };
                    changed = true;
                }
            });
        }
        changed
    }

    // ------------------------------------------------------------------ menu

    pub fn menu_bar(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(RichText::new("Calibro").strong().color(ACCENT));
                ui.menu_button("File", |ui| {
                    if ui.button("New project").clicked() {
                        self.request(Pending::New);
                    }
                    if ui.button("Open…").clicked() {
                        self.request(Pending::Open);
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        self.save(false);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Save as…").clicked() {
                        self.save(true);
                    }
                    ui.separator();
                    if ui.button("Import STEP / STL / OBJ…").clicked() {
                        self.files.pick(Purpose::Import, IMPORT_FILTERS);
                    }
                    ui.menu_button("Export all visible", |ui| self.export_menu(ui, false));
                    ui.add_enabled_ui(!self.sel.bodies.is_empty(), |ui| {
                        ui.menu_button("Export selected bodies", |ui| self.export_menu(ui, true));
                    });
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Save viewport image (PNG)…").clicked() {
                        self.save_image(frame);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.separator();
                        if ui.button("Quit").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
                #[cfg(target_arch = "wasm32")]
                let _ = &frame;
                ui.menu_button("Edit", |ui| {
                    let ul = self
                        .ed
                        .undo_label()
                        .map(|s| format!("Undo {s}"))
                        .unwrap_or("Undo".into());
                    if ui
                        .add_enabled(self.ed.can_undo(), egui::Button::new(ul))
                        .clicked()
                    {
                        self.ed.undo();
                    }
                    let rl = self
                        .ed
                        .redo_label()
                        .map(|s| format!("Redo {s}"))
                        .unwrap_or("Redo".into());
                    if ui
                        .add_enabled(self.ed.can_redo(), egui::Button::new(rl))
                        .clicked()
                    {
                        self.ed.redo();
                    }
                    ui.separator();
                    if ui.button("Delete selection").clicked() {
                        self.delete_selection();
                    }
                });
                ui.menu_button("View", |ui| {
                    for v in ["Top", "Front", "Right", "Back", "Left", "Bottom", "Iso"] {
                        if ui.button(format!("{v} view")).clicked() {
                            self.set_view(v);
                        }
                    }
                    ui.separator();
                    let mut ortho = self.ed.doc().camera.orthographic;
                    if ui.checkbox(&mut ortho, "Orthographic").changed() {
                        self.ed.apply_silent(|d| d.camera.orthographic = ortho);
                    }
                    if ui.button("Zoom to fit (F)").clicked() {
                        self.zoom_to_fit();
                    }
                    ui.separator();
                    ui.radio_value(
                        &mut self.display,
                        DisplayMode::ShadedEdges,
                        "Shaded with edges",
                    );
                    ui.radio_value(&mut self.display, DisplayMode::Shaded, "Shaded");
                    ui.radio_value(&mut self.display, DisplayMode::Wireframe, "Wireframe");
                    ui.radio_value(&mut self.display, DisplayMode::XRay, "X-ray");
                    ui.separator();
                    ui.checkbox(&mut self.show_grid, "Grid");
                    ui.checkbox(&mut self.show_tools, "Tool panel");
                    ui.checkbox(&mut self.show_inspector, "Inspector panel");
                    ui.checkbox(&mut self.section.enabled, "Section view");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard shortcuts").clicked() {
                        self.show_shortcuts = true;
                    }
                    if ui.button("About Calibro").clicked() {
                        self.show_about = true;
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(self.ed.can_undo(), egui::Button::new("Undo"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    self.ed.undo();
                }
                if ui
                    .add_enabled(self.ed.can_redo(), egui::Button::new("Redo"))
                    .on_hover_text("Redo (Ctrl+Shift+Z)")
                    .clicked()
                {
                    self.ed.redo();
                }
            });
        });
    }

    fn export_menu(&mut self, ui: &mut egui::Ui, selected: bool) {
        for (label, fmt, tip) in [
            (
                "STEP (.step) — exact CAD",
                "step",
                "Exact B-rep solids for Fusion, SolidWorks, FreeCAD… (no history)",
            ),
            (
                "glTF binary (.glb)",
                "glb",
                "Meshes + materials for Blender, web viewers, game engines",
            ),
            (
                "OBJ + MTL (.obj)",
                "obj",
                "Meshes + colours for Blender, Maya, Cinema 4D, 3ds Max",
            ),
            (
                "STL binary (.stl)",
                "stl",
                "Watertight triangle mesh for 3D printing (mm)",
            ),
            ("STL ASCII (.stl)", "stl-ascii", "Text STL, larger files"),
        ] {
            if ui.button(label).on_hover_text(tip).clicked() {
                self.export(fmt, selected);
                ui.close();
            }
        }
    }

    // ------------------------------------------------------------------ status bar

    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let fresh = self.message.as_ref().map(|m| self.time - m.2 < 8.0).unwrap_or(false);
                if let (Some((msg, err, _)), true) = (&self.message, fresh) {
                    ui.colored_label(if *err { ERR_RED } else { ui.visuals().text_color() }, msg);
                } else if self.picking_plane {
                    ui.label("Pick a sketch plane: use the buttons in the inspector, or click a flat face");
                } else {
                    ui.weak(self.tool.hint());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let tris: usize = self.ed.model().bodies.values().map(|b| b.mesh.triangle_count()).sum();
                    ui.weak(format!("{} · {} bodies · {} tris", self.ed.doc().units.suffix(), self.ed.model().bodies.len(), tris));
                    if self.ed.is_dirty() {
                        ui.colored_label(ACCENT, "unsaved");
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------ left: tools

    pub fn left_panel(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.ctx().content_rect().width() < 1200.0;
        let width = if narrow { 150.0 } else { 190.0 };
        egui::Panel::left("tools").resizable(false).exact_size(width).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let in_sketch = self.sketch.is_some();
                let w = ui.available_width();
                let tool_btn = |ui: &mut egui::Ui, selected: bool, label: &str, tip: &str| -> bool {
                    ui.add_sized([w, 26.0], egui::Button::selectable(selected, label)).on_hover_text(tip).clicked()
                };
                ui.add_space(4.0);
                if tool_btn(ui, self.tool == Tool::Select, "Select", "Select faces, bodies, regions (Esc)") {
                    self.tool = Tool::Select;
                }
                if tool_btn(ui, self.tool == Tool::Measure, "Measure", "Distance between two points (M)") {
                    self.tool = Tool::Measure;
                    self.measure = Measure::default();
                }
                ui.separator();
                ui.label(RichText::new("SKETCH").small().strong());
                if !in_sketch {
                    if tool_btn(ui, self.picking_plane, "New sketch", "Choose a plane or click a flat face (S)") {
                        self.picking_plane = !self.picking_plane;
                    }
                } else {
                    for (t, label, tip) in [
                        (Tool::Line, "Line", "L"),
                        (Tool::Rectangle, "Rectangle", "R"),
                        (Tool::Circle, "Circle", "C"),
                        (Tool::Arc, "Arc", "A"),
                        (Tool::Polygon, "Polygon", "P"),
                    ] {
                        if tool_btn(ui, self.tool == t, label, tip) {
                            self.tool = t;
                            if let Some(s) = &mut self.sketch {
                                s.clicks.clear();
                                s.chain_start = None;
                            }
                        }
                    }
                    if ui.add_sized([w, 28.0], egui::Button::new(RichText::new("Finish sketch").strong())).clicked() {
                        self.finish_sketch();
                    }
                }
                ui.separator();
                ui.add_enabled_ui(!in_sketch, |ui| {
                    ui.label(RichText::new("SOLID").small().strong());
                    if tool_btn(ui, false, "Extrude", "Extrude selected sketch regions (E)") {
                        self.cmd_extrude();
                    }
                    if tool_btn(ui, false, "Revolve", "Revolve selected regions around an axis") {
                        self.cmd_revolve();
                    }
                    if tool_btn(ui, false, "Box", "Add a box") {
                        self.cmd_primitive(Primitive::Box { min: [0.0; 3], size: [40.0, 30.0, 20.0] });
                    }
                    if tool_btn(ui, false, "Cylinder", "Add a cylinder") {
                        self.cmd_primitive(Primitive::Cylinder { base: [0.0; 3], radius: 15.0, height: 30.0 });
                    }
                    if tool_btn(ui, false, "Sphere", "Add a sphere") {
                        self.cmd_primitive(Primitive::Sphere { center: [0.0, 0.0, 20.0], radius: 20.0 });
                    }
                    ui.separator();
                    ui.label(RichText::new("MODIFY").small().strong());
                    if tool_btn(ui, false, "Union", "Merge selected bodies into the first one") {
                        self.cmd_boolean(BooleanOp::Union);
                    }
                    if tool_btn(ui, false, "Subtract", "Cut the other selected bodies from the first one") {
                        self.cmd_boolean(BooleanOp::Subtract);
                    }
                    if tool_btn(ui, false, "Intersect", "Keep only the overlap") {
                        self.cmd_boolean(BooleanOp::Intersect);
                    }
                    if tool_btn(ui, false, "Move / Rotate", "Transform selected bodies with exact values") {
                        self.cmd_on_selected_bodies("Move", |bodies| FeatureKind::Transform {
                            bodies,
                            transform: Transform::default(),
                            copy: false,
                        });
                    }
                    if tool_btn(ui, false, "Linear pattern", "Copies along a direction") {
                        self.cmd_on_selected_bodies("Linear pattern", |b| FeatureKind::LinearPattern {
                            body: b[0],
                            direction: [1.0, 0.0, 0.0],
                            count: 3,
                            spacing: 50.0,
                        });
                    }
                    if tool_btn(ui, false, "Circular pattern", "Copies around an axis") {
                        self.cmd_on_selected_bodies("Circular pattern", |b| FeatureKind::CircularPattern {
                            body: b[0],
                            axis_origin: [0.0; 3],
                            axis_dir: [0.0, 0.0, 1.0],
                            count: 6,
                            total_angle_deg: 360.0,
                        });
                    }
                    if tool_btn(ui, false, "Mirror", "Mirror copy across a plane") {
                        self.cmd_on_selected_bodies("Mirror", |b| FeatureKind::Mirror { body: b[0], plane: Plane::YZ });
                    }
                    ui.add_enabled_ui(false, |ui| {
                        for (l, why) in [
                            ("Fillet", "Planned: the truck kernel has no fillet operation yet (see FEATURE_TRACKER.md)"),
                            ("Chamfer", "Planned: needs kernel support (see FEATURE_TRACKER.md)"),
                            ("Shell", "Planned: needs offset surfaces in the kernel (see FEATURE_TRACKER.md)"),
                            ("Sweep / Loft", "Planned: kernel has the building blocks, feature not implemented yet"),
                        ] {
                            ui.add_sized([w, 24.0], egui::Button::new(l)).on_disabled_hover_text(why);
                        }
                    });
                });
            });
        });
    }

    // ------------------------------------------------------------------ right: bodies, history, inspector

    pub fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(if ui.ctx().content_rect().width() < 1200.0 {
                260.0
            } else {
                310.0
            })
            .min_size(220.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::CollapsingHeader::new(RichText::new("Bodies").strong())
                        .default_open(true)
                        .show(ui, |ui| self.bodies_ui(ui));
                    egui::CollapsingHeader::new(RichText::new("History").strong())
                        .default_open(true)
                        .show(ui, |ui| self.history_ui(ui));
                    ui.separator();
                    self.inspector_ui(ui);
                });
            });
    }

    fn bodies_ui(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<BodyId> = self.ed.model().bodies.keys().copied().collect();
        if ids.is_empty() {
            ui.weak("No bodies yet. Sketch a shape and extrude it, or add a primitive.");
        }
        for b in ids {
            let visible = self
                .ed
                .doc()
                .body_props(b)
                .map(|p| p.visible)
                .unwrap_or(true);
            let color = self
                .ed
                .doc()
                .body_props(b)
                .map(|p| p.material.color)
                .unwrap_or(Material::default().color);
            let is_mesh = matches!(self.ed.model().bodies[&b].geometry, BodyGeometry::Mesh);
            ui.horizontal(|ui| {
                let eye = if visible { "Hide" } else { "Show" };
                if ui.small_button(eye).on_hover_text("Show / hide").clicked() {
                    self.ed.apply("Toggle visibility", |d| {
                        let p = d.body_props_mut(b);
                        p.visible = !p.visible;
                    });
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 3.0, Color32::from_rgb((color[0] * 255.0) as u8, (color[1] * 255.0) as u8, (color[2] * 255.0) as u8));
                let selected = self.sel.bodies.contains(&b);
                let mut text = RichText::new(self.body_name(b));
                if is_mesh {
                    text = text.italics();
                }
                let resp = ui.selectable_label(selected, text);
                if resp.clicked() {
                    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                    if !add {
                        self.sel = Selection::default();
                    }
                    if selected && add {
                        self.sel.bodies.retain(|x| *x != b);
                    } else {
                        self.sel.bodies.push(b);
                    }
                }
                if is_mesh {
                    resp.on_hover_text("Reference mesh (imported): can be viewed, measured and exported as a mesh, but not used in solid operations");
                }
            });
        }
    }

    fn history_ui(&mut self, ui: &mut egui::Ui) {
        let feats: Vec<(FeatureId, String, &'static str, bool, bool)> = self
            .ed
            .doc()
            .features
            .iter()
            .map(|f| {
                (
                    f.id,
                    f.name.clone(),
                    f.kind.type_name(),
                    f.suppressed,
                    f.hidden,
                )
            })
            .collect();
        if feats.is_empty() {
            ui.weak("The feature history appears here. Every step stays editable.");
            return;
        }
        let rollback = self.ed.doc().rollback;
        for (i, (id, name, ty, suppressed, hidden)) in feats.iter().enumerate() {
            if rollback == Some(i) {
                ui.horizontal(|ui| {
                    ui.colored_label(ACCENT, "--- rolled back here ---");
                    if ui.small_button("Roll forward").clicked() {
                        self.ed.apply("Roll forward", |d| d.rollback = None);
                    }
                });
            }
            let status = self
                .ed
                .model()
                .status
                .get(id)
                .cloned()
                .unwrap_or(FeatureStatus::Ok);
            let icon = "";
            let mut text = RichText::new(format!("{icon}{name}"));
            match &status {
                FeatureStatus::Failed(_) => text = text.color(ERR_RED),
                FeatureStatus::Warning(_) => text = text.color(ACCENT),
                FeatureStatus::Suppressed | FeatureStatus::RolledBack => {
                    text = text.weak().strikethrough()
                }
                _ => {}
            }
            if let Some(SolveStatus::FullyConstrained) = self.ed.model().sketch_status.get(id) {
                text = text.color(OK_GREEN);
            }
            let id = *id;
            ui.horizontal(|ui| {
                if *ty == "Sketch" {
                    let eye = if *hidden { "Show" } else { "Hide" };
                    if ui.small_button(eye).clicked() {
                        self.ed.apply("Toggle sketch visibility", |d| {
                            let f = d.feature_mut(id).unwrap();
                            f.hidden = !f.hidden;
                        });
                    }
                }
                let resp = ui.selectable_label(self.sel.feature == Some(id), text);
                let resp = match &status {
                    FeatureStatus::Failed(e) | FeatureStatus::Warning(e) => resp.on_hover_text(e),
                    _ => resp,
                };
                if resp.clicked() {
                    self.sel = Selection {
                        feature: Some(id),
                        ..Default::default()
                    };
                }
                if resp.double_clicked() && *ty == "Sketch" && self.sketch.is_none() {
                    self.enter_sketch(id);
                }
                resp.context_menu(|ui| {
                    if *ty == "Sketch" && ui.button("Edit sketch").clicked() {
                        self.enter_sketch(id);
                        ui.close();
                    }
                    let label = if *suppressed {
                        "Unsuppress"
                    } else {
                        "Suppress"
                    };
                    if ui.button(label).clicked() {
                        self.ed.apply(label, |d| {
                            let f = d.feature_mut(id).unwrap();
                            f.suppressed = !f.suppressed;
                        });
                        ui.close();
                    }
                    if ui.button("Roll back to before this").clicked() {
                        self.ed.apply("Roll back", |d| d.rollback = Some(i));
                        ui.close();
                    }
                    if ui.button("Delete (and dependents)").clicked() {
                        self.sel = Selection {
                            feature: Some(id),
                            ..Default::default()
                        };
                        self.delete_selection();
                        ui.close();
                    }
                });
            });
        }
    }

    // ------------------------------------------------------------------ inspector

    fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        if self.picking_plane && self.sketch.is_none() {
            ui.label("Choose a sketch plane:");
            ui.horizontal(|ui| {
                for (n, p) in [
                    ("Top", Plane::XY),
                    ("Front", Plane::XZ),
                    ("Right", Plane::YZ),
                ] {
                    if ui.button(n).clicked() {
                        self.start_sketch(p, None);
                    }
                }
            });
            ui.weak("…or click a flat face of a body in the viewport.");
            return;
        }
        if self.sketch.is_some() {
            self.sketch_inspector(ui);
            return;
        }
        if self.tool == Tool::Measure {
            self.measure_inspector(ui);
            return;
        }
        if !self.sel.regions.is_empty() {
            ui.label(format!(
                "{} sketch region(s) selected",
                self.sel.regions.len()
            ));
            ui.horizontal(|ui| {
                if ui.button("Extrude").clicked() {
                    self.cmd_extrude();
                }
                if ui.button("Revolve").clicked() {
                    self.cmd_revolve();
                }
            });
            return;
        }
        if let Some(fid) = self.sel.feature {
            self.feature_inspector(ui, fid);
            return;
        }
        if let Some((b, face)) = self.sel.face {
            self.face_inspector(ui, b, face);
        }
        if let Some((b, e)) = self.sel.edge {
            if let Some(body) = self.ed.model().bodies.get(&b) {
                if let Some(pl) = body.mesh.edges.get(e) {
                    let len = crate::picking::polyline_length(pl);
                    ui.label(format!("Edge length: {}", self.fmt_len(len)));
                }
            }
        }
        if !self.sel.bodies.is_empty() {
            let bodies = self.sel.bodies.clone();
            for b in bodies {
                self.body_inspector(ui, b);
                ui.separator();
            }
            return;
        }
        // nothing selected: document properties
        ui.label(RichText::new("Project").strong());
        let mut meta = self.ed.doc().metadata.clone();
        let mut units = self.ed.doc().units;
        egui::Grid::new("meta").num_columns(2).show(ui, |ui| {
            ui.label("Title");
            ui.text_edit_singleline(&mut meta.title);
            ui.end_row();
            ui.label("Author");
            ui.text_edit_singleline(&mut meta.author);
            ui.end_row();
            ui.label("Units");
            egui::ComboBox::from_id_salt("units")
                .selected_text(units.suffix())
                .show_ui(ui, |ui| {
                    for u in [
                        Units::Millimetres,
                        Units::Centimetres,
                        Units::Metres,
                        Units::Inches,
                    ] {
                        ui.selectable_value(&mut units, u, u.suffix());
                    }
                });
            ui.end_row();
        });
        if meta != self.ed.doc().metadata || units != self.ed.doc().units {
            self.ed.apply_coalesced("Edit project info", 0, |d| {
                d.metadata = meta;
                d.units = units;
            });
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Getting started").strong());
        ui.label("1. Press S (or New sketch) and pick a plane.\n2. Draw a rectangle or circle.\n3. Finish the sketch, click inside the shape.\n4. Press E to extrude, then type the distance here.\n5. Save with Ctrl+S — the whole history stays editable.");
    }

    pub fn fmt_len(&self, mm: f64) -> String {
        let u = self.ed.doc().units;
        format!("{:.3} {}", mm / u.in_mm(), u.suffix())
    }

    fn measure_inspector(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Measure").strong());
        match (self.measure.a, self.measure.b) {
            (Some(a), Some(b)) => {
                let d = b - a;
                egui::Grid::new("meas").show(ui, |ui| {
                    ui.label("Distance");
                    ui.strong(self.fmt_len(d.length()));
                    ui.end_row();
                    for (n, v) in [("ΔX", d.x), ("ΔY", d.y), ("ΔZ", d.z)] {
                        ui.label(n);
                        ui.label(self.fmt_len(v));
                        ui.end_row();
                    }
                });
            }
            (Some(_), None) => {
                ui.label("Click the second point");
            }
            _ => {
                ui.label("Click the first point on a face or edge");
            }
        }
        if ui.button("Reset").clicked() {
            self.measure = Measure::default();
        }
    }

    fn face_inspector(&mut self, ui: &mut egui::Ui, b: BodyId, face: u32) {
        let Some(body) = self.ed.model().bodies.get(&b) else {
            return;
        };
        let m = &body.mesh;
        let mut area = 0.0;
        for t in m.indices.chunks_exact(3) {
            if m.face_ids[t[0] as usize] != face {
                continue;
            }
            let p = |i: u32| glam::DVec3::from(m.positions[i as usize].map(|x| x as f64));
            area += (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() * 0.5;
        }
        let plane = m.face_planes.get(face as usize).copied().flatten();
        ui.label(RichText::new("Face").strong());
        let k = self.ed.doc().units.in_mm();
        ui.label(format!(
            "Area: {:.3} {}²",
            area / (k * k),
            self.ed.doc().units.suffix()
        ));
        if let Some(pl) = plane {
            let n = pl.normal();
            ui.label(format!(
                "Planar, normal ({:.3}, {:.3}, {:.3})",
                n[0], n[1], n[2]
            ));
            if ui.button("Sketch on this face").clicked() {
                self.start_sketch(pl, Some(b));
            }
        } else {
            ui.weak("Curved face");
        }
        ui.separator();
    }

    fn body_inspector(&mut self, ui: &mut egui::Ui, b: BodyId) {
        let Some(body) = self.ed.model().bodies.get(&b) else {
            return;
        };
        let mesh = body.mesh.clone();
        let is_solid = body.solid().is_some();
        let mut props = self.ed.doc().body_props(b).cloned().unwrap_or(BodyProps {
            name: self.body_name(b),
            material: Material::default(),
            visible: true,
        });
        let before = props.clone();
        ui.label(RichText::new("Body").strong());
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut props.name);
        });
        ui.horizontal(|ui| {
            ui.label("Material");
            egui::ComboBox::from_id_salt(("mat", b))
                .selected_text(props.material.name.clone())
                .show_ui(ui, |ui| {
                    for m in material_presets() {
                        if ui
                            .selectable_label(m.name == props.material.name, &m.name)
                            .clicked()
                        {
                            props.material = m;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Colour");
            let mut c = props.material.color;
            if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() {
                props.material.color = c;
                props.material.name = "Custom".into();
            }
        });
        ui.add(egui::Slider::new(&mut props.material.metallic, 0.0..=1.0).text("Metallic"));
        ui.add(egui::Slider::new(&mut props.material.roughness, 0.02..=1.0).text("Roughness"));
        ui.checkbox(&mut props.visible, "Visible");
        if props != before {
            self.ed
                .apply_coalesced("Edit body", b.feature * 1000 + b.index as u64, |d| {
                    *d.body_props_mut(b) = props
                });
        }
        let k = self.ed.doc().units.in_mm();
        let u = self.ed.doc().units.suffix();
        ui.collapsing("Measurements", |ui| {
            ui.label(format!("Volume: {:.3} {u}³", mesh.volume() / (k * k * k)));
            ui.label(format!(
                "Surface area: {:.3} {u}²",
                mesh.surface_area() / (k * k)
            ));
            let s = [0, 1, 2].map(|i| (mesh.bbox_max[i] - mesh.bbox_min[i]) as f64 / k);
            ui.label(format!("Size: {:.3} × {:.3} × {:.3} {u}", s[0], s[1], s[2]));
            ui.weak("(volume and area from the display tessellation)");
        });
        if !is_solid {
            ui.weak("Reference mesh — solid operations are not available.");
        }
        if ui.button("Show feature that created it").clicked() {
            self.sel = Selection {
                feature: Some(b.feature),
                ..Default::default()
            };
        }
    }

    fn feature_inspector(&mut self, ui: &mut egui::Ui, fid: FeatureId) {
        let Some(f) = self.ed.doc().feature(fid).cloned() else {
            self.sel.feature = None;
            return;
        };
        let units = self.ed.doc().units;
        ui.horizontal(|ui| {
            ui.label(RichText::new(f.kind.type_name()).strong());
            match self.ed.model().status.get(&fid) {
                Some(FeatureStatus::Failed(e)) => {
                    ui.colored_label(ERR_RED, "failed").on_hover_text(e);
                }
                Some(FeatureStatus::Warning(e)) => {
                    ui.colored_label(ACCENT, e);
                }
                _ => {}
            }
        });
        if let Some(FeatureStatus::Failed(e)) = self.ed.model().status.get(&fid) {
            ui.colored_label(ERR_RED, format!("Error: {e}"));
        }
        let mut name = f.name.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut name);
        });
        if name != f.name {
            self.ed
                .apply_coalesced("Rename", fid, |d| d.feature_mut(fid).unwrap().name = name);
        }
        let mut k = f.kind.clone();
        let mut changed = false;
        match &mut k {
            FeatureKind::Sketch { plane, sketch, .. } => {
                let o = plane.origin;
                ui.label(format!(
                    "Plane origin ({:.2}, {:.2}, {:.2})",
                    o[0], o[1], o[2]
                ));
                ui.label(format!(
                    "{} curves, {} constraints",
                    sketch.curves.len(),
                    sketch.constraints.len()
                ));
                if let Some(st) = self.ed.model().sketch_status.get(&fid) {
                    ui.label(status_text(st));
                }
                if ui.button("Edit sketch").clicked() {
                    self.enter_sketch(fid);
                    return;
                }
            }
            FeatureKind::Extrude {
                distance,
                symmetric,
                body,
                profiles,
                ..
            } => {
                ui.horizontal(|ui| {
                    ui.label("Distance");
                    changed |= length_edit(ui, distance, units);
                });
                changed |= ui
                    .checkbox(symmetric, "Symmetric (both directions)")
                    .changed();
                if ui.button("Flip direction").clicked() {
                    *distance = -*distance;
                    changed = true;
                }
                changed |= self.body_mode_ui(ui, body, fid);
                if let ProfileSelection::Seeds { points } = profiles {
                    ui.weak(format!("{} region(s)", points.len()));
                }
                ui.weak("Tip: drag the orange arrow in the viewport.");
            }
            FeatureKind::Revolve {
                axis,
                angle_deg,
                body,
                sketch,
                ..
            } => {
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    changed |= angle_edit(ui, angle_deg);
                });
                let lines: Vec<(u32, String)> =
                    match self.ed.doc().feature(*sketch).map(|f| &f.kind) {
                        Some(FeatureKind::Sketch { sketch, .. }) => sketch
                            .curves
                            .iter()
                            .filter(|(_, c)| matches!(c.kind, CurveKind::Line { .. }))
                            .map(|(id, c)| {
                                (
                                    *id,
                                    format!(
                                        "Line {id}{}",
                                        if c.construction {
                                            " (construction)"
                                        } else {
                                            ""
                                        }
                                    ),
                                )
                            })
                            .collect(),
                        _ => vec![],
                    };
                let cur = match axis {
                    RevolveAxis::SketchX => "Sketch X axis".to_string(),
                    RevolveAxis::SketchY => "Sketch Y axis".to_string(),
                    RevolveAxis::Line { curve } => format!("Line {curve}"),
                };
                ui.horizontal(|ui| {
                    ui.label("Axis");
                    egui::ComboBox::from_id_salt(("axis", fid))
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(*axis == RevolveAxis::SketchX, "Sketch X axis")
                                .clicked()
                            {
                                *axis = RevolveAxis::SketchX;
                                changed = true;
                            }
                            if ui
                                .selectable_label(*axis == RevolveAxis::SketchY, "Sketch Y axis")
                                .clicked()
                            {
                                *axis = RevolveAxis::SketchY;
                                changed = true;
                            }
                            for (id, label) in &lines {
                                if ui
                                    .selectable_label(
                                        *axis == RevolveAxis::Line { curve: *id },
                                        label,
                                    )
                                    .clicked()
                                {
                                    *axis = RevolveAxis::Line { curve: *id };
                                    changed = true;
                                }
                            }
                        });
                });
                changed |= self.body_mode_ui(ui, body, fid);
            }
            FeatureKind::Primitive { primitive, body } => {
                match primitive {
                    Primitive::Box { min, size } => {
                        ui.label("Corner");
                        changed |= vec3_edit(ui, min, units);
                        ui.label("Size");
                        changed |= vec3_edit(ui, size, units);
                    }
                    Primitive::Cylinder {
                        base,
                        radius,
                        height,
                    } => {
                        ui.label("Base centre");
                        changed |= vec3_edit(ui, base, units);
                        ui.horizontal(|ui| {
                            ui.label("Radius");
                            changed |= length_edit(ui, radius, units);
                            ui.label("Height");
                            changed |= length_edit(ui, height, units);
                        });
                    }
                    Primitive::Sphere { center, radius } => {
                        ui.label("Centre");
                        changed |= vec3_edit(ui, center, units);
                        ui.horizontal(|ui| {
                            ui.label("Radius");
                            changed |= length_edit(ui, radius, units);
                        });
                    }
                }
                changed |= self.body_mode_ui(ui, body, fid);
            }
            FeatureKind::Boolean {
                op,
                target,
                tools,
                keep_tools,
            } => {
                ui.horizontal(|ui| {
                    for (o, n) in [
                        (BooleanOp::Union, "Union"),
                        (BooleanOp::Subtract, "Subtract"),
                        (BooleanOp::Intersect, "Intersect"),
                    ] {
                        if ui.selectable_label(*op == o, n).clicked() {
                            *op = o;
                            changed = true;
                        }
                    }
                });
                ui.label(format!("Target: {}", self.body_name(*target)));
                ui.label(format!(
                    "Tools: {}",
                    tools
                        .iter()
                        .map(|t| self.body_name(*t))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                changed |= ui.checkbox(keep_tools, "Keep tool bodies").changed();
            }
            FeatureKind::Transform {
                bodies,
                transform,
                copy,
            } => {
                ui.label(format!(
                    "Bodies: {}",
                    bodies
                        .iter()
                        .map(|t| self.body_name(*t))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                ui.label("Move");
                changed |= vec3_edit(ui, &mut transform.translate, units);
                ui.horizontal(|ui| {
                    ui.label("Rotate");
                    changed |= angle_edit(ui, &mut transform.rotate_deg);
                    ui.label("around");
                    changed |= dir_combo(ui, &format!("rot{fid}"), &mut transform.rotate_axis);
                });
                ui.label("Pivot");
                changed |= vec3_edit(ui, &mut transform.pivot, units);
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut transform.scale)
                                .speed(0.01)
                                .range(0.001..=1000.0),
                        )
                        .changed();
                });
                changed |= ui.checkbox(copy, "Create copies").changed();
                ui.weak("Tip: drag the coloured arrows in the viewport.");
            }
            FeatureKind::LinearPattern {
                direction,
                count,
                spacing,
                ..
            } => {
                ui.horizontal(|ui| {
                    ui.label("Direction");
                    changed |= dir_combo(ui, &format!("lp{fid}"), direction);
                });
                ui.horizontal(|ui| {
                    ui.label("Count");
                    changed |= ui.add(egui::DragValue::new(count).range(2..=200)).changed();
                    ui.label("Spacing");
                    changed |= length_edit(ui, spacing, units);
                });
            }
            FeatureKind::CircularPattern {
                axis_origin,
                axis_dir,
                count,
                total_angle_deg,
                ..
            } => {
                ui.horizontal(|ui| {
                    ui.label("Axis");
                    changed |= dir_combo(ui, &format!("cp{fid}"), axis_dir);
                });
                ui.label("Axis origin");
                changed |= vec3_edit(ui, axis_origin, units);
                ui.horizontal(|ui| {
                    ui.label("Count");
                    changed |= ui.add(egui::DragValue::new(count).range(2..=360)).changed();
                    ui.label("Angle");
                    changed |= angle_edit(ui, total_angle_deg);
                });
            }
            FeatureKind::Mirror { plane, .. } => {
                ui.horizontal(|ui| {
                    ui.label("Plane");
                    changed |= plane_combo(ui, &format!("mp{fid}"), plane);
                });
            }
            FeatureKind::ImportedMesh { resource } | FeatureKind::ImportedSolid { resource } => {
                ui.label(format!("Stored in the project as '{resource}'"));
            }
        }
        if changed {
            let label = format!("Edit {}", f.kind.type_name());
            let old_distance = match &f.kind {
                FeatureKind::Extrude { distance, .. } => *distance,
                _ => 0.0,
            };
            self.ed.apply_coalesced(&label, fid, |d| {
                d.feature_mut(fid).unwrap().kind = k;
                d.auto_extrude_mode(fid, old_distance);
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            let mut sup = f.suppressed;
            if ui.checkbox(&mut sup, "Suppressed").changed() {
                self.ed
                    .apply("Suppress", |d| d.feature_mut(fid).unwrap().suppressed = sup);
            }
            if ui.button("Delete").clicked() {
                self.delete_selection();
            }
            if ui.button("Done").clicked() {
                self.sel.feature = None;
            }
        });
        let deps = self.ed.doc().dependents(fid);
        if !deps.is_empty() {
            ui.weak(format!(
                "{} later feature(s) depend on this and update automatically.",
                deps.len()
            ));
        }
    }

    // ------------------------------------------------------------------ sketch inspector

    fn sketch_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(s) = &self.sketch else { return };
        let fid = s.feature;
        let Some(FeatureKind::Sketch { sketch, .. }) =
            self.ed.doc().feature(fid).map(|f| f.kind.clone())
        else {
            self.sketch = None;
            return;
        };
        let units = self.ed.doc().units;
        ui.label(
            RichText::new(format!(
                "Editing {}",
                self.ed
                    .doc()
                    .feature(fid)
                    .map(|f| f.name.clone())
                    .unwrap_or_default()
            ))
            .strong(),
        );
        if let Some(st) = self.ed.model().sketch_status.get(&fid) {
            ui.label(status_text(st));
        }
        let regions = self
            .ed
            .model()
            .profiles
            .get(&fid)
            .map(|p| p.len())
            .unwrap_or(0);
        ui.weak(format!("{} closed region(s)", regions));
        if let Some(s) = &mut self.sketch {
            ui.checkbox(&mut s.grid_snap, "Snap to grid");
            if self.tool == Tool::Polygon {
                ui.horizontal(|ui| {
                    ui.label("Sides");
                    ui.add(egui::DragValue::new(&mut s.polygon_sides).range(3..=64));
                });
            }
        }
        ui.separator();

        // ---- selection & constraint buttons
        let pts = self.sel.sk_points.clone();
        let crv = self.sel.sk_curves.clone();
        let kinds: Vec<CurveKind> = crv
            .iter()
            .filter_map(|c| sketch.curves.get(c).map(|c| c.kind))
            .collect();
        let lines: Vec<u32> = crv
            .iter()
            .copied()
            .filter(|c| {
                matches!(
                    sketch.curves.get(c).map(|c| c.kind),
                    Some(CurveKind::Line { .. })
                )
            })
            .collect();
        let circles: Vec<u32> = crv
            .iter()
            .copied()
            .filter(|c| {
                matches!(
                    sketch.curves.get(c).map(|c| c.kind),
                    Some(CurveKind::Circle { .. } | CurveKind::Arc { .. })
                )
            })
            .collect();
        if pts.is_empty() && crv.is_empty() {
            ui.weak(
                "Select tool: click points or curves to add constraints. Drag points to move them.",
            );
        } else {
            ui.label(format!(
                "Selected: {} point(s), {} curve(s)",
                pts.len(),
                crv.len()
            ));
        }
        let mut add: Vec<Constraint> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            if lines.len() == 1 && pts.is_empty() && circles.is_empty() {
                let l = lines[0];
                if ui.button("Horizontal").clicked() {
                    add.push(Constraint::Horizontal { line: l });
                }
                if ui.button("Vertical").clicked() {
                    add.push(Constraint::Vertical { line: l });
                }
                if ui.button("Length").clicked() {
                    let (a, b) = sketch.line_points(l).unwrap();
                    let (pa, pb) = (sketch.points[&a], sketch.points[&b]);
                    add.push(Constraint::Length {
                        line: l,
                        value: ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt(),
                    });
                }
            }
            if lines.len() == 2 && circles.is_empty() {
                let (a, b) = (lines[0], lines[1]);
                if ui.button("Parallel").clicked() {
                    add.push(Constraint::Parallel { a, b });
                }
                if ui.button("Perpendicular").clicked() {
                    add.push(Constraint::Perpendicular { a, b });
                }
                if ui.button("Equal").clicked() {
                    add.push(Constraint::Equal { a, b });
                }
                if ui.button("Angle").clicked() {
                    let d = |l: u32| {
                        let (p, q) = sketch.line_points(l).unwrap();
                        let (p, q) = (sketch.points[&p], sketch.points[&q]);
                        (q.y - p.y).atan2(q.x - p.x)
                    };
                    let mut ang = (d(b) - d(a)).to_degrees();
                    while ang > 180.0 {
                        ang -= 360.0;
                    }
                    while ang < -180.0 {
                        ang += 360.0;
                    }
                    add.push(Constraint::Angle { a, b, degrees: ang });
                }
            }
            if lines.len() == 1 && circles.len() == 1 && ui.button("Tangent").clicked() {
                add.push(Constraint::Tangent {
                    a: lines[0],
                    b: circles[0],
                });
            }
            if circles.len() == 2 && lines.is_empty() {
                if ui.button("Equal").clicked() {
                    add.push(Constraint::Equal {
                        a: circles[0],
                        b: circles[1],
                    });
                }
                if ui.button("Concentric").clicked() {
                    add.push(Constraint::Concentric {
                        a: circles[0],
                        b: circles[1],
                    });
                }
                if ui.button("Tangent").clicked() {
                    add.push(Constraint::Tangent {
                        a: circles[0],
                        b: circles[1],
                    });
                }
            }
            if circles.len() == 1 && lines.is_empty() && pts.is_empty() {
                let r = sketch.radius_of(circles[0]).unwrap_or(1.0);
                if ui.button("Radius").clicked() {
                    add.push(Constraint::Radius {
                        curve: circles[0],
                        value: r,
                    });
                }
                if ui.button("Diameter").clicked() {
                    add.push(Constraint::Diameter {
                        curve: circles[0],
                        value: 2.0 * r,
                    });
                }
            }
            if pts.len() == 2 && crv.is_empty() {
                let (a, b) = (pts[0], pts[1]);
                let (pa, pb) = (sketch.points[&a], sketch.points[&b]);
                if ui.button("Coincident").clicked() {
                    add.push(Constraint::Coincident { a, b });
                }
                if ui.button("Distance").clicked() {
                    add.push(Constraint::Distance {
                        a,
                        b,
                        value: ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt(),
                    });
                }
                if ui.button("Horizontal dist.").clicked() {
                    add.push(Constraint::HorizontalDistance {
                        a,
                        b,
                        value: (pb.x - pa.x).abs(),
                    });
                }
                if ui.button("Vertical dist.").clicked() {
                    add.push(Constraint::VerticalDistance {
                        a,
                        b,
                        value: (pb.y - pa.y).abs(),
                    });
                }
            }
            if pts.len() == 2 && lines.len() == 1 && ui.button("Symmetric").clicked() {
                add.push(Constraint::Symmetric {
                    a: pts[0],
                    b: pts[1],
                    axis: lines[0],
                });
            }
            if pts.len() == 1 && crv.is_empty() {
                let p = sketch.points[&pts[0]];
                if ui.button("Fix").clicked() {
                    add.push(Constraint::Fixed {
                        point: pts[0],
                        x: p.x,
                        y: p.y,
                    });
                }
            }
            if pts.len() == 1 && crv.len() == 1 {
                if ui.button("On curve").clicked() {
                    add.push(Constraint::PointOnCurve {
                        point: pts[0],
                        curve: crv[0],
                    });
                }
                if lines.len() == 1 && ui.button("Midpoint").clicked() {
                    add.push(Constraint::Midpoint {
                        point: pts[0],
                        line: lines[0],
                    });
                }
            }
        });
        for c in add {
            self.add_sketch_constraint(fid, c);
        }
        if !crv.is_empty() {
            ui.horizontal(|ui| {
                let all_cons = kinds
                    .iter()
                    .zip(&crv)
                    .all(|(_, c)| sketch.curves[c].construction);
                if ui
                    .button(if all_cons {
                        "Make regular"
                    } else {
                        "Make construction"
                    })
                    .clicked()
                {
                    let crv = crv.clone();
                    self.ed.apply("Toggle construction", |d| {
                        if let Some((_, sk)) = d.sketch_mut(fid) {
                            for c in &crv {
                                if let Some(cv) = sk.curves.get_mut(c) {
                                    cv.construction = !all_cons;
                                }
                            }
                        }
                    });
                }
                if ui.button("Delete").clicked() {
                    self.delete_selection();
                }
            });
        }
        ui.separator();
        // ---- constraints & dimensions list
        ui.label(RichText::new("Constraints & dimensions").strong());
        let mut remove = None;
        let mut edit: Option<(u32, f64)> = None;
        for (id, c) in &sketch.constraints {
            ui.horizontal(|ui| {
                let sel = self.sel.sk_constraint == Some(*id);
                if ui.selectable_label(sel, c.label()).clicked() {
                    self.sel.sk_constraint = if sel { None } else { Some(*id) };
                }
                let mut c2 = c.clone();
                if let Some(v) = c2.value_mut() {
                    let changed = if matches!(c, Constraint::Angle { .. }) {
                        angle_edit(ui, v)
                    } else {
                        length_edit(ui, v, units)
                    };
                    if changed {
                        edit = Some((*id, *v));
                    }
                }
                if ui.small_button("x").on_hover_text("Remove").clicked() {
                    remove = Some(*id);
                }
            });
        }
        if sketch.constraints.is_empty() {
            ui.weak("None yet. Horizontal/vertical are added automatically while drawing.");
        }
        if let Some(id) = remove {
            self.ed.apply("Remove constraint", |d| {
                if let Some((_, sk)) = d.sketch_mut(fid) {
                    sk.remove_constraint(id);
                    let _ = calibro_core::sketch::solve(sk);
                }
            });
        }
        if let Some((id, v)) = edit {
            self.set_dimension(fid, id, v);
        }
    }

    /// Adds a constraint unless it conflicts with the existing ones.
    pub fn add_sketch_constraint(&mut self, fid: FeatureId, c: Constraint) {
        let Some(FeatureKind::Sketch { sketch, .. }) =
            self.ed.doc().feature(fid).map(|f| f.kind.clone())
        else {
            return;
        };
        let mut trial = sketch.clone();
        trial.add_constraint(c.clone());
        match calibro_core::sketch::solve(&mut trial) {
            Ok(r) if matches!(r.status, SolveStatus::Conflicting { .. }) => {
                self.error(format!(
                    "{} conflicts with existing constraints — not added",
                    c.label()
                ));
            }
            Ok(_) => {
                self.ed.apply(&format!("Add {}", c.label()), |d| {
                    if let Some((_, sk)) = d.sketch_mut(fid) {
                        *sk = trial;
                    }
                });
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Changes a dimension value and re-solves (one undo step per drag).
    pub fn set_dimension(&mut self, fid: FeatureId, cid: u32, value: f64) {
        let Some(FeatureKind::Sketch { sketch, .. }) =
            self.ed.doc().feature(fid).map(|f| f.kind.clone())
        else {
            return;
        };
        let mut trial = sketch.clone();
        if let Some(v) = trial.constraints.get_mut(&cid).and_then(|c| c.value_mut()) {
            *v = value;
        }
        match calibro_core::sketch::solve(&mut trial) {
            Ok(r) if matches!(r.status, SolveStatus::Conflicting { .. }) => {
                self.error("That value can't be satisfied with the other constraints");
            }
            _ => {
                self.ed
                    .apply_coalesced("Edit dimension", cid as u64 + (fid << 32), |d| {
                        if let Some((_, sk)) = d.sketch_mut(fid) {
                            *sk = trial;
                        }
                    });
            }
        }
    }
}

pub fn status_text(st: &SolveStatus) -> RichText {
    match st {
        SolveStatus::FullyConstrained => RichText::new("Fully defined").color(OK_GREEN),
        SolveStatus::UnderConstrained { dof } => {
            RichText::new(format!("Under-defined: {dof} degrees of freedom")).color(UNDER_BLUE)
        }
        SolveStatus::Redundant { dof } => {
            RichText::new(format!("Redundant constraints ({dof} DOF left)")).color(ACCENT)
        }
        SolveStatus::Conflicting { .. } => {
            RichText::new("Conflicting constraints - remove one").color(ERR_RED)
        }
    }
}
