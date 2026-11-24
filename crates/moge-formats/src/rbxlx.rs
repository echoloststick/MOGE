//! rbxlx / rbxmx (XML) importer.
use crate::error::FormatError;
use crate::xml::{self, Element};
use crate::Import;
use moge_core::value::IDENTITY_ROT;
use moge_core::{Id, Tree, Value};
use std::collections::HashMap;

fn mal(m: impl Into<String>) -> FormatError {
    FormatError::Malformed(m.into())
}

pub fn load(text: &str, tree: &mut Tree) -> Result<Import, FormatError> {
    let root = xml::parse(text)?;
    if root.name != "roblox" {
        return Err(mal("Root element is not <roblox>"));
    }
    let mut st = State::default();
    let mut roots = Vec::new();
    for it in root.children.iter().filter(|c| c.name == "Item") {
        roots.push(st.item(tree, it, None)?);
    }
    for (id, prop, referent) in std::mem::take(&mut st.pending) {
        match st.refs.get(&referent) {
            Some(target) => {
                let _ = tree.set_prop(id, &prop, Value::Ref(Some(*target)));
            }
            None => st.warnings.push(format!("Property '{prop}' references unknown instance '{referent}'")),
        }
    }
    Ok(Import { roots, warnings: st.warnings })
}

#[derive(Default)]
struct State {
    refs: HashMap<String, Id>,
    pending: Vec<(Id, String, String)>,
    warnings: Vec<String>,
}

impl State {
    fn item(&mut self, tree: &mut Tree, el: &Element, parent: Option<Id>) -> Result<Id, FormatError> {
        let class = el.attr("class").ok_or_else(|| mal("<Item> without a class attribute"))?;
        let id = tree.create(class);
        if let Some(p) = parent {
            tree.set_parent(id, Some(p)).map_err(|e| mal(e.to_string()))?;
        }
        if let Some(r) = el.attr("referent") {
            self.refs.insert(r.to_string(), id);
        }
        if let Some(props) = el.child("Properties") {
            for p in &props.children {
                self.property(tree, id, class, p);
            }
        }
        for c in el.children.iter().filter(|c| c.name == "Item") {
            self.item(tree, c, Some(id))?;
        }
        Ok(id)
    }

    fn property(&mut self, tree: &mut Tree, id: Id, class: &str, p: &Element) {
        let Some(name) = p.attr("name") else { return };
        if p.name == "Ref" {
            let t = p.text.trim();
            if t.is_empty() || t == "null" {
                let _ = tree.set_prop(id, name, Value::Ref(None));
            } else {
                self.pending.push((id, name.to_string(), t.to_string()));
            }
            return;
        }
        let Some(v) = read_value(p) else {
            self.warnings.push(format!("Could not read {class}.{name} ({})", p.name));
            return;
        };
        match (name, &v) {
            ("Tags", Value::Bytes(b)) => {
                if let Some(n) = tree.node_mut(id) {
                    n.tags = b.split(|c| *c == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
                }
            }
            _ => {
                let _ = tree.set_prop(id, name, v);
            }
        }
    }
}

fn num(s: &str) -> Option<f64> {
    let t = s.trim();
    match t {
        "INF" | "inf" | "Infinity" => Some(f64::INFINITY),
        "-INF" | "-inf" | "-Infinity" => Some(f64::NEG_INFINITY),
        "NAN" | "nan" | "NaN" => Some(f64::NAN),
        _ => t.parse().ok(),
    }
}

fn comp(el: &Element, name: &str) -> Option<f32> {
    num(&el.child(name)?.text).map(|v| v as f32)
}

fn unpack_color(v: u32) -> [f32; 3] {
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

fn numbers(s: &str) -> Option<Vec<f32>> {
    s.split_whitespace().map(|t| num(t).map(|v| v as f32)).collect()
}

fn read_value(p: &Element) -> Option<Value> {
    let t = p.text.trim();
    Some(match p.name.as_str() {
        "string" | "ProtectedString" => Value::String(p.text.clone()),
        "Content" => Value::String(match p.child("url") {
            Some(u) => u.text.trim().to_string(),
            None if p.child("null").is_some() => String::new(),
            None => t.to_string(),
        }),
        "BinaryString" => Value::Bytes(base64(t)?),
        "bool" => Value::Bool(t.eq_ignore_ascii_case("true")),
        "int" | "int64" => Value::Int(t.parse().ok()?),
        "float" | "double" => Value::Float(num(t)?),
        "token" => Value::Enum(t.parse().ok()?),
        "BrickColor" => Value::BrickColor(t.parse().ok()?),
        "Color3" => match (comp(p, "R"), comp(p, "G"), comp(p, "B")) {
            (Some(r), Some(g), Some(b)) => Value::Color3([r, g, b]),
            _ => Value::Color3(unpack_color(t.parse().ok()?)),
        },
        "Color3uint8" => Value::Color3(unpack_color(t.parse().ok()?)),
        "Vector2" => Value::Vector2([comp(p, "X")?, comp(p, "Y")?]),
        "Vector3" => Value::Vector3([comp(p, "X")?, comp(p, "Y")?, comp(p, "Z")?]),
        "CoordinateFrame" | "CFrame" => {
            let pos = [comp(p, "X")?, comp(p, "Y")?, comp(p, "Z")?];
            let mut rot = IDENTITY_ROT;
            for (k, n) in ["R00", "R01", "R02", "R10", "R11", "R12", "R20", "R21", "R22"].iter().enumerate() {
                rot[k] = comp(p, n)?;
            }
            Value::CFrame { pos, rot }
        }
        "UDim" => Value::UDim(comp(p, "S")?, num(&p.child("O")?.text)? as i32),
        "UDim2" => Value::UDim2 {
            sx: comp(p, "XS")?,
            ox: num(&p.child("XO")?.text)? as i32,
            sy: comp(p, "YS")?,
            oy: num(&p.child("YO")?.text)? as i32,
        },
        "Rect2D" | "Rect" => {
            let (mn, mx) = (p.child("min")?, p.child("max")?);
            Value::Rect { min: [comp(mn, "X")?, comp(mn, "Y")?], max: [comp(mx, "X")?, comp(mx, "Y")?] }
        }
        "NumberRange" => {
            let n = numbers(t)?;
            Value::NumberRange(*n.first()?, *n.get(1)?)
        }
        "NumberSequence" => {
            let n = numbers(t)?;
            if n.len() % 3 != 0 {
                return None;
            }
            Value::NumberSequence(n.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
        }
        "ColorSequence" => {
            let n = numbers(t)?;
            if n.len() % 5 != 0 {
                return None;
            }
            Value::ColorSequence(n.chunks(5).map(|c| (c[0], [c[1], c[2], c[3]])).collect())
        }
        "Font" => Value::Font {
            family: p.child("Family").map(|f| f.child("url").map(|u| u.text.trim()).unwrap_or(f.text.trim()).to_string()).unwrap_or_default(),
            weight: p.child("Weight").and_then(|w| w.text.trim().parse().ok()).unwrap_or(400),
            style: match p.child("Style").map(|s| s.text.trim()) {
                Some("Italic") | Some("1") => 1,
                _ => 0,
            },
        },
        "Faces" => Value::Faces(p.child("faces")?.text.trim().parse().ok()?),
        "Axes" => Value::Axes(p.child("axes")?.text.trim().parse().ok()?),
        other => Value::Raw { ty: other.to_string(), text: t.to_string() },
    })
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" version="4">
<Item class="Workspace" referent="RBX1"><Properties><string name="Name">Workspace</string><float name="Gravity">196.2</float></Properties>
 <Item class="Model" referent="RBX2"><Properties>
   <string name="Name">M &amp; Co</string>
   <Ref name="PrimaryPart">RBX3</Ref>
   <BinaryString name="Tags">YWIAY2Q=</BinaryString>
 </Properties>
  <Item class="Part" referent="RBX3"><Properties>
   <string name="Name">P</string><bool name="Anchored">true</bool><token name="Material">288</token>
   <Vector3 name="Size"><X>4</X><Y>1</Y><Z>2</Z></Vector3>
   <CoordinateFrame name="CFrame"><X>1</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
   <Color3 name="Color"><R>1</R><G>0</G><B>0.5</B></Color3>
   <Color3 name="Color3_old">4294901760</Color3>
   <UDim2 name="Pos"><XS>0.5</XS><XO>3</XO><YS>0</YS><YO>-4</YO></UDim2>
   <Content name="Tex"><url>rbxassetid://9</url></Content>
   <Content name="Empty"><null></null></Content>
   <float name="Inf">INF</float>
   <NumberSequence name="NS">0 1 0 1 0 0 </NumberSequence>
   <Weird name="W">hello</Weird>
  </Properties></Item>
 </Item>
</Item>
</roblox>"#;

    #[test]
    fn imports_sample() {
        let mut t = Tree::new();
        let imp = load(SAMPLE, &mut t).unwrap();
        assert_eq!(imp.roots.len(), 1);
        let ws = imp.roots[0];
        let m = t.node(ws).unwrap().children[0];
        let p = t.node(m).unwrap().children[0];
        assert_eq!(t.name(m), Some("M & Co"));
        assert_eq!(t.get_prop(m, "PrimaryPart"), Some(&Value::Ref(Some(p))));
        assert_eq!(t.node(m).unwrap().tags, ["ab", "cd"]);
        assert_eq!(t.get_prop(p, "Anchored"), Some(&Value::Bool(true)));
        assert_eq!(t.get_prop(p, "Material"), Some(&Value::Enum(288)));
        assert_eq!(t.get_prop(p, "Size"), Some(&Value::Vector3([4.0, 1.0, 2.0])));
        assert_eq!(t.get_prop(p, "CFrame"), Some(&Value::CFrame { pos: [1.0, 2.0, 3.0], rot: IDENTITY_ROT }));
        assert_eq!(t.get_prop(p, "Color"), Some(&Value::Color3([1.0, 0.0, 0.5])));
        assert_eq!(t.get_prop(p, "Color3_old"), Some(&Value::Color3([1.0, 0.0, 0.0])));
        assert_eq!(t.get_prop(p, "Pos"), Some(&Value::UDim2 { sx: 0.5, ox: 3, sy: 0.0, oy: -4 }));
        assert_eq!(t.get_prop(p, "Tex"), Some(&Value::String("rbxassetid://9".into())));
        assert_eq!(t.get_prop(p, "Empty"), Some(&Value::String(String::new())));
        assert_eq!(t.get_prop(p, "Inf"), Some(&Value::Float(f64::INFINITY)));
        assert_eq!(t.get_prop(p, "NS"), Some(&Value::NumberSequence(vec![[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]])));
        assert_eq!(t.get_prop(p, "W"), Some(&Value::Raw { ty: "Weird".into(), text: "hello".into() }));
        assert_eq!(t.get_prop(ws, "Gravity"), Some(&Value::Float(196.2)));
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    }

    #[test]
    fn rejects_wrong_root_and_warns_on_bad_ref() {
        assert!(load("<other/>", &mut Tree::new()).is_err());
        let mut t = Tree::new();
        let imp = load(r#"<roblox><Item class="Model" referent="A"><Properties><Ref name="PrimaryPart">ZZ</Ref><int name="X">oops</int></Properties></Item></roblox>"#, &mut t).unwrap();
        assert_eq!(imp.warnings.len(), 2);
    }

    #[test]
    fn deep_hierarchies_do_not_overflow_the_stack() {
        let depth = 550;
        let xml = format!("<roblox>{}{}</roblox>", "<Item class=\"Folder\">".repeat(depth), "</Item>".repeat(depth));
        let mut t = Tree::new();
        let imp = load(&xml, &mut t).unwrap();
        assert_eq!(t.descendants(imp.roots[0]).len(), depth - 1);
        let too_deep = format!("<roblox>{}</roblox>", "<Item class=\"Folder\">".repeat(5000));
        assert!(load(&too_deep, &mut Tree::new()).is_err());
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVs\nbG8").unwrap(), b"hello");
        assert!(base64("a$b").is_none());
    }
}
