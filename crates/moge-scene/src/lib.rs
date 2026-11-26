//! Builds the renderable 3D scene (parts, lighting, sky) from the instance tree, and picks parts with rays.
pub mod tools;

use moge_core::color::brick_rgb;
use moge_core::{Id, Tree, Value};
use moge_math::{model_matrix, Mat4};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
}

#[derive(Clone, Debug)]
pub struct PartInst {
    pub id: Id,
    pub shape: Shape,
    pub pos: [f32; 3],
    pub rot: [f32; 9],
    pub size: [f32; 3],
    pub model: Mat4,
    pub color: [f32; 3],
    pub alpha: f32,
    /// Roblox Enum.Material value (drives surface shading in the renderer).
    pub material: u32,
}

#[derive(Clone, Debug)]
pub struct SkyInfo {
    pub clock_time: f32,
    pub star_count: u32,
    pub sun_size: f32,
    pub moon_size: f32,
    pub celestial_bodies: bool,
    /// Bk, Dn, Ft, Lf, Rt, Up texture URIs when the Sky instance sets them all.
    pub skybox: Option<[String; 6]>,
}

#[derive(Clone, Debug)]
pub struct Lighting {
    pub brightness: f32,
    pub ambient: [f32; 3],
    pub outdoor_ambient: [f32; 3],
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
    pub sky: SkyInfo,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            brightness: 3.0,
            ambient: [0.0; 3],
            outdoor_ambient: [0.5; 3],
            fog_color: [0.75, 0.75, 0.75],
            fog_start: 0.0,
            fog_end: 100_000.0,
            sky: SkyInfo { clock_time: 14.0, star_count: 3000, sun_size: 21.0, moon_size: 11.0, celestial_bodies: true, skybox: None },
        }
    }
}

#[derive(Debug, Default)]
pub struct Scene {
    pub parts: Vec<PartInst>,
    pub lighting: Lighting,
    index: HashMap<Id, usize>,
}

fn vec3(tree: &Tree, id: Id, name: &str, d: [f32; 3]) -> [f32; 3] {
    match tree.get_prop(id, name) {
        Some(Value::Vector3(v)) => *v,
        _ => d,
    }
}
fn color3(tree: &Tree, id: Id, name: &str, d: [f32; 3]) -> [f32; 3] {
    match tree.get_prop(id, name) {
        Some(Value::Color3(c)) => *c,
        _ => d,
    }
}
fn num(tree: &Tree, id: Id, name: &str, d: f32) -> f32 {
    match tree.get_prop(id, name) {
        Some(Value::Float(f)) => *f as f32,
        Some(Value::Int(i)) => *i as f32,
        _ => d,
    }
}

fn shape_of(tree: &Tree, id: Id, class: &str) -> Option<Shape> {
    Some(match class {
        "Part" => match tree.get_prop(id, "Shape") {
            Some(Value::Enum(0)) => Shape::Ball,
            Some(Value::Enum(2)) => Shape::Cylinder,
            Some(Value::Enum(3)) => Shape::Wedge,
            Some(Value::Enum(4)) => Shape::CornerWedge,
            _ => Shape::Block,
        },
        "WedgePart" => Shape::Wedge,
        "CornerWedgePart" => Shape::CornerWedge,
        "SpawnLocation" | "Seat" | "VehicleSeat" | "MeshPart" | "UnionOperation" | "TrussPart" | "NegateOperation" => Shape::Block,
        _ => return None,
    })
}

pub fn part_from(tree: &Tree, id: Id) -> Option<PartInst> {
    let class = tree.node(id)?.class.clone();
    let shape = shape_of(tree, id, &class)?;
    let (pos, rot) = match tree.get_prop(id, "CFrame") {
        Some(Value::CFrame { pos, rot }) => (*pos, *rot),
        _ => ([0.0; 3], moge_core::value::IDENTITY_ROT),
    };
    let mut size = vec3(tree, id, "Size", [4.0, 1.0, 2.0]);
    for s in size.iter_mut() {
        if !s.is_finite() || *s < 0.001 {
            *s = 0.001;
        }
    }
    let color = match tree.get_prop(id, "Color") {
        Some(Value::Color3(c)) => *c,
        _ => match tree.get_prop(id, "BrickColor") {
            Some(Value::BrickColor(b)) => brick_rgb(*b),
            _ => brick_rgb(194),
        },
    };
    let material = match tree.get_prop(id, "Material") {
        Some(Value::Enum(m)) => *m,
        _ => 256,
    };
    Some(PartInst {
        id,
        shape,
        pos,
        rot,
        size,
        model: model_matrix(pos, &rot, size),
        color,
        alpha: (1.0 - num(tree, id, "Transparency", 0.0)).clamp(0.0, 1.0),
        material,
    })
}

fn sky_of(tree: &Tree, lighting: Id) -> SkyInfo {
    let mut sky = Lighting::default().sky;
    sky.clock_time = match tree.get_prop(lighting, "ClockTime") {
        Some(Value::Float(f)) => *f as f32,
        _ => match tree.get_prop(lighting, "TimeOfDay") {
            Some(Value::String(s)) => parse_time_of_day(s).unwrap_or(sky.clock_time),
            _ => sky.clock_time,
        },
    };
    if let Some(s) = tree.find_first_child_of_class(lighting, "Sky") {
        sky.star_count = num(tree, s, "StarCount", 3000.0).max(0.0) as u32;
        sky.sun_size = num(tree, s, "SunAngularSize", 21.0);
        sky.moon_size = num(tree, s, "MoonAngularSize", 11.0);
        sky.celestial_bodies = !matches!(tree.get_prop(s, "CelestialBodiesShown"), Some(Value::Bool(false)));
        let faces: Vec<String> = ["SkyboxBk", "SkyboxDn", "SkyboxFt", "SkyboxLf", "SkyboxRt", "SkyboxUp"]
            .iter()
            .map(|k| match tree.get_prop(s, k) {
                Some(Value::String(t)) => t.clone(),
                _ => String::new(),
            })
            .collect();
        if faces.iter().all(|f| !f.is_empty()) {
            sky.skybox = Some([faces[0].clone(), faces[1].clone(), faces[2].clone(), faces[3].clone(), faces[4].clone(), faces[5].clone()]);
        }
    }
    sky
}

/// "HH:MM:SS" -> hours (0..24).
pub fn parse_time_of_day(s: &str) -> Option<f32> {
    let mut it = s.trim().split(':');
    let h: f32 = it.next()?.parse().ok()?;
    let m: f32 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0.0))?;
    let sec: f32 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0.0))?;
    if !(0.0..=24.0).contains(&h) || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) {
        return None;
    }
    Some(h + m / 60.0 + sec / 3600.0)
}

/// Unit vector from the world toward the sun for a Roblox clock time.
pub fn sun_direction(clock_time: f32) -> [f32; 3] {
    let h = (clock_time - 12.0) / 12.0 * std::f32::consts::PI;
    let d = [-h.sin(), h.cos(), -0.3 * h.cos()];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    [d[0] / l, d[1] / l, d[2] / l]
}

pub fn build(tree: &Tree) -> Scene {
    let mut scene = Scene::default();
    if let Some(l) = tree.find_first_child_of_class(Tree::ROOT, "Lighting") {
        scene.lighting = Lighting {
            brightness: num(tree, l, "Brightness", 3.0),
            ambient: color3(tree, l, "Ambient", [0.0; 3]),
            outdoor_ambient: color3(tree, l, "OutdoorAmbient", [0.5; 3]),
            fog_color: color3(tree, l, "FogColor", [0.75; 3]),
            fog_start: num(tree, l, "FogStart", 0.0),
            fog_end: num(tree, l, "FogEnd", 100_000.0),
            sky: sky_of(tree, l),
        };
    }
    if let Some(ws) = tree.find_first_child_of_class(Tree::ROOT, "Workspace") {
        for id in tree.descendants(ws) {
            if let Some(p) = part_from(tree, id) {
                scene.index.insert(id, scene.parts.len());
                scene.parts.push(p);
            }
        }
    }
    scene
}

impl Scene {
    pub fn part(&self, id: Id) -> Option<&PartInst> {
        self.index.get(&id).map(|i| &self.parts[*i])
    }

    /// Parts that belong to the given instances (the instances themselves and their descendants).
    pub fn parts_under(&self, tree: &Tree, ids: &[Id]) -> Vec<&PartInst> {
        let mut out = Vec::new();
        for &id in ids {
            if let Some(p) = self.part(id) {
                out.push(p);
            }
            for d in tree.descendants(id) {
                if let Some(p) = self.part(d) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Bounding sphere of the parts under `ids`: (center, radius).
    pub fn bounds(&self, tree: &Tree, ids: &[Id]) -> Option<([f32; 3], f32)> {
        let parts = self.parts_under(tree, ids);
        if parts.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &parts {
            for c in corners(p) {
                for k in 0..3 {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
        }
        let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let r = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() / 2.0;
        Some((center, r))
    }

    /// Nearest part hit by the ray: (id, distance).
    pub fn pick(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<(Id, f32)> {
        let mut best: Option<(Id, f32)> = None;
        for p in &self.parts {
            if let Some(t) = hit(p, origin, dir) {
                if best.map(|(_, b)| t < b).unwrap_or(true) {
                    best = Some((p.id, t));
                }
            }
        }
        best
    }
}

/// The 8 world-space corners of a part's oriented box (selection outlines).
pub fn corners(p: &PartInst) -> [[f32; 3]; 8] {
    let mut out = [[0.0; 3]; 8];
    for (i, o) in out.iter_mut().enumerate() {
        let l = [
            if i & 1 == 0 { -0.5 } else { 0.5 } * p.size[0],
            if i & 2 == 0 { -0.5 } else { 0.5 } * p.size[1],
            if i & 4 == 0 { -0.5 } else { 0.5 } * p.size[2],
        ];
        for r in 0..3 {
            o[r] = p.pos[r] + p.rot[r * 3] * l[0] + p.rot[r * 3 + 1] * l[1] + p.rot[r * 3 + 2] * l[2];
        }
    }
    out
}

fn hit(p: &PartInst, o: [f32; 3], d: [f32; 3]) -> Option<f32> {
    let rel = [o[0] - p.pos[0], o[1] - p.pos[1], o[2] - p.pos[2]];
    let (mut lo, mut ld) = ([0.0f32; 3], [0.0f32; 3]);
    for k in 0..3 {
        // Rotation is row-major, so Rᵀ·v uses column k of each row.
        lo[k] = (p.rot[k] * rel[0] + p.rot[3 + k] * rel[1] + p.rot[6 + k] * rel[2]) / p.size[k];
        ld[k] = (p.rot[k] * d[0] + p.rot[3 + k] * d[1] + p.rot[6 + k] * d[2]) / p.size[k];
    }
    if p.shape == Shape::Ball {
        let a = ld[0] * ld[0] + ld[1] * ld[1] + ld[2] * ld[2];
        let b = 2.0 * (lo[0] * ld[0] + lo[1] * ld[1] + lo[2] * ld[2]);
        let c = lo[0] * lo[0] + lo[1] * lo[1] + lo[2] * lo[2] - 0.25;
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 || a < 1e-12 {
            return None;
        }
        let s = disc.sqrt();
        let (t0, t1) = ((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
        return if t0 > 0.0 { Some(t0) } else if t1 > 0.0 { Some(t1) } else { None };
    }
    let (mut tmin, mut tmax) = (f32::NEG_INFINITY, f32::INFINITY);
    for k in 0..3 {
        if ld[k].abs() < 1e-12 {
            if lo[k].abs() > 0.5 {
                return None;
            }
        } else {
            let (a, b) = ((-0.5 - lo[k]) / ld[k], (0.5 - lo[k]) / ld[k]);
            tmin = tmin.max(a.min(b));
            tmax = tmax.min(a.max(b));
        }
    }
    if tmax < tmin.max(0.0) {
        None
    } else {
        Some(if tmin > 0.0 { tmin } else { tmax })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moge_core::template::baseplate;
    use moge_math::euler_yxz_to_rot;

    fn one_part(shape: u32, size: [f32; 3], pos: [f32; 3], rot: [f32; 9]) -> (Tree, Id) {
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let p = t.create("Part");
        t.set_parent(p, Some(ws)).unwrap();
        t.set_prop(p, "Shape", Value::Enum(shape)).unwrap();
        t.set_prop(p, "Size", Value::Vector3(size)).unwrap();
        t.set_prop(p, "CFrame", Value::CFrame { pos, rot }).unwrap();
        (t, p)
    }

    #[test]
    fn builds_baseplate_scene() {
        let s = build(&baseplate());
        assert_eq!(s.parts.len(), 2);
        assert_eq!(s.lighting.sky.clock_time, 14.0);
        assert!(s.lighting.sky.skybox.is_none());
        assert!(s.parts.iter().all(|p| p.alpha == 1.0));
    }

    #[test]
    fn pick_block_ray_and_miss() {
        let (t, p) = one_part(1, [4.0, 2.0, 2.0], [0.0, 0.0, 0.0], moge_core::value::IDENTITY_ROT);
        let s = build(&t);
        let h = s.pick([0.0, 0.0, 10.0], [0.0, 0.0, -1.0]).unwrap();
        assert_eq!(h.0, p);
        assert!((h.1 - 9.0).abs() < 1e-4);
        assert!(s.pick([0.0, 5.0, 10.0], [0.0, 0.0, -1.0]).is_none());
        assert!(s.pick([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]).is_none(), "must not hit behind the origin");
        assert!(s.pick([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).is_some(), "ray starting inside hits the exit face");
    }

    #[test]
    fn pick_respects_rotation_and_ball_shape() {
        let rot = euler_yxz_to_rot(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        let (t, _) = one_part(1, [10.0, 2.0, 2.0], [0.0; 3], rot);
        let s = build(&t);
        assert!(s.pick([0.0, 0.0, 20.0], [0.0, 0.0, -1.0]).map(|h| (h.1 - 15.0).abs() < 1e-3).unwrap_or(false), "long axis rotated onto Z");
        assert!(s.pick([20.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).map(|h| (h.1 - 19.0).abs() < 1e-3).unwrap_or(false));

        let (t, _) = one_part(0, [4.0, 4.0, 4.0], [0.0; 3], moge_core::value::IDENTITY_ROT);
        let s = build(&t);
        assert!(s.pick([1.9, 1.9, 10.0], [0.0, 0.0, -1.0]).is_none(), "corner of the bounding box is outside the sphere");
        assert!((s.pick([0.0, 0.0, 10.0], [0.0, 0.0, -1.0]).unwrap().1 - 8.0).abs() < 1e-3);
    }

    #[test]
    fn nearest_part_wins_and_bounds() {
        let mut t = baseplate();
        let ws = t.find_first_child(Tree::ROOT, "Workspace").unwrap();
        let p = t.create("Part");
        t.set_parent(p, Some(ws)).unwrap();
        t.set_prop(p, "Size", Value::Vector3([2.0, 2.0, 2.0])).unwrap();
        t.set_prop(p, "CFrame", Value::CFrame { pos: [0.0, 5.0, 0.0], rot: moge_core::value::IDENTITY_ROT }).unwrap();
        let s = build(&t);
        assert_eq!(s.pick([0.0, 50.0, 0.0], [0.0, -1.0, 0.0]).unwrap().0, p);
        let (c, r) = s.bounds(&t, &[p]).unwrap();
        assert_eq!(c, [0.0, 5.0, 0.0]);
        assert!((r - 3f32.sqrt()).abs() < 1e-4);
        assert!(s.bounds(&t, &[ws]).is_some() && s.bounds(&t, &[]).is_none());
    }

    #[test]
    fn time_and_sun() {
        assert_eq!(parse_time_of_day("14:30:00"), Some(14.5));
        assert_eq!(parse_time_of_day("25:00:00"), None);
        assert_eq!(parse_time_of_day("abc"), None);
        assert!(sun_direction(12.0)[1] > 0.9);
        assert!(sun_direction(0.0)[1] < -0.9);
        assert!(sun_direction(6.0)[1].abs() < 1e-3);
    }
}
