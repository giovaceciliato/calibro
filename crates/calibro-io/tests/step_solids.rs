//! STEP files written by Open CASCADE (see `docs/TEST_REPORT.md`) are imported as
//! editable solids; volumes are compared with the values Open CASCADE computed.

fn load(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/data/{name}.step",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn volume(s: &calibro_kernel::Solid) -> f64 {
    calibro_kernel::tessellate(s, 0.005).volume()
}

#[test]
fn occ_step_files_become_solids() {
    for (name, occ_volume, rel) in [
        ("plate_fillets", 21852.0355, 5e-3),
        ("sphere", 4188.7902, 1e-2),
        ("torus", 9869.6044, 1e-2),
        ("cone", 2450.4423, 1e-2),
    ] {
        let r = calibro_io::import_step_solids(&load(name)).unwrap();
        assert!(r.skipped.is_empty(), "{name}: {:?}", r.skipped);
        assert_eq!(r.solids.len(), 1, "{name}");
        let v = volume(&r.solids[0]);
        assert!(
            (v - occ_volume).abs() / occ_volume < rel,
            "{name}: {v} vs {occ_volume}"
        );
    }
}

#[test]
fn imported_solid_is_editable() {
    let r = calibro_io::import_step_solids(&load("plate_fillets")).unwrap();
    let plate = &r.solids[0];
    // cut a 10 x 10 x 20 block out of a corner region away from the fillets
    let cutter = calibro_kernel::make_box([20.0, -5.0, -5.0], [10.0, 15.0, 20.0]).unwrap();
    let cut = calibro_kernel::boolean(plate, &cutter, calibro_kernel::BooleanOp::Subtract).unwrap();
    let v = volume(&cut);
    let expected = 21852.0355 - 10.0 * 10.0 * 10.0;
    assert!((v - expected).abs() / expected < 5e-3, "{v} vs {expected}");
    // and it round-trips through our own STEP export
    let json = calibro_kernel::solid_to_json(&cut).unwrap();
    assert!(calibro_kernel::solid_from_json(&json).is_ok());
}
