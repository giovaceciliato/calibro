//! Builds a demo of the Modify tools (chamfer, fillet, shell, sweep, loft):
//! `cargo run -p calibro-io --example modify_tools -- examples/modify-tools.calibro`
use calibro_core::kernel::Plane;
use calibro_core::sketch::Sketch;
use calibro_core::*;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "modify-tools.calibro".into());
    let mut doc = Document::new();
    doc.metadata.title = "Modify tools".into();

    // 1. A tray: box, two chamfered top edges, a filleted bottom edge, shelled open at the top.
    let b = doc.add_feature(
        "Tray block",
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
    doc.add_feature(
        "Hollow",
        FeatureKind::Shell {
            body,
            open_faces: vec![Plane {
                origin: [0.0, 0.0, 20.0],
                ..Plane::XY
            }],
            thickness: 2.0,
        },
    );
    doc.add_feature(
        "Top bevels",
        FeatureKind::Chamfer {
            body,
            edges: vec![
                [[0.0, 0.0, 20.0], [40.0, 0.0, 20.0]],
                [[0.0, 30.0, 20.0], [40.0, 30.0, 20.0]],
            ],
            distance: 1.0,
        },
    );
    doc.add_feature(
        "Bottom round",
        FeatureKind::Fillet {
            body,
            edges: vec![[[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]]],
            radius: 3.0,
        },
    );

    // 2. A pipe: circle on the Front plane swept along a line + quarter arc.
    let mut prof = Sketch::new();
    prof.add_circle([70.0, 5.0], 4.0);
    let ps = doc.add_feature(
        "Pipe profile",
        FeatureKind::Sketch {
            plane: Plane::XZ,
            sketch: prof,
            on_body: None,
        },
    );
    let mut path = Sketch::new();
    let a = path.add_point(70.0, 0.0);
    let p1 = path.add_point(70.0, 30.0);
    let p2 = path.add_point(90.0, 50.0);
    let c = path.add_point(90.0, 30.0);
    path.add_line(a, p1);
    path.add_arc(c, p2, p1);
    // path at z = 5 so it passes through the circle centre
    let pp = doc.add_feature(
        "Pipe path",
        FeatureKind::Sketch {
            plane: Plane {
                origin: [0.0, 0.0, 5.0],
                ..Plane::XY
            },
            sketch: path,
            on_body: None,
        },
    );
    doc.add_feature(
        "Pipe",
        FeatureKind::Sweep {
            sketch: ps,
            profiles: ProfileSelection::All,
            path: pp,
            body: BodyMode::NewBody,
        },
    );

    // 3. A loft from a square to a circle.
    let mut s1 = Sketch::new();
    s1.add_rectangle([110.0, 0.0], [130.0, 20.0]);
    let l1 = doc.add_feature(
        "Loft base",
        FeatureKind::Sketch {
            plane: Plane::XY,
            sketch: s1,
            on_body: None,
        },
    );
    let mut s2 = Sketch::new();
    s2.add_circle([120.0, 10.0], 6.0);
    let l2 = doc.add_feature(
        "Loft top",
        FeatureKind::Sketch {
            plane: Plane {
                origin: [0.0, 0.0, 25.0],
                ..Plane::XY
            },
            sketch: s2,
            on_body: None,
        },
    );
    doc.add_feature(
        "Transition",
        FeatureKind::Loft {
            from_sketch: l1,
            from_seed: [120.0, 10.0],
            to_sketch: l2,
            to_seed: [120.0, 10.0],
            body: BodyMode::NewBody,
        },
    );
    for f in &mut doc.features {
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            f.hidden = true;
        }
    }
    doc.camera.yaw = -1.1;
    doc.camera.pitch = 0.55;
    doc.camera.target = [65.0, 20.0, 10.0];
    doc.camera.distance = 200.0;

    let ed = Editor::new(doc.clone());
    let mut ok = true;
    for f in &doc.features {
        let st = &ed.model().status[&f.id];
        ok &= !st.is_failed();
        println!("{:<14} {:?}", f.name, st);
    }
    std::fs::write(&out, calibro_project::save_to_bytes(&doc).unwrap()).unwrap();
    println!("bodies: {}  -> {out}", ed.model().bodies.len());
    assert!(ok, "a feature failed");
}
