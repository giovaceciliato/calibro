//! # calibro-core
//!
//! Platform-independent heart of Calibro: the editable [`Document`] (feature
//! history), the parametric recompute [`Engine`], and the [`Editor`] session
//! with undo/redo and unsaved-change tracking. No UI or file-system code here.

pub mod document;
pub mod editor;
pub mod recompute;

pub use document::*;
pub use editor::Editor;
pub use recompute::{BodyGeometry, Engine, EvaluatedBody, FeatureStatus, Model};

pub use calibro_kernel as kernel;
pub use calibro_sketch as sketch;

#[cfg(test)]
mod tests {
    use super::*;
    use calibro_kernel::{BooleanOp, Plane};
    use calibro_sketch::{Constraint, Sketch};

    fn plate_doc() -> (Document, FeatureId, FeatureId, ConstraintIds) {
        let mut doc = Document::new();
        let mut s = Sketch::new();
        let [bottom, right, _, _] = s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let (a, _) = s.line_points(bottom).unwrap();
        s.add_constraint(Constraint::Fixed {
            point: a,
            x: 0.0,
            y: 0.0,
        });
        let w = s.add_constraint(Constraint::Length {
            line: bottom,
            value: 40.0,
        });
        let h = s.add_constraint(Constraint::Length {
            line: right,
            value: 20.0,
        });
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XY,
                sketch: s,
                on_body: None,
                attach: None,
            },
        );
        let ex = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::All,
                distance: 5.0,
                symmetric: false,
                body: BodyMode::NewBody,
            },
        );
        (doc, sk, ex, ConstraintIds { w, h })
    }

    struct ConstraintIds {
        w: u32,
        #[allow(dead_code)]
        h: u32,
    }

    fn vol(ed: &Editor, b: BodyId) -> f64 {
        ed.model().bodies[&b].mesh.volume()
    }

    #[test]
    fn sketch_extrude_and_parametric_edit() {
        let (doc, sk, ex, c) = plate_doc();
        let mut ed = Editor::new(doc);
        let body = BodyId {
            feature: ex,
            index: 0,
        };
        assert!((vol(&ed, body) - 4000.0).abs() < 1e-3);
        assert_eq!(
            ed.model().sketch_status[&sk],
            calibro_sketch::SolveStatus::FullyConstrained
        );
        // Edit the width dimension in the sketch: downstream extrude updates.
        ed.apply("Edit width", |d| {
            let (_, s) = d.sketch_mut(sk).unwrap();
            *s.constraints.get_mut(&c.w).unwrap().value_mut().unwrap() = 60.0;
            calibro_sketch::solve(s).unwrap();
        });
        assert!((vol(&ed, body) - 6000.0).abs() < 1e-3, "{}", vol(&ed, body));
        // Edit the extrude distance.
        ed.apply("Edit distance", |d| {
            if let FeatureKind::Extrude { distance, .. } = &mut d.feature_mut(ex).unwrap().kind {
                *distance = 10.0;
            }
        });
        assert!((vol(&ed, body) - 12000.0).abs() < 1e-3);
        assert!(ed.is_dirty());
        // Undo twice returns to the original geometry.
        ed.undo();
        ed.undo();
        assert!((vol(&ed, body) - 4000.0).abs() < 1e-3);
        assert!(
            !ed.is_dirty(),
            "undo back to the saved state should be clean"
        );
        ed.redo();
        assert!((vol(&ed, body) - 6000.0).abs() < 1e-3);
        assert!(ed.is_dirty());
    }

    #[test]
    fn cut_with_cylinder_and_cache_reuse() {
        let (mut doc, _, ex, _) = plate_doc();
        let body = BodyId {
            feature: ex,
            index: 0,
        };
        doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Cylinder {
                    base: [20.0, 10.0, -1.0],
                    radius: 4.0,
                    height: 10.0,
                },
                body: BodyMode::Cut { target: body },
            },
        );
        let mut ed = Editor::new(doc);
        let exp = 4000.0 - std::f64::consts::PI * 16.0 * 5.0;
        assert!(
            (vol(&ed, body) - exp).abs() / exp < 5e-3,
            "{}",
            vol(&ed, body)
        );
        assert_eq!(ed.model().bodies.len(), 1);
        // A no-op recompute uses the cache only.
        ed.recompute();
        assert_eq!(ed.engine().last_evaluations, 0);
        assert!(ed.engine().last_cache_hits >= 2);
    }

    #[test]
    fn boolean_feature_and_dependency_failure() {
        let mut doc = Document::new();
        let a = doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [0.0; 3],
                    size: [10.0; 3],
                },
                body: BodyMode::NewBody,
            },
        );
        let b = doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [5.0; 3],
                    size: [10.0; 3],
                },
                body: BodyMode::NewBody,
            },
        );
        let ba = BodyId {
            feature: a,
            index: 0,
        };
        let bb = BodyId {
            feature: b,
            index: 0,
        };
        let u = doc.add_feature(
            "",
            FeatureKind::Boolean {
                op: BooleanOp::Union,
                target: ba,
                tools: vec![bb],
                keep_tools: false,
            },
        );
        let mut ed = Editor::new(doc);
        assert_eq!(ed.model().bodies.len(), 1);
        assert!((vol(&ed, ba) - 1875.0).abs() < 1e-2);
        assert_eq!(ed.doc().dependents(b), vec![u]);
        // Make the second box invalid: it fails, and the boolean that uses it fails too.
        ed.apply("break", |d| {
            if let FeatureKind::Primitive {
                primitive: Primitive::Box { size, .. },
                ..
            } = &mut d.feature_mut(b).unwrap().kind
            {
                size[0] = -1.0;
            }
        });
        assert!(ed.model().status[&b].is_failed());
        assert!(ed.model().status[&u].is_failed());
        // The first box still exists.
        assert!((vol(&ed, ba) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn join_boss_sketched_on_top_face() {
        let (mut doc, _, ex, _) = plate_doc();
        let body = BodyId {
            feature: ex,
            index: 0,
        };
        let mut s = Sketch::new();
        s.add_circle([20.0, 10.0], 4.0);
        let top = Plane {
            origin: [0.0, 0.0, 5.0],
            ..Plane::XY
        };
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: top,
                sketch: s,
                on_body: Some(body),
                attach: None,
            },
        );
        let boss = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::All,
                distance: 6.0,
                symmetric: false,
                body: BodyMode::Join { target: body },
            },
        );
        let pocket_sk = {
            let mut s = Sketch::new();
            s.add_rectangle([2.0, 2.0], [8.0, 8.0]);
            doc.add_feature(
                "",
                FeatureKind::Sketch {
                    plane: top,
                    sketch: s,
                    on_body: Some(body),
                    attach: None,
                },
            )
        };
        let pocket = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: pocket_sk,
                profiles: ProfileSelection::All,
                distance: -3.0,
                symmetric: false,
                body: BodyMode::Cut { target: body },
            },
        );
        let ed = Editor::new(doc);
        assert_eq!(
            ed.model().status[&boss],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&boss]
        );
        assert_eq!(
            ed.model().status[&pocket],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&pocket]
        );
        let exp = 4000.0 + std::f64::consts::PI * 16.0 * 6.0 - 36.0 * 3.0;
        let v = vol(&ed, body);
        assert!((v - exp).abs() / exp < 3e-3, "{v} vs {exp}");
    }

    #[test]
    fn pushing_a_face_sketch_inward_cuts_like_shapr3d() {
        let (mut doc, _, ex, _) = plate_doc();
        let body = BodyId {
            feature: ex,
            index: 0,
        };
        let mut s = Sketch::new();
        s.add_rectangle([10.0, 5.0], [20.0, 15.0]);
        let top = Plane {
            origin: [0.0, 0.0, 5.0],
            ..Plane::XY
        };
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: top,
                sketch: s,
                on_body: Some(body),
                attach: None,
            },
        );
        let e2 = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::All,
                distance: 10.0,
                symmetric: false,
                body: BodyMode::Join { target: body },
            },
        );
        let mut ed = Editor::new(doc);
        let v0 = vol(&ed, body);
        assert!((v0 - (4000.0 + 1000.0)).abs() < 1e-2, "{v0}");
        // drag the distance to -3 (into the plate): becomes a pocket
        ed.apply("drag", |d| {
            if let FeatureKind::Extrude { distance, .. } = &mut d.feature_mut(e2).unwrap().kind {
                *distance = -3.0;
            }
            assert!(d.auto_extrude_mode(e2, 10.0));
        });
        let v1 = vol(&ed, body);
        assert!((v1 - (4000.0 - 300.0)).abs() < 1e-2, "{v1}");
        // a second inward change keeps Cut; dragging back out returns to Join
        ed.apply("drag", |d| {
            if let FeatureKind::Extrude { distance, .. } = &mut d.feature_mut(e2).unwrap().kind {
                *distance = 4.0;
            }
            assert!(d.auto_extrude_mode(e2, -3.0));
        });
        assert!((vol(&ed, body) - 4400.0).abs() < 1e-2);
        // through-all cut (deeper than the plate) leaves a hole
        ed.apply("drag", |d| {
            if let FeatureKind::Extrude { distance, .. } = &mut d.feature_mut(e2).unwrap().kind {
                *distance = -8.0;
            }
            d.auto_extrude_mode(e2, 4.0);
        });
        assert!((vol(&ed, body) - 3500.0).abs() < 1e-2, "{}", vol(&ed, body));
    }

    #[test]
    fn modify_features_fillet_chamfer_shell() {
        let mut doc = Document::new();
        let b = doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [0.0; 3],
                    size: [40.0, 30.0, 20.0],
                },
                body: BodyMode::NewBody,
            },
        );
        let body = BodyId {
            feature: b,
            index: 0,
        };
        let sh = doc.add_feature(
            "",
            FeatureKind::Shell {
                body,
                open_faces: vec![Plane {
                    origin: [0.0, 0.0, 20.0],
                    ..Plane::XY
                }],
                thickness: 2.0,
            },
        );
        let ch = doc.add_feature(
            "",
            FeatureKind::Chamfer {
                body,
                edges: vec![[[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]]],
                distance: 1.0,
            },
        );
        let fi = doc.add_feature(
            "",
            FeatureKind::Fillet {
                body,
                edges: vec![[[40.0, 0.0, 0.0], [40.0, 30.0, 0.0]]],
                radius: 1.0,
            },
        );
        let mut ed = Editor::new(doc);
        for id in [sh, ch, fi] {
            assert_eq!(
                ed.model().status[&id],
                FeatureStatus::Ok,
                "{:?}",
                ed.model().status[&id]
            );
        }
        let v = vol(&ed, body);
        let shelled = 24000.0 - 36.0 * 26.0 * 18.0;
        assert!(v < shelled && v > shelled - 60.0, "{v}");
        // parametric: thicker walls -> more material
        ed.apply("thicker", |d| {
            if let FeatureKind::Shell { thickness, .. } = &mut d.feature_mut(sh).unwrap().kind {
                *thickness = 3.0;
            }
        });
        assert!(vol(&ed, body) > v);
        // shell after a chamfer (the chamfer leaves hair-thin steps; still convex)
        let sh2 = ed.apply("shell after chamfer", |d| {
            d.feature_mut(sh).unwrap().suppressed = true;
            d.feature_mut(fi).unwrap().suppressed = true; // fillets make curved faces
            d.add_feature(
                "",
                FeatureKind::Shell {
                    body,
                    open_faces: vec![Plane {
                        origin: [0.0, 0.0, 20.0],
                        ..Plane::XY
                    }],
                    thickness: 2.0,
                },
            )
        });
        assert_eq!(
            ed.model().status[&sh2],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&sh2]
        );
        ed.apply("unsuppress", |d| {
            d.feature_mut(sh).unwrap().suppressed = false;
            d.feature_mut(fi).unwrap().suppressed = false;
            d.feature_mut(sh2).unwrap().suppressed = true;
        });
        // an edge that disappears after an earlier edit fails with a clear reason
        ed.apply("resize", |d| {
            if let FeatureKind::Primitive {
                primitive: Primitive::Box { size, .. },
                ..
            } = &mut d.feature_mut(b).unwrap().kind
            {
                size[0] = 50.0;
            }
        });
        assert!(ed.model().status[&fi].is_failed());
    }

    #[test]
    fn sweep_and_loft_features() {
        use calibro_sketch::Sketch;
        let mut doc = Document::new();
        // profile: circle r=2 on the XZ plane at the origin
        let mut prof = Sketch::new();
        prof.add_circle([0.0, 0.0], 2.0);
        let ps = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XZ,
                sketch: prof,
                on_body: None,
                attach: None,
            },
        );
        // path on the XY plane: line along +Y then a quarter arc turning to +X
        let mut path = Sketch::new();
        let a = path.add_point(0.0, 0.0);
        let b = path.add_point(0.0, 20.0);
        let c = path.add_point(10.0, 30.0);
        let ctr = path.add_point(10.0, 20.0);
        path.add_line(a, b);
        path.add_arc(ctr, c, b); // CCW from c to b == clockwise from b to c
        let pp = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XY,
                sketch: path,
                on_body: None,
                attach: None,
            },
        );
        let sw = doc.add_feature(
            "",
            FeatureKind::Sweep {
                sketch: ps,
                profiles: ProfileSelection::All,
                path: pp,
                body: BodyMode::NewBody,
            },
        );
        // loft between a square at z=0 and a smaller square at z=10
        let mut s1 = Sketch::new();
        s1.add_rectangle([30.0, 0.0], [40.0, 10.0]);
        let l1 = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XY,
                sketch: s1,
                on_body: None,
                attach: None,
            },
        );
        let mut s2 = Sketch::new();
        s2.add_rectangle([32.0, 2.0], [38.0, 8.0]);
        let l2 = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane {
                    origin: [0.0, 0.0, 10.0],
                    ..Plane::XY
                },
                sketch: s2,
                on_body: None,
                attach: None,
            },
        );
        let lo = doc.add_feature(
            "",
            FeatureKind::Loft {
                from_sketch: l1,
                from_seed: [35.0, 5.0],
                to_sketch: l2,
                to_seed: [35.0, 5.0],
                body: BodyMode::NewBody,
            },
        );
        let ed = Editor::new(doc);
        assert_eq!(
            ed.model().status[&sw],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&sw]
        );
        assert_eq!(
            ed.model().status[&lo],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&lo]
        );
        let pi = std::f64::consts::PI;
        let vs = vol(
            &ed,
            BodyId {
                feature: sw,
                index: 0,
            },
        );
        let exp = pi * 4.0 * (20.0 + pi / 2.0 * 10.0);
        assert!((vs - exp).abs() / exp < 2e-2, "sweep {vs} vs {exp}");
        let vl = vol(
            &ed,
            BodyId {
                feature: lo,
                index: 0,
            },
        );
        let expl = 10.0 / 3.0 * (100.0 + 36.0 + 60.0);
        assert!((vl - expl).abs() / expl < 1e-3, "loft {vl} vs {expl}");
    }

    #[test]
    fn sketch_on_face_follows_the_face() {
        use calibro_sketch::Sketch;
        let mut doc = Document::new();
        let b = doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [0.0; 3],
                    size: [40.0, 30.0, 20.0],
                },
                body: BodyMode::NewBody,
            },
        );
        let body = BodyId {
            feature: b,
            index: 0,
        };
        let mut s = Sketch::new();
        s.add_circle([20.0, 15.0], 5.0);
        let top = Plane {
            origin: [0.0, 0.0, 20.0],
            ..Plane::XY
        };
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: top,
                sketch: s,
                on_body: Some(body),
                attach: Some(FaceAttach {
                    normal: [0.0, 0.0, 1.0],
                    offset: 20.0,
                    anchor: [20.0, 15.0, 20.0],
                }),
            },
        );
        let ex = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::All,
                distance: 10.0,
                symmetric: false,
                body: BodyMode::Join { target: body },
            },
        );
        let mut ed = Editor::new(doc);
        assert_eq!(ed.model().status[&ex], FeatureStatus::Ok);
        assert!((ed.model().bodies[&body].mesh.bbox_max[2] - 30.0).abs() < 1e-3);
        // make the box taller: the sketch and the boss move up with the top face
        ed.apply("taller", |d| {
            if let FeatureKind::Primitive {
                primitive: Primitive::Box { size, .. },
                ..
            } = &mut d.feature_mut(b).unwrap().kind
            {
                size[2] = 35.0;
            }
        });
        assert_eq!(ed.model().status[&sk], FeatureStatus::Ok);
        assert!((ed.model().sketch_plane(ed.doc(), sk).unwrap().origin[2] - 35.0).abs() < 1e-9);
        let m = &ed.model().bodies[&body].mesh;
        assert!((m.bbox_max[2] - 45.0).abs() < 1e-3, "{:?}", m.bbox_max);
        let v = m.volume();
        let exp = 40.0 * 30.0 * 35.0 + std::f64::consts::PI * 25.0 * 10.0;
        assert!((v - exp).abs() / exp < 2e-3, "{v} vs {exp}");
        // the body disappears (suppressed): sketch stays put and says why
        ed.apply("suppress box", |d| {
            d.feature_mut(b).unwrap().suppressed = true
        });
        assert!(matches!(ed.model().status[&sk], FeatureStatus::Warning(_)));
        assert!((ed.model().sketch_plane(ed.doc(), sk).unwrap().origin[2] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn extrude_region_of_crossing_circles() {
        use calibro_sketch::Sketch;
        let mut doc = Document::new();
        let mut s = Sketch::new();
        s.add_circle([0.0, 0.0], 10.0);
        s.add_circle([10.0, 0.0], 10.0);
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XY,
                sketch: s,
                on_body: None,
                attach: None,
            },
        );
        let ex = doc.add_feature(
            "",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::Seeds {
                    points: vec![[5.0, 0.0]],
                }, // the lens
                distance: 2.0,
                symmetric: false,
                body: BodyMode::NewBody,
            },
        );
        let ed = Editor::new(doc);
        assert_eq!(ed.model().profiles[&sk].len(), 3);
        assert_eq!(
            ed.model().status[&ex],
            FeatureStatus::Ok,
            "{:?}",
            ed.model().status[&ex]
        );
        let lens = 200.0 * (0.5f64).acos() - 5.0 * 300f64.sqrt();
        let v = vol(
            &ed,
            BodyId {
                feature: ex,
                index: 0,
            },
        );
        assert!(
            (v - 2.0 * lens).abs() / (2.0 * lens) < 1e-2,
            "{v} vs {}",
            2.0 * lens
        );
    }

    #[test]
    fn rollback_marker() {
        let (mut doc, _, ex, _) = plate_doc();
        doc.rollback = Some(1);
        let ed = Editor::new(doc);
        assert!(ed.model().bodies.is_empty());
        assert_eq!(ed.model().status[&ex], FeatureStatus::RolledBack);
    }

    #[test]
    fn patterns_and_mirror() {
        let mut doc = Document::new();
        let a = doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [1.0, 0.0, 0.0],
                    size: [2.0; 3],
                },
                body: BodyMode::NewBody,
            },
        );
        let ba = BodyId {
            feature: a,
            index: 0,
        };
        doc.add_feature(
            "",
            FeatureKind::LinearPattern {
                body: ba,
                direction: [0.0, 1.0, 0.0],
                count: 4,
                spacing: 5.0,
            },
        );
        doc.add_feature(
            "",
            FeatureKind::CircularPattern {
                body: ba,
                axis_origin: [0.0; 3],
                axis_dir: [0.0, 0.0, 1.0],
                count: 6,
                total_angle_deg: 360.0,
            },
        );
        doc.add_feature(
            "",
            FeatureKind::Mirror {
                body: ba,
                plane: Plane::YZ,
            },
        );
        let ed = Editor::new(doc);
        assert_eq!(ed.model().bodies.len(), 1 + 3 + 5 + 1);
        assert!(ed.model().failed_features().is_empty());
    }

    #[test]
    fn delete_feature_removes_dependents() {
        let (mut doc, sk, ex, _) = plate_doc();
        let gone = doc.delete_feature(sk);
        assert!(gone.contains(&ex));
        assert!(doc.features.is_empty());
    }

    #[test]
    fn revolve_feature() {
        let mut doc = Document::new();
        let mut s = Sketch::new();
        s.add_rectangle([5.0, 0.0], [10.0, 10.0]);
        let sk = doc.add_feature(
            "",
            FeatureKind::Sketch {
                plane: Plane::XZ,
                sketch: s,
                on_body: None,
                attach: None,
            },
        );
        let r = doc.add_feature(
            "",
            FeatureKind::Revolve {
                sketch: sk,
                profiles: ProfileSelection::All,
                axis: RevolveAxis::SketchY,
                angle_deg: 180.0,
                body: BodyMode::NewBody,
            },
        );
        let ed = Editor::new(doc);
        let v = vol(
            &ed,
            BodyId {
                feature: r,
                index: 0,
            },
        );
        let exp = std::f64::consts::PI * 75.0 * 10.0 / 2.0;
        assert!((v - exp).abs() / exp < 5e-3, "{v} vs {exp}");
    }

    #[test]
    fn mesh_resource_round_trip_and_validation() {
        let m = MeshResource {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
        };
        let b = m.to_bytes();
        assert_eq!(MeshResource::from_bytes(&b).unwrap(), m);
        assert!(MeshResource::from_bytes(&b[..20]).is_err());
        assert!(MeshResource::from_bytes(b"nope").is_err());
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    #[test]
    fn coalesced_edits_make_one_undo_step() {
        let mut ed = Editor::default();
        let id = ed.apply("add", |d| {
            d.add_feature(
                "",
                FeatureKind::Primitive {
                    primitive: Primitive::Sphere {
                        center: [0.0; 3],
                        radius: 1.0,
                    },
                    body: BodyMode::NewBody,
                },
            )
        });
        for i in 0..20 {
            ed.apply_coalesced("radius", id, |d| {
                if let FeatureKind::Primitive {
                    primitive: Primitive::Sphere { radius, .. },
                    ..
                } = &mut d.feature_mut(id).unwrap().kind
                {
                    *radius = 1.0 + i as f64 * 0.1;
                }
            });
        }
        ed.end_gesture();
        ed.undo();
        let FeatureKind::Primitive {
            primitive: Primitive::Sphere { radius, .. },
            ..
        } = ed.doc().feature(id).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(radius, 1.0);
        ed.undo();
        assert!(ed.doc().features.is_empty());
    }
}
