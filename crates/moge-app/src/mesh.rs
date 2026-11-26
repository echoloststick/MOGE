//! Unit part meshes (pos3 + normal3, unit-box space, centered on the origin). Back-face culling is disabled,
//! so winding is irrelevant; normals are explicit.
use moge_scene::Shape;
use std::ops::Range;

pub const SHAPES: usize = 5;

pub fn shape_index(s: Shape) -> usize {
    match s {
        Shape::Block => 0,
        Shape::Ball => 1,
        Shape::Cylinder => 2,
        Shape::Wedge => 3,
        Shape::CornerWedge => 4,
    }
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn push(out: &mut Vec<f32>, p: V3, n: V3) {
    out.extend_from_slice(&p);
    out.extend_from_slice(&n);
}

/// Flat-shaded convex polygon, fan-triangulated.
fn poly(out: &mut Vec<f32>, pts: &[V3], interior: V3) {
    let mut n = norm(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])));
    let to = sub(pts[0], interior);
    if n[0] * to[0] + n[1] * to[1] + n[2] * to[2] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    for i in 1..pts.len() - 1 {
        push(out, pts[0], n);
        push(out, pts[i], n);
        push(out, pts[i + 1], n);
    }
}

fn convex(out: &mut Vec<f32>, faces: &[&[V3]]) {
    let mut c = [0.0f32; 3];
    let mut k = 0.0;
    for f in faces {
        for p in f.iter() {
            for i in 0..3 {
                c[i] += p[i];
            }
            k += 1.0;
        }
    }
    let interior = [c[0] / k, c[1] / k, c[2] / k];
    for f in faces {
        poly(out, f, interior);
    }
}

fn block(out: &mut Vec<f32>) {
    let h = 0.5;
    let c = |x: f32, y: f32, z: f32| [x * h, y * h, z * h];
    convex(
        out,
        &[
            &[c(1., -1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(1., -1., 1.)],
            &[c(-1., -1., -1.), c(-1., 1., -1.), c(-1., 1., 1.), c(-1., -1., 1.)],
            &[c(-1., 1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(-1., 1., 1.)],
            &[c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), c(-1., -1., 1.)],
            &[c(-1., -1., 1.), c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.)],
            &[c(-1., -1., -1.), c(1., -1., -1.), c(1., 1., -1.), c(-1., 1., -1.)],
        ],
    );
}

/// Vertical wall at +Z; the slope faces up and toward -Z.
fn wedge(out: &mut Vec<f32>) {
    let (a, b, c, d) = ([-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]);
    let (e, f) = ([-0.5, 0.5, 0.5], [0.5, 0.5, 0.5]);
    convex(out, &[&[a, b, c, d], &[d, c, f, e], &[a, b, f, e], &[a, d, e], &[b, c, f]]);
}

/// Four triangles over a square base, apex above the (+X, +Z) corner.
fn corner_wedge(out: &mut Vec<f32>) {
    let (a, b, c, d) = ([-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]);
    let t = [0.5, 0.5, 0.5];
    convex(out, &[&[a, b, c, d], &[b, c, t], &[c, d, t], &[a, b, t], &[a, d, t]]);
}

fn sphere(out: &mut Vec<f32>) {
    let (lon, lat) = (24usize, 14usize);
    let pt = |i: usize, j: usize| {
        let th = std::f32::consts::PI * j as f32 / lat as f32;
        let ph = std::f32::consts::TAU * i as f32 / lon as f32;
        let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
        ([n[0] * 0.5, n[1] * 0.5, n[2] * 0.5], n)
    };
    for j in 0..lat {
        for i in 0..lon {
            let (p00, n00) = pt(i, j);
            let (p10, n10) = pt(i + 1, j);
            let (p01, n01) = pt(i, j + 1);
            let (p11, n11) = pt(i + 1, j + 1);
            for (p, n) in [(p00, n00), (p10, n10), (p11, n11), (p00, n00), (p11, n11), (p01, n01)] {
                push(out, p, n);
            }
        }
    }
}

/// Axis along X (Roblox cylinders), radius 0.5.
fn cylinder(out: &mut Vec<f32>) {
    let seg = 32usize;
    let ring = |i: usize| {
        let a = std::f32::consts::TAU * i as f32 / seg as f32;
        (a.cos(), a.sin())
    };
    for i in 0..seg {
        let (c0, s0) = ring(i);
        let (c1, s1) = ring(i + 1);
        let (p0, p1) = ([-0.5, 0.5 * c0, 0.5 * s0], [-0.5, 0.5 * c1, 0.5 * s1]);
        let (q0, q1) = ([0.5, 0.5 * c0, 0.5 * s0], [0.5, 0.5 * c1, 0.5 * s1]);
        let (n0, n1) = ([0.0, c0, s0], [0.0, c1, s1]);
        for (p, n) in [(p0, n0), (q0, n0), (q1, n1), (p0, n0), (q1, n1), (p1, n1)] {
            push(out, p, n);
        }
        for (sx, x) in [(-1.0f32, -0.5f32), (1.0, 0.5)] {
            push(out, [x, 0.0, 0.0], [sx, 0.0, 0.0]);
            push(out, [x, 0.5 * c0, 0.5 * s0], [sx, 0.0, 0.0]);
            push(out, [x, 0.5 * c1, 0.5 * s1], [sx, 0.0, 0.0]);
        }
    }
}

/// All meshes in one vertex array plus the vertex range of each shape (indexed by `shape_index`).
pub fn build() -> (Vec<f32>, [Range<u32>; SHAPES]) {
    let mut out = Vec::new();
    let mut ranges: [Range<u32>; SHAPES] = [0..0, 0..0, 0..0, 0..0, 0..0];
    let makers: [fn(&mut Vec<f32>); SHAPES] = [block, sphere, cylinder, wedge, corner_wedge];
    for (i, m) in makers.iter().enumerate() {
        let start = (out.len() / 6) as u32;
        m(&mut out);
        ranges[i] = start..(out.len() / 6) as u32;
    }
    (out, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meshes_are_well_formed() {
        let (v, r) = build();
        assert_eq!(v.len() % 18, 0, "whole triangles only");
        assert_eq!(r[0].end - r[0].start, 36, "block = 12 triangles");
        assert_eq!(r[3].end - r[3].start, 6 + 6 + 6 + 3 + 3, "wedge = 2+2+2+1+1 triangles");
        assert_eq!(r[4].end - r[4].start, 6 + 3 * 4, "corner wedge = 2+4 triangles");
        for rg in r.iter() {
            assert!(rg.end > rg.start);
        }
        // Normals are unit length and every vertex lies in the unit box.
        for t in v.chunks(6) {
            let l = (t[3] * t[3] + t[4] * t[4] + t[5] * t[5]).sqrt();
            assert!((l - 1.0).abs() < 1e-3, "{l}");
            assert!(t[..3].iter().all(|c| c.abs() <= 0.5 + 1e-5));
        }
    }

    #[test]
    fn block_normals_point_away_from_center() {
        let (v, r) = build();
        for t in v[(r[0].start as usize * 6)..(r[0].end as usize * 6)].chunks(6) {
            let dot = t[0] * t[3] + t[1] * t[4] + t[2] * t[5];
            assert!(dot > 0.4, "{dot}");
        }
    }
}
