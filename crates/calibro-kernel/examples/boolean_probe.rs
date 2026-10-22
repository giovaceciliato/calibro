//! Probes which boolean combinations the truck kernel handles (regression harness):
//! `cargo run --release -p calibro-kernel --example boolean_probe`
use calibro_kernel::*;
fn main() {
    let bx = || make_box([-12.0, -12.0, -12.0], [24.0, 24.0, 24.0]).unwrap();
    let cases: Vec<(&str, Solid, Solid)> = vec![
        (
            "box - cylinder (through)",
            bx(),
            make_cylinder([0.0, 0.0, -20.0], 5.0, 40.0).unwrap(),
        ),
        (
            "box - cylinder (blind)",
            bx(),
            make_cylinder([0.0, 0.0, 0.0], 5.0, 20.0).unwrap(),
        ),
        ("box ∩ sphere", bx(), make_sphere([0.0; 3], 15.0).unwrap()),
        ("box ∩ sphere(rev half-disc)", bx(), {
            let mut s = calibro_sketch::Sketch::new();
            let c = s.add_point(0.0, 0.0);
            let a = s.add_point(0.0, -15.0);
            let b = s.add_point(0.0, 15.0);
            s.add_arc(c, a, b);
            s.add_line(b, a);
            let p = calibro_sketch::find_profiles(&s);
            revolve(&Plane::XZ, &p, [0.0, 0.0], [0.0, 1.0], 360.0)
                .unwrap()
                .remove(0)
        }),
        (
            "cyl ∪ cyl (cross)",
            make_cylinder([0.0, 0.0, -20.0], 5.0, 40.0).unwrap(),
            {
                let c = make_cylinder([0.0, 0.0, -20.0], 4.0, 40.0).unwrap();
                transform(
                    &c,
                    &Transform {
                        rotate_axis: [1.0, 0.0, 0.0],
                        rotate_deg: 90.0,
                        ..Default::default()
                    },
                )
                .unwrap()
            },
        ),
        (
            "box - box (shifted, no coplanar)",
            bx(),
            make_box([5.0, 5.0, 5.0], [20.0, 20.0, 20.0]).unwrap(),
        ),
    ];
    for (name, a, b) in cases {
        for op in [BooleanOp::Union, BooleanOp::Subtract, BooleanOp::Intersect] {
            let r = boolean(&a, &b, op);
            let s = match &r {
                Ok(s) => format!("OK  vol={:.1}", tessellate(s, 0.02).volume()),
                Err(e) => format!("ERR {}", e.to_string().chars().take(60).collect::<String>()),
            };
            println!("{name:<34} {op:<9?} {s}");
        }
    }
}
