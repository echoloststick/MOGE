//! rbxlx (XML) exporter used for "Export" and for publishing through Open Cloud.
use crate::normalize::serialized_name;
use moge_core::template::is_base_part;
use moge_core::{b64, ApiDump, Id, Tree, Value};
use std::collections::HashMap;
use std::fmt::Write;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\t' | '\n' => o.push(c),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(o, "&#x{:X};", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

fn cdata(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !((*c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r'))).collect();
    format!("<![CDATA[{}]]>", clean.replace("]]>", "]]]]><![CDATA[>"))
}

fn f(x: f32) -> String {
    if x.is_nan() {
        "NAN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "INF".into() } else { "-INF".into() }
    } else {
        x.to_string()
    }
}

fn fd(x: f64) -> String {
    if x.is_nan() {
        "NAN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "INF".into() } else { "-INF".into() }
    } else {
        x.to_string()
    }
}

fn pack_color(c: [f32; 3]) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    0xFF00_0000 | (q(c[0]) << 16) | (q(c[1]) << 8) | q(c[2])
}

fn prop_xml(out: &mut String, name: &str, v: &Value, vtype: &str, refs: &HashMap<Id, String>) {
    let n = esc(name);
    match v {
        Value::Bool(b) => {
            let _ = writeln!(out, "<bool name=\"{n}\">{b}</bool>");
        }
        Value::Int(i) => {
            let t = if vtype == "int64" { "int64" } else { "int" };
            let _ = writeln!(out, "<{t} name=\"{n}\">{i}</{t}>");
        }
        Value::Float(x) => {
            let t = if vtype == "double" { "double" } else { "float" };
            let _ = writeln!(out, "<{t} name=\"{n}\">{}</{t}>", fd(*x));
        }
        Value::String(s) => match vtype {
            "ProtectedString" => {
                let _ = writeln!(out, "<ProtectedString name=\"{n}\">{}</ProtectedString>", cdata(s));
            }
            "Content" => {
                if s.is_empty() {
                    let _ = writeln!(out, "<Content name=\"{n}\"><null></null></Content>");
                } else {
                    let _ = writeln!(out, "<Content name=\"{n}\"><url>{}</url></Content>", esc(s));
                }
            }
            _ => {
                let _ = writeln!(out, "<string name=\"{n}\">{}</string>", esc(s));
            }
        },
        Value::Bytes(b) => {
            let _ = writeln!(out, "<BinaryString name=\"{n}\">{}</BinaryString>", b64::encode(b));
        }
        Value::Vector2(a) => {
            let _ = writeln!(out, "<Vector2 name=\"{n}\"><X>{}</X><Y>{}</Y></Vector2>", f(a[0]), f(a[1]));
        }
        Value::Vector3(a) => {
            let _ = writeln!(out, "<Vector3 name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>", f(a[0]), f(a[1]), f(a[2]));
        }
        Value::Color3(c) => {
            if name == "Color3uint8" {
                let _ = writeln!(out, "<Color3uint8 name=\"{n}\">{}</Color3uint8>", pack_color(*c));
            } else {
                let _ = writeln!(out, "<Color3 name=\"{n}\"><R>{}</R><G>{}</G><B>{}</B></Color3>", f(c[0]), f(c[1]), f(c[2]));
            }
        }
        Value::UDim(s, o) => {
            let _ = writeln!(out, "<UDim name=\"{n}\"><S>{}</S><O>{o}</O></UDim>", f(*s));
        }
        Value::UDim2 { sx, ox, sy, oy } => {
            let _ = writeln!(out, "<UDim2 name=\"{n}\"><XS>{}</XS><XO>{ox}</XO><YS>{}</YS><YO>{oy}</YO></UDim2>", f(*sx), f(*sy));
        }
        Value::CFrame { pos, rot } => {
            let _ = write!(out, "<CoordinateFrame name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z>", f(pos[0]), f(pos[1]), f(pos[2]));
            for (i, k) in ["R00", "R01", "R02", "R10", "R11", "R12", "R20", "R21", "R22"].iter().enumerate() {
                let _ = write!(out, "<{k}>{}</{k}>", f(rot[i]));
            }
            out.push_str("</CoordinateFrame>\n");
        }
        Value::Rect { min, max } => {
            let _ = writeln!(
                out,
                "<Rect2D name=\"{n}\"><min><X>{}</X><Y>{}</Y></min><max><X>{}</X><Y>{}</Y></max></Rect2D>",
                f(min[0]), f(min[1]), f(max[0]), f(max[1])
            );
        }
        Value::NumberRange(a, b) => {
            let _ = writeln!(out, "<NumberRange name=\"{n}\">{} {} </NumberRange>", f(*a), f(*b));
        }
        Value::NumberSequence(k) => {
            let body: String = k.iter().map(|p| format!("{} {} {} ", f(p[0]), f(p[1]), f(p[2]))).collect();
            let _ = writeln!(out, "<NumberSequence name=\"{n}\">{body}</NumberSequence>");
        }
        Value::ColorSequence(k) => {
            let body: String = k.iter().map(|(t, c)| format!("{} {} {} {} 0 ", f(*t), f(c[0]), f(c[1]), f(c[2]))).collect();
            let _ = writeln!(out, "<ColorSequence name=\"{n}\">{body}</ColorSequence>");
        }
        Value::BrickColor(c) => {
            let _ = writeln!(out, "<int name=\"{n}\">{c}</int>");
        }
        Value::Enum(e) => {
            let _ = writeln!(out, "<token name=\"{n}\">{e}</token>");
        }
        Value::Ref(r) => match r.and_then(|id| refs.get(&id)) {
            Some(rf) => {
                let _ = writeln!(out, "<Ref name=\"{n}\">{rf}</Ref>");
            }
            None => {
                let _ = writeln!(out, "<Ref name=\"{n}\">null</Ref>");
            }
        },
        Value::Font { family, weight, style } => {
            let st = if *style == 1 { "Italic" } else { "Normal" };
            let _ = writeln!(
                out,
                "<Font name=\"{n}\"><Family><url>{}</url></Family><Weight>{weight}</Weight><Style>{st}</Style></Font>",
                esc(family)
            );
        }
        Value::Faces(x) => {
            let _ = writeln!(out, "<Faces name=\"{n}\"><faces>{x}</faces></Faces>");
        }
        Value::Axes(x) => {
            let _ = writeln!(out, "<Axes name=\"{n}\"><axes>{x}</axes></Axes>");
        }
        Value::Raw { ty, text } => {
            if ty.chars().all(|c| c.is_ascii_alphanumeric()) && !ty.is_empty() {
                let _ = writeln!(out, "<{ty} name=\"{n}\">{}</{ty}>", esc(text));
            }
        }
    }
}

/// Serializes every service below the DataModel.
pub fn write_rbxlx(tree: &Tree, dump: &ApiDump) -> String {
    let mut order: Vec<Id> = Vec::new();
    let top = tree.node(Tree::ROOT).map(|n| n.children.clone()).unwrap_or_default();
    for &s in &top {
        order.push(s);
        order.extend(tree.descendants(s));
    }
    let refs: HashMap<Id, String> = order.iter().enumerate().map(|(i, id)| (*id, format!("RBX{i}"))).collect();
    let mut out = String::with_capacity(order.len() * 200);
    out.push_str("<roblox xmlns:xmime=\"http://www.w3.org/2005/05/xmlmime\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"http://www.roblox.com/roblox.xsd\" version=\"4\">\n");
    out.push_str("<Meta name=\"ExplicitAutoJoints\">true</Meta>\n");
    for &s in &top {
        write_item(&mut out, tree, dump, s, &refs);
    }
    out.push_str("</roblox>\n");
    out
}

fn write_item(out: &mut String, tree: &Tree, dump: &ApiDump, root: Id, refs: &HashMap<Id, String>) {
    // Iterative to survive deep hierarchies: (id, entering?)
    let mut stack: Vec<(Id, bool)> = vec![(root, true)];
    while let Some((id, enter)) = stack.pop() {
        if !enter {
            out.push_str("</Item>\n");
            continue;
        }
        let Some(n) = tree.node(id) else { continue };
        let _ = writeln!(out, "<Item class=\"{}\" referent=\"{}\">", esc(&n.class), refs.get(&id).map(String::as_str).unwrap_or("RBX"));
        out.push_str("<Properties>\n");
        let part = is_base_part(&n.class) || dump.is_a(&n.class, "BasePart");
        if !n.props.contains_key("Name") {
            let _ = writeln!(out, "<string name=\"Name\">{}</string>", esc(&n.class));
        }
        for (k, v) in &n.props {
            let info = dump.property(&n.class, k);
            if info.map(|i| i.read_only || !i.can_save).unwrap_or(false) {
                continue;
            }
            let vtype = info.map(|i| i.value_type.as_str()).unwrap_or(if k == "Source" { "ProtectedString" } else { "" });
            prop_xml(out, serialized_name(part, k), v, vtype, refs);
        }
        if !n.tags.is_empty() {
            let _ = writeln!(out, "<BinaryString name=\"Tags\">{}</BinaryString>", b64::encode(n.tags.join("\0").as_bytes()));
        }
        out.push_str("</Properties>\n");
        stack.push((id, false));
        for &c in n.children.iter().rev() {
            stack.push((c, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moge_core::template::baseplate;

    fn dump() -> ApiDump {
        ApiDump::from_json(
            r#"{"Classes":[{"Name":"Instance","Superclass":"<<<ROOT>>>","Members":[]},{"Name":"BasePart","Superclass":"Instance","Members":[]},
            {"Name":"Part","Superclass":"BasePart","Members":[]},{"Name":"Script","Superclass":"Instance","Members":[
              {"MemberType":"Property","Name":"Source","ValueType":{"Category":"Primitive","Name":"ProtectedString"},"Security":"None"}]}],"Enums":[]}"#,
        )
        .unwrap()
    }

    #[test]
    fn baseplate_round_trips_through_xml() {
        let t = baseplate();
        let d = dump();
        let xml = write_rbxlx(&t, &d);
        assert!(xml.contains("<Color3uint8 name=\"Color3uint8\">") && xml.contains("name=\"size\""));
        let mut t2 = Tree::new();
        let imp = crate::load_xml(&xml, &mut t2).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        let ws2 = imp.roots.iter().copied().find(|r| t2.node(*r).unwrap().class == "Workspace").unwrap();
        let bp = t2.find_first_child(ws2, "Baseplate").unwrap();
        assert_eq!(t2.get_prop(bp, "Size"), Some(&Value::Vector3([512.0, 20.0, 512.0])));
        assert_eq!(t2.get_prop(bp, "Anchored"), Some(&Value::Bool(true)));
        let Some(Value::Color3(c)) = t2.get_prop(bp, "Color") else { panic!("color lost") };
        assert!((c[0] - 0.388).abs() < 0.003 && (c[1] - 0.372).abs() < 0.003);
        let Some(Value::CFrame { pos, .. }) = t2.get_prop(bp, "CFrame") else { panic!() };
        assert_eq!(*pos, [0.0, -10.0, 0.0]);
        assert_eq!(imp.roots.len(), t.node(Tree::ROOT).unwrap().children.len());
    }

    #[test]
    fn scripts_refs_tags_and_escaping() {
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let s = t.create("Script");
        t.set_parent(s, Some(ws)).unwrap();
        let src = "print(\"<a & b>\")\n-- ]]> tricky\n";
        t.set_prop(s, "Source", Value::String(src.into())).unwrap();
        t.set_prop(s, "Name", Value::String("Weird <Name> & \u{1}".into())).unwrap();
        t.set_prop(ws, "PrimaryPart", Value::Ref(Some(s))).unwrap();
        t.node_mut(s).unwrap().tags = vec!["x".into(), "y".into()];
        let xml = write_rbxlx(&t, &dump());
        let mut t2 = Tree::new();
        let imp = crate::load_xml(&xml, &mut t2).unwrap();
        let ws2 = imp.roots[0];
        let s2 = t2.node(ws2).unwrap().children[0];
        assert_eq!(t2.get_prop(s2, "Source"), Some(&Value::String(src.into())));
        assert_eq!(t2.name(s2), Some("Weird <Name> & \u{1}"));
        assert_eq!(t2.get_prop(ws2, "PrimaryPart"), Some(&Value::Ref(Some(s2))));
        assert_eq!(t2.node(s2).unwrap().tags, ["x", "y"]);
    }

    #[test]
    fn deep_trees_do_not_overflow() {
        let mut t = Tree::new();
        let mut parent = t.create("Workspace");
        t.set_parent(parent, Some(Tree::ROOT)).unwrap();
        for _ in 0..400 {
            let c = t.create("Folder");
            t.set_parent(c, Some(parent)).unwrap();
            parent = c;
        }
        let xml = write_rbxlx(&t, &dump());
        assert_eq!(xml.matches("<Item ").count(), 401);
        assert!(crate::load_xml(&xml, &mut Tree::new()).is_ok());
    }
}
