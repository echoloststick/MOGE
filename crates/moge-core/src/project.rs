//! Project save/load (`.moge` JSON). Versioned; unknown value tags are preserved as errors, never silently dropped.
use crate::b64;
use crate::json::{self, num32, parse_num32, Json};
use crate::tree::Tree;
use crate::value::{Id, Value};
use std::collections::HashMap;

pub const FORMAT: &str = "moge-project";
pub const VERSION: u32 = 1;

fn n(f: f32) -> Json {
    num32(f)
}
fn tag(t: &str, rest: Vec<Json>) -> Json {
    let mut v = vec![Json::Str(t.into())];
    v.extend(rest);
    Json::Arr(v)
}
fn f64j(f: f64) -> Json {
    if f.is_finite() {
        Json::Num(f)
    } else if f.is_nan() {
        Json::Str("nan".into())
    } else if f > 0.0 {
        Json::Str("inf".into())
    } else {
        Json::Str("-inf".into())
    }
}

pub fn value_to_json(v: &Value, index_of: &HashMap<Id, usize>) -> Json {
    match v {
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => tag("i", vec![Json::Num(*i as f64)]),
        Value::Float(f) => f64j(*f),
        Value::String(s) => Json::Str(s.clone()),
        Value::Bytes(b) => tag("b", vec![Json::Str(b64::encode(b))]),
        Value::Vector2(a) => tag("v2", a.iter().map(|x| n(*x)).collect()),
        Value::Vector3(a) => tag("v3", a.iter().map(|x| n(*x)).collect()),
        Value::Color3(a) => tag("c3", a.iter().map(|x| n(*x)).collect()),
        Value::UDim(s, o) => tag("ud", vec![n(*s), Json::Num(*o as f64)]),
        Value::UDim2 { sx, ox, sy, oy } => tag("u2", vec![n(*sx), Json::Num(*ox as f64), n(*sy), Json::Num(*oy as f64)]),
        Value::CFrame { pos, rot } => tag("cf", pos.iter().chain(rot.iter()).map(|x| n(*x)).collect()),
        Value::Rect { min, max } => tag("rc", vec![n(min[0]), n(min[1]), n(max[0]), n(max[1])]),
        Value::NumberRange(a, b) => tag("nr", vec![n(*a), n(*b)]),
        Value::NumberSequence(k) => tag("ns", k.iter().flatten().map(|x| n(*x)).collect()),
        Value::ColorSequence(k) => tag("cs", k.iter().flat_map(|(t, c)| [n(*t), n(c[0]), n(c[1]), n(c[2])]).collect()),
        Value::BrickColor(c) => tag("bc", vec![Json::Num(*c as f64)]),
        Value::Enum(e) => tag("en", vec![Json::Num(*e as f64)]),
        Value::Ref(r) => tag("rf", vec![r.and_then(|id| index_of.get(&id)).map(|i| Json::Num(*i as f64)).unwrap_or(Json::Null)]),
        Value::Font { family, weight, style } => tag("ft", vec![Json::Str(family.clone()), Json::Num(*weight as f64), Json::Num(*style as f64)]),
        Value::Faces(f) => tag("fc", vec![Json::Num(*f as f64)]),
        Value::Axes(a) => tag("ax", vec![Json::Num(*a as f64)]),
        Value::Raw { ty, text } => tag("raw", vec![Json::Str(ty.clone()), Json::Str(text.clone())]),
    }
}

fn floats(a: &[Json]) -> Result<Vec<f32>, String> {
    a.iter().map(|j| parse_num32(j).ok_or_else(|| "Expected a number".to_string())).collect()
}

fn ints(a: &[Json]) -> Result<Vec<i64>, String> {
    a.iter().map(|j| j.as_f64().map(|f| f as i64).ok_or_else(|| "Expected an integer".to_string())).collect()
}

/// `refs` collects (node, prop, target index) so references can be resolved after all nodes exist.
pub fn value_from_json(j: &Json) -> Result<(Value, Option<usize>), String> {
    Ok((
        match j {
            Json::Bool(b) => Value::Bool(*b),
            Json::Num(f) => Value::Float(*f),
            Json::Str(s) => match s.as_str() {
                _ => Value::String(s.clone()),
            },
            Json::Arr(a) => {
                let t = a.first().and_then(|t| t.as_str()).ok_or("Value array without a type tag")?;
                let r = &a[1..];
                let need = |k: usize| if r.len() == k { Ok(()) } else { Err(format!("Value '{t}' expects {k} components, found {}", r.len())) };
                match t {
                    "i" => {
                        need(1)?;
                        Value::Int(ints(r)?[0])
                    }
                    "b" => {
                        need(1)?;
                        Value::Bytes(b64::decode(r[0].as_str().ok_or("Invalid binary value")?).ok_or("Invalid base64 data")?)
                    }
                    "v2" => {
                        need(2)?;
                        let f = floats(r)?;
                        Value::Vector2([f[0], f[1]])
                    }
                    "v3" => {
                        need(3)?;
                        let f = floats(r)?;
                        Value::Vector3([f[0], f[1], f[2]])
                    }
                    "c3" => {
                        need(3)?;
                        let f = floats(r)?;
                        Value::Color3([f[0], f[1], f[2]])
                    }
                    "ud" => {
                        need(2)?;
                        Value::UDim(parse_num32(&r[0]).ok_or("Invalid UDim")?, ints(&r[1..])?[0] as i32)
                    }
                    "u2" => {
                        need(4)?;
                        Value::UDim2 {
                            sx: parse_num32(&r[0]).ok_or("Invalid UDim2")?,
                            ox: ints(&r[1..2])?[0] as i32,
                            sy: parse_num32(&r[2]).ok_or("Invalid UDim2")?,
                            oy: ints(&r[3..4])?[0] as i32,
                        }
                    }
                    "cf" => {
                        need(12)?;
                        let f = floats(r)?;
                        let mut rot = [0.0; 9];
                        rot.copy_from_slice(&f[3..12]);
                        Value::CFrame { pos: [f[0], f[1], f[2]], rot }
                    }
                    "rc" => {
                        need(4)?;
                        let f = floats(r)?;
                        Value::Rect { min: [f[0], f[1]], max: [f[2], f[3]] }
                    }
                    "nr" => {
                        need(2)?;
                        let f = floats(r)?;
                        Value::NumberRange(f[0], f[1])
                    }
                    "ns" => {
                        let f = floats(r)?;
                        if f.len() % 3 != 0 {
                            return Err("Invalid NumberSequence".into());
                        }
                        Value::NumberSequence(f.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
                    }
                    "cs" => {
                        let f = floats(r)?;
                        if f.len() % 4 != 0 {
                            return Err("Invalid ColorSequence".into());
                        }
                        Value::ColorSequence(f.chunks(4).map(|c| (c[0], [c[1], c[2], c[3]])).collect())
                    }
                    "bc" => {
                        need(1)?;
                        Value::BrickColor(ints(r)?[0] as u32)
                    }
                    "en" => {
                        need(1)?;
                        Value::Enum(ints(r)?[0] as u32)
                    }
                    "rf" => {
                        need(1)?;
                        return Ok((Value::Ref(None), r[0].as_f64().map(|f| f as usize)));
                    }
                    "ft" => {
                        need(3)?;
                        let i = ints(&r[1..])?;
                        Value::Font { family: r[0].as_str().ok_or("Invalid Font")?.to_string(), weight: i[0] as u16, style: i[1] as u8 }
                    }
                    "fc" => {
                        need(1)?;
                        Value::Faces(ints(r)?[0] as u8)
                    }
                    "ax" => {
                        need(1)?;
                        Value::Axes(ints(r)?[0] as u8)
                    }
                    "raw" => {
                        need(2)?;
                        Value::Raw { ty: r[0].as_str().ok_or("Invalid raw value")?.to_string(), text: r[1].as_str().ok_or("Invalid raw value")?.to_string() }
                    }
                    other => return Err(format!("Unknown value type tag '{other}'")),
                }
            }
            _ => return Err("Unsupported value".into()),
        },
        None,
    ))
}

fn special_float(s: &str) -> Option<f64> {
    match s {
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        "nan" => Some(f64::NAN),
        _ => None,
    }
}

/// Serializes everything below the DataModel (services and their descendants).
pub fn save(tree: &Tree, name: &str) -> String {
    let mut order: Vec<Id> = Vec::new();
    for &s in &tree.node(Tree::ROOT).map(|n| n.children.clone()).unwrap_or_default() {
        order.push(s);
        order.extend(tree.descendants(s));
    }
    let index_of: HashMap<Id, usize> = order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut nodes = Vec::with_capacity(order.len());
    for &id in &order {
        let Some(nd) = tree.node(id) else { continue };
        let parent = nd.parent.and_then(|p| index_of.get(&p)).map(|i| Json::Num(*i as f64)).unwrap_or(Json::Null);
        let props = nd.props.iter().map(|(k, v)| (k.clone(), value_to_json(v, &index_of))).collect();
        let attrs = nd.attributes.iter().map(|(k, v)| (k.clone(), value_to_json(v, &index_of))).collect();
        let mut o = vec![("c".to_string(), Json::Str(nd.class.clone())), ("p".to_string(), parent), ("props".to_string(), Json::Obj(props))];
        if !nd.attributes.is_empty() {
            o.push(("attrs".into(), Json::Obj(attrs)));
        }
        if !nd.tags.is_empty() {
            o.push(("tags".into(), Json::Arr(nd.tags.iter().map(|t| Json::Str(t.clone())).collect())));
        }
        nodes.push(Json::Obj(o));
    }
    Json::Obj(vec![
        ("format".into(), Json::Str(FORMAT.into())),
        ("version".into(), Json::Num(VERSION as f64)),
        ("name".into(), Json::Str(name.into())),
        ("nodes".into(), Json::Arr(nodes)),
    ])
    .stringify()
}

pub struct Loaded {
    pub tree: Tree,
    pub name: String,
}

pub fn load(text: &str) -> Result<Loaded, String> {
    let root = json::parse(text).map_err(|e| format!("Project file is not valid JSON: {e}"))?;
    if root.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err("This file is not a MOGE project".into());
    }
    let version = root.get("version").and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    if version == 0 || version > VERSION {
        return Err(format!("Unsupported project version {version}; this build reads up to {VERSION}"));
    }
    let list = root.get("nodes").and_then(|n| n.as_arr()).ok_or("Project has no 'nodes' array")?;
    let mut tree = Tree::new();
    let mut ids: Vec<Id> = Vec::with_capacity(list.len());
    let mut pending: Vec<(Id, String, usize)> = Vec::new();
    for (i, nd) in list.iter().enumerate() {
        let class = nd.get("c").and_then(|c| c.as_str()).ok_or_else(|| format!("Node {i} has no class"))?;
        let id = tree.create(class);
        ids.push(id);
        match nd.get("p") {
            Some(Json::Num(p)) => {
                let p = *p as usize;
                if p >= i {
                    return Err(format!("Node {i} references a parent that is not defined before it"));
                }
                tree.set_parent(id, Some(ids[p])).map_err(|e| e.to_string())?;
            }
            _ => tree.set_parent(id, Some(Tree::ROOT)).map_err(|e| e.to_string())?,
        }
        for (field, is_attr) in [("props", false), ("attrs", true)] {
            if let Some(Json::Obj(o)) = nd.get(field) {
                for (k, v) in o {
                    let (val, r) = match v {
                        Json::Str(s) if special_float(s).is_some() => (Value::Float(special_float(s).unwrap_or(0.0)), None),
                        _ => value_from_json(v).map_err(|e| format!("Node {i} property '{k}': {e}"))?,
                    };
                    if let Some(t) = r {
                        pending.push((id, k.clone(), t));
                    } else if is_attr {
                        tree.node_mut(id).map(|n| n.attributes.insert(k.clone(), val));
                    } else {
                        tree.set_prop(id, k, val).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        if let Some(Json::Arr(t)) = nd.get("tags") {
            if let Some(n) = tree.node_mut(id) {
                n.tags = t.iter().filter_map(|x| x.as_str().map(String::from)).collect();
            }
        }
    }
    for (id, prop, t) in pending {
        if let Some(target) = ids.get(t) {
            tree.set_prop(id, &prop, Value::Ref(Some(*target))).map_err(|e| e.to_string())?;
        }
    }
    Ok(Loaded { tree, name: root.get("name").and_then(|n| n.as_str()).unwrap_or("Untitled").to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::IDENTITY_ROT;

    #[test]
    fn round_trips_every_value_kind() {
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let m = t.create("Model");
        let p = t.create("Part");
        t.set_parent(m, Some(ws)).unwrap();
        t.set_parent(p, Some(m)).unwrap();
        let vals = vec![
            ("A", Value::Bool(true)),
            ("B", Value::Int(-9)),
            ("C", Value::Float(0.1)),
            ("D", Value::Float(f64::INFINITY)),
            ("E", Value::String("q\"\n é".into())),
            ("F", Value::Bytes(vec![0, 1, 255])),
            ("G", Value::Vector2([1.5, -2.0])),
            ("H", Value::Vector3([1.0, 2.0, 3.25])),
            ("I", Value::Color3([0.1, 0.2, 0.3])),
            ("J", Value::UDim(0.5, -3)),
            ("K", Value::UDim2 { sx: 1.0, ox: 2, sy: 0.25, oy: -4 }),
            ("L", Value::CFrame { pos: [1.0, 2.0, 3.0], rot: IDENTITY_ROT }),
            ("M", Value::Rect { min: [0.0, 1.0], max: [2.0, 3.0] }),
            ("N", Value::NumberRange(1.0, 2.0)),
            ("O", Value::NumberSequence(vec![[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]])),
            ("P", Value::ColorSequence(vec![(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])])),
            ("Q", Value::BrickColor(21)),
            ("R", Value::Enum(288)),
            ("S", Value::Font { family: "rbxasset://fonts/families/Arial.json".into(), weight: 400, style: 0 }),
            ("T", Value::Faces(5)),
            ("U", Value::Axes(3)),
            ("V", Value::Raw { ty: "Odd".into(), text: "x".into() }),
            ("PrimaryPart", Value::Ref(Some(p))),
        ];
        for (k, v) in &vals {
            t.set_prop(m, k, v.clone()).unwrap();
        }
        t.node_mut(m).unwrap().tags = vec!["a".into(), "b".into()];
        t.node_mut(m).unwrap().attributes.insert("Hp".into(), Value::Float(10.0));
        let out = save(&t, "My Game");
        let l = load(&out).unwrap();
        assert_eq!(l.name, "My Game");
        let ws2 = l.tree.node(Tree::ROOT).unwrap().children[0];
        let m2 = l.tree.node(ws2).unwrap().children[0];
        let p2 = l.tree.node(m2).unwrap().children[0];
        for (k, v) in &vals {
            let want = if *k == "PrimaryPart" { Value::Ref(Some(p2)) } else { v.clone() };
            assert_eq!(l.tree.get_prop(m2, k), Some(&want), "property {k}");
        }
        assert_eq!(l.tree.node(m2).unwrap().tags, ["a", "b"]);
        assert_eq!(l.tree.node(m2).unwrap().attributes.get("Hp"), Some(&Value::Float(10.0)));
    }

    #[test]
    fn rejects_bad_files() {
        assert!(load("not json").is_err());
        assert!(load("{\"format\":\"x\"}").is_err());
        assert!(load("{\"format\":\"moge-project\",\"version\":99,\"nodes\":[]}").is_err());
        assert!(load("{\"format\":\"moge-project\",\"version\":1,\"nodes\":[{\"c\":\"A\",\"p\":5}]}").is_err());
        assert!(load("{\"format\":\"moge-project\",\"version\":1,\"nodes\":[{\"c\":\"A\",\"props\":{\"X\":[\"zz\",1]}}]}").is_err());
        assert!(load("{\"format\":\"moge-project\",\"version\":1,\"nodes\":[{\"c\":\"A\",\"props\":{\"X\":[\"v3\",1]}}]}").is_err());
    }
}
