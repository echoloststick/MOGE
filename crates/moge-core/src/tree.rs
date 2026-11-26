//! Instance tree (arena). Ids are never reused, so stale ids are detected instead of aliasing.
use crate::api::ApiDump;
use crate::assets;
use crate::value::{Id, Value};
use std::collections::{BTreeMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TreeError {
    Dead(Id),
    Cycle,
    RootImmutable,
    UnknownClass(String),
    NotCreatable(String),
    UnknownProperty { class: String, prop: String },
    ReadOnly { class: String, prop: String },
    TypeMismatch { class: String, prop: String, expected: String, got: String },
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::Dead(id) => write!(f, "Instance {id} no longer exists"),
            TreeError::Cycle => write!(f, "An instance cannot be parented to itself or to one of its descendants"),
            TreeError::RootImmutable => write!(f, "The DataModel root cannot be reparented or destroyed"),
            TreeError::UnknownClass(c) => write!(f, "Unknown class '{c}'"),
            TreeError::NotCreatable(c) => write!(f, "Class '{c}' cannot be created"),
            TreeError::UnknownProperty { class, prop } => write!(f, "'{prop}' is not a valid property of {class}"),
            TreeError::ReadOnly { class, prop } => write!(f, "Property {class}.{prop} is read-only"),
            TreeError::TypeMismatch { class, prop, expected, got } => {
                write!(f, "Property {class}.{prop} expects {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for TreeError {}

#[derive(Debug, Clone)]
pub struct Node {
    pub class: String,
    pub parent: Option<Id>,
    pub children: Vec<Id>,
    pub props: BTreeMap<String, Value>,
    pub attributes: BTreeMap<String, Value>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<Option<Node>>,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub const ROOT: Id = 0;

    pub fn new() -> Tree {
        Tree { nodes: vec![Some(Node::new("DataModel"))] }
    }

    /// Creates a detached instance.
    pub fn create(&mut self, class: &str) -> Id {
        self.nodes.push(Some(Node::new(class)));
        self.nodes.len() - 1
    }

    pub fn create_checked(&mut self, dump: &ApiDump, class: &str, parent: Id) -> Result<Id, TreeError> {
        let info = dump.class(class).ok_or_else(|| TreeError::UnknownClass(class.to_string()))?;
        if !info.creatable {
            return Err(TreeError::NotCreatable(class.to_string()));
        }
        self.require(parent)?;
        let id = self.create(class);
        self.set_parent(id, Some(parent))?;
        Ok(id)
    }

    pub fn node(&self, id: Id) -> Option<&Node> {
        self.nodes.get(id).and_then(|n| n.as_ref())
    }

    pub fn node_mut(&mut self, id: Id) -> Option<&mut Node> {
        self.nodes.get_mut(id).and_then(|n| n.as_mut())
    }

    pub fn is_alive(&self, id: Id) -> bool {
        self.node(id).is_some()
    }

    fn require(&self, id: Id) -> Result<&Node, TreeError> {
        self.node(id).ok_or(TreeError::Dead(id))
    }

    pub fn name(&self, id: Id) -> Option<&str> {
        let n = self.node(id)?;
        match n.props.get("Name") {
            Some(Value::String(s)) => Some(s),
            _ => Some(&n.class),
        }
    }

    pub fn set_parent(&mut self, id: Id, parent: Option<Id>) -> Result<(), TreeError> {
        self.attach(id, parent, None)
    }

    /// Moves `id` under `parent` at `index` (append when None). Used by undo to restore exact positions.
    pub fn attach(&mut self, id: Id, parent: Option<Id>, index: Option<usize>) -> Result<(), TreeError> {
        if id == Self::ROOT {
            return Err(TreeError::RootImmutable);
        }
        let old = self.require(id)?.parent;
        if let Some(p) = parent {
            self.require(p)?;
            let mut cur = Some(p);
            while let Some(c) = cur {
                if c == id {
                    return Err(TreeError::Cycle);
                }
                cur = self.node(c).and_then(|n| n.parent);
            }
        }
        if let Some(o) = old {
            if let Some(n) = self.node_mut(o) {
                n.children.retain(|c| *c != id);
            }
        }
        if let Some(p) = parent {
            if let Some(n) = self.node_mut(p) {
                let at = index.unwrap_or(n.children.len()).min(n.children.len());
                n.children.insert(at, id);
            }
        }
        if let Some(n) = self.node_mut(id) {
            n.parent = parent;
        }
        Ok(())
    }

    pub fn index_in_parent(&self, id: Id) -> Option<usize> {
        let p = self.node(id)?.parent?;
        self.node(p)?.children.iter().position(|c| *c == id)
    }

    /// True when the instance is reachable from the DataModel (not deleted/detached).
    pub fn is_attached(&self, id: Id) -> bool {
        let mut cur = Some(id);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == Self::ROOT {
                return true;
            }
            cur = self.node(c).and_then(|n| n.parent);
            guard += 1;
            if guard > 100_000 {
                return false;
            }
        }
        false
    }

    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.nodes.iter().enumerate().filter(|(_, n)| n.is_some()).map(|(i, _)| i)
    }

    pub fn remove_prop(&mut self, id: Id, name: &str) -> Option<Value> {
        self.node_mut(id)?.props.remove(name)
    }

    /// Deep copy (detached). Ref properties that point inside the copied subtree are remapped.
    pub fn clone_subtree(&mut self, id: Id) -> Result<Id, TreeError> {
        self.require(id)?;
        let mut old: Vec<Id> = vec![id];
        old.extend(self.descendants(id));
        let mut map = std::collections::HashMap::new();
        for &o in &old {
            let n = self.node(o).cloned().ok_or(TreeError::Dead(o))?;
            let new = self.create(&n.class);
            if let Some(nn) = self.node_mut(new) {
                nn.props = n.props;
                nn.attributes = n.attributes;
                nn.tags = n.tags;
            }
            map.insert(o, new);
        }
        for &o in &old {
            let new = map[&o];
            if let Some(Some(p)) = self.node(o).map(|n| n.parent) {
                if let Some(np) = map.get(&p) {
                    self.attach(new, Some(*np), None)?;
                }
            }
            if let Some(n) = self.node_mut(new) {
                for v in n.props.values_mut() {
                    if let Value::Ref(Some(r)) = v {
                        if let Some(m) = map.get(r) {
                            *v = Value::Ref(Some(*m));
                        }
                    }
                }
            }
        }
        Ok(map[&id])
    }

    /// Destroys an instance and its subtree; Ref properties pointing into it become nil.
    pub fn destroy(&mut self, id: Id) -> Result<(), TreeError> {
        if id == Self::ROOT {
            return Err(TreeError::RootImmutable);
        }
        self.require(id)?;
        let mut dead: Vec<Id> = self.descendants(id);
        dead.push(id);
        self.set_parent(id, None)?;
        let set: HashSet<Id> = dead.iter().copied().collect();
        for d in &dead {
            self.nodes[*d] = None;
        }
        for n in self.nodes.iter_mut().flatten() {
            for v in n.props.values_mut() {
                if let Value::Ref(Some(r)) = v {
                    if set.contains(r) {
                        *v = Value::Ref(None);
                    }
                }
            }
        }
        Ok(())
    }

    /// Pre-order descendants (excluding `id`).
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack: Vec<Id> = self.node(id).map(|n| n.children.iter().rev().copied().collect()).unwrap_or_default();
        while let Some(c) = stack.pop() {
            out.push(c);
            if let Some(n) = self.node(c) {
                stack.extend(n.children.iter().rev().copied());
            }
        }
        out
    }

    pub fn find_first_child(&self, id: Id, name: &str) -> Option<Id> {
        self.node(id)?.children.iter().copied().find(|c| self.name(*c) == Some(name))
    }

    pub fn find_first_child_of_class(&self, id: Id, class: &str) -> Option<Id> {
        self.node(id)?.children.iter().copied().find(|c| self.node(*c).map(|n| n.class == class).unwrap_or(false))
    }

    /// Roblox-style full name ("Workspace.Model.Part"); the DataModel is omitted.
    pub fn full_name(&self, id: Id) -> Option<String> {
        self.require(id).ok()?;
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c != Self::ROOT {
                parts.push(self.name(c)?.to_string());
            }
            cur = self.node(c)?.parent;
        }
        parts.reverse();
        Some(parts.join("."))
    }

    /// Unchecked write. "Parent" is routed through `set_parent`.
    pub fn set_prop(&mut self, id: Id, name: &str, value: Value) -> Result<(), TreeError> {
        if name == "Parent" {
            return match value {
                Value::Ref(p) => self.set_parent(id, p),
                _ => Err(TreeError::TypeMismatch {
                    class: self.require(id)?.class.clone(),
                    prop: "Parent".into(),
                    expected: "Instance".into(),
                    got: value.type_name().into(),
                }),
            };
        }
        self.nodes.get_mut(id).and_then(|n| n.as_mut()).ok_or(TreeError::Dead(id))?.props.insert(name.to_string(), value);
        Ok(())
    }

    pub fn get_prop(&self, id: Id, name: &str) -> Option<&Value> {
        self.node(id)?.props.get(name)
    }

    /// Validates and converts a value for `prop` (type check, int->float, image id -> rbxthumb).
    pub fn prepare_value(&self, dump: &ApiDump, id: Id, prop: &str, value: Value) -> Result<Value, TreeError> {
        let class = self.require(id)?.class.clone();
        let info = dump
            .property(&class, prop)
            .ok_or_else(|| TreeError::UnknownProperty { class: class.clone(), prop: prop.to_string() })?;
        if info.read_only {
            return Err(TreeError::ReadOnly { class, prop: prop.to_string() });
        }
        let got = value.type_name().to_string();
        let mut v = info.coerce(value).ok_or_else(|| TreeError::TypeMismatch {
            class: class.clone(),
            prop: prop.to_string(),
            expected: info.value_type.clone(),
            got,
        })?;
        if info.value_type == "Content" && assets::is_image_property(prop) {
            let converted = if let Value::String(s) = &v { assets::to_rbxthumb(s) } else { None };
            if let Some(t) = converted {
                v = Value::String(t);
            }
        }
        Ok(v)
    }

    /// Editor-facing write: validates against the API Dump, coerces the value and converts
    /// image ids (rbxassetid, URLs, bare numbers) into rbxthumb URIs.
    pub fn set_prop_checked(&mut self, dump: &ApiDump, id: Id, prop: &str, value: Value) -> Result<(), TreeError> {
        let v = self.prepare_value(dump, id, prop, value)?;
        self.set_prop(id, prop, v)
    }

    /// Current value, or the zero value of the declared type when unset.
    pub fn get_or_default(&self, dump: &ApiDump, id: Id, prop: &str) -> Option<Value> {
        let n = self.node(id)?;
        if let Some(v) = n.props.get(prop) {
            return Some(v.clone());
        }
        let info = dump.property(&n.class, prop)?;
        match info.category.as_str() {
            "Enum" => Some(Value::Enum(0)),
            "Class" => Some(Value::Ref(None)),
            _ => Value::default_for(&info.value_type),
        }
    }

    /// Attaches imported roots to the game. DataModel roots are unwrapped; service roots are
    /// merged into the existing service (children moved, properties overwritten); anything else
    /// becomes a direct child of the game. Returns the ids that now live under the game.
    pub fn merge_place(&mut self, dump: &ApiDump, roots: &[Id]) -> Vec<Id> {
        let mut out = Vec::new();
        for &r in roots {
            let Some(class) = self.node(r).map(|n| n.class.clone()) else { continue };
            if class == "DataModel" {
                let kids = self.node(r).map(|n| n.children.clone()).unwrap_or_default();
                out.extend(self.merge_place(dump, &kids));
                let _ = self.destroy(r);
                continue;
            }
            let is_service = dump.class(&class).map(|c| c.service).unwrap_or(false);
            let existing = if is_service { self.find_first_child_of_class(Self::ROOT, &class) } else { None };
            match existing {
                Some(e) if e != r => {
                    let (kids, props, tags) = {
                        let n = self.node(r).unwrap();
                        (n.children.clone(), n.props.clone(), n.tags.clone())
                    };
                    for k in kids {
                        let _ = self.set_parent(k, Some(e));
                    }
                    if let Some(en) = self.node_mut(e) {
                        for (k, v) in props {
                            if k != "Name" {
                                en.props.insert(k, v);
                            }
                        }
                        for t in tags {
                            if !en.tags.contains(&t) {
                                en.tags.push(t);
                            }
                        }
                    }
                    let _ = self.destroy(r);
                    out.push(e);
                }
                _ => {
                    if self.set_parent(r, Some(Self::ROOT)).is_ok() {
                        out.push(r);
                    }
                }
            }
        }
        out
    }
}

impl Node {
    fn new(class: &str) -> Node {
        Node { class: class.to_string(), parent: None, children: Vec::new(), props: BTreeMap::new(), attributes: BTreeMap::new(), tags: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TEST_DUMP;

    #[test]
    fn parenting_cycles_and_names() {
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let m = t.create("Model");
        let p = t.create("Part");
        t.set_parent(m, Some(ws)).unwrap();
        t.set_parent(p, Some(m)).unwrap();
        t.set_prop(p, "Name", Value::String("Brick".into())).unwrap();
        assert_eq!(t.full_name(p).unwrap(), "Workspace.Model.Brick");
        assert_eq!(t.set_parent(m, Some(p)), Err(TreeError::Cycle));
        assert_eq!(t.set_parent(m, Some(m)), Err(TreeError::Cycle));
        assert_eq!(t.set_parent(Tree::ROOT, Some(ws)), Err(TreeError::RootImmutable));
        assert_eq!(t.descendants(ws), vec![m, p]);
        assert_eq!(t.find_first_child(m, "Brick"), Some(p));
    }

    #[test]
    fn destroy_clears_refs_and_rejects_stale_ids() {
        let mut t = Tree::new();
        let m = t.create("Model");
        let p = t.create("Part");
        t.set_parent(m, Some(Tree::ROOT)).unwrap();
        t.set_parent(p, Some(m)).unwrap();
        t.set_prop(m, "PrimaryPart", Value::Ref(Some(p))).unwrap();
        t.destroy(p).unwrap();
        assert!(!t.is_alive(p));
        assert_eq!(t.get_prop(m, "PrimaryPart"), Some(&Value::Ref(None)));
        assert!(t.node(m).unwrap().children.is_empty());
        assert_eq!(t.set_parent(p, Some(m)), Err(TreeError::Dead(p)));
        let q = t.create("Part");
        assert_ne!(q, p, "ids must not be reused");
    }

    #[test]
    fn checked_writes_convert_images_only() {
        let d = ApiDump::from_json(TEST_DUMP).unwrap();
        let mut t = Tree::new();
        let decal = t.create_checked(&d, "Decal", Tree::ROOT).unwrap();
        let sound = t.create_checked(&d, "Sound", Tree::ROOT).unwrap();
        t.set_prop_checked(&d, decal, "Texture", Value::String("rbxassetid://5".into())).unwrap();
        t.set_prop_checked(&d, sound, "SoundId", Value::String("rbxassetid://5".into())).unwrap();
        assert_eq!(t.get_prop(decal, "Texture"), Some(&Value::String("rbxthumb://type=Asset&id=5&w=420&h=420".into())));
        assert_eq!(t.get_prop(sound, "SoundId"), Some(&Value::String("rbxassetid://5".into())));
        assert!(matches!(t.set_prop_checked(&d, decal, "ClassName", Value::String("x".into())), Err(TreeError::ReadOnly { .. })));
        assert!(matches!(t.set_prop_checked(&d, decal, "Nope", Value::Bool(true)), Err(TreeError::UnknownProperty { .. })));
        assert!(matches!(t.create_checked(&d, "BasePart", Tree::ROOT), Err(TreeError::NotCreatable(_))));
        assert!(matches!(t.create_checked(&d, "Zzz", Tree::ROOT), Err(TreeError::UnknownClass(_))));
        t.set_prop_checked(&d, sound, "Parent", Value::Ref(Some(decal))).unwrap();
        assert_eq!(t.node(sound).unwrap().parent, Some(decal));
    }

    #[test]
    fn defaults_and_merge_place() {
        let d = ApiDump::from_json(TEST_DUMP).unwrap();
        let mut t = Tree::new();
        let ws = t.create_checked(&d, "Workspace", Tree::ROOT).unwrap();
        let p = t.create_checked(&d, "Part", ws).unwrap();
        assert_eq!(t.get_or_default(&d, p, "Position"), Some(Value::Vector3([0.0; 3])));
        assert_eq!(t.get_or_default(&d, p, "Material"), Some(Value::Enum(0)));

        let dm = t.create("DataModel");
        let ws2 = t.create("Workspace");
        let part2 = t.create("Part");
        t.set_parent(ws2, Some(dm)).unwrap();
        t.set_parent(part2, Some(ws2)).unwrap();
        t.set_prop(ws2, "Gravity", Value::Float(50.0)).unwrap();
        let loose = t.create("Part");
        let merged = t.merge_place(&d, &[dm, loose]);
        assert_eq!(merged, vec![ws, loose]);
        assert_eq!(t.node(ws).unwrap().children, vec![p, part2]);
        assert_eq!(t.get_prop(ws, "Gravity"), Some(&Value::Float(50.0)));
        assert_eq!(t.node(loose).unwrap().parent, Some(Tree::ROOT));
        assert!(!t.is_alive(dm) && !t.is_alive(ws2));
    }
}
