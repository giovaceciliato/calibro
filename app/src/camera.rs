//! Orbit camera (Z-up), projection, and screen <-> world conversions.

use calibro_core::CameraState;
use glam::{DMat4, DVec3, DVec4, Mat4, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

pub struct CameraView {
    pub eye: DVec3,
    pub view: DMat4,
    pub view_proj: DMat4,
    pub inv_view_proj: DMat4,
    pub viewport: Vec2,
    pub state: CameraState,
}

/// Right-handed view matrix (camera looks down -Z).
fn look_at_rh(eye: DVec3, center: DVec3, up: DVec3) -> DMat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    DMat4::from_cols(
        DVec4::new(s.x, u.x, -f.x, 0.0),
        DVec4::new(s.y, u.y, -f.y, 0.0),
        DVec4::new(s.z, u.z, -f.z, 0.0),
        DVec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
    )
}

/// Right-handed perspective projection with a 0..1 depth range (wgpu convention).
fn perspective_rh(fov_y: f64, aspect: f64, near: f64, far: f64) -> DMat4 {
    let f = 1.0 / (0.5 * fov_y).tan();
    let r = far / (near - far);
    DMat4::from_cols(
        DVec4::new(f / aspect, 0.0, 0.0, 0.0),
        DVec4::new(0.0, f, 0.0, 0.0),
        DVec4::new(0.0, 0.0, r, -1.0),
        DVec4::new(0.0, 0.0, r * near, 0.0),
    )
}

/// Right-handed orthographic projection with a 0..1 depth range.
fn orthographic_rh(l: f64, r: f64, b: f64, t: f64, near: f64, far: f64) -> DMat4 {
    let (w, h, d) = (1.0 / (r - l), 1.0 / (t - b), 1.0 / (near - far));
    DMat4::from_cols(
        DVec4::new(2.0 * w, 0.0, 0.0, 0.0),
        DVec4::new(0.0, 2.0 * h, 0.0, 0.0),
        DVec4::new(0.0, 0.0, d, 0.0),
        DVec4::new(-(l + r) * w, -(t + b) * h, d * near, 1.0),
    )
}

/// Just short of straight up/down so the view direction never becomes parallel to Z.
const MAX_PITCH: f64 = std::f64::consts::FRAC_PI_2 - 1e-4;

pub fn eye_dir(state: &CameraState) -> DVec3 {
    let (sy, cy) = state.yaw.sin_cos();
    let (sp, cp) = state.pitch.sin_cos();
    DVec3::new(cp * cy, cp * sy, sp)
}

impl CameraView {
    pub fn new(state: &CameraState, viewport: Vec2) -> Self {
        let target = DVec3::from(state.target);
        let dir = eye_dir(state);
        let eye = target + dir * state.distance;
        // keep "up" well defined when looking straight down/up
        let up = if dir.z.abs() > 0.999 {
            DVec3::new(-state.yaw.cos(), -state.yaw.sin(), 0.0) * dir.z.signum()
        } else {
            DVec3::Z
        };
        let view = look_at_rh(eye, target, up);
        let aspect = (viewport.x.max(1.0) / viewport.y.max(1.0)) as f64;
        let far = state.distance * 50.0 + 10_000.0;
        let proj = if state.orthographic {
            let h = state.distance * (state.fov_deg.to_radians() * 0.5).tan();
            orthographic_rh(-h * aspect, h * aspect, -h, h, -far, far)
        } else {
            let near = (state.distance * 0.002).max(0.01);
            perspective_rh(state.fov_deg.to_radians(), aspect, near, far)
        };
        let view_proj = proj * view;
        Self {
            eye,
            view,
            view_proj,
            inv_view_proj: view_proj.inverse(),
            viewport,
            state: state.clone(),
        }
    }

    pub fn view_proj_f32(&self) -> Mat4 {
        self.view_proj.as_mat4()
    }

    /// World point -> viewport pixel coordinates (origin top-left). None if behind camera.
    pub fn project(&self, p: DVec3) -> Option<Vec2> {
        let c = self.view_proj * DVec4::new(p.x, p.y, p.z, 1.0);
        if c.w <= 1e-9 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some(Vec2::new(
            ((ndc.x * 0.5 + 0.5) * self.viewport.x as f64) as f32,
            ((0.5 - ndc.y * 0.5) * self.viewport.y as f64) as f32,
        ))
    }

    /// Ray through a viewport pixel.
    pub fn ray(&self, px: Vec2) -> Ray {
        let x = (px.x / self.viewport.x.max(1.0)) as f64 * 2.0 - 1.0;
        let y = 1.0 - (px.y / self.viewport.y.max(1.0)) as f64 * 2.0;
        let near = self.inv_view_proj * DVec4::new(x, y, 0.0, 1.0);
        let far = self.inv_view_proj * DVec4::new(x, y, 1.0, 1.0);
        let n = near.truncate() / near.w;
        let f = far.truncate() / far.w;
        Ray {
            origin: n,
            dir: (f - n).normalize(),
        }
    }

    /// World-space size of one pixel at point `p`.
    pub fn pixel_size_at(&self, p: DVec3) -> f64 {
        let h = if self.state.orthographic {
            2.0 * self.state.distance * (self.state.fov_deg.to_radians() * 0.5).tan()
        } else {
            let d = (p - self.eye).length().max(1e-6);
            2.0 * d * (self.state.fov_deg.to_radians() * 0.5).tan()
        };
        h / self.viewport.y.max(1.0) as f64
    }
}

pub fn orbit(state: &mut CameraState, dx: f32, dy: f32) {
    state.yaw -= dx as f64 * 0.008;
    state.pitch = (state.pitch + dy as f64 * 0.008).clamp(-MAX_PITCH, MAX_PITCH);
}

pub fn pan(state: &mut CameraState, view: &CameraView, dx: f32, dy: f32) {
    let px = view.pixel_size_at(DVec3::from(state.target));
    let right = DVec3::new(view.view.x_axis.x, view.view.y_axis.x, view.view.z_axis.x);
    let up = DVec3::new(view.view.x_axis.y, view.view.y_axis.y, view.view.z_axis.y);
    let t = DVec3::from(state.target) - right * dx as f64 * px + up * dy as f64 * px;
    state.target = t.to_array();
}

/// Zooms towards the point under the cursor.
pub fn zoom(state: &mut CameraState, view: &CameraView, factor: f64, cursor: Option<Vec2>) {
    let old = state.distance;
    state.distance = (state.distance * factor).clamp(0.05, 1.0e6);
    if let Some(c) = cursor {
        // move target along the cursor ray so the point under the cursor stays put
        let ray = view.ray(c);
        let target = DVec3::from(state.target);
        let n = (view.eye - target).normalize();
        let denom = ray.dir.dot(n);
        if denom.abs() > 1e-9 {
            let t = (target - ray.origin).dot(n) / denom;
            let hit = ray.origin + ray.dir * t;
            let k = 1.0 - state.distance / old;
            state.target = (target + (hit - target) * k).to_array();
        }
    }
}

/// Standard views.
pub fn set_view(state: &mut CameraState, name: &str) {
    let (yaw, pitch) = match name {
        "Top" => (
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2 - 1e-4,
        ),
        "Bottom" => (
            -std::f64::consts::FRAC_PI_2,
            -std::f64::consts::FRAC_PI_2 + 1e-4,
        ),
        "Front" => (-std::f64::consts::FRAC_PI_2, 0.0),
        "Back" => (std::f64::consts::FRAC_PI_2, 0.0),
        "Right" => (0.0, 0.0),
        "Left" => (std::f64::consts::PI, 0.0),
        _ => (-0.9, 0.55),
    };
    state.yaw = yaw;
    state.pitch = pitch;
}

/// Points the camera along a plane's normal (used when entering a sketch).
pub fn look_at_plane(state: &mut CameraState, normal: [f64; 3], origin: [f64; 3]) {
    let n = DVec3::from(normal).normalize();
    state.pitch = n.z.clamp(-1.0, 1.0).asin().clamp(-MAX_PITCH, MAX_PITCH);
    if n.x.abs() + n.y.abs() > 1e-9 {
        state.yaw = n.y.atan2(n.x);
    } else {
        state.yaw = -std::f64::consts::FRAC_PI_2;
    }
    state.target = origin;
}

/// Frames a bounding box.
pub fn fit(state: &mut CameraState, min: DVec3, max: DVec3) {
    let c = (min + max) * 0.5;
    let r = ((max - min).length() * 0.5).max(5.0);
    state.target = c.to_array();
    state.distance = r / (state.fov_deg.to_radians() * 0.5).sin() * 1.15;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_ray_agree() {
        let s = CameraState::default();
        let v = CameraView::new(&s, Vec2::new(800.0, 600.0));
        let p = DVec3::new(10.0, -5.0, 3.0);
        let px = v.project(p).unwrap();
        let r = v.ray(px);
        // distance from p to the ray is ~0
        let d = (p - r.origin).cross(r.dir).length();
        assert!(d < 1e-4, "{d}"); // 0.1 µm at mm scale
                                  // target projects to the center
        let c = v.project(DVec3::from(s.target)).unwrap();
        assert!((c - Vec2::new(400.0, 300.0)).length() < 1e-3);
    }

    #[test]
    fn ortho_and_top_view() {
        let mut s = CameraState {
            orthographic: true,
            ..Default::default()
        };
        set_view(&mut s, "Top");
        let v = CameraView::new(&s, Vec2::new(800.0, 600.0));
        let a = v.project(DVec3::new(10.0, 0.0, 0.0)).unwrap();
        let b = v.project(DVec3::new(0.0, 10.0, 0.0)).unwrap();
        assert!(a.x > 400.0 && (a.y - 300.0).abs() < 1e-2, "{a:?}"); // +X to the right
        assert!(b.y < 300.0 && (b.x - 400.0).abs() < 1e-2, "{b:?}"); // +Y up
    }
}
