//! The 3D viewport: navigation, picking, sketch drawing tools, drag handles,
//! overlays, and assembling each rendered frame.

use crate::app::*;
use crate::camera::{self, CameraView};
use crate::panels::{ACCENT, ERR_RED, OK_GREEN, UNDER_BLUE};
use crate::picking;
use crate::renderer::{BodyDraw, FrameInput, Globals, LineVertex, ObjectParams};
use calibro_core::kernel::Plane;
use calibro_core::sketch::{Constraint, CurveKind, PointId, Sketch, SolveStatus};
use calibro_core::*;
use egui::{Align2, Color32, FontId, Pos2, Stroke};
use glam::{DVec3, Vec2};

const SNAP_PX: f32 = 10.0;

fn nice_step(x: f64) -> f64 {
    let p = 10f64.powf(x.max(1e-9).log10().floor());
    let m = x / p;
    p * if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    }
}

fn to_pos(rect: egui::Rect, v: Vec2) -> Pos2 {
    rect.min + egui::vec2(v.x, v.y)
}

fn c32(c: Color32) -> [f32; 4] {
    let r = c.to_srgba_unmultiplied();
    [
        r[0] as f32 / 255.0,
        r[1] as f32 / 255.0,
        r[2] as f32 / 255.0,
        r[3] as f32 / 255.0,
    ]
}

fn label(painter: &egui::Painter, pos: Pos2, text: &str, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(12.0), color);
    let r = Align2::CENTER_CENTER
        .anchor_size(pos, galley.size())
        .expand2(egui::vec2(4.0, 1.5));
    painter.rect_filled(r, 3.0, Color32::from_white_alpha(225));
    painter.galley(r.min + egui::vec2(4.0, 1.5), galley, color);
}

fn arc_points(center: [f64; 2], r: f64, a0: f64, sweep: f64, n: usize) -> Vec<[f64; 2]> {
    (0..=n)
        .map(|i| {
            let t = a0 + sweep * i as f64 / n as f64;
            [center[0] + r * t.cos(), center[1] + r * t.sin()]
        })
        .collect()
}

/// Polylines (in sketch coordinates) for every curve: (curve id, construction, points).
fn sketch_polylines(sk: &Sketch) -> Vec<(u32, bool, Vec<[f64; 2]>)> {
    let mut out = Vec::new();
    for (id, c) in &sk.curves {
        let pts = match c.kind {
            CurveKind::Line { a, b } => vec![sk.points[&a].xy(), sk.points[&b].xy()],
            CurveKind::Circle { center, radius } => arc_points(
                sk.points[&center].xy(),
                radius,
                0.0,
                std::f64::consts::TAU,
                72,
            ),
            CurveKind::Arc { center, start, end } => {
                let c0 = sk.points[&center].xy();
                let s = sk.points[&start].xy();
                let e = sk.points[&end].xy();
                let (a0, sweep) = calibro_core::sketch::profile::arc_angles(c0, s, e, true);
                let r = ((s[0] - c0[0]).powi(2) + (s[1] - c0[1]).powi(2)).sqrt();
                arc_points(c0, r, a0, sweep, 48)
            }
        };
        out.push((*id, c.construction, pts));
    }
    out
}

impl CalibroApp {
    pub fn viewport_panel(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                self.viewport_rect = rect;
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                let size = Vec2::new(rect.width(), rect.height());
                self.navigate(ui, &resp, size);
                let view = CameraView::new(&self.ed.doc().camera, size);
                let pointer = resp
                    .hover_pos()
                    .map(|p| Vec2::new(p.x - rect.min.x, p.y - rect.min.y));

                // interaction (may change the document)
                let handle_active = self.handles(ui, &resp, &view, pointer);
                if !handle_active {
                    if self.sketch.is_some() {
                        self.sketch_input(ui, &resp, &view, pointer);
                    } else {
                        self.select_input(ui, &resp, &view, pointer);
                    }
                }
                // re-create the view in case the camera changed during interaction
                let view = CameraView::new(&self.ed.doc().camera, size);

                // render
                let ppp = ui.ctx().pixels_per_point();
                let px = [
                    (rect.width() * ppp).round() as u32,
                    (rect.height() * ppp).round() as u32,
                ];
                if let (Some(rs), true) = (frame.wgpu_render_state(), self.renderer.is_some()) {
                    let input = self.build_frame(&view, rect);
                    let renderer = self.renderer.as_mut().unwrap();
                    let recreated = renderer.ensure_targets(&rs.device, px);
                    renderer.render(&rs.device, &rs.queue, &input);
                    let t = renderer.targets.as_ref().unwrap();
                    let mut egui_r = rs.renderer.write();
                    match (renderer.texture_id, recreated) {
                        (None, _) => {
                            renderer.texture_id = Some(egui_r.register_native_texture(
                                &rs.device,
                                &t.resolve,
                                egui_wgpu::wgpu::FilterMode::Linear,
                            ));
                        }
                        (Some(id), true) => egui_r.update_egui_texture_from_wgpu_texture(
                            &rs.device,
                            &t.resolve,
                            egui_wgpu::wgpu::FilterMode::Linear,
                            id,
                        ),
                        _ => {}
                    }
                    drop(egui_r);
                    if let Some(id) = renderer.texture_id {
                        ui.painter().image(
                            id,
                            rect,
                            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                } else {
                    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(40));
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "3D graphics (wgpu) are not available on this device",
                        FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                }

                let painter = ui.painter_at(rect);
                self.draw_overlays(&painter, rect, &view, pointer);
                self.viewport_buttons(ui, rect);
                self.view = Some(view);
            });
    }

    // ------------------------------------------------------------------ navigation

    fn navigate(&mut self, ui: &egui::Ui, resp: &egui::Response, size: Vec2) {
        let view = CameraView::new(&self.ed.doc().camera, size);
        let (scroll, zoom_delta, shift, multi, touches) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.modifiers.shift,
                i.multi_touch(),
                i.any_touches(),
            )
        });
        let mut cam = self.ed.doc().camera.clone();
        let cursor = resp
            .hover_pos()
            .map(|p| Vec2::new(p.x - resp.rect.min.x, p.y - resp.rect.min.y));
        if resp.hovered() {
            if scroll.y != 0.0 && !ui.input(|i| i.modifiers.command) {
                camera::zoom(&mut cam, &view, (-scroll.y as f64 * 0.0015).exp(), cursor);
            }
            if (zoom_delta - 1.0).abs() > 1e-4 {
                camera::zoom(&mut cam, &view, 1.0 / zoom_delta as f64, cursor);
            }
        }
        if let Some(mt) = multi {
            if resp.hovered() || resp.dragged() {
                camera::pan(
                    &mut cam,
                    &view,
                    mt.translation_delta.x,
                    mt.translation_delta.y,
                );
            }
        }
        let d = resp.drag_delta();
        if resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Secondary) && shift)
        {
            camera::pan(&mut cam, &view, d.x, d.y);
        } else if resp.dragged_by(egui::PointerButton::Secondary) {
            camera::orbit(&mut cam, d.x, d.y);
        } else if touches
            && multi.is_none()
            && resp.dragged_by(egui::PointerButton::Primary)
            && self.sketch.is_none()
            && self.handle.is_none()
        {
            // one-finger drag on touch screens orbits
            camera::orbit(&mut cam, d.x, d.y);
        }
        if cam != self.ed.doc().camera {
            self.ed.apply_silent(|doc| doc.camera = cam);
        }
    }

    // ------------------------------------------------------------------ selection & measure

    fn visible_meshes(&self) -> Vec<(BodyId, std::sync::Arc<calibro_kernel::BodyMesh>)> {
        self.ed
            .model()
            .bodies
            .values()
            .filter(|b| {
                self.ed
                    .doc()
                    .body_props(b.id)
                    .map(|p| p.visible)
                    .unwrap_or(true)
            })
            .map(|b| (b.id, b.mesh.clone()))
            .collect()
    }

    fn clip_plane(&self) -> Option<(DVec3, f64)> {
        if !self.section.enabled {
            return None;
        }
        let mut n = DVec3::ZERO;
        n[self.section.axis] = 1.0;
        let mut d = self.section.offset;
        if self.section.flip {
            n = -n;
            d = -d;
        }
        Some((n, d))
    }

    /// Visible sketch region under the pointer: (sketch feature, profile index, distance along ray).
    fn region_under(&self, ray: &camera::Ray) -> Option<(FeatureId, usize, f64)> {
        let mut best: Option<(FeatureId, usize, f64)> = None;
        for f in &self.ed.doc().features {
            let FeatureKind::Sketch { plane, .. } = &f.kind else {
                continue;
            };
            if f.hidden || self.ed.model().status.get(&f.id) == Some(&FeatureStatus::RolledBack) {
                continue;
            }
            let Some(profiles) = self.ed.model().profiles.get(&f.id) else {
                continue;
            };
            let Some(hit) =
                picking::ray_plane(ray, DVec3::from(plane.origin), DVec3::from(plane.normal()))
            else {
                continue;
            };
            let local = plane.to_local(hit.to_array());
            let t = (hit - ray.origin).length();
            let found = profiles
                .iter()
                .enumerate()
                .filter(|(_, p)| p.contains(local))
                .min_by(|a, b| a.1.area().total_cmp(&b.1.area()));
            if let Some((i, _)) = found {
                if best.map(|b| t < b.2).unwrap_or(true) {
                    best = Some((f.id, i, t));
                }
            }
        }
        best
    }

    fn select_input(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        view: &CameraView,
        pointer: Option<Vec2>,
    ) {
        let meshes = self.visible_meshes();
        let clip = self.clip_plane();
        self.hover_face = None;
        self.hover_region = None;
        let Some(p) = pointer else { return };
        let ray = view.ray(p);
        let face = picking::pick_face(&ray, meshes.iter().map(|(id, m)| (*id, &**m)), clip);
        let region = self.region_under(&ray);
        let region_wins = match (region, face) {
            (Some(r), Some(f)) => r.2 <= f.t + 1e-3,
            (Some(_), None) => true,
            _ => false,
        };
        if region_wins {
            self.hover_region = region.map(|r| (r.0, r.1));
        } else if let Some(f) = face {
            self.hover_face = Some((f.body, f.face));
        }
        let edge_mode = matches!(
            self.display,
            DisplayMode::ShadedEdges | DisplayMode::Wireframe | DisplayMode::XRay
        );

        if self.tool == Tool::Measure {
            if resp.clicked_by(egui::PointerButton::Primary) {
                let edge =
                    picking::pick_edge(view, p, 8.0, meshes.iter().map(|(id, m)| (*id, &**m)));
                let mut point = edge.map(|e| e.point).or(face.map(|f| f.point));
                // snap to edge endpoints (vertices)
                if let Some(e) = edge {
                    let pl = &self.ed.model().bodies[&e.body].mesh.edges[e.edge];
                    for v in [pl.first(), pl.last()].into_iter().flatten() {
                        let w = DVec3::from(v.map(|x| x as f64));
                        if view
                            .project(w)
                            .map(|s| (s - p).length() < SNAP_PX)
                            .unwrap_or(false)
                        {
                            point = Some(w);
                        }
                    }
                }
                if let Some(pt) = point {
                    if self.measure.a.is_none() || self.measure.b.is_some() {
                        self.measure = Measure {
                            a: Some(pt),
                            b: None,
                        };
                    } else {
                        self.measure.b = Some(pt);
                    }
                }
            }
            return;
        }

        if !resp.clicked_by(egui::PointerButton::Primary) {
            return;
        }
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        let now = self.time;
        let double = now - self.last_click.0 < 0.35 && (self.last_click.1 - p).length() < 6.0;
        self.last_click = (now, p);

        // choosing a sketch plane by clicking a flat face
        if self.picking_plane {
            if let Some(f) = face {
                let mesh = &self.ed.model().bodies[&f.body].mesh;
                if let Some(Some(pl)) = mesh.face_planes.get(f.face as usize) {
                    // keep sketch coordinates aligned with the world origin
                    let n = DVec3::from(pl.normal());
                    let o = n * n.dot(DVec3::from(pl.origin));
                    let plane = Plane::from_origin_normal(o.to_array(), n.to_array());
                    self.start_sketch(plane, Some(f.body));
                } else {
                    self.error("That face is curved — pick a flat face or a standard plane");
                }
            }
            return;
        }

        if edge_mode && !region_wins {
            if let Some(e) =
                picking::pick_edge(view, p, 6.0, meshes.iter().map(|(id, m)| (*id, &**m)))
            {
                if face
                    .map(|f| f.body == e.body || (view.eye - e.point).length() <= f.t + 1e-3)
                    .unwrap_or(true)
                {
                    if !add {
                        self.sel = Selection::default();
                    }
                    self.sel.edge = Some((e.body, e.edge));
                    if !self.sel.bodies.contains(&e.body) {
                        self.sel.bodies.push(e.body);
                    }
                    return;
                }
            }
        }
        if region_wins {
            let (fid, idx, _) = region.unwrap();
            let seed = self.ed.model().profiles[&fid][idx].interior_point();
            if !add {
                self.sel = Selection::default();
            }
            if let Some(pos) = self
                .sel
                .regions
                .iter()
                .position(|r| r.0 == fid && self.region_index(r.0, r.1) == Some(idx))
            {
                self.sel.regions.remove(pos);
            } else {
                self.sel.regions.push((fid, seed));
            }
            return;
        }
        match face {
            Some(f) => {
                if !add {
                    self.sel = Selection::default();
                }
                if double {
                    // double-click selects the whole body
                    self.sel.face = None;
                    if !self.sel.bodies.contains(&f.body) {
                        self.sel.bodies.push(f.body);
                    }
                } else if add && self.sel.bodies.contains(&f.body) {
                    self.sel.bodies.retain(|b| *b != f.body);
                } else {
                    self.sel.face = Some((f.body, f.face));
                    if !self.sel.bodies.contains(&f.body) {
                        self.sel.bodies.push(f.body);
                    }
                }
            }
            None => {
                if !add {
                    self.sel = Selection::default();
                }
            }
        }
    }

    fn region_index(&self, fid: FeatureId, seed: [f64; 2]) -> Option<usize> {
        let profiles = self.ed.model().profiles.get(&fid)?;
        profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.contains(seed))
            .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
            .map(|(i, _)| i)
    }

    // ------------------------------------------------------------------ sketch tools

    fn active_sketch(&self) -> Option<(FeatureId, Plane, Sketch)> {
        let fid = self.sketch.as_ref()?.feature;
        match self.ed.doc().feature(fid).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { plane, sketch, .. }) => Some((fid, *plane, sketch.clone())),
            _ => None,
        }
    }

    fn sketch_input(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        view: &CameraView,
        pointer: Option<Vec2>,
    ) {
        let Some((fid, plane, sk)) = self.active_sketch() else {
            self.sketch = None;
            return;
        };
        let Some(p) = pointer else {
            if let Some(s) = &mut self.sketch {
                s.cursor = None;
            }
            return;
        };
        let ray = view.ray(p);
        let Some(hit) =
            picking::ray_plane(&ray, DVec3::from(plane.origin), DVec3::from(plane.normal()))
        else {
            return;
        };
        let mut pos = plane.to_local(hit.to_array());
        let screen = |q: [f64; 2]| view.project(DVec3::from(plane.to_world(q)));

        // snapping to existing points, then the origin, then the grid
        let mut snapped: Option<PointId> = None;
        let mut best = SNAP_PX;
        for (id, pt) in &sk.points {
            if let Some(s) = screen(pt.xy()) {
                let d = (s - p).length();
                if d < best {
                    best = d;
                    snapped = Some(*id);
                }
            }
        }
        let tool = self.tool;
        let s = self.sketch.as_mut().unwrap();
        if let Some(id) = snapped {
            pos = sk.points[&id].xy();
        } else if screen([0.0, 0.0])
            .map(|o| (o - p).length() < SNAP_PX)
            .unwrap_or(false)
        {
            pos = [0.0, 0.0];
        } else if s.grid_snap && tool.is_sketch_tool() {
            let step = nice_step(view.pixel_size_at(hit) * 12.0);
            pos = [
                (pos[0] / step).round() * step,
                (pos[1] / step).round() * step,
            ];
        }
        // horizontal / vertical inference for lines
        let mut infer_h = false;
        let mut infer_v = false;
        if tool == Tool::Line && snapped.is_none() {
            if let Some((last, _)) = s.clicks.last() {
                let (dx, dy) = (pos[0] - last[0], pos[1] - last[1]);
                let tol = 0.06;
                if dx.abs() > 1e-9 && (dy / dx).abs() < tol {
                    pos[1] = last[1];
                    infer_h = true;
                } else if dy.abs() > 1e-9 && (dx / dy).abs() < tol {
                    pos[0] = last[0];
                    infer_v = true;
                }
            }
        }
        s.cursor = Some(pos);
        s.snapped = snapped;

        if resp.clicked_by(egui::PointerButton::Secondary) {
            if s.clicks.is_empty() {
                self.tool = Tool::Select;
            }
            s.clicks.clear();
            s.chain_start = None;
            return;
        }

        if tool == Tool::Select {
            self.sketch_select(ui, resp, view, &plane, &sk, p, pos, snapped, fid);
            return;
        }
        if !resp.clicked_by(egui::PointerButton::Primary) {
            return;
        }
        let click = (pos, snapped);
        let sides = s.polygon_sides;
        match tool {
            Tool::Line => {
                if let Some((last_pos, last_id)) = s.clicks.last().copied() {
                    if (last_pos[0] - pos[0]).hypot(last_pos[1] - pos[1]) < 1e-9 {
                        return;
                    }
                    let closes = snapped.is_some() && snapped == s.chain_start;
                    let new_b = self.ed.apply("Draw line", |d| {
                        let (_, sk) = d.sketch_mut(fid).unwrap();
                        let a = last_id.unwrap_or_else(|| sk.add_point(last_pos[0], last_pos[1]));
                        let b = snapped.unwrap_or_else(|| sk.add_point(pos[0], pos[1]));
                        let l = sk.add_line(a, b);
                        if infer_h {
                            sk.add_constraint(Constraint::Horizontal { line: l });
                        } else if infer_v {
                            sk.add_constraint(Constraint::Vertical { line: l });
                        }
                        let _ = calibro_core::sketch::solve(sk);
                        (a, b)
                    });
                    let s = self.sketch.as_mut().unwrap();
                    if s.chain_start.is_none() {
                        s.chain_start = Some(new_b.0);
                    }
                    if closes {
                        s.clicks.clear();
                        s.chain_start = None;
                    } else {
                        s.clicks = vec![(pos, Some(new_b.1))];
                    }
                } else {
                    s.clicks.push(click);
                    s.chain_start = snapped;
                }
            }
            Tool::Rectangle | Tool::Circle | Tool::Polygon => {
                if s.clicks.is_empty() {
                    s.clicks.push(click);
                    return;
                }
                let (c0, c0_id) = s.clicks[0];
                s.clicks.clear();
                let r = (pos[0] - c0[0]).hypot(pos[1] - c0[1]);
                if r < 1e-9 {
                    return;
                }
                let label = match tool {
                    Tool::Rectangle => "Draw rectangle",
                    Tool::Circle => "Draw circle",
                    _ => "Draw polygon",
                };
                self.ed.apply(label, |d| {
                    let (_, sk) = d.sketch_mut(fid).unwrap();
                    match tool {
                        Tool::Rectangle => {
                            sk.add_rectangle(c0, pos);
                        }
                        Tool::Circle => {
                            let c = sk.add_circle(c0, r);
                            if let Some(existing) = c0_id {
                                // reuse the snapped point as the centre
                                if let Some(CurveKind::Circle { center, .. }) =
                                    sk.curves.get(&c).map(|c| c.kind)
                                {
                                    if let Some(cv) = sk.curves.get_mut(&c) {
                                        cv.kind = CurveKind::Circle {
                                            center: existing,
                                            radius: r,
                                        };
                                    }
                                    sk.points.remove(&center);
                                }
                            }
                        }
                        _ => {
                            sk.add_polygon(c0, r, sides as usize);
                        }
                    }
                    let _ = calibro_core::sketch::solve(sk);
                });
            }
            Tool::Arc => {
                s.clicks.push(click);
                if s.clicks.len() < 3 {
                    return;
                }
                let [(c, cid), (st, sid), (en, eid)] = [s.clicks[0], s.clicks[1], s.clicks[2]];
                s.clicks.clear();
                let r = (st[0] - c[0]).hypot(st[1] - c[1]);
                let ang = (en[1] - c[1]).atan2(en[0] - c[0]);
                let en = if eid.is_some() {
                    en
                } else {
                    [c[0] + r * ang.cos(), c[1] + r * ang.sin()]
                };
                if r < 1e-9 {
                    return;
                }
                self.ed.apply("Draw arc", |d| {
                    let (_, sk) = d.sketch_mut(fid).unwrap();
                    let cp = cid.unwrap_or_else(|| sk.add_point(c[0], c[1]));
                    let sp = sid.unwrap_or_else(|| sk.add_point(st[0], st[1]));
                    let ep = eid.unwrap_or_else(|| sk.add_point(en[0], en[1]));
                    sk.add_arc(cp, sp, ep);
                    let _ = calibro_core::sketch::solve(sk);
                });
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sketch_select(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        view: &CameraView,
        plane: &Plane,
        sk: &Sketch,
        p: Vec2,
        pos: [f64; 2],
        snapped: Option<PointId>,
        fid: FeatureId,
    ) {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        // dragging points: egui reports drag-start after some movement, so pick the
        // point nearest to where the press began.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .map(|q| Vec2::new(q.x - resp.rect.min.x, q.y - resp.rect.min.y))
                .unwrap_or(p);
            let mut best: Option<(PointId, f32)> = None;
            for (id, pt) in &sk.points {
                if let Some(sp) = view.project(DVec3::from(plane.to_world(pt.xy()))) {
                    let d = (sp - origin).length();
                    if d < SNAP_PX && best.map(|b| d < b.1).unwrap_or(true) {
                        best = Some((*id, d));
                    }
                }
            }
            if let Some((id, _)) = best.or(snapped.map(|s| (s, 0.0))) {
                self.sketch.as_mut().unwrap().dragging = Some(id);
            }
        }
        let dragging = self.sketch.as_ref().and_then(|s| s.dragging);
        if let Some(id) = dragging {
            if resp.dragged_by(egui::PointerButton::Primary) {
                // free position (no snapping) while dragging
                let ray = view.ray(p);
                if let Some(hit) =
                    picking::ray_plane(&ray, DVec3::from(plane.origin), DVec3::from(plane.normal()))
                {
                    let target = plane.to_local(hit.to_array());
                    self.ed.apply_coalesced("Move point", id as u64, |d| {
                        if let Some((_, s)) = d.sketch_mut(fid) {
                            let _ = calibro_core::sketch::drag_point(s, id, target);
                        }
                    });
                }
            }
            if resp.drag_stopped() {
                self.sketch.as_mut().unwrap().dragging = None;
            }
            return;
        }
        if !resp.clicked_by(egui::PointerButton::Primary) {
            return;
        }
        let _ = pos;
        if !add {
            self.sel.clear_sketch();
        }
        if let Some(id) = snapped {
            if let Some(i) = self.sel.sk_points.iter().position(|x| *x == id) {
                self.sel.sk_points.remove(i);
            } else {
                self.sel.sk_points.push(id);
            }
            return;
        }
        // nearest curve on screen
        let mut best: Option<(u32, f32)> = None;
        for (id, _, pts) in sketch_polylines(sk) {
            for w in pts.windows(2) {
                let (Some(a), Some(b)) = (
                    view.project(DVec3::from(plane.to_world(w[0]))),
                    view.project(DVec3::from(plane.to_world(w[1]))),
                ) else {
                    continue;
                };
                let ab = b - a;
                let t = if ab.length_squared() > 0.0 {
                    ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = (a + ab * t - p).length();
                if d < 8.0 && best.map(|b| d < b.1).unwrap_or(true) {
                    best = Some((id, d));
                }
            }
        }
        if let Some((id, _)) = best {
            if let Some(i) = self.sel.sk_curves.iter().position(|x| *x == id) {
                self.sel.sk_curves.remove(i);
            } else {
                self.sel.sk_curves.push(id);
            }
        }
    }

    // ------------------------------------------------------------------ drag handles

    /// Extrude distance arrow, and move arrows for the transform being edited.
    fn handle_specs(&self, view: &CameraView) -> Vec<(HandleKind, DVec3, DVec3, f64, Color32)> {
        let mut out = Vec::new();
        if self.sketch.is_some() {
            return out;
        }
        let Some(fid) = self.sel.feature else {
            return out;
        };
        let Some(f) = self.ed.doc().feature(fid) else {
            return out;
        };
        match &f.kind {
            FeatureKind::Extrude {
                sketch,
                profiles,
                distance,
                symmetric,
                body,
            } => {
                let Some(FeatureKind::Sketch { plane, .. }) =
                    self.ed.doc().feature(*sketch).map(|f| &f.kind)
                else {
                    return out;
                };
                let seed = match profiles {
                    ProfileSelection::Seeds { points } if !points.is_empty() => points[0],
                    _ => self
                        .ed
                        .model()
                        .profiles
                        .get(sketch)
                        .and_then(|p| p.first())
                        .map(|p| p.interior_point())
                        .unwrap_or([0.0, 0.0]),
                };
                let n = DVec3::from(plane.normal());
                let base = DVec3::from(plane.to_world(seed));
                let tip = if *symmetric {
                    base + n * distance * 0.5
                } else {
                    base + n * *distance
                };
                // Red while the extrusion removes material, like Shapr3D's cut preview.
                let col = if matches!(body, BodyMode::Cut { .. }) {
                    ERR_RED
                } else {
                    ACCENT
                };
                out.push((HandleKind::ExtrudeDistance(fid), tip, n, *distance, col));
            }
            FeatureKind::Transform {
                bodies, transform, ..
            } => {
                let Some(b) = bodies.first().and_then(|b| self.ed.model().bodies.get(b)) else {
                    return out;
                };
                let c = (DVec3::from(b.mesh.bbox_min.map(|x| x as f64))
                    + DVec3::from(b.mesh.bbox_max.map(|x| x as f64)))
                    * 0.5;
                let len = view.pixel_size_at(c) * 70.0;
                for (axis, col) in [
                    (0, Color32::from_rgb(220, 60, 60)),
                    (1, Color32::from_rgb(60, 170, 60)),
                    (2, Color32::from_rgb(60, 100, 230)),
                ] {
                    let mut d = DVec3::ZERO;
                    d[axis] = 1.0;
                    out.push((
                        HandleKind::Translate(fid, axis),
                        c + d * len,
                        d,
                        transform.translate[axis],
                        col,
                    ));
                }
            }
            _ => {}
        }
        out
    }

    /// Returns true if a handle consumed the pointer this frame.
    fn handles(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        view: &CameraView,
        pointer: Option<Vec2>,
    ) -> bool {
        let Some(p) = pointer.or_else(|| {
            resp.interact_pointer_pos()
                .map(|q| Vec2::new(q.x - resp.rect.min.x, q.y - resp.rect.min.y))
        }) else {
            return self.handle.is_some();
        };
        if let Some(h) = &self.handle {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let delta = (p - h.start_px).dot(h.dir_screen) / h.px_per_unit.max(1e-6);
                let fine = ui.input(|i| i.modifiers.shift);
                let step = if fine {
                    0.1
                } else {
                    nice_step(1.0 / h.px_per_unit as f64 * 4.0).max(0.1)
                };
                let mut v = h.start_value + delta as f64;
                v = (v / step).round() * step;
                let kind = h.kind;
                match kind {
                    HandleKind::ExtrudeDistance(fid) => {
                        if v.abs() < 1e-9 {
                            v = step;
                        }
                        self.ed.apply_coalesced("Edit Extrude", fid, |d| {
                            let mut old = v;
                            if let FeatureKind::Extrude { distance, .. } =
                                &mut d.feature_mut(fid).unwrap().kind
                            {
                                old = *distance;
                                *distance = v;
                            }
                            d.auto_extrude_mode(fid, old);
                        });
                    }
                    HandleKind::Translate(fid, axis) => {
                        self.ed.apply_coalesced("Edit Transform", fid, |d| {
                            if let FeatureKind::Transform { transform, .. } =
                                &mut d.feature_mut(fid).unwrap().kind
                            {
                                transform.translate[axis] = v;
                            }
                        });
                    }
                }
                return true;
            }
            self.handle = None;
            return true;
        }
        if !resp.drag_started_by(egui::PointerButton::Primary) {
            return false;
        }
        // egui reports drag-start after a small movement: hit-test where the press began.
        let start = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| resp.interact_pointer_pos())
            .map(|q| Vec2::new(q.x - resp.rect.min.x, q.y - resp.rect.min.y))
            .unwrap_or(p);
        for (kind, tip, dir, value, _) in self.handle_specs(view) {
            let Some(s) = view.project(tip) else { continue };
            if (s - start).length() > 14.0 {
                continue;
            }
            let Some(s2) = view.project(tip + dir) else {
                continue;
            };
            let ds = s2 - s;
            let len = ds.length();
            if len < 1e-4 {
                continue;
            }
            let k = match kind {
                HandleKind::ExtrudeDistance(fid) => {
                    match self.ed.doc().feature(fid).map(|f| &f.kind) {
                        Some(FeatureKind::Extrude {
                            symmetric: true, ..
                        }) => 0.5,
                        _ => 1.0,
                    }
                }
                _ => 1.0,
            };
            self.handle = Some(HandleDrag {
                kind,
                start_value: value,
                start_px: start,
                dir_screen: ds / len,
                px_per_unit: len * k,
            });
            return true;
        }
        false
    }

    // ------------------------------------------------------------------ frame assembly

    fn build_frame(&mut self, view: &CameraView, _rect: egui::Rect) -> FrameInput {
        let cam = &self.ed.doc().camera;
        let right = DVec3::new(view.view.x_axis.x, view.view.y_axis.x, view.view.z_axis.x);
        let up = DVec3::new(view.view.x_axis.y, view.view.y_axis.y, view.view.z_axis.y);
        let to_eye = (view.eye - DVec3::from(cam.target)).normalize();
        let light = (to_eye * 0.9 + up * 0.55 - right * 0.35).normalize();
        let (clip, flags) = match self.clip_plane() {
            Some((n, d)) => (
                [n.x as f32, n.y as f32, n.z as f32, d as f32],
                [1.0, view.viewport.y, 0.0, 0.0],
            ),
            None => ([0.0; 4], [0.0, view.viewport.y, 0.0, 0.0]),
        };
        let globals = Globals {
            view_proj: view.view_proj_f32().to_cols_array_2d(),
            eye: [view.eye.x as f32, view.eye.y as f32, view.eye.z as f32, 1.0],
            light: [light.x as f32, light.y as f32, light.z as f32, 0.0],
            clip,
            flags,
        };
        let highlight = c32(ACCENT);
        let in_sketch = self.sketch.is_some();
        let mut bodies = Vec::new();
        for b in self.ed.model().bodies.values() {
            let props = self.ed.doc().body_props(b.id).cloned();
            if !props.as_ref().map(|p| p.visible).unwrap_or(true) {
                continue;
            }
            let mat = props.map(|p| p.material).unwrap_or_default();
            let selected_face = self
                .sel
                .face
                .filter(|f| f.0 == b.id)
                .map(|f| f.1 as i32)
                .unwrap_or(-1);
            let hover_face = self
                .hover_face
                .filter(|f| f.0 == b.id)
                .map(|f| f.1 as i32)
                .unwrap_or(-1);
            let body_sel = self.sel.bodies.contains(&b.id)
                && self.sel.face.map(|f| f.0 != b.id).unwrap_or(true);
            let xray = self.display == DisplayMode::XRay || in_sketch;
            bodies.push(BodyDraw {
                id: b.id,
                mesh: b.mesh.clone(),
                params: ObjectParams {
                    color: mat.color,
                    highlight: [
                        highlight[0],
                        highlight[1],
                        highlight[2],
                        if body_sel { 0.28 } else { 0.0 },
                    ],
                    ids: [selected_face, hover_face, 0, 0],
                    material: [
                        mat.metallic,
                        mat.roughness,
                        if xray { 0.28 } else { 0.0 },
                        0.0,
                    ],
                },
                show_faces: self.display != DisplayMode::Wireframe,
                show_edges: self.display != DisplayMode::Shaded,
                xray,
                edge_color: [0.08, 0.09, 0.11, 0.9],
            });
        }

        let mut lines = Vec::new();
        // grid on the XY plane, or on the active sketch plane
        let active_plane = self.active_sketch().map(|(_, p, _)| p);
        if self.show_grid {
            let plane = active_plane.unwrap_or(Plane::XY);
            let step = nice_step(cam.distance / 12.0);
            let n = 30i32;
            let center = plane.to_local(cam.target);
            let c = [
                (center[0] / step).round() * step,
                (center[1] / step).round() * step,
            ];
            let ext = n as f64 * step;
            for i in -n..=n {
                let major = ((c[0] / step).round() as i64 + i as i64) % 10 == 0;
                let fade = 1.0 - (i.abs() as f32 / n as f32);
                let a = if major { 0.35 } else { 0.16 } * fade;
                let col = [0.35, 0.37, 0.42, a];
                let x = c[0] + i as f64 * step;
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([x, c[1] - ext])),
                    color: [col[0], col[1], col[2], 0.0],
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([x, c[1]])),
                    color: col,
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([x, c[1]])),
                    color: col,
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([x, c[1] + ext])),
                    color: [col[0], col[1], col[2], 0.0],
                });
                let major = ((c[1] / step).round() as i64 + i as i64) % 10 == 0;
                let a = if major { 0.35 } else { 0.16 } * fade;
                let col = [0.35, 0.37, 0.42, a];
                let y = c[1] + i as f64 * step;
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([c[0] - ext, y])),
                    color: [col[0], col[1], col[2], 0.0],
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([c[0], y])),
                    color: col,
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([c[0], y])),
                    color: col,
                });
                lines.push(LineVertex {
                    pos: f32x3(plane.to_world([c[0] + ext, y])),
                    color: [col[0], col[1], col[2], 0.0],
                });
            }
            // world axes
            let l = (ext * 0.8) as f32;
            for (d, col) in [
                ([1.0, 0.0, 0.0], [0.85, 0.2, 0.2, 0.8]),
                ([0.0, 1.0, 0.0], [0.2, 0.65, 0.2, 0.8]),
                ([0.0, 0.0, 1.0], [0.2, 0.35, 0.85, 0.8]),
            ] {
                lines.push(LineVertex {
                    pos: [0.0; 3],
                    color: col,
                });
                lines.push(LineVertex {
                    pos: [d[0] * l, d[1] * l, d[2] * l],
                    color: col,
                });
            }
        }

        // inactive visible sketches: curves + region fills
        let mut fills = Vec::new();
        let active_id = self.sketch.as_ref().map(|s| s.feature);
        let feats: Vec<(FeatureId, Plane, Sketch, bool)> = self
            .ed
            .doc()
            .features
            .iter()
            .filter_map(|f| match &f.kind {
                FeatureKind::Sketch { plane, sketch, .. } => {
                    Some((f.id, *plane, sketch.clone(), f.hidden))
                }
                _ => None,
            })
            .collect();
        for (fid, plane, sk, hidden) in feats {
            if hidden || self.ed.model().status.get(&fid) == Some(&FeatureStatus::RolledBack) {
                continue;
            }
            if Some(fid) != active_id {
                for (_, cons, pts) in sketch_polylines(&sk) {
                    let col = if cons {
                        [0.5, 0.5, 0.55, 0.6]
                    } else {
                        [0.12, 0.3, 0.75, 0.95]
                    };
                    for w in pts.windows(2) {
                        lines.push(LineVertex {
                            pos: f32x3(plane.to_world(w[0])),
                            color: col,
                        });
                        lines.push(LineVertex {
                            pos: f32x3(plane.to_world(w[1])),
                            color: col,
                        });
                    }
                }
            }
            // region fills
            let Some(profiles) = self.ed.model().profiles.get(&fid).cloned() else {
                continue;
            };
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{profiles:?}{plane:?}").hash(&mut h);
                h.finish()
            };
            let cached = self
                .fill_cache
                .get(&fid)
                .filter(|c| c.0 == key)
                .map(|c| c.1.clone());
            let tris = match cached {
                Some(t) => t,
                None => {
                    let t: Vec<Vec<[f32; 3]>> = profiles
                        .iter()
                        .map(|p| {
                            calibro_core::kernel::profile_fill(&plane, p, 0.05).unwrap_or_default()
                        })
                        .collect();
                    self.fill_cache.insert(fid, (key, t.clone()));
                    t
                }
            };
            for (i, t) in tris.iter().enumerate() {
                let selected = self
                    .sel
                    .regions
                    .iter()
                    .any(|r| r.0 == fid && self.region_index(fid, r.1) == Some(i));
                let hovered = self.hover_region == Some((fid, i));
                let col = if selected {
                    [highlight[0], highlight[1], highlight[2], 0.45]
                } else if hovered {
                    [0.3, 0.55, 0.95, 0.32]
                } else {
                    [0.3, 0.55, 0.95, 0.14]
                };
                // lift fills a hair off the plane so they win against coplanar faces
                let n = plane.normal();
                let lift = (view.pixel_size_at(DVec3::from(plane.origin)) * 0.5) as f32;
                fills.extend(t.iter().map(|p| LineVertex {
                    pos: [
                        p[0] + n[0] as f32 * lift,
                        p[1] + n[1] as f32 * lift,
                        p[2] + n[2] as f32 * lift,
                    ],
                    color: col,
                }));
            }
        }

        let mut overlay = Vec::new();
        if let Some((b, e)) = self.sel.edge {
            if let Some(pl) = self
                .ed
                .model()
                .bodies
                .get(&b)
                .and_then(|b| b.mesh.edges.get(e))
            {
                for w in pl.windows(2) {
                    overlay.push(LineVertex {
                        pos: w[0],
                        color: highlight,
                    });
                    overlay.push(LineVertex {
                        pos: w[1],
                        color: highlight,
                    });
                }
            }
        }
        if let (Some(a), Some(b)) = (self.measure.a, self.measure.b) {
            overlay.push(LineVertex {
                pos: a.as_vec3().to_array(),
                color: [0.1, 0.1, 0.1, 1.0],
            });
            overlay.push(LineVertex {
                pos: b.as_vec3().to_array(),
                color: [0.1, 0.1, 0.1, 1.0],
            });
        }
        FrameInput {
            globals,
            bodies,
            lines,
            overlay_lines: overlay,
            fills,
        }
    }

    // ------------------------------------------------------------------ 2D overlays

    fn draw_overlays(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        view: &CameraView,
        pointer: Option<Vec2>,
    ) {
        let proj = |w: DVec3| view.project(w).map(|v| to_pos(rect, v));

        // active sketch
        if let (Some(s), Some((fid, plane, sk))) = (&self.sketch, self.active_sketch()) {
            let status = self.ed.model().sketch_status.get(&fid).cloned();
            let base = match status {
                Some(SolveStatus::FullyConstrained) => OK_GREEN,
                Some(SolveStatus::Conflicting { .. }) => ERR_RED,
                _ => UNDER_BLUE,
            };
            let to = |q: [f64; 2]| proj(DVec3::from(plane.to_world(q)));
            for (id, cons, pts) in sketch_polylines(&sk) {
                let sel = self.sel.sk_curves.contains(&id);
                let col = if sel {
                    ACCENT
                } else if cons {
                    Color32::from_gray(120)
                } else {
                    base
                };
                let w = if sel { 2.5 } else { 1.8 };
                let screen: Vec<Pos2> = pts.iter().filter_map(|q| to(*q)).collect();
                if cons {
                    for seg in screen.windows(2) {
                        painter.extend(egui::Shape::dashed_line(
                            seg,
                            Stroke::new(1.2, col),
                            6.0,
                            4.0,
                        ));
                    }
                } else {
                    painter.add(egui::Shape::line(screen, Stroke::new(w, col)));
                }
            }
            for (id, pt) in &sk.points {
                if let Some(p) = to(pt.xy()) {
                    let sel = self.sel.sk_points.contains(id);
                    let hov = s.snapped == Some(*id);
                    let r = if hov || sel { 4.5 } else { 3.0 };
                    painter.circle_filled(p, r, if sel { ACCENT } else { base });
                    if hov {
                        painter.circle_stroke(p, 7.0, Stroke::new(1.0, ACCENT));
                    }
                }
            }
            // origin marker
            if let Some(o) = to([0.0, 0.0]) {
                painter.circle_stroke(o, 4.0, Stroke::new(1.5, Color32::from_gray(60)));
            }
            // constraint glyphs and dimension labels
            let mid = |c: u32| -> Option<[f64; 2]> {
                match sk.curves.get(&c)?.kind {
                    CurveKind::Line { a, b } => {
                        let (pa, pb) = (sk.points.get(&a)?, sk.points.get(&b)?);
                        Some([(pa.x + pb.x) * 0.5, (pa.y + pb.y) * 0.5])
                    }
                    CurveKind::Circle { center, radius } => {
                        let c = sk.points.get(&center)?;
                        Some([c.x + radius * 0.707, c.y + radius * 0.707])
                    }
                    CurveKind::Arc { center, start, .. } => {
                        let c = sk.points.get(&center)?;
                        let s = sk.points.get(&start)?;
                        Some([(c.x + s.x) * 0.5, (c.y + s.y) * 0.5])
                    }
                }
            };
            let units = self.ed.doc().units;
            let fmt = |v: f64| {
                let x = v / units.in_mm();
                let s = format!("{x:.3}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            };
            for (cid, c) in &sk.constraints {
                let sel = self.sel.sk_constraint == Some(*cid);
                let col = if sel { ACCENT } else { Color32::from_gray(50) };
                let (text, at): (String, Option<[f64; 2]>) = match c {
                    Constraint::Horizontal { line } => ("H".into(), mid(*line)),
                    Constraint::Vertical { line } => ("V".into(), mid(*line)),
                    Constraint::Parallel { a, .. } => ("par".into(), mid(*a)),
                    Constraint::Perpendicular { a, .. } => ("perp".into(), mid(*a)),
                    Constraint::Equal { a, .. } => ("eq".into(), mid(*a)),
                    Constraint::Tangent { a, .. } => ("tan".into(), mid(*a)),
                    Constraint::Concentric { a, .. } => ("conc".into(), mid(*a)),
                    Constraint::Length { line, value } => (fmt(*value), mid(*line)),
                    Constraint::Radius { curve, value } => {
                        (format!("R{}", fmt(*value)), mid(*curve))
                    }
                    Constraint::Diameter { curve, value } => {
                        (format!("Ø{}", fmt(*value)), mid(*curve))
                    }
                    Constraint::Angle { b, degrees, .. } => (format!("{degrees:.1}°"), mid(*b)),
                    Constraint::Distance { a, b, value }
                    | Constraint::HorizontalDistance { a, b, value }
                    | Constraint::VerticalDistance { a, b, value } => {
                        let (pa, pb) = (sk.points.get(a), sk.points.get(b));
                        (
                            fmt(*value),
                            pa.zip(pb)
                                .map(|(p, q)| [(p.x + q.x) * 0.5, (p.y + q.y) * 0.5]),
                        )
                    }
                    Constraint::Fixed { point, .. } => {
                        ("fix".into(), sk.points.get(point).map(|p| p.xy()))
                    }
                    _ => continue,
                };
                if let Some(p) = at.and_then(to) {
                    let off = if c.is_dimension() {
                        egui::vec2(0.0, -12.0)
                    } else {
                        egui::vec2(10.0, 10.0)
                    };
                    label(painter, p + off, &text, col);
                }
            }
            // rubber band preview
            if let Some(cur) = s.cursor {
                let preview = Stroke::new(1.5, ACCENT);
                let cp = to(cur);
                if let Some(cp) = cp {
                    painter.circle_stroke(cp, 3.0, preview);
                }
                if let (Some((first, _)), Some(cp)) = (s.clicks.first(), cp) {
                    let f = *first;
                    match self.tool {
                        Tool::Line => {
                            if let Some(a) = s.clicks.last().and_then(|c| to(c.0)) {
                                painter.line_segment([a, cp], preview);
                                let len = (cur[0] - s.clicks.last().unwrap().0[0])
                                    .hypot(cur[1] - s.clicks.last().unwrap().0[1]);
                                label(
                                    painter,
                                    cp + egui::vec2(14.0, -14.0),
                                    &format!("{} {}", fmt(len), units.suffix()),
                                    ACCENT,
                                );
                            }
                        }
                        Tool::Rectangle => {
                            let corners = [f, [cur[0], f[1]], cur, [f[0], cur[1]], f];
                            let pts: Vec<Pos2> = corners.iter().filter_map(|q| to(*q)).collect();
                            painter.add(egui::Shape::line(pts, preview));
                            label(
                                painter,
                                cp + egui::vec2(14.0, -14.0),
                                &format!(
                                    "{} x {}",
                                    fmt((cur[0] - f[0]).abs()),
                                    fmt((cur[1] - f[1]).abs())
                                ),
                                ACCENT,
                            );
                        }
                        Tool::Circle | Tool::Polygon | Tool::Arc => {
                            let r = (cur[0] - f[0]).hypot(cur[1] - f[1]);
                            let pts: Vec<Pos2> = arc_points(f, r, 0.0, std::f64::consts::TAU, 64)
                                .into_iter()
                                .filter_map(to)
                                .collect();
                            painter.add(egui::Shape::line(
                                pts,
                                Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                            ));
                            if let Some(c) = to(f) {
                                painter.line_segment([c, cp], preview);
                            }
                            label(
                                painter,
                                cp + egui::vec2(14.0, -14.0),
                                &format!("R {}", fmt(r)),
                                ACCENT,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        // drag handles
        for (kind, tip, dir, value, col) in self.handle_specs(view) {
            let base = match kind {
                HandleKind::ExtrudeDistance(_) => tip - dir * value,
                HandleKind::Translate(..) => tip - dir * view.pixel_size_at(tip) * 70.0,
            };
            if let (Some(a), Some(b)) = (proj(base), proj(tip)) {
                let active = self
                    .handle
                    .as_ref()
                    .map(|h| h.kind == kind)
                    .unwrap_or(false);
                let hovered = pointer
                    .map(|p| (to_pos(rect, p) - b).length() < 14.0)
                    .unwrap_or(false);
                painter.line_segment([a, b], Stroke::new(2.5, col));
                painter.circle_filled(b, if active || hovered { 9.0 } else { 7.0 }, col);
                painter.circle_stroke(
                    b,
                    if active || hovered { 9.0 } else { 7.0 },
                    Stroke::new(1.5, Color32::WHITE),
                );
                if let HandleKind::ExtrudeDistance(fid) = kind {
                    let mode = match self.ed.doc().feature(fid).map(|f| &f.kind) {
                        Some(FeatureKind::Extrude {
                            body: BodyMode::Cut { .. },
                            ..
                        }) => "  cut",
                        Some(FeatureKind::Extrude {
                            body: BodyMode::Join { .. },
                            ..
                        }) => "  join",
                        Some(FeatureKind::Extrude {
                            body: BodyMode::Intersect { .. },
                            ..
                        }) => "  intersect",
                        _ => "",
                    };
                    label(
                        painter,
                        b + egui::vec2(18.0, -16.0),
                        &format!("{}{mode}", self.fmt_len(value)),
                        col,
                    );
                }
            }
        }

        // measurement
        if let Some(a) = self.measure.a.and_then(proj) {
            painter.circle_filled(a, 4.0, Color32::BLACK);
            if let Some(bw) = self.measure.b {
                if let Some(b) = proj(bw) {
                    painter.circle_filled(b, 4.0, Color32::BLACK);
                    let d = (bw - self.measure.a.unwrap()).length();
                    label(painter, a + (b - a) * 0.5, &self.fmt_len(d), Color32::BLACK);
                }
            }
        }

        // axis triad (bottom-left)
        let origin = rect.left_bottom() + egui::vec2(46.0, -46.0);
        let t = DVec3::from(self.ed.doc().camera.target);
        let px = view.pixel_size_at(t);
        for (d, name, col) in [
            (DVec3::X, "X", Color32::from_rgb(220, 60, 60)),
            (DVec3::Y, "Y", Color32::from_rgb(60, 160, 60)),
            (DVec3::Z, "Z", Color32::from_rgb(60, 100, 230)),
        ] {
            if let (Some(a), Some(b)) = (view.project(t), view.project(t + d * px * 30.0)) {
                let v = b - a;
                let e = origin + egui::vec2(v.x, v.y);
                painter.line_segment([origin, e], Stroke::new(2.5, col));
                painter.text(
                    e + egui::vec2(v.x, v.y).normalized() * 8.0,
                    Align2::CENTER_CENTER,
                    name,
                    FontId::proportional(12.0),
                    col,
                );
            }
        }
    }

    fn viewport_buttons(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        egui::Area::new(egui::Id::unique("view_buttons"))
            .fixed_pos(rect.right_top() + egui::vec2(-8.0, 8.0))
            .pivot(Align2::RIGHT_TOP)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    // Never wider than the viewport: wrap onto two lines when narrow.
                    ui.set_max_width((rect.width() - 32.0).max(120.0));
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .selectable_label(self.show_tools, "Tools")
                            .on_hover_text("Show / hide the tool panel")
                            .clicked()
                        {
                            self.show_tools = !self.show_tools;
                        }
                        if ui
                            .selectable_label(self.show_inspector, "Panel")
                            .on_hover_text("Show / hide bodies, history and inspector")
                            .clicked()
                        {
                            self.show_inspector = !self.show_inspector;
                        }
                        ui.separator();
                        for v in ["Top", "Front", "Right", "Iso"] {
                            if ui.small_button(v).clicked() {
                                self.set_view(v);
                            }
                        }
                        if ui.small_button("Fit").clicked() {
                            self.zoom_to_fit();
                        }
                        let ortho = self.ed.doc().camera.orthographic;
                        if ui.selectable_label(ortho, "Ortho").clicked() {
                            self.ed.apply_silent(|d| d.camera.orthographic = !ortho);
                        }
                        egui::ComboBox::from_id_salt("display")
                            .width(96.0)
                            .selected_text(match self.display {
                                DisplayMode::ShadedEdges => "Shaded+edges",
                                DisplayMode::Shaded => "Shaded",
                                DisplayMode::Wireframe => "Wireframe",
                                DisplayMode::XRay => "X-ray",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.display,
                                    DisplayMode::ShadedEdges,
                                    "Shaded+edges",
                                );
                                ui.selectable_value(
                                    &mut self.display,
                                    DisplayMode::Shaded,
                                    "Shaded",
                                );
                                ui.selectable_value(
                                    &mut self.display,
                                    DisplayMode::Wireframe,
                                    "Wireframe",
                                );
                                ui.selectable_value(&mut self.display, DisplayMode::XRay, "X-ray");
                            });
                        ui.toggle_value(&mut self.section.enabled, "Section");
                    });
                    if self.section.enabled {
                        ui.horizontal(|ui| {
                            for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                                ui.radio_value(&mut self.section.axis, i, *n);
                            }
                            ui.checkbox(&mut self.section.flip, "Flip");
                        });
                        let (mut lo, mut hi) = (-100.0f64, 100.0f64);
                        let axis = self.section.axis;
                        let mut any = false;
                        for b in self.ed.model().bodies.values() {
                            if !any {
                                lo = f64::INFINITY;
                                hi = f64::NEG_INFINITY;
                                any = true;
                            }
                            lo = lo.min(b.mesh.bbox_min[axis] as f64);
                            hi = hi.max(b.mesh.bbox_max[axis] as f64);
                        }
                        ui.add(egui::Slider::new(&mut self.section.offset, lo..=hi).text("Offset"));
                    }
                });
            });
    }
}

fn f32x3(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}
