//! Viewport manipulation math: handle picking, axis dragging, snapping, move/scale/rotate.
use moge_math::{transform, Mat4};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Scale,
    Rotate,
}

pub fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 { (v / step).round() * step } else { v }
}

/// Pixel position of a world point (None when it is behind the camera).
pub fn project(vp: &Mat4, p: [f32; 3], w: f32, h: f32) -> Option<(f32, f32)> {
    let c = transform(vp, [p[0], p[1], p[2], 1.0]);
    if c[3] <= 1e-5 {
        return None;
    }
    Some(((c[0] / c[3] + 1.0) * 0.5 * w, (1.0 - c[1] / c[3]) * 0.5 * h))
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-6 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    ((p.0 - (a.0 + t * dx)).powi(2) + (p.1 - (a.1 + t * dy)).powi(2)).sqrt()
}

/// Index of the axis handle nearest to the pointer within `radius` pixels.
pub fn pick_handle(vp: &Mat4, origin: [f32; 3], axes: &[[f32; 3]; 3], len: f32, px: f32, py: f32, w: f32, h: f32, radius: f32) -> Option<usize> {
    let a = project(vp, origin, w, h)?;
    let mut best: Option<(usize, f32)> = None;
    for (i, ax) in axes.iter().enumerate() {
        let end = [origin[0] + ax[0] * len, origin[1] + ax[1] * len, origin[2] + ax[2] * len];
        let Some(b) = project(vp, end, w, h) else { continue };
        let d = seg_dist((px, py), a, b);
        if d <= radius && best.map(|x| d < x.1).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|b| b.0)
}

/// World distance along `axis` that corresponds to a pointer drag of (dx, dy) pixels.
pub fn axis_amount(vp: &Mat4, origin: [f32; 3], axis: [f32; 3], dx: f32, dy: f32, w: f32, h: f32) -> f32 {
    let p1 = [origin[0] + axis[0], origin[1] + axis[1], origin[2] + axis[2]];
    let (Some(a), Some(b)) = (project(vp, origin, w, h), project(vp, p1, w, h)) else { return 0.0 };
    let s = (b.0 - a.0, b.1 - a.1);
    let l2 = s.0 * s.0 + s.1 * s.1;
    if l2 < 1e-4 { 0.0 } else { (dx * s.0 + dy * s.1) / l2 }
}

pub fn move_by(pos: [f32; 3], axis: [f32; 3], amount: f32, step: f32) -> [f32; 3] {
    let a = snap(amount, step);
    [pos[0] + axis[0] * a, pos[1] + axis[1] * a, pos[2] + axis[2] * a]
}

pub fn scale_by(size: [f32; 3], axis_index: usize, amount: f32, step: f32) -> [f32; 3] {
    let mut s = size;
    s[axis_index] = snap(size[axis_index] + amount, step).max(0.05);
    s
}

/// Rotates `rot` (row-major) by `degrees` about the world `axis`.
pub fn rotate_by(rot: &[f32; 9], axis: [f32; 3], degrees: f32, step: f32) -> [f32; 9] {
    let a = snap(degrees, step).to_radians();
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-9);
    let (x, y, z) = (axis[0] / l, axis[1] / l, axis[2] / l);
    let (s, c) = a.sin_cos();
    let t = 1.0 - c;
    let r = [t * x * x + c, t * x * y - s * z, t * x * z + s * y, t * x * y + s * z, t * y * y + c, t * y * z - s * x, t * x * z - s * y, t * y * z + s * x, t * z * z + c];
    moge_math::mat3_mul(&r, rot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moge_math::{euler_yxz_to_rot, OrbitCamera};

    fn cam() -> (OrbitCamera, Mat4) {
        let c = OrbitCamera { target: [0.0; 3], yaw: 0.0, pitch: 0.0, dist: 10.0 };
        let vp = c.view_proj(2.0);
        (c, vp)
    }

    #[test]
    fn snapping_and_projection() {
        assert_eq!(snap(1.26, 0.5), 1.5);
        assert_eq!(snap(1.26, 0.0), 1.26);
        let (_, vp) = cam();
        let (x, y) = project(&vp, [0.0; 3], 1000.0, 500.0).unwrap();
        assert!((x - 500.0).abs() < 0.5 && (y - 250.0).abs() < 0.5);
        assert!(project(&vp, [0.0, 0.0, 50.0], 1000.0, 500.0).is_none(), "behind the camera");
    }

    #[test]
    fn handle_pick_and_axis_drag() {
        let (_, vp) = cam();
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let end = project(&vp, [3.0, 0.0, 0.0], 1000.0, 500.0).unwrap();
        assert_eq!(pick_handle(&vp, [0.0; 3], &axes, 3.0, end.0 - 5.0, end.1 + 3.0, 1000.0, 500.0, 20.0), Some(0));
        let up = project(&vp, [0.0, 3.0, 0.0], 1000.0, 500.0).unwrap();
        assert_eq!(pick_handle(&vp, [0.0; 3], &axes, 3.0, up.0, up.1, 1000.0, 500.0, 20.0), Some(1));
        assert_eq!(pick_handle(&vp, [0.0; 3], &axes, 3.0, 900.0, 450.0, 1000.0, 500.0, 20.0), None);
        let px_per_unit = project(&vp, [1.0, 0.0, 0.0], 1000.0, 500.0).unwrap().0 - 500.0;
        let amt = axis_amount(&vp, [0.0; 3], [1.0, 0.0, 0.0], px_per_unit * 2.0, 0.0, 1000.0, 500.0);
        assert!((amt - 2.0).abs() < 0.05, "{amt}");
        assert_eq!(axis_amount(&vp, [0.0; 3], [0.0, 0.0, 1.0], 100.0, 0.0, 1000.0, 500.0), 0.0, "axis pointing at the camera cannot be dragged");
    }

    #[test]
    fn move_scale_rotate() {
        assert_eq!(move_by([1.0, 2.0, 3.0], [0.0, 1.0, 0.0], 2.3, 1.0), [1.0, 4.0, 3.0]);
        assert_eq!(scale_by([4.0, 1.0, 2.0], 0, -10.0, 0.5)[0], 0.05);
        assert_eq!(scale_by([4.0, 1.0, 2.0], 2, 1.2, 0.5), [4.0, 1.0, 3.0]);
        let r = rotate_by(&euler_yxz_to_rot(0.0, 0.0, 0.0), [0.0, 1.0, 0.0], 90.0, 15.0);
        let want = euler_yxz_to_rot(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        for k in 0..9 {
            assert!((r[k] - want[k]).abs() < 1e-5);
        }
        assert_eq!(rotate_by(&r, [0.0, 1.0, 0.0], 0.0, 15.0), r);
    }
}
