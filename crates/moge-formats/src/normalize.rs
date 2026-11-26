//! Maps Roblox serialization names to API names after import (and back on export).
use moge_core::{Id, Tree, Value};

/// (serialized name, API name)
const RENAMES: &[(&str, &str)] = &[("size", "Size"), ("shape", "Shape"), ("Color3uint8", "Color")];

pub fn serialized_name<'a>(class_is_part: bool, api: &'a str) -> &'a str {
    if !class_is_part {
        return api;
    }
    match api {
        "Size" => "size",
        "Shape" => "shape",
        "Color" => "Color3uint8",
        other => other,
    }
}

pub fn run(tree: &mut Tree, roots: &[Id]) {
    let mut all: Vec<Id> = Vec::new();
    for &r in roots {
        all.push(r);
        all.extend(tree.descendants(r));
    }
    for id in all {
        let Some(node) = tree.node_mut(id) else { continue };
        for (ser, api) in RENAMES {
            // Only part-like classes use these serialized names; GUI objects keep their own "Size".
            if *ser == "size" && !node.props.contains_key(*ser) {
                continue;
            }
            if let Some(v) = node.props.remove(*ser) {
                if *ser == "Color3uint8" && !matches!(v, Value::Color3(_)) {
                    node.props.insert((*ser).to_string(), v);
                    continue;
                }
                node.props.insert((*api).to_string(), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_part_properties() {
        let mut t = Tree::new();
        let p = t.create("Part");
        t.set_prop(p, "size", Value::Vector3([1.0, 2.0, 3.0])).unwrap();
        t.set_prop(p, "shape", Value::Enum(1)).unwrap();
        t.set_prop(p, "Color3uint8", Value::Color3([1.0, 0.0, 0.0])).unwrap();
        run(&mut t, &[p]);
        assert_eq!(t.get_prop(p, "Size"), Some(&Value::Vector3([1.0, 2.0, 3.0])));
        assert_eq!(t.get_prop(p, "Shape"), Some(&Value::Enum(1)));
        assert_eq!(t.get_prop(p, "Color"), Some(&Value::Color3([1.0, 0.0, 0.0])));
        assert!(t.get_prop(p, "size").is_none() && t.get_prop(p, "Color3uint8").is_none());
        assert_eq!(serialized_name(true, "Size"), "size");
        assert_eq!(serialized_name(false, "Size"), "Size");
    }
}
