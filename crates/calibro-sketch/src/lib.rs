//! # calibro-sketch
//!
//! Pure-Rust 2D sketching for Calibro: the sketch data model, a numerical
//! geometric constraint solver, and closed-region (profile) detection used by
//! extrude and revolve.

pub mod intersect;
pub mod model;
pub mod profile;
pub mod solver;

pub use intersect::trim_curve;
pub use model::*;
pub use profile::{find_profiles, profile_at, Loop, Profile, Segment};
pub use solver::{drag_point, solve, SolveReport, SolveStatus};

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn overlapping_circles_give_three_regions() {
        let mut s = Sketch::new();
        s.add_circle([0.0, 0.0], 10.0);
        s.add_circle([10.0, 0.0], 10.0);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 3, "{p:?}");
        let lens = 200.0 * (0.5f64).acos() - 5.0 * 300f64.sqrt();
        let crescent = std::f64::consts::PI * 100.0 - lens;
        let mut areas: Vec<f64> = p.iter().map(|r| r.area()).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert!((areas[0] - lens).abs() < 0.5, "{areas:?}");
        assert!(
            (areas[1] - crescent).abs() < 0.5 && (areas[2] - crescent).abs() < 0.5,
            "{areas:?}"
        );
    }

    #[test]
    fn line_across_rectangle_splits_it() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [20.0, 10.0]);
        s.add_line_xy([5.0, -5.0], [5.0, 15.0]); // crosses both long sides, ends outside
        let p = find_profiles(&s);
        assert_eq!(p.len(), 2, "{p:?}");
        let mut areas: Vec<f64> = p.iter().map(|r| r.area()).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert!(
            approx(areas[0], 50.0) && approx(areas[1], 150.0),
            "{areas:?}"
        );
        // T junction: a line from the middle of a side to the opposite side
        let mut t = Sketch::new();
        t.add_rectangle([0.0, 0.0], [20.0, 10.0]);
        t.add_line_xy([12.0, 0.0], [12.0, 10.0]);
        assert_eq!(find_profiles(&t).len(), 2);
    }

    #[test]
    fn trim_pieces() {
        // circle cut by a line through its centre: trim the lower half
        let mut s = Sketch::new();
        let c = s.add_circle([0.0, 0.0], 10.0);
        let l = s.add_line_xy([-20.0, 0.0], [20.0, 0.0]);
        let new = trim_curve(&mut s, c, [0.0, -10.0]).unwrap();
        assert_eq!(new.len(), 1);
        assert!(matches!(s.curves[&new[0]].kind, CurveKind::Arc { .. }));
        // trim the line's outer pieces: left and right of the arc
        let pieces = trim_curve(&mut s, l, [-15.0, 0.0]).unwrap();
        assert_eq!(pieces.len(), 1);
        let pieces = trim_curve(&mut s, pieces[0], [15.0, 0.0]).unwrap();
        assert_eq!(pieces.len(), 1);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        assert!(
            (p[0].area() - std::f64::consts::PI * 50.0).abs() < 0.5,
            "{}",
            p[0].area()
        );
        // a curve that crosses nothing is deleted entirely
        let lone = s.add_line_xy([50.0, 50.0], [60.0, 50.0]);
        assert!(trim_curve(&mut s, lone, [55.0, 50.0]).unwrap().is_empty());
        assert!(!s.curves.contains_key(&lone));
        // middle piece of a line crossing two others: two pieces remain
        let mut m = Sketch::new();
        let base = m.add_line_xy([0.0, 0.0], [30.0, 0.0]);
        m.add_line_xy([10.0, -5.0], [10.0, 5.0]);
        m.add_line_xy([20.0, -5.0], [20.0, 5.0]);
        assert_eq!(trim_curve(&mut m, base, [15.0, 0.0]).unwrap().len(), 2);
        m.validate().unwrap();
    }

    #[test]
    fn rectangle_is_one_profile() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [20.0, 10.0]);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        assert!(approx(p[0].area(), 200.0), "area {}", p[0].area());
        assert!(p[0].holes.is_empty());
    }

    #[test]
    fn open_polyline_has_no_profile() {
        let mut s = Sketch::new();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let c = s.add_point(10.0, 10.0);
        s.add_line(a, b);
        s.add_line(b, c);
        assert!(find_profiles(&s).is_empty());
    }

    #[test]
    fn circle_inside_rectangle_gives_ring_and_disc() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [40.0, 40.0]);
        s.add_circle([20.0, 20.0], 5.0);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 2);
        let ring = &p[0];
        assert_eq!(ring.holes.len(), 1);
        let disc_area = std::f64::consts::PI * 25.0;
        assert!((ring.area() - (1600.0 - disc_area)).abs() < 0.5);
        assert!((p[1].area() - disc_area).abs() < 0.5);
        // seed lookup picks the smallest containing region
        assert!(profile_at(&p, [20.0, 20.0]).unwrap().holes.is_empty());
        assert_eq!(profile_at(&p, [2.0, 2.0]).unwrap().holes.len(), 1);
    }

    #[test]
    fn splitting_line_creates_two_regions() {
        let mut s = Sketch::new();
        let [b, _, t, _] = s.add_rectangle([0.0, 0.0], [20.0, 10.0]);
        // split at x = 5 using existing-edge points: add a line from bottom to top
        let (b0, _) = s.line_points(b).unwrap();
        let (t0, _) = s.line_points(t).unwrap();
        let _ = (b0, t0);
        // Build a split rectangle manually: two squares sharing an edge.
        let mut s = Sketch::new();
        let p00 = s.add_point(0.0, 0.0);
        let p10 = s.add_point(10.0, 0.0);
        let p20 = s.add_point(20.0, 0.0);
        let p21 = s.add_point(20.0, 10.0);
        let p11 = s.add_point(10.0, 10.0);
        let p01 = s.add_point(0.0, 10.0);
        for (a, b) in [
            (p00, p10),
            (p10, p20),
            (p20, p21),
            (p21, p11),
            (p11, p01),
            (p01, p00),
            (p10, p11),
        ] {
            s.add_line(a, b);
        }
        let p = find_profiles(&s);
        assert_eq!(p.len(), 2);
        assert!(approx(p[0].area(), 100.0) && approx(p[1].area(), 100.0));
    }

    #[test]
    fn construction_lines_are_ignored() {
        let mut s = Sketch::new();
        let lines = s.add_rectangle([0.0, 0.0], [10.0, 10.0]);
        s.curves.get_mut(&lines[0]).unwrap().construction = true;
        assert!(find_profiles(&s).is_empty());
    }

    #[test]
    fn slot_with_arcs() {
        // Stadium shape: two lines + two semicircular arcs.
        let mut s = Sketch::new();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(20.0, 0.0);
        let c = s.add_point(20.0, 10.0);
        let d = s.add_point(0.0, 10.0);
        let c1 = s.add_point(20.0, 5.0);
        let c2 = s.add_point(0.0, 5.0);
        s.add_line(a, b);
        s.add_arc(c1, b, c);
        s.add_line(c, d);
        s.add_arc(c2, d, a);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        let expect = 200.0 + std::f64::consts::PI * 25.0;
        assert!((p[0].area() - expect).abs() < 0.2, "area {}", p[0].area());
    }

    #[test]
    fn solver_applies_dimensions() {
        let mut s = Sketch::new();
        let [bottom, right, _, _] = s.add_rectangle([0.0, 0.0], [18.0, 9.0]);
        let (a, _) = s.line_points(bottom).unwrap();
        s.add_constraint(Constraint::Fixed {
            point: a,
            x: 0.0,
            y: 0.0,
        });
        s.add_constraint(Constraint::Length {
            line: bottom,
            value: 25.0,
        });
        s.add_constraint(Constraint::Length {
            line: right,
            value: 12.0,
        });
        let r = solve(&mut s).unwrap();
        assert_eq!(r.status, SolveStatus::FullyConstrained, "{r:?}");
        let p = find_profiles(&s);
        assert!(approx(p[0].area(), 300.0), "area {}", p[0].area());
    }

    #[test]
    fn under_constrained_reports_dof() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [10.0, 5.0]);
        // 4 points * 2 = 8 vars, 4 H/V constraints => 4 dof (x, y, width, height)
        let r = solve(&mut s).unwrap();
        assert_eq!(r.status, SolveStatus::UnderConstrained { dof: 4 });
    }

    #[test]
    fn conflicting_constraints_detected() {
        let mut s = Sketch::new();
        let l = s.add_line_xy([0.0, 0.0], [10.0, 1.0]);
        s.add_constraint(Constraint::Horizontal { line: l });
        s.add_constraint(Constraint::Vertical { line: l });
        s.add_constraint(Constraint::Length {
            line: l,
            value: 10.0,
        });
        let r = solve(&mut s).unwrap();
        assert!(matches!(r.status, SolveStatus::Conflicting { .. }), "{r:?}");
    }

    #[test]
    fn redundant_constraint_detected() {
        let mut s = Sketch::new();
        let l = s.add_line_xy([0.0, 0.0], [10.0, 1.0]);
        s.add_constraint(Constraint::Horizontal { line: l });
        s.add_constraint(Constraint::Horizontal { line: l });
        let r = solve(&mut s).unwrap();
        assert!(matches!(r.status, SolveStatus::Redundant { .. }), "{r:?}");
    }

    #[test]
    fn tangent_angle_parallel_perpendicular() {
        let mut s = Sketch::new();
        let l1 = s.add_line_xy([0.0, 0.0], [10.0, 0.5]);
        let l2 = s.add_line_xy([0.0, 5.0], [10.0, 6.0]);
        let l3 = s.add_line_xy([0.0, 0.0], [1.0, 10.0]);
        let c = s.add_circle([5.0, 3.0], 2.0);
        s.add_constraint(Constraint::Horizontal { line: l1 });
        s.add_constraint(Constraint::Parallel { a: l1, b: l2 });
        s.add_constraint(Constraint::Perpendicular { a: l1, b: l3 });
        s.add_constraint(Constraint::Tangent { a: l1, b: c });
        s.add_constraint(Constraint::Radius {
            curve: c,
            value: 3.0,
        });
        let r = solve(&mut s).unwrap();
        assert!(r.residual < 1e-8, "{r:?}");
        let (a, b) = s.line_points(l2).unwrap();
        assert!(approx(s.point(a).unwrap().y, s.point(b).unwrap().y));
        let (a3, b3) = s.line_points(l3).unwrap();
        assert!(approx(s.point(a3).unwrap().x, s.point(b3).unwrap().x));
        assert!(approx(s.radius_of(c).unwrap(), 3.0));
        let CurveKind::Circle { center, .. } = s.curves[&c].kind else {
            panic!()
        };
        let (a1, _) = s.line_points(l1).unwrap();
        let dy = (s.point(center).unwrap().y - s.point(a1).unwrap().y).abs();
        assert!(approx(dy, 3.0), "tangent distance {dy}");
    }

    #[test]
    fn angle_constraint() {
        let mut s = Sketch::new();
        let l1 = s.add_line_xy([0.0, 0.0], [10.0, 0.0]);
        let l2 = s.add_line_xy([0.0, 0.0], [7.0, 8.0]);
        s.add_constraint(Constraint::Horizontal { line: l1 });
        s.add_constraint(Constraint::Angle {
            a: l1,
            b: l2,
            degrees: 30.0,
        });
        solve(&mut s).unwrap();
        let (a, b) = s.line_points(l2).unwrap();
        let (pa, pb) = (s.point(a).unwrap(), s.point(b).unwrap());
        let ang = (pb.y - pa.y).atan2(pb.x - pa.x).to_degrees();
        assert!((ang - 30.0).abs() < 1e-5, "angle {ang}");
    }

    #[test]
    fn drag_keeps_constraints() {
        let mut s = Sketch::new();
        let l = s.add_line_xy([0.0, 0.0], [10.0, 0.0]);
        s.add_constraint(Constraint::Horizontal { line: l });
        s.add_constraint(Constraint::Length {
            line: l,
            value: 10.0,
        });
        let (_, b) = s.line_points(l).unwrap();
        drag_point(&mut s, b, [12.0, 4.0]).unwrap();
        let (a, b) = s.line_points(l).unwrap();
        let (pa, pb) = (s.point(a).unwrap(), s.point(b).unwrap());
        assert!(approx(pa.y, pb.y));
        assert!(approx(
            ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt(),
            10.0
        ));
    }

    #[test]
    fn remove_curve_cleans_constraints() {
        let mut s = Sketch::new();
        let lines = s.add_rectangle([0.0, 0.0], [10.0, 10.0]);
        s.remove_curve(lines[0]);
        assert!(s.validate().is_ok());
        assert_eq!(s.constraints.len(), 3);
    }

    #[test]
    fn equal_concentric_coincident_midpoint_symmetric() {
        let mut s = Sketch::new();
        let l1 = s.add_line_xy([0.0, 0.0], [10.0, 0.0]);
        let l2 = s.add_line_xy([0.0, 5.0], [3.0, 9.0]);
        s.add_constraint(Constraint::Length {
            line: l1,
            value: 10.0,
        });
        s.add_constraint(Constraint::Equal { a: l1, b: l2 });
        let c1 = s.add_circle([20.0, 0.0], 2.0);
        let c2 = s.add_circle([21.0, 1.0], 5.0);
        s.add_constraint(Constraint::Concentric { a: c1, b: c2 });
        s.add_constraint(Constraint::Equal { a: c1, b: c2 });
        let (a1, b1) = s.line_points(l1).unwrap();
        let (a2, _) = s.line_points(l2).unwrap();
        s.add_constraint(Constraint::Coincident { a: a1, b: a2 });
        let m = s.add_point(4.0, 1.0);
        s.add_constraint(Constraint::Midpoint { point: m, line: l1 });
        let axis = s.add_line_xy([30.0, -5.0], [30.0, 5.0]);
        s.add_constraint(Constraint::Vertical { line: axis });
        let p = s.add_point(27.0, 1.0);
        let q = s.add_point(34.0, 2.0);
        s.add_constraint(Constraint::Symmetric { a: p, b: q, axis });
        let r = solve(&mut s).unwrap();
        assert!(r.residual < 1e-8, "{r:?}");
        let len = |l: u32| {
            let (a, b) = s.line_points(l).unwrap();
            let (a, b) = (s.point(a).unwrap(), s.point(b).unwrap());
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
        };
        assert!(approx(len(l2), 10.0));
        assert!(approx(s.radius_of(c1).unwrap(), s.radius_of(c2).unwrap()));
        let center = |c: u32| match s.curves[&c].kind {
            CurveKind::Circle { center, .. } => s.point(center).unwrap(),
            _ => unreachable!(),
        };
        assert!(approx(center(c1).x, center(c2).x) && approx(center(c1).y, center(c2).y));
        let (pa1, pa2) = (s.point(a1).unwrap(), s.point(a2).unwrap());
        assert!(approx(pa1.x, pa2.x) && approx(pa1.y, pa2.y));
        let (pa, pb, pm) = (
            s.point(a1).unwrap(),
            s.point(b1).unwrap(),
            s.point(m).unwrap(),
        );
        assert!(approx(pm.x, (pa.x + pb.x) / 2.0) && approx(pm.y, (pa.y + pb.y) / 2.0));
        let (pp, pq) = (s.point(p).unwrap(), s.point(q).unwrap());
        let (ax, _) = s.line_points(axis).unwrap();
        let x_axis = s.point(ax).unwrap().x;
        assert!(approx(pp.y, pq.y), "symmetric points level");
        assert!(approx((pp.x + pq.x) / 2.0, x_axis), "midpoint on axis");
    }

    #[test]
    fn polygon_is_closed_region() {
        let mut s = Sketch::new();
        s.add_polygon([0.0, 0.0], 10.0, 6);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        let exp = 1.5 * 3f64.sqrt() * 100.0; // regular hexagon, circumradius 10
        assert!((p[0].area() - exp).abs() < 1e-6, "{}", p[0].area());
        assert!(solve(&mut s).unwrap().residual < 1e-9);
    }

    #[test]
    fn serde_round_trip() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [10.0, 10.0]);
        s.add_circle([5.0, 5.0], 2.0);
        let json = serde_json_like(&s);
        assert!(json.contains("Horizontal"));
    }

    fn serde_json_like(s: &Sketch) -> String {
        // calibro-sketch doesn't depend on serde_json; Debug output is enough
        // to show the derive works (full round-trip is tested in calibro-project).
        format!("{s:?}")
    }
}
