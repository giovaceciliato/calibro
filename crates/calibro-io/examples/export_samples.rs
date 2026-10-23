//! Writes a sample part in every export format, for independent verification:
//! `cargo run -p calibro-io --example export_samples -- <output-dir>`
use calibro_core::kernel::Plane;
use calibro_core::sketch::Sketch;
use calibro_core::*;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "samples".into()));
    std::fs::create_dir_all(&out).unwrap();
    let mut doc = Document::new();
    doc.metadata.title = "Sample bracket".into();
    let mut s = Sketch::new();
    s.add_rectangle([0.0, 0.0], [60.0, 40.0]);
    s.add_circle([15.0, 20.0], 6.0);
    let sk = doc.add_feature(
        "Base sketch",
        FeatureKind::Sketch {
            plane: Plane::XY,
            sketch: s,
            on_body: None,
            attach: None,
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
    let body = BodyId {
        feature: plate,
        index: 0,
    };
    doc.add_feature(
        "Boss",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [42.0, 20.0, 4.0],
                radius: 9.0,
                height: 16.0,
            },
            body: BodyMode::Join { target: body },
        },
    );
    doc.add_feature(
        "Bore",
        FeatureKind::Primitive {
            primitive: Primitive::Cylinder {
                base: [42.0, 20.0, -1.0],
                radius: 4.0,
                height: 30.0,
            },
            body: BodyMode::Cut { target: body },
        },
    );
    doc.body_props_mut(body).material = material_presets()[3].clone();
    let ed = Editor::new(doc.clone());
    for (id, st) in &ed.model().status {
        println!("feature {id}: {st:?}");
    }
    let m = &ed.model().bodies[&body].mesh;
    println!(
        "volume_mm3={:.4} area_mm2={:.4} triangles={}",
        m.volume(),
        m.surface_area(),
        m.triangle_count()
    );
    let bodies = calibro_io::collect_bodies(&doc, ed.model(), None);
    std::fs::write(
        out.join("sample.stl"),
        calibro_io::export_stl_binary(&bodies).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("sample_ascii.stl"),
        calibro_io::export_stl_ascii(&bodies).unwrap(),
    )
    .unwrap();
    let (obj, mtl) = calibro_io::export_obj(&bodies, "sample.mtl").unwrap();
    std::fs::write(out.join("sample.obj"), obj).unwrap();
    std::fs::write(out.join("sample.mtl"), mtl).unwrap();
    std::fs::write(
        out.join("sample.glb"),
        calibro_io::export_glb(&bodies).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("sample.3mf"),
        calibro_io::export_3mf(&bodies).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("sample.step"),
        calibro_io::export_step(&bodies, "sample.step").unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("sample.calibro"),
        calibro_project::save_to_bytes(&doc).unwrap(),
    )
    .unwrap();
    println!("written to {}", out.display());
}
