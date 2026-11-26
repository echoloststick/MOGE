//! Roblox data types exposed to Luau: Vector3, Vector2, Color3, UDim, UDim2, CFrame and Enum.
use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use moge_core::ApiDump;
use moge_math::{euler_yxz_to_rot, mat3_mul, rot_to_euler_yxz};
use std::sync::Arc;

macro_rules! from_lua_ud {
    ($($t:ty),*) => {$(
        impl mlua::FromLua for $t {
            fn from_lua(v: Value, _: &Lua) -> Result<Self> {
                match &v {
                    Value::UserData(u) => Ok((*u.borrow::<Self>()?).clone()),
                    _ => Err(mlua::Error::runtime(concat!("expected a ", stringify!($t), " value"))),
                }
            }
        }
    )*};
}
from_lua_ud!(Vector3, Vector2, Color3, UDim, UDim2, CFrame, EnumItem);

pub fn fmt_num(f: f32) -> String {
    if f.fract() == 0.0 && f.abs() < 1e9 {
        format!("{}", f as i64)
    } else {
        let s = format!("{f:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

pub fn ud<T: UserData + Clone + 'static>(v: &Value) -> Option<T> {
    match v {
        Value::UserData(u) => u.borrow::<T>().ok().map(|b| (*b).clone()),
        _ => None,
    }
}

pub fn wrap<T: UserData + 'static>(lua: &Lua, t: T) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(t)?))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector3(pub [f32; 3]);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector2(pub [f32; 2]);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color3(pub [f32; 3]);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UDim(pub f32, pub i32);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UDim2 {
    pub sx: f32,
    pub ox: i32,
    pub sy: f32,
    pub oy: i32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CFrame {
    pub pos: [f32; 3],
    pub rot: [f32; 9],
}
#[derive(Clone)]
pub struct EnumRoot(pub Arc<ApiDump>);
#[derive(Clone)]
pub struct EnumType {
    pub dump: Arc<ApiDump>,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EnumItem {
    pub ty: String,
    pub name: String,
    pub value: u32,
}

const IDENT: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn rot_apply(r: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [r[0] * v[0] + r[1] * v[1] + r[2] * v[2], r[3] * v[0] + r[4] * v[1] + r[5] * v[2], r[6] * v[0] + r[7] * v[1] + r[8] * v[2]]
}

fn rot_t(r: &[f32; 9]) -> [f32; 9] {
    [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]]
}

impl CFrame {
    pub fn compose(&self, o: &CFrame) -> CFrame {
        let p = rot_apply(&self.rot, o.pos);
        CFrame { pos: [p[0] + self.pos[0], p[1] + self.pos[1], p[2] + self.pos[2]], rot: mat3_mul(&self.rot, &o.rot) }
    }
    pub fn inverse(&self) -> CFrame {
        let rt = rot_t(&self.rot);
        let p = rot_apply(&rt, self.pos);
        CFrame { pos: [-p[0], -p[1], -p[2]], rot: rt }
    }
    fn point(&self, v: [f32; 3]) -> [f32; 3] {
        let p = rot_apply(&self.rot, v);
        [p[0] + self.pos[0], p[1] + self.pos[1], p[2] + self.pos[2]]
    }
}

fn rx(a: f32) -> [f32; 9] {
    let (s, c) = a.sin_cos();
    [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c]
}
fn ry(a: f32) -> [f32; 9] {
    let (s, c) = a.sin_cos();
    [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c]
}
fn rz(a: f32) -> [f32; 9] {
    let (s, c) = a.sin_cos();
    [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl UserData for Vector3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("X", |_, t| Ok(t.0[0]));
        f.add_field_method_get("Y", |_, t| Ok(t.0[1]));
        f.add_field_method_get("Z", |_, t| Ok(t.0[2]));
        f.add_field_method_get("Magnitude", |_, t| Ok(dot(t.0, t.0).sqrt()));
        f.add_field_method_get("Unit", |_, t| Ok(Vector3(norm(t.0))));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Dot", |_, t, o: Vector3| Ok(dot(t.0, o.0)));
        m.add_method("Cross", |_, t, o: Vector3| Ok(Vector3(cross(t.0, o.0))));
        m.add_method("Lerp", |_, t, (o, a): (Vector3, f32)| Ok(Vector3([t.0[0] + (o.0[0] - t.0[0]) * a, t.0[1] + (o.0[1] - t.0[1]) * a, t.0[2] + (o.0[2] - t.0[2]) * a])));
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (Vector3, Vector3)| Ok(Vector3([a.0[0] + b.0[0], a.0[1] + b.0[1], a.0[2] + b.0[2]])));
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (Vector3, Vector3)| Ok(Vector3([a.0[0] - b.0[0], a.0[1] - b.0[1], a.0[2] - b.0[2]])));
        m.add_meta_function(MetaMethod::Unm, |_, (a, _b): (Vector3, Value)| Ok(Vector3([-a.0[0], -a.0[1], -a.0[2]])));
        m.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| match (ud::<Vector3>(&a), ud::<Vector3>(&b), num(&a), num(&b)) {
            (Some(x), Some(y), _, _) => Ok(Vector3([x.0[0] * y.0[0], x.0[1] * y.0[1], x.0[2] * y.0[2]])),
            (Some(x), None, _, Some(k)) | (None, Some(x), Some(k), _) => Ok(Vector3([x.0[0] * k as f32, x.0[1] * k as f32, x.0[2] * k as f32])),
            _ => Err(mlua::Error::runtime("attempt to multiply a Vector3 with an incompatible value")),
        });
        m.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| match (ud::<Vector3>(&a), ud::<Vector3>(&b), num(&b)) {
            (Some(x), Some(y), _) => Ok(Vector3([x.0[0] / y.0[0], x.0[1] / y.0[1], x.0[2] / y.0[2]])),
            (Some(x), None, Some(k)) => Ok(Vector3([x.0[0] / k as f32, x.0[1] / k as f32, x.0[2] / k as f32])),
            _ => Err(mlua::Error::runtime("attempt to divide a Vector3 by an incompatible value")),
        });
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<Vector3>(&a).is_some() && ud::<Vector3>(&a) == ud::<Vector3>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}, {}", fmt_num(t.0[0]), fmt_num(t.0[1]), fmt_num(t.0[2]))));
    }
}

impl UserData for Vector2 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("X", |_, t| Ok(t.0[0]));
        f.add_field_method_get("Y", |_, t| Ok(t.0[1]));
        f.add_field_method_get("Magnitude", |_, t| Ok((t.0[0] * t.0[0] + t.0[1] * t.0[1]).sqrt()));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (Vector2, Vector2)| Ok(Vector2([a.0[0] + b.0[0], a.0[1] + b.0[1]])));
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (Vector2, Vector2)| Ok(Vector2([a.0[0] - b.0[0], a.0[1] - b.0[1]])));
        m.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| match (ud::<Vector2>(&a), num(&b), num(&a), ud::<Vector2>(&b)) {
            (Some(x), Some(k), _, _) | (None, _, Some(k), Some(x)) => Ok(Vector2([x.0[0] * k as f32, x.0[1] * k as f32])),
            _ => Err(mlua::Error::runtime("attempt to multiply a Vector2 with an incompatible value")),
        });
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<Vector2>(&a).is_some() && ud::<Vector2>(&a) == ud::<Vector2>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}", fmt_num(t.0[0]), fmt_num(t.0[1]))));
    }
}

impl UserData for Color3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("R", |_, t| Ok(t.0[0]));
        f.add_field_method_get("G", |_, t| Ok(t.0[1]));
        f.add_field_method_get("B", |_, t| Ok(t.0[2]));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Lerp", |_, t, (o, a): (Color3, f32)| Ok(Color3([t.0[0] + (o.0[0] - t.0[0]) * a, t.0[1] + (o.0[1] - t.0[1]) * a, t.0[2] + (o.0[2] - t.0[2]) * a])));
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<Color3>(&a).is_some() && ud::<Color3>(&a) == ud::<Color3>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}, {}", fmt_num(t.0[0]), fmt_num(t.0[1]), fmt_num(t.0[2]))));
    }
}

impl UserData for UDim {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Scale", |_, t| Ok(t.0));
        f.add_field_method_get("Offset", |_, t| Ok(t.1));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<UDim>(&a).is_some() && ud::<UDim>(&a) == ud::<UDim>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}", fmt_num(t.0), t.1)));
    }
}

impl UserData for UDim2 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("X", |_, t| Ok(UDim(t.sx, t.ox)));
        f.add_field_method_get("Y", |_, t| Ok(UDim(t.sy, t.oy)));
        f.add_field_method_get("Width", |_, t| Ok(UDim(t.sx, t.ox)));
        f.add_field_method_get("Height", |_, t| Ok(UDim(t.sy, t.oy)));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (UDim2, UDim2)| Ok(UDim2 { sx: a.sx + b.sx, ox: a.ox + b.ox, sy: a.sy + b.sy, oy: a.oy + b.oy }));
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (UDim2, UDim2)| Ok(UDim2 { sx: a.sx - b.sx, ox: a.ox - b.ox, sy: a.sy - b.sy, oy: a.oy - b.oy }));
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<UDim2>(&a).is_some() && ud::<UDim2>(&a) == ud::<UDim2>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{{{}, {}}}, {{{}, {}}}", fmt_num(t.sx), t.ox, fmt_num(t.sy), t.oy)));
    }
}

impl UserData for CFrame {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Position", |_, t| Ok(Vector3(t.pos)));
        f.add_field_method_get("X", |_, t| Ok(t.pos[0]));
        f.add_field_method_get("Y", |_, t| Ok(t.pos[1]));
        f.add_field_method_get("Z", |_, t| Ok(t.pos[2]));
        f.add_field_method_get("LookVector", |_, t| Ok(Vector3([-t.rot[2], -t.rot[5], -t.rot[8]])));
        f.add_field_method_get("RightVector", |_, t| Ok(Vector3([t.rot[0], t.rot[3], t.rot[6]])));
        f.add_field_method_get("UpVector", |_, t| Ok(Vector3([t.rot[1], t.rot[4], t.rot[7]])));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Inverse", |_, t, ()| Ok(t.inverse()));
        m.add_method("ToWorldSpace", |_, t, o: CFrame| Ok(t.compose(&o)));
        m.add_method("ToObjectSpace", |_, t, o: CFrame| Ok(t.inverse().compose(&o)));
        m.add_method("PointToWorldSpace", |_, t, v: Vector3| Ok(Vector3(t.point(v.0))));
        m.add_method("PointToObjectSpace", |_, t, v: Vector3| Ok(Vector3(t.inverse().point(v.0))));
        m.add_method("ToOrientation", |_, t, ()| {
            let e = rot_to_euler_yxz(&t.rot);
            Ok((e[0], e[1], e[2]))
        });
        m.add_meta_function(MetaMethod::Mul, |lua, (a, b): (CFrame, Value)| {
            if let Some(o) = ud::<CFrame>(&b) {
                wrap(lua, a.compose(&o))
            } else if let Some(v) = ud::<Vector3>(&b) {
                wrap(lua, Vector3(a.point(v.0)))
            } else {
                Err(mlua::Error::runtime("attempt to multiply a CFrame with an incompatible value"))
            }
        });
        m.add_meta_function(MetaMethod::Add, |_, (a, v): (CFrame, Vector3)| Ok(CFrame { pos: [a.pos[0] + v.0[0], a.pos[1] + v.0[1], a.pos[2] + v.0[2]], rot: a.rot }));
        m.add_meta_function(MetaMethod::Sub, |_, (a, v): (CFrame, Vector3)| Ok(CFrame { pos: [a.pos[0] - v.0[0], a.pos[1] - v.0[1], a.pos[2] - v.0[2]], rot: a.rot }));
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<CFrame>(&a).is_some() && ud::<CFrame>(&a) == ud::<CFrame>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            let mut parts = vec![fmt_num(t.pos[0]), fmt_num(t.pos[1]), fmt_num(t.pos[2])];
            parts.extend(t.rot.iter().map(|r| fmt_num(*r)));
            Ok(parts.join(", "))
        });
    }
}

impl UserData for EnumRoot {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, t, key: String| {
            if t.0.enum_names().any(|n| n == key) {
                wrap(lua, EnumType { dump: t.0.clone(), name: key })
            } else {
                Err(mlua::Error::runtime(format!("{key} is not a valid member of Enum")))
            }
        });
        m.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Enum".to_string()));
    }
}

impl UserData for EnumType {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, t, key: String| match t.dump.enum_value(&t.name, &key) {
            Some(v) => wrap(lua, EnumItem { ty: t.name.clone(), name: key, value: v }),
            None => Err(mlua::Error::runtime(format!("{key} is not a valid member of Enum.{}", t.name))),
        });
        m.add_method("GetEnumItems", |lua, t, ()| {
            let items: Vec<Value> = t.dump.enum_items(&t.name).iter().map(|(n, v)| wrap(lua, EnumItem { ty: t.name.clone(), name: n.clone(), value: *v })).collect::<Result<_>>()?;
            lua.create_sequence_from(items)
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("Enum.{}", t.name)));
    }
}

impl UserData for EnumItem {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("Name", |_, t| Ok(t.name.clone()));
        f.add_field_method_get("Value", |_, t| Ok(t.value));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| Ok(ud::<EnumItem>(&a).is_some() && ud::<EnumItem>(&a) == ud::<EnumItem>(&b)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("Enum.{}.{}", t.ty, t.name)));
    }
}

pub fn type_name_of(v: &Value) -> &'static str {
    match v {
        Value::UserData(_) => {
            if ud::<Vector3>(v).is_some() {
                "Vector3"
            } else if ud::<Vector2>(v).is_some() {
                "Vector2"
            } else if ud::<Color3>(v).is_some() {
                "Color3"
            } else if ud::<UDim2>(v).is_some() {
                "UDim2"
            } else if ud::<UDim>(v).is_some() {
                "UDim"
            } else if ud::<CFrame>(v).is_some() {
                "CFrame"
            } else if ud::<EnumItem>(v).is_some() {
                "EnumItem"
            } else {
                crate::inst::userdata_type_name(v)
            }
        }
        other => other.type_name(),
    }
}

fn f(v: Option<f64>) -> f32 {
    v.unwrap_or(0.0) as f32
}

/// Installs the constructor tables and the Enum global.
pub fn register(lua: &Lua, dump: &Arc<ApiDump>) -> Result<()> {
    let g = lua.globals();

    let t: Table = lua.create_table()?;
    t.set("new", lua.create_function(|_, (x, y, z): (Option<f64>, Option<f64>, Option<f64>)| Ok(Vector3([f(x), f(y), f(z)])))?)?;
    t.set("zero", Vector3([0.0; 3]))?;
    t.set("one", Vector3([1.0; 3]))?;
    t.set("xAxis", Vector3([1.0, 0.0, 0.0]))?;
    t.set("yAxis", Vector3([0.0, 1.0, 0.0]))?;
    t.set("zAxis", Vector3([0.0, 0.0, 1.0]))?;
    g.set("Vector3", t)?;

    let t = lua.create_table()?;
    t.set("new", lua.create_function(|_, (x, y): (Option<f64>, Option<f64>)| Ok(Vector2([f(x), f(y)])))?)?;
    t.set("zero", Vector2([0.0; 2]))?;
    t.set("one", Vector2([1.0; 2]))?;
    g.set("Vector2", t)?;

    let t = lua.create_table()?;
    t.set("new", lua.create_function(|_, (r, gg, b): (Option<f64>, Option<f64>, Option<f64>)| Ok(Color3([f(r).clamp(0.0, 1.0), f(gg).clamp(0.0, 1.0), f(b).clamp(0.0, 1.0)])))?)?;
    t.set("fromRGB", lua.create_function(|_, (r, gg, b): (Option<f64>, Option<f64>, Option<f64>)| Ok(Color3([(f(r) / 255.0).clamp(0.0, 1.0), (f(gg) / 255.0).clamp(0.0, 1.0), (f(b) / 255.0).clamp(0.0, 1.0)])))?)?;
    t.set(
        "fromHSV",
        lua.create_function(|_, (h, s, v): (f64, f64, f64)| {
            let (h, s, v) = ((h.rem_euclid(1.0) * 6.0) as f32, s.clamp(0.0, 1.0) as f32, v.clamp(0.0, 1.0) as f32);
            let (i, fr) = (h.floor(), h - h.floor());
            let (p, q, tt) = (v * (1.0 - s), v * (1.0 - s * fr), v * (1.0 - s * (1.0 - fr)));
            let rgb = match i as i32 % 6 {
                0 => [v, tt, p],
                1 => [q, v, p],
                2 => [p, v, tt],
                3 => [p, q, v],
                4 => [tt, p, v],
                _ => [v, p, q],
            };
            Ok(Color3(rgb))
        })?,
    )?;
    g.set("Color3", t)?;

    let t = lua.create_table()?;
    t.set("new", lua.create_function(|_, (s, o): (Option<f64>, Option<f64>)| Ok(UDim(f(s), f(o) as i32)))?)?;
    g.set("UDim", t)?;

    let t = lua.create_table()?;
    t.set("new", lua.create_function(|_, (a, b, c, d): (Option<f64>, Option<f64>, Option<f64>, Option<f64>)| Ok(UDim2 { sx: f(a), ox: f(b) as i32, sy: f(c), oy: f(d) as i32 }))?)?;
    t.set("fromScale", lua.create_function(|_, (a, b): (Option<f64>, Option<f64>)| Ok(UDim2 { sx: f(a), ox: 0, sy: f(b), oy: 0 }))?)?;
    t.set("fromOffset", lua.create_function(|_, (a, b): (Option<f64>, Option<f64>)| Ok(UDim2 { sx: 0.0, ox: f(a) as i32, sy: 0.0, oy: f(b) as i32 }))?)?;
    g.set("UDim2", t)?;

    let t = lua.create_table()?;
    t.set(
        "new",
        lua.create_function(|_, args: mlua::MultiValue| {
            let v: Vec<Value> = args.into_iter().collect();
            if v.is_empty() {
                return Ok(CFrame { pos: [0.0; 3], rot: IDENT });
            }
            if v.len() == 1 {
                if let Some(p) = ud::<Vector3>(&v[0]) {
                    return Ok(CFrame { pos: p.0, rot: IDENT });
                }
            }
            let n: Vec<f32> = v.iter().filter_map(|x| num(x).map(|n| n as f32)).collect();
            match n.len() {
                3 => Ok(CFrame { pos: [n[0], n[1], n[2]], rot: IDENT }),
                12 => Ok(CFrame { pos: [n[0], n[1], n[2]], rot: [n[3], n[4], n[5], n[6], n[7], n[8], n[9], n[10], n[11]] }),
                _ => Err(mlua::Error::runtime("CFrame.new expects (), (Vector3), (x, y, z) or 12 numbers")),
            }
        })?,
    )?;
    t.set("identity", CFrame { pos: [0.0; 3], rot: IDENT })?;
    t.set(
        "Angles",
        lua.create_function(|_, (a, b, c): (Option<f64>, Option<f64>, Option<f64>)| Ok(CFrame { pos: [0.0; 3], rot: mat3_mul(&mat3_mul(&rx(f(a)), &ry(f(b))), &rz(f(c))) }))?,
    )?;
    let from_yxz = lua.create_function(|_, (a, b, c): (Option<f64>, Option<f64>, Option<f64>)| Ok(CFrame { pos: [0.0; 3], rot: euler_yxz_to_rot(f(a), f(b), f(c)) }))?;
    t.set("fromEulerAnglesYXZ", from_yxz.clone())?;
    t.set("fromOrientation", from_yxz)?;
    t.set(
        "lookAt",
        lua.create_function(|_, (at, target, up): (Vector3, Vector3, Option<Vector3>)| {
            let back = norm([at.0[0] - target.0[0], at.0[1] - target.0[1], at.0[2] - target.0[2]]);
            let up = up.map(|u| u.0).unwrap_or([0.0, 1.0, 0.0]);
            let right = norm(cross(up, back));
            let upv = cross(back, right);
            Ok(CFrame { pos: at.0, rot: [right[0], upv[0], back[0], right[1], upv[1], back[1], right[2], upv[2], back[2]] })
        })?,
    )?;
    g.set("CFrame", t)?;

    g.set("Enum", EnumRoot(dump.clone()))?;
    Ok(())
}
