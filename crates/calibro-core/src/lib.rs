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
