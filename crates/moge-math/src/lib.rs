//! Column-major 4x4 matrices (WGSL layout), right-handed view, clip-space depth 0..1 (wgpu/Vulkan).

pub type Mat4 = [f32; 16];

pub const IDENTITY: Mat4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// `a * b` (apply `b` first, then `a`).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

pub fn transform(m: &Mat4, v: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (r, o) in out.iter_mut().enumerate() {
        *o = (0..4).map(|k| m[k * 4 + r] * v[k]).sum();
    }
    out
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy * 0.5).tan();
    let d = near - far;
    [f / aspect, 0., 0., 0., 0., f, 0., 0., 0., 0., far / d, -1., 0., 0., near * far / d, 0.]
}

pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let z = norm(sub(eye, target));
    let x = norm(cross(up, z));
    let y = cross(z, x);
    [x[0], y[0], z[0], 0., x[1], y[1], z[1], 0., x[2], y[2], z[2], 0., -dot(x, eye), -dot(y, eye), -dot(z, eye), 1.]
}

/// Orbit camera around `target`: drag to rotate, pinch/scroll to zoom.
#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        OrbitCamera { target: [0.0; 3], yaw: 0.6, pitch: 0.4, dist: 4.0 }
    }
}

impl OrbitCamera {
    pub const MIN_DIST: f32 = 0.5;
    pub const MAX_DIST: f32 = 500.0;
    const MAX_PITCH: f32 = 1.5;

    pub fn eye(&self) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [self.target[0] + self.dist * cp * sy, self.target[1] + self.dist * sp, self.target[2] + self.dist * cp * cy]
    }

    /// Rotate by a pointer delta in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-Self::MAX_PITCH, Self::MAX_PITCH);
    }

    /// Multiply the distance (factor < 1 zooms in).
    pub fn zoom(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.dist = (self.dist * factor).clamp(Self::MIN_DIST, Self::MAX_DIST);
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let far = (self.dist * 4.0).max(100.0);
        mul(&perspective(60f32.to_radians(), aspect.max(0.01), 0.05, far), &look_at(self.eye(), self.target, [0.0, 1.0, 0.0]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn identity_and_multiplication() {
        let m = look_at([1., 2., 3.], [0., 0., 0.], [0., 1., 0.]);
        assert_eq!(mul(&IDENTITY, &m), m);
        assert_eq!(mul(&m, &IDENTITY), m);
    }

    #[test]
    fn look_at_puts_eye_at_origin_and_target_in_front() {
        let eye = [3.0, 2.0, 5.0];
        let v = look_at(eye, [0., 0., 0.], [0., 1., 0.]);
        let e = transform(&v, [eye[0], eye[1], eye[2], 1.0]);
        assert!(close(e[0], 0.0) && close(e[1], 0.0) && close(e[2], 0.0));
        let d = (eye[0] * eye[0] + eye[1] * eye[1] + eye[2] * eye[2]).sqrt();
        let t = transform(&v, [0., 0., 0., 1.]);
        assert!(close(t[0], 0.0) && close(t[1], 0.0) && close(t[2], -d));
    }

    #[test]
    fn perspective_maps_near_and_far_to_0_and_1() {
        let p = perspective(1.0, 1.5, 0.1, 100.0);
        let n = transform(&p, [0., 0., -0.1, 1.]);
        let f = transform(&p, [0., 0., -100.0, 1.]);
        assert!(close(n[2] / n[3], 0.0));
        assert!(close(f[2] / f[3], 1.0));
    }

    #[test]
    fn orbit_target_is_centered_and_visible() {
        let cam = OrbitCamera::default();
        let c = transform(&cam.view_proj(2.0), [0., 0., 0., 1.]);
        assert!(close(c[0] / c[3], 0.0) && close(c[1] / c[3], 0.0));
        let z = c[2] / c[3];
        assert!(z > 0.0 && z < 1.0);
    }

    #[test]
    fn orbit_and_zoom_are_clamped() {
        let mut cam = OrbitCamera::default();
        cam.orbit(0.0, 1e6);
        assert!(cam.pitch <= 1.5);
        cam.orbit(0.0, -1e6);
        assert!(cam.pitch >= -1.5);
        cam.zoom(0.0);
        cam.zoom(f32::NAN);
        assert_eq!(cam.dist, 4.0);
        cam.zoom(1e-9);
        assert_eq!(cam.dist, OrbitCamera::MIN_DIST);
        cam.zoom(1e12);
        assert_eq!(cam.dist, OrbitCamera::MAX_DIST);
    }
}
