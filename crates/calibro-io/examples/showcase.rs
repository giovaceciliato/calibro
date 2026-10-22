//! Builds a showcase project (bracket + revolved knob + patterned pins):
//! `cargo run -p calibro-io --example showcase -- out.calibro`
use calibro_core::kernel::{BooleanOp, Plane};
use calibro_core::sketch::{Constraint, Sketch};
use calibro_core::*;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "showcase.calibro".into());
    let mut doc = Document::new();
    doc.metadata.title = "Showcase".into();
    // Base plate with two holes, fully dimensioned
    let mut s = Sketch::new();
    let [b, r, _, _] = s.add_rectangle([0.0, 0.0], [80.0, 50.0]);
    let (p0, _) = s.line_points(b).unwrap();
    s.add_constraint(Constraint::Fixed {
        point: p0,
        x: 0.0,
        y: 0.0,
    });
    s.add_constraint(Constraint::Length {
        line: b,
        value: 80.0,
    });
    s.add_constraint(Constraint::Length {
        line: r,
        value: 50.0,
    });
    s.add_circle([15.0, 25.0], 5.0);
    s.add_circle([65.0, 25.0], 5.0);
    calibro_core::sketch::solve(&mut s).unwrap();
    let sk = doc.add_feature(
        "Base sketch",
        FeatureKind::Sketch {
            plane: Plane::XY,
            sketch: s,
            on_body: None,
        },
    );
    let plate = doc.add_feature(
        "Plate",
        FeatureKind::Extrude {
            sketch: sk,
            profiles: ProfileSelection::Seeds {
                points: vec![[2.0, 2.0]],
            },
            distance: 8.0,
            symmetric: false,
            body: BodyMode::NewBody,
        },
    );
    let pb = BodyId {
        feature: plate,
        index: 0,
    };
    doc.feature_mut(sk).unwrap().hidden = true;
    // Boss on top (sketched on the top face)
    let mut s2 = Sketch::new();
    s2.add_circle([40.0, 25.0], 12.0);
    let top = Plane {
        origin: [0.0, 0.0, 8.0],
        ..Plane::XY
    };
    let sk2 = doc.add_feature(
        "Boss sketch",
        FeatureKind::Sketch {
            plane: top,
            sketch: s2,
            on_body: Some(pb),
        },
    );
    doc.add_feature(
        "Boss",
        FeatureKind::Extrude {
            sketch: sk2,
            profiles: ProfileSelection::All,
            distance: 18.0,
            symmetric: false,
            body: BodyMode::Join { target: pb },
        },
    );
    doc.feature_mut(sk2).unwrap().hidden = true;
    doc.add_feature(
        "Bore",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [40.0, 25.0, -1.0],
                radius: 6.0,
                height: 40.0,
            },
            body: BodyMode::Cut { target: pb },
        },
    );
    // Revolved knob next to it
    let mut s3 = Sketch::new();
    let pts = [
        [0.0, 0.0],
        [14.0, 0.0],
        [14.0, 6.0],
        [9.0, 10.0],
        [9.0, 22.0],
        [0.0, 22.0],
    ];
    let ids: Vec<_> = pts.iter().map(|p| s3.add_point(p[0], p[1])).collect();
    for i in 0..ids.len() {
        s3.add_line(ids[i], ids[(i + 1) % ids.len()]);
    }
    let front = Plane {
        origin: [115.0, 25.0, 0.0],
        ..Plane::XZ
    };
    let sk3 = doc.add_feature(
        "Knob profile",
        FeatureKind::Sketch {
            plane: front,
            sketch: s3,
            on_body: None,
        },
    );
    let knob = doc.add_feature(
        "Knob",
        FeatureKind::Revolve {
            sketch: sk3,
            profiles: ProfileSelection::All,
            axis: RevolveAxis::SketchY,
            angle_deg: 360.0,
            body: BodyMode::NewBody,
        },
    );
    doc.feature_mut(sk3).unwrap().hidden = true;
    // Pins, circular pattern around the boss
    let pin = doc.add_feature(
        "Pin",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [40.0, 43.0, 8.0],
                radius: 2.5,
                height: 10.0,
            },
            body: BodyMode::NewBody,
        },
    );
    doc.add_feature(
        "Pin pattern",
        FeatureKind::CircularPattern {
            body: BodyId {
                feature: pin,
                index: 0,
            },
            axis_origin: [40.0, 25.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            count: 4,
            total_angle_deg: 360.0,
        },
    );
    // Two crossing cylinders united (a pipe tee)
    let c1 = doc.add_feature(
        "Pipe",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [115.0, -25.0, 0.0],
                radius: 8.0,
                height: 30.0,
            },
            body: BodyMode::NewBody,
        },
    );
    let c2 = doc.add_feature(
        "Branch",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [115.0, -25.0, -15.0],
                radius: 6.0,
                height: 30.0,
            },
            body: BodyMode::NewBody,
        },
    );
    doc.add_feature(
        "Tilt branch",
        FeatureKind::Transform {
            bodies: vec![BodyId {
                feature: c2,
                index: 0,
            }],
            transform: calibro_core::kernel::Transform {
                rotate_axis: [1.0, 0.0, 0.0],
                rotate_deg: 90.0,
                pivot: [115.0, -25.0, 15.0],
                translate: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            copy: false,
        },
    );
    doc.add_feature(
        "Tee",
        FeatureKind::Boolean {
            op: BooleanOp::Union,
            target: BodyId {
                feature: c1,
                index: 0,
            },
            tools: vec![BodyId {
                feature: c2,
                index: 0,
            }],
            keep_tools: false,
        },
    );
    let sph = c1;
    let mats = material_presets();
    let set = |doc: &mut Document, b: BodyId, name: &str, m: usize| {
        let p = doc.body_props_mut(b);
        p.name = name.into();
        p.material = mats[m].clone();
    };
    set(&mut doc, pb, "Bracket", 3);
    set(
        &mut doc,
        BodyId {
            feature: knob,
            index: 0,
        },
        "Knob",
        9,
    );
    set(
        &mut doc,
        BodyId {
            feature: sph,
            index: 0,
        },
        "Pipe tee",
        5,
    );
    set(
        &mut doc,
        BodyId {
            feature: pin,
            index: 0,
        },
        "Pin",
        2,
    );
    doc.camera.yaw = -1.05;
    doc.camera.pitch = 0.5;
    doc.camera.target = [60.0, 5.0, 10.0];
    doc.camera.distance = 230.0;
    let ed = Editor::new(doc.clone());
    for f in &doc.features {
        println!("{:<14} {:?}", f.name, ed.model().status[&f.id]);
    }
    std::fs::write(&out, calibro_project::save_to_bytes(&doc).unwrap()).unwrap();
    println!("bodies: {}  -> {out}", ed.model().bodies.len());
}
