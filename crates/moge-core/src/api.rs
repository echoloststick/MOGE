//! Class/property/enum metadata loaded from the Roblox API Dump (Full-API-Dump.json).
use crate::json::{self, Json};
use crate::value::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PropInfo {
    pub name: String,
    pub value_type: String,
    pub category: String,
    pub read_only: bool,
    pub hidden: bool,
    pub deprecated: bool,
    pub can_save: bool,
}

const KNOWN_TYPES: [&str; 14] = [
    "Vector2", "Vector3", "Color3", "UDim", "UDim2", "CFrame", "Rect", "NumberRange", "NumberSequence", "ColorSequence", "BrickColor",
    "Font", "Faces", "Axes",
];

impl PropInfo {
    /// Validates `v` against the declared type, converting where lossless (int -> float).
    pub fn coerce(&self, v: Value) -> Option<Value> {
        match (self.category.as_str(), v) {
            ("Enum", v @ Value::Enum(_)) | ("Class", v @ Value::Ref(_)) => Some(v),
            ("Enum" | "Class", _) => None,
            (_, v) => self.coerce_plain(v),
        }
    }

    fn coerce_plain(&self, v: Value) -> Option<Value> {
        let t = self.value_type.as_str();
        match (t, v) {
            ("bool", v @ Value::Bool(_)) => Some(v),
            ("int" | "int64", v @ Value::Int(_)) => Some(v),
            ("float" | "double", v @ Value::Float(_)) => Some(v),
            ("float" | "double", Value::Int(i)) => Some(Value::Float(i as f64)),
            ("string" | "Content" | "ProtectedString" | "BinaryString" | "SharedString", v @ (Value::String(_) | Value::Bytes(_))) => Some(v),
            ("bool" | "int" | "int64" | "float" | "double" | "string" | "Content" | "ProtectedString" | "BinaryString" | "SharedString", _) => None,
            (t, v) => {
                if KNOWN_TYPES.contains(&t) && v.type_name() != t {
                    None
                } else {
                    Some(v)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub superclass: Option<String>,
    pub props: Vec<PropInfo>,
    pub creatable: bool,
    pub service: bool,
    pub deprecated: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EnumInfo {
    pub name: String,
    pub items: Vec<(String, u32)>,
}

#[derive(Debug, Default)]
pub struct ApiDump {
    classes: HashMap<String, ClassInfo>,
    order: Vec<String>,
    enums: HashMap<String, EnumInfo>,
}

fn tags(j: &Json) -> Vec<&str> {
    j.get("Tags").and_then(|t| t.as_arr()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default()
}

fn write_security(m: &Json) -> &str {
    match m.get("Security") {
        Some(Json::Str(s)) => s,
        Some(o @ Json::Obj(_)) => o.get("Write").and_then(|w| w.as_str()).unwrap_or("None"),
        _ => "None",
    }
}

impl ApiDump {
    pub fn from_json(text: &str) -> Result<ApiDump, String> {
        let root = json::parse(text)?;
        let classes = root.get("Classes").and_then(|c| c.as_arr()).ok_or("API Dump is missing the 'Classes' array")?;
        let mut dump = ApiDump::default();
        for c in classes {
            let name = c.get("Name").and_then(|n| n.as_str()).ok_or("API Dump class without a name")?.to_string();
            let superclass = c.get("Superclass").and_then(|s| s.as_str()).filter(|s| *s != "<<<ROOT>>>").map(String::from);
            let ct = tags(c);
            let mut props = Vec::new();
            for m in c.get("Members").and_then(|m| m.as_arr()).unwrap_or(&[]) {
                if m.get("MemberType").and_then(|t| t.as_str()) != Some("Property") {
                    continue;
                }
                let Some(pname) = m.get("Name").and_then(|n| n.as_str()) else { continue };
                let vt = m.get("ValueType");
                let mt = tags(m);
                let ws = write_security(m);
                props.push(PropInfo {
                    name: pname.to_string(),
                    value_type: vt.and_then(|v| v.get("Name")).and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    category: vt.and_then(|v| v.get("Category")).and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    read_only: mt.contains(&"ReadOnly") || !matches!(ws, "None" | "PluginSecurity"),
                    hidden: mt.contains(&"NotBrowsable"),
                    deprecated: mt.contains(&"Deprecated"),
                    can_save: m.get("Serialization").and_then(|s| s.get("CanSave")).and_then(|b| b.as_bool()).unwrap_or(true),
                });
            }
            dump.order.push(name.clone());
            dump.classes.insert(
                name.clone(),
                ClassInfo {
                    name,
                    superclass,
                    props,
                    creatable: !ct.contains(&"NotCreatable"),
                    service: ct.contains(&"Service"),
                    deprecated: ct.contains(&"Deprecated"),
                    hidden: ct.contains(&"NotBrowsable"),
                },
            );
        }
        for e in root.get("Enums").and_then(|e| e.as_arr()).unwrap_or(&[]) {
            let Some(name) = e.get("Name").and_then(|n| n.as_str()) else { continue };
            let items = e
                .get("Items")
                .and_then(|i| i.as_arr())
                .unwrap_or(&[])
                .iter()
                .filter_map(|i| Some((i.get("Name")?.as_str()?.to_string(), i.get("Value")?.as_f64()? as u32)))
                .collect();
            dump.enums.insert(name.to_string(), EnumInfo { name: name.to_string(), items });
        }
        Ok(dump)
    }

    pub fn class(&self, name: &str) -> Option<&ClassInfo> {
        self.classes.get(name)
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// The class followed by its ancestors (Instance last).
    pub fn chain(&self, class: &str) -> Vec<&ClassInfo> {
        let mut out = Vec::new();
        let mut cur = self.classes.get(class);
        while let Some(c) = cur {
            if out.len() > 64 {
                break;
            }
            out.push(c);
            cur = c.superclass.as_deref().and_then(|s| self.classes.get(s));
        }
        out
    }

    pub fn is_a(&self, class: &str, base: &str) -> bool {
        self.chain(class).iter().any(|c| c.name == base)
    }

    pub fn property(&self, class: &str, name: &str) -> Option<&PropInfo> {
        self.chain(class).into_iter().find_map(|c| c.props.iter().find(|p| p.name == name))
    }

    /// All properties, base class first, each name once.
    pub fn properties(&self, class: &str) -> Vec<&PropInfo> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for c in self.chain(class).into_iter().rev() {
            for p in &c.props {
                if seen.insert(p.name.as_str()) {
                    out.push(p);
                }
            }
        }
        out
    }

    pub fn enum_item_name(&self, en: &str, value: u32) -> Option<&str> {
        self.enums.get(en)?.items.iter().find(|(_, v)| *v == value).map(|(n, _)| n.as_str())
    }

    pub fn enum_value(&self, en: &str, item: &str) -> Option<u32> {
        self.enums.get(en)?.items.iter().find(|(n, _)| n == item).map(|(_, v)| *v)
    }

    /// Class search for the "Insert Object" hub: exact, prefix, word-start, then substring matches.
    pub fn search(&self, query: &str, only_creatable: bool, limit: usize) -> Vec<&ClassInfo> {
        let q = query.trim().to_lowercase();
        let mut scored: Vec<(u32, &ClassInfo)> = Vec::new();
        for name in &self.order {
            let c = &self.classes[name];
            if c.hidden || (only_creatable && !c.creatable) {
                continue;
            }
            let lower = c.name.to_lowercase();
            let base = if q.is_empty() {
                Some(3)
            } else if lower == q {
                Some(0)
            } else if lower.starts_with(&q) {
                Some(1)
            } else if c.name.char_indices().any(|(i, ch)| i > 0 && ch.is_uppercase() && lower[i..].starts_with(&q)) {
                Some(2)
            } else if lower.contains(&q) {
                Some(3)
            } else {
                None
            };
            if let Some(s) = base {
                scored.push((s + if c.deprecated { 10 } else { 0 }, c));
            }
        }
        scored.sort_by(|a, b| (a.0, a.1.name.len(), &a.1.name).cmp(&(b.0, b.1.name.len(), &b.1.name)));
        scored.into_iter().take(limit).map(|(_, c)| c).collect()
    }
}

#[cfg(test)]
pub(crate) const TEST_DUMP: &str = r#"{"Classes":[
 {"Name":"Instance","Superclass":"<<<ROOT>>>","Tags":["NotCreatable"],"Members":[
   {"MemberType":"Property","Name":"Name","ValueType":{"Category":"Primitive","Name":"string"},"Security":{"Read":"None","Write":"None"}},
   {"MemberType":"Property","Name":"Parent","ValueType":{"Category":"Class","Name":"Instance"},"Security":{"Read":"None","Write":"None"}},
   {"MemberType":"Property","Name":"ClassName","ValueType":{"Category":"Primitive","Name":"string"},"Tags":["ReadOnly"],"Security":{"Read":"None","Write":"None"}},
   {"MemberType":"Function","Name":"Destroy"}]},
 {"Name":"BasePart","Superclass":"Instance","Tags":["NotCreatable"],"Members":[
   {"MemberType":"Property","Name":"Position","ValueType":{"Category":"DataType","Name":"Vector3"},"Security":"None"},
   {"MemberType":"Property","Name":"Transparency","ValueType":{"Category":"Primitive","Name":"float"},"Security":"None"},
   {"MemberType":"Property","Name":"Material","ValueType":{"Category":"Enum","Name":"Material"},"Security":"None"}]},
 {"Name":"Part","Superclass":"BasePart","Members":[]},
 {"Name":"Decal","Superclass":"Instance","Members":[
   {"MemberType":"Property","Name":"Texture","ValueType":{"Category":"DataType","Name":"Content"},"Security":"None"}]},
 {"Name":"Sound","Superclass":"Instance","Members":[
   {"MemberType":"Property","Name":"SoundId","ValueType":{"Category":"DataType","Name":"Content"},"Security":"None"}]},
 {"Name":"Workspace","Superclass":"Instance","Tags":["Service"],"Members":[
   {"MemberType":"Property","Name":"Gravity","ValueType":{"Category":"Primitive","Name":"float"},"Security":"None"}]}
],"Enums":[{"Name":"Material","Items":[{"Name":"Plastic","Value":256},{"Name":"Neon","Value":288}]}]}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_and_properties() {
        let d = ApiDump::from_json(TEST_DUMP).unwrap();
        assert!(d.is_a("Part", "Instance") && !d.is_a("Decal", "BasePart"));
        let names: Vec<_> = d.properties("Part").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Name", "Parent", "ClassName", "Position", "Transparency", "Material"]);
        assert!(d.property("Part", "ClassName").unwrap().read_only);
        assert_eq!(d.enum_item_name("Material", 288), Some("Neon"));
        assert_eq!(d.enum_value("Material", "Plastic"), Some(256));
    }

    #[test]
    fn search_ranks_and_filters() {
        let d = ApiDump::from_json(TEST_DUMP).unwrap();
        let r: Vec<_> = d.search("part", true, 10).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(r, ["Part"]);
        let r: Vec<_> = d.search("part", false, 10).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(r, ["Part", "BasePart"]);
        assert!(d.search("zzz", false, 10).is_empty());
    }

    #[test]
    fn coercion_rules() {
        let d = ApiDump::from_json(TEST_DUMP).unwrap();
        let t = d.property("Part", "Transparency").unwrap();
        assert_eq!(t.coerce(Value::Int(1)), Some(Value::Float(1.0)));
        assert_eq!(t.coerce(Value::String("x".into())), None);
        assert!(d.property("Part", "Material").unwrap().coerce(Value::Enum(288)).is_some());
        assert!(d.property("Part", "Material").unwrap().coerce(Value::Int(288)).is_none());
        assert!(d.property("Part", "Position").unwrap().coerce(Value::Vector2([0.0; 2])).is_none());
    }
}
