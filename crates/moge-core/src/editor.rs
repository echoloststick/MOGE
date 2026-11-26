//! Editing session: selection, undoable commands, clipboard and Properties-panel rows.
//!
//! Deleting never destroys immediately: the instance is detached (like `Parent = nil`) so undo can restore it
//! at the exact index. Detached instances are destroyed when their history entry is discarded.
use crate::api::ApiDump;
use crate::template;
use crate::tree::Tree;
use crate::value::{Id, Value, IDENTITY_ROT};
use moge_math::{euler_yxz_to_rot, rot_to_euler_yxz};
use std::sync::Arc;

type R<T> = Result<T, String>;

const MAX_HISTORY: usize = 200;

#[derive(Clone, Debug)]
enum Cmd {
    Set { id: Id, name: String, old: Option<Value>, new: Value },
    Move { id: Id, from: (Option<Id>, usize), to: (Option<Id>, usize) },
    Insert { id: Id, parent: Id, index: usize },
    Delete { id: Id, parent: Id, index: usize },
}

struct Entry {
    label: String,
    cmds: Vec<Cmd>,
}

/// One row of the Properties panel.
#[derive(Clone, Debug)]
pub struct PropRow {
    pub name: String,
    pub type_name: String,
    /// "Primitive", "Enum", "Class" or "DataType".
    pub category: String,
    pub group: String,
    pub value: Value,
    pub read_only: bool,
}

pub struct Editor {
    pub tree: Tree,
    pub dump: Arc<ApiDump>,
    pub selection: Vec<Id>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    clipboard: Vec<Id>,
    pub dirty: bool,
    /// Incremented on every tree change; UIs use it to know when to rebuild caches.
    pub revision: u64,
    live: bool,
}

impl Editor {
    pub fn new(tree: Tree, dump: Arc<ApiDump>) -> Editor {
        Editor { tree, dump, selection: Vec::new(), undo: Vec::new(), redo: Vec::new(), clipboard: Vec::new(), dirty: false, revision: 0, live: false }
    }

    pub fn replace_tree(&mut self, tree: Tree) {
        self.tree = tree;
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
        self.clipboard.clear();
        self.dirty = false;
        self.revision += 1;
    }

    fn is_service(&self, id: Id) -> bool {
        self.tree.node(id).and_then(|n| self.dump.class(&n.class)).map(|c| c.service).unwrap_or(false)
    }

    // ---------------------------------------------------------------- selection

    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids.into_iter().filter(|i| *i != Tree::ROOT && self.tree.is_attached(*i)).collect();
    }

    pub fn toggle_select(&mut self, id: Id) {
        if let Some(p) = self.selection.iter().position(|s| *s == id) {
            self.selection.remove(p);
        } else if id != Tree::ROOT && self.tree.is_attached(id) {
            self.selection.push(id);
        }
    }

    fn prune_selection(&mut self) {
        let keep: Vec<Id> = self.selection.iter().copied().filter(|i| self.tree.is_attached(*i)).collect();
        self.selection = keep;
    }

    // ---------------------------------------------------------------- history plumbing

    fn apply(&mut self, c: &Cmd, forward: bool) -> R<()> {
        let e = |x: crate::tree::TreeError| x.to_string();
        match (c, forward) {
            (Cmd::Set { id, name, new, .. }, true) => self.tree.set_prop(*id, name, new.clone()).map_err(e),
            (Cmd::Set { id, name, old, .. }, false) => match old {
                Some(v) => self.tree.set_prop(*id, name, v.clone()).map_err(e),
                None => {
                    self.tree.remove_prop(*id, name);
                    Ok(())
                }
            },
            (Cmd::Move { id, to, .. }, true) => self.tree.attach(*id, to.0, Some(to.1)).map_err(e),
            (Cmd::Move { id, from, .. }, false) => self.tree.attach(*id, from.0, Some(from.1)).map_err(e),
            (Cmd::Insert { id, parent, index }, true) => self.tree.attach(*id, Some(*parent), Some(*index)).map_err(e),
            (Cmd::Insert { id, .. }, false) => self.tree.attach(*id, None, None).map_err(e),
            (Cmd::Delete { id, .. }, true) => self.tree.attach(*id, None, None).map_err(e),
            (Cmd::Delete { id, parent, index }, false) => self.tree.attach(*id, Some(*parent), Some(*index)).map_err(e),
        }
    }

    fn do_cmd(&mut self, cmds: &mut Vec<Cmd>, c: Cmd) -> R<()> {
        self.apply(&c, true)?;
        cmds.push(c);
        Ok(())
    }

    fn drop_entry(&mut self, e: Entry) {
        for c in e.cmds {
            if let Cmd::Insert { id, .. } | Cmd::Delete { id, .. } = c {
                if id != Tree::ROOT && self.tree.node(id).map(|n| n.parent.is_none()).unwrap_or(false) {
                    let _ = self.tree.destroy(id);
                }
            }
        }
    }

    fn commit(&mut self, label: &str, cmds: Vec<Cmd>) {
        if cmds.is_empty() {
            return;
        }
        for e in std::mem::take(&mut self.redo) {
            self.drop_entry(e);
        }
        self.undo.push(Entry { label: label.to_string(), cmds });
        if self.undo.len() > MAX_HISTORY {
            let old = self.undo.remove(0);
            self.drop_entry(old);
        }
        self.dirty = true;
        self.revision += 1;
        self.prune_selection();
    }

    fn transact<F>(&mut self, label: &str, f: F) -> R<()>
    where
        F: FnOnce(&mut Editor, &mut Vec<Cmd>) -> R<()>,
    {
        self.live = false;
        let mut cmds = Vec::new();
        match f(self, &mut cmds) {
            Ok(()) => {
                self.commit(label, cmds);
                Ok(())
            }
            Err(e) => {
                for c in cmds.iter().rev() {
                    let _ = self.apply(c, false);
                }
                self.drop_entry(Entry { label: String::new(), cmds });
                Err(e)
            }
        }
    }

    /// Runs an edit that replaces the previous live edit instead of stacking a new undo step.
    /// Use it for drags and typing; call `live_end` when the gesture finishes.
    pub fn live_edit<F: FnOnce(&mut Editor) -> R<()>>(&mut self, f: F) -> R<()> {
        if self.live {
            self.undo();
            self.redo.clear();
            self.live = false;
        }
        let before = self.undo.len();
        let r = f(self);
        self.live = self.undo.len() > before;
        r
    }

    pub fn live_end(&mut self) {
        self.live = false;
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn undo(&mut self) -> Option<String> {
        let e = self.undo.pop()?;
        for c in e.cmds.iter().rev() {
            let _ = self.apply(c, false);
        }
        let label = e.label.clone();
        self.redo.push(e);
        self.dirty = true;
        self.revision += 1;
        self.prune_selection();
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let e = self.redo.pop()?;
        for c in e.cmds.iter() {
            let _ = self.apply(c, true);
        }
        let label = e.label.clone();
        self.undo.push(e);
        self.dirty = true;
        self.revision += 1;
        self.prune_selection();
        Some(label)
    }

    // ---------------------------------------------------------------- structure edits

    fn check_parent(&self, parent: Id) -> R<()> {
        if !self.tree.is_alive(parent) {
            return Err("The target parent no longer exists".into());
        }
        if parent == Tree::ROOT {
            return Err("Instances must be parented under a service or another instance".into());
        }
        Ok(())
    }

    /// Creates a new instance with sensible defaults and selects it.
    pub fn insert(&mut self, class: &str, parent: Id, pos: [f32; 3]) -> R<Id> {
        let info = self.dump.class(class).ok_or_else(|| format!("Unknown class '{class}'"))?;
        if !info.creatable {
            return Err(format!("Class '{class}' cannot be created"));
        }
        self.check_parent(parent)?;
        let dump = self.dump.clone();
        let mut made = 0;
        self.transact(&format!("Insert {class}"), |ed, cmds| {
            let id = ed.tree.create(class);
            ed.tree.set_prop(id, "Name", Value::String(class.to_string())).map_err(|e| e.to_string())?;
            for (k, v) in template::insert_defaults(class, pos) {
                if dump.property(class, k).is_some() {
                    ed.tree.set_prop(id, k, v).map_err(|e| e.to_string())?;
                }
            }
            let index = ed.tree.node(parent).map(|n| n.children.len()).unwrap_or(0);
            ed.do_cmd(cmds, Cmd::Insert { id, parent, index })?;
            made = id;
            Ok(())
        })?;
        self.selection = vec![made];
        Ok(made)
    }

    /// Top-level subset of `ids`: alive, not services, with no selected ancestor.
    fn top_level(&self, ids: &[Id]) -> Vec<Id> {
        ids.iter()
            .copied()
            .filter(|i| *i != Tree::ROOT && self.tree.is_attached(*i) && !self.is_service(*i))
            .filter(|i| {
                let mut cur = self.tree.node(*i).and_then(|n| n.parent);
                while let Some(c) = cur {
                    if ids.contains(&c) {
                        return false;
                    }
                    cur = self.tree.node(c).and_then(|n| n.parent);
                }
                true
            })
            .collect()
    }

    fn detach(&mut self, cmds: &mut Vec<Cmd>, id: Id) -> R<()> {
        let parent = self.tree.node(id).and_then(|n| n.parent).ok_or("Instance is already detached")?;
        let index = self.tree.index_in_parent(id).unwrap_or(0);
        self.do_cmd(cmds, Cmd::Delete { id, parent, index })
    }

    pub fn delete(&mut self, ids: &[Id]) -> R<usize> {
        let tops = self.top_level(ids);
        if tops.is_empty() {
            return Err("Nothing to delete (services cannot be deleted)".into());
        }
        let n = tops.len();
        self.transact(&format!("Delete {n} instance(s)"), |ed, cmds| {
            for id in &tops {
                ed.detach(cmds, *id)?;
            }
            Ok(())
        })?;
        Ok(n)
    }

    fn move_to(&mut self, cmds: &mut Vec<Cmd>, id: Id, parent: Id, index: Option<usize>) -> R<()> {
        let from = (self.tree.node(id).and_then(|n| n.parent), self.tree.index_in_parent(id).unwrap_or(0));
        let at = index.unwrap_or_else(|| self.tree.node(parent).map(|n| n.children.len()).unwrap_or(0));
        // Moving inside the same parent: the index refers to the list after removal.
        self.do_cmd(cmds, Cmd::Move { id, from, to: (Some(parent), at) })?;
        let actual = self.tree.index_in_parent(id).unwrap_or(at);
        if let Some(Cmd::Move { to, .. }) = cmds.last_mut() {
            to.1 = actual;
        }
        Ok(())
    }

    pub fn reparent(&mut self, ids: &[Id], new_parent: Id, index: Option<usize>) -> R<()> {
        self.check_parent(new_parent)?;
        let tops = self.top_level(ids);
        if tops.is_empty() {
            return Err("Nothing to move".into());
        }
        self.transact("Reparent", |ed, cmds| {
            for (k, id) in tops.iter().enumerate() {
                ed.move_to(cmds, *id, new_parent, index.map(|i| i + k))?;
            }
            Ok(())
        })
    }

    fn clone_into(&mut self, cmds: &mut Vec<Cmd>, src: Id, parent: Id, index: usize) -> R<Id> {
        let copy = self.tree.clone_subtree(src).map_err(|e| e.to_string())?;
        self.do_cmd(cmds, Cmd::Insert { id: copy, parent, index })?;
        Ok(copy)
    }

    pub fn duplicate(&mut self, ids: &[Id]) -> R<Vec<Id>> {
        let tops = self.top_level(ids);
        if tops.is_empty() {
            return Err("Nothing to duplicate".into());
        }
        let mut out = Vec::new();
        self.transact("Duplicate", |ed, cmds| {
            for id in &tops {
                let parent = ed.tree.node(*id).and_then(|n| n.parent).ok_or("Instance has no parent")?;
                let at = ed.tree.index_in_parent(*id).map(|i| i + 1).unwrap_or(0);
                out.push(ed.clone_into(cmds, *id, parent, at)?);
            }
            Ok(())
        })?;
        self.selection = out.clone();
        Ok(out)
    }

    pub fn copy(&mut self, ids: &[Id]) -> R<usize> {
        let tops = self.top_level(ids);
        if tops.is_empty() {
            return Err("Nothing to copy".into());
        }
        for old in std::mem::take(&mut self.clipboard) {
            let _ = self.tree.destroy(old);
        }
        for id in &tops {
            let c = self.tree.clone_subtree(*id).map_err(|e| e.to_string())?;
            self.clipboard.push(c);
        }
        Ok(tops.len())
    }

    pub fn cut(&mut self, ids: &[Id]) -> R<usize> {
        let n = self.copy(ids)?;
        self.delete(ids)?;
        Ok(n)
    }

    pub fn has_clipboard(&self) -> bool {
        !self.clipboard.is_empty()
    }

    pub fn paste(&mut self, parent: Id) -> R<Vec<Id>> {
        self.check_parent(parent)?;
        if self.clipboard.is_empty() {
            return Err("The clipboard is empty".into());
        }
        let src = self.clipboard.clone();
        let mut out = Vec::new();
        self.transact("Paste", |ed, cmds| {
            for s in &src {
                let at = ed.tree.node(parent).map(|n| n.children.len()).unwrap_or(0);
                out.push(ed.clone_into(cmds, *s, parent, at)?);
            }
            Ok(())
        })?;
        self.selection = out.clone();
        Ok(out)
    }

    /// Wraps the selection in a new Model (Ctrl+G).
    pub fn group(&mut self, ids: &[Id]) -> R<Id> {
        let tops = self.top_level(ids);
        let first = *tops.first().ok_or("Nothing to group")?;
        let parent = self.tree.node(first).and_then(|n| n.parent).ok_or("Instance has no parent")?;
        self.check_parent(parent)?;
        let mut model = 0;
        self.transact("Group", |ed, cmds| {
            let m = ed.tree.create("Model");
            ed.tree.set_prop(m, "Name", Value::String("Model".into())).map_err(|e| e.to_string())?;
            let index = ed.tree.index_in_parent(first).unwrap_or(0);
            ed.do_cmd(cmds, Cmd::Insert { id: m, parent, index })?;
            for id in &tops {
                ed.move_to(cmds, *id, m, None)?;
            }
            model = m;
            Ok(())
        })?;
        self.selection = vec![model];
        Ok(model)
    }

    /// Moves the children of each Model/Folder to its parent and removes the container (Ctrl+U).
    pub fn ungroup(&mut self, ids: &[Id]) -> R<Vec<Id>> {
        let tops = self.top_level(ids);
        let mut freed = Vec::new();
        self.transact("Ungroup", |ed, cmds| {
            for g in &tops {
                let class = ed.tree.node(*g).map(|n| n.class.clone()).unwrap_or_default();
                if !matches!(class.as_str(), "Model" | "Folder") {
                    continue;
                }
                let parent = ed.tree.node(*g).and_then(|n| n.parent).ok_or("Instance has no parent")?;
                let kids = ed.tree.node(*g).map(|n| n.children.clone()).unwrap_or_default();
                let mut at = ed.tree.index_in_parent(*g).unwrap_or(0);
                for k in kids {
                    ed.move_to(cmds, k, parent, Some(at))?;
                    at += 1;
                    freed.push(k);
                }
                ed.detach(cmds, *g)?;
            }
            if freed.is_empty() {
                return Err("Select a Model or Folder that contains children".into());
            }
            Ok(())
        })?;
        self.selection = freed.clone();
        Ok(freed)
    }

    /// Creates a script from a loaded file under `parent` (one undo step) and selects it.
    pub fn import_script(&mut self, file: &crate::script_file::ScriptFile, parent: Id) -> R<Id> {
        let info = self.dump.class(file.class).ok_or_else(|| format!("Unknown class '{}'", file.class))?;
        if !info.creatable {
            return Err(format!("Class '{}' cannot be created", file.class));
        }
        self.check_parent(parent)?;
        let mut made = 0;
        self.transact(&format!("Import {}", file.name), |ed, cmds| {
            let id = ed.tree.create(file.class);
            ed.tree.set_prop(id, "Name", Value::String(file.name.clone())).map_err(|e| e.to_string())?;
            ed.tree.set_prop(id, "Source", Value::String(file.source.clone())).map_err(|e| e.to_string())?;
            let index = ed.tree.node(parent).map(|n| n.children.len()).unwrap_or(0);
            ed.do_cmd(cmds, Cmd::Insert { id, parent, index })?;
            made = id;
            Ok(())
        })?;
        self.selection = vec![made];
        Ok(made)
    }

    /// Replaces the Source of an existing script from file bytes (one undo step).
    pub fn load_source(&mut self, id: Id, bytes: &[u8]) -> R<()> {
        let class = self.tree.node(id).map(|n| n.class.clone()).ok_or("The script no longer exists")?;
        if !matches!(class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
            return Err(format!("{class} is not a script"));
        }
        if bytes.len() > crate::script_file::MAX_BYTES {
            return Err("File is too large".into());
        }
        let src = crate::script_file::decode_text(bytes)?;
        self.set_value(&[id], "Source", Value::String(src))
    }

    // ---------------------------------------------------------------- properties

    pub fn rename(&mut self, id: Id, name: &str) -> R<()> {
        self.set_value(&[id], "Name", Value::String(name.to_string()))
    }

    fn cframe_of(&self, id: Id) -> ([f32; 3], [f32; 9]) {
        match self.tree.get_prop(id, "CFrame") {
            Some(Value::CFrame { pos, rot }) => (*pos, *rot),
            _ => ([0.0; 3], IDENTITY_ROT),
        }
    }

    fn is_part(&self, id: Id) -> bool {
        self.tree.node(id).map(|n| self.dump.is_a(&n.class, "BasePart") || template::is_base_part(&n.class)).unwrap_or(false)
    }

    /// Value shown in the Properties panel, including the virtual Position/Orientation of parts.
    pub fn get_value(&self, id: Id, name: &str) -> Option<Value> {
        if self.is_part(id) && (name == "Position" || name == "Orientation") {
            let (pos, rot) = self.cframe_of(id);
            return Some(Value::Vector3(if name == "Position" {
                pos
            } else {
                let e = rot_to_euler_yxz(&rot);
                [e[0].to_degrees(), e[1].to_degrees(), e[2].to_degrees()]
            }));
        }
        self.tree.get_or_default(&self.dump, id, name)
    }

    /// Sets a property on every id (one undo step). Ids whose class lacks the property are skipped; an error is
    /// returned only when no id accepted the change.
    pub fn set_value(&mut self, ids: &[Id], name: &str, value: Value) -> R<()> {
        let targets: Vec<Id> = ids.iter().copied().filter(|i| self.tree.is_alive(*i)).collect();
        if targets.is_empty() {
            return Err("No valid instance selected".into());
        }
        if name == "Parent" {
            return match value {
                Value::Ref(Some(p)) => self.reparent(&targets, p, None),
                _ => Err("Parent must be an instance".into()),
            };
        }
        let mut planned: Vec<(Id, String, Value)> = Vec::new();
        let mut first_err: Option<String> = None;
        for id in &targets {
            let r: R<(String, Value)> = if self.is_part(*id) && (name == "Position" || name == "Orientation") {
                match &value {
                    Value::Vector3(v) => {
                        let (pos, rot) = self.cframe_of(*id);
                        let nv = if name == "Position" {
                            Value::CFrame { pos: *v, rot }
                        } else {
                            Value::CFrame { pos, rot: euler_yxz_to_rot(v[0].to_radians(), v[1].to_radians(), v[2].to_radians()) }
                        };
                        Ok(("CFrame".to_string(), nv))
                    }
                    other => Err(format!("Property {name} expects Vector3, got {}", other.type_name())),
                }
            } else {
                self.tree.prepare_value(&self.dump, *id, name, value.clone()).map(|v| (name.to_string(), v)).map_err(|e| e.to_string())
            };
            match r {
                Ok((n, v)) => planned.push((*id, n, v)),
                Err(e) => first_err = first_err.or(Some(e)),
            }
        }
        if planned.is_empty() {
            return Err(first_err.unwrap_or_else(|| "Property could not be set".into()));
        }
        self.transact(&format!("Set {name}"), |ed, cmds| {
            for (id, n, v) in planned {
                let old = ed.tree.get_prop(id, &n).cloned();
                if old.as_ref() == Some(&v) {
                    continue;
                }
                ed.do_cmd(cmds, Cmd::Set { id, name: n, old, new: v })?;
            }
            Ok(())
        })
    }

    /// Sets different property values on different instances as one undo step (viewport drags).
    pub fn set_many(&mut self, label: &str, items: Vec<(Id, String, Value)>) -> R<()> {
        let mut planned = Vec::new();
        for (id, name, v) in items {
            let v = self.tree.prepare_value(&self.dump, id, &name, v).map_err(|e| e.to_string())?;
            planned.push((id, name, v));
        }
        self.transact(label, |ed, cmds| {
            for (id, n, v) in planned {
                let old = ed.tree.get_prop(id, &n).cloned();
                if old.as_ref() != Some(&v) {
                    ed.do_cmd(cmds, Cmd::Set { id, name: n, old, new: v })?;
                }
            }
            Ok(())
        })
    }

    /// Call after writing to `tree` directly (live previews) so caches rebuild.
    pub fn mark_changed(&mut self) {
        self.revision += 1;
    }

    /// Rows for the Properties panel, grouped (Appearance, Data, then the rest alphabetically).
    pub fn properties(&self, id: Id) -> Vec<PropRow> {
        let Some(node) = self.tree.node(id) else { return Vec::new() };
        let mut rows: Vec<PropRow> = Vec::new();
        for p in self.dump.properties(&node.class) {
            if p.hidden || p.deprecated || p.name == "Parent" {
                continue;
            }
            let Some(value) = self.get_value(id, &p.name) else { continue };
            rows.push(PropRow {
                name: p.name.clone(),
                type_name: p.value_type.clone(),
                category: p.category.clone(),
                group: p.group.clone(),
                value,
                read_only: p.read_only,
            });
        }
        if self.is_part(id) {
            for n in ["Position", "Orientation"] {
                if !rows.iter().any(|r| r.name == n) {
                    if let Some(value) = self.get_value(id, n) {
                        rows.push(PropRow { name: n.into(), type_name: "Vector3".into(), category: "DataType".into(), group: "Data".into(), value, read_only: false });
                    }
                }
            }
        }
        let rank = |g: &str| match g {
            "Appearance" => 0,
            "Data" => 1,
            _ => 2,
        };
        rows.sort_by(|a, b| (rank(&a.group), &a.group, &a.name).cmp(&(rank(&b.group), &b.group, &b.name)));
        rows
    }

    /// Case-insensitive name search below the DataModel (Explorer filter).
    pub fn find_by_name(&self, query: &str, limit: usize) -> Vec<Id> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        self.tree
            .descendants(Tree::ROOT)
            .into_iter()
            .filter(|i| self.tree.name(*i).map(|n| n.to_lowercase().contains(&q)).unwrap_or(false))
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"{"Classes":[
     {"Name":"Instance","Superclass":"<<<ROOT>>>","Tags":["NotCreatable"],"Members":[
       {"MemberType":"Property","Name":"Name","Category":"Data","ValueType":{"Category":"Primitive","Name":"string"},"Security":{"Read":"None","Write":"None"}},
       {"MemberType":"Property","Name":"Parent","Category":"Data","ValueType":{"Category":"Class","Name":"Instance"},"Security":{"Read":"None","Write":"None"}}]},
     {"Name":"BasePart","Superclass":"Instance","Tags":["NotCreatable"],"Members":[
       {"MemberType":"Property","Name":"CFrame","Category":"Data","Tags":["NotBrowsable"],"ValueType":{"Category":"DataType","Name":"CFrame"},"Security":"None"},
       {"MemberType":"Property","Name":"Size","Category":"Data","ValueType":{"Category":"DataType","Name":"Vector3"},"Security":"None"},
       {"MemberType":"Property","Name":"Transparency","Category":"Appearance","ValueType":{"Category":"Primitive","Name":"float"},"Security":"None"}]},
     {"Name":"Part","Superclass":"BasePart","Members":[]},
     {"Name":"Model","Superclass":"Instance","Members":[]},
     {"Name":"Folder","Superclass":"Instance","Members":[]},
     {"Name":"Workspace","Superclass":"Instance","Tags":["NotCreatable","Service"],"Members":[]}
    ],"Enums":[]}"#;

    fn ed() -> (Editor, Id) {
        let dump = Arc::new(ApiDump::from_json(DUMP).unwrap());
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        t.set_prop(ws, "Name", Value::String("Workspace".into())).unwrap();
        (Editor::new(t, dump), ws)
    }

    fn kids(e: &Editor, id: Id) -> Vec<Id> {
        e.tree.node(id).unwrap().children.clone()
    }

    #[test]
    fn live_edits_collapse_into_one_undo_step() {
        let (mut e, ws) = ed();
        let p = e.insert("Part", ws, [0.0; 3]).unwrap();
        for k in 1..=5 {
            e.live_edit(|ed| ed.set_value(&[p], "Transparency", Value::Float(k as f64 / 10.0))).unwrap();
        }
        e.live_end();
        assert_eq!(e.tree.get_prop(p, "Transparency"), Some(&Value::Float(0.5)));
        e.undo();
        assert_eq!(e.tree.get_prop(p, "Transparency"), None, "one undo reverts the whole drag");
        assert_eq!(kids(&e, ws), vec![p], "and nothing else was undone");
        e.redo();
        assert_eq!(e.tree.get_prop(p, "Transparency"), Some(&Value::Float(0.5)));
        // A no-op live edit must not swallow the previous entry.
        e.live_edit(|ed| ed.set_value(&[p], "Transparency", Value::Float(0.5))).unwrap();
        e.live_edit(|ed| ed.set_value(&[p], "Transparency", Value::Float(0.5))).unwrap();
        assert_eq!(kids(&e, ws), vec![p]);
    }

    #[test]
    fn insert_set_undo_redo() {
        let (mut e, ws) = ed();
        let p = e.insert("Part", ws, [0.0, 5.0, 0.0]).unwrap();
        assert_eq!(e.tree.get_prop(p, "Size"), Some(&Value::Vector3([4.0, 1.0, 2.0])));
        e.set_value(&[p], "Transparency", Value::Int(1)).unwrap();
        assert_eq!(e.tree.get_prop(p, "Transparency"), Some(&Value::Float(1.0)));
        e.undo();
        assert_eq!(e.tree.get_prop(p, "Transparency"), None);
        e.undo();
        assert!(kids(&e, ws).is_empty());
        assert!(e.selection.is_empty());
        e.redo();
        e.redo();
        assert_eq!(e.tree.get_prop(p, "Transparency"), Some(&Value::Float(1.0)));
        assert!(e.set_value(&[p], "Transparency", Value::String("x".into())).is_err());
        assert!(e.insert("BasePart", ws, [0.0; 3]).is_err());
        assert!(e.insert("Part", Tree::ROOT, [0.0; 3]).is_err());
    }

    #[test]
    fn delete_restores_exact_position_and_new_action_discards_redo() {
        let (mut e, ws) = ed();
        let a = e.insert("Part", ws, [0.0; 3]).unwrap();
        let b = e.insert("Part", ws, [0.0; 3]).unwrap();
        let c = e.insert("Part", ws, [0.0; 3]).unwrap();
        e.delete(&[b]).unwrap();
        assert_eq!(kids(&e, ws), vec![a, c]);
        e.undo();
        assert_eq!(kids(&e, ws), vec![a, b, c]);
        e.undo();
        assert!(e.tree.is_alive(c) && !e.tree.is_attached(c));
        e.insert("Model", ws, [0.0; 3]).unwrap();
        assert!(!e.tree.is_alive(c), "discarded redo must free the detached instance");
        assert!(e.delete(&[ws]).is_err(), "services cannot be deleted");
    }

    #[test]
    fn reparent_within_and_across_parents_undoes_cleanly() {
        let (mut e, ws) = ed();
        let a = e.insert("Part", ws, [0.0; 3]).unwrap();
        let b = e.insert("Part", ws, [0.0; 3]).unwrap();
        let m = e.insert("Model", ws, [0.0; 3]).unwrap();
        e.reparent(&[a, b], m, None).unwrap();
        assert_eq!(kids(&e, m), vec![a, b]);
        assert_eq!(kids(&e, ws), vec![m]);
        assert!(e.reparent(&[m], a, None).is_err(), "cycle must be rejected");
        assert_eq!(kids(&e, ws), vec![m], "failed op must leave the tree untouched");
        e.reparent(&[a], m, Some(1)).unwrap();
        assert_eq!(kids(&e, m), vec![b, a]);
        e.undo();
        assert_eq!(kids(&e, m), vec![a, b]);
        e.undo();
        assert_eq!(kids(&e, ws), vec![a, b, m]);
    }

    #[test]
    fn duplicate_remaps_internal_refs_and_group_ungroup_round_trip() {
        let (mut e, ws) = ed();
        let m = e.insert("Model", ws, [0.0; 3]).unwrap();
        let p = e.insert("Part", m, [0.0; 3]).unwrap();
        e.tree.set_prop(m, "PrimaryPart", Value::Ref(Some(p))).unwrap();
        let d = e.duplicate(&[m]).unwrap()[0];
        let dp = kids(&e, d)[0];
        assert_eq!(e.tree.get_prop(d, "PrimaryPart"), Some(&Value::Ref(Some(dp))));
        assert_eq!(kids(&e, ws), vec![m, d]);

        let g = e.group(&[m, d]).unwrap();
        assert_eq!(kids(&e, ws), vec![g]);
        assert_eq!(kids(&e, g), vec![m, d]);
        e.ungroup(&[g]).unwrap();
        assert_eq!(kids(&e, ws), vec![m, d]);
        e.undo();
        e.undo();
        assert_eq!(kids(&e, ws), vec![m, d]);
    }

    #[test]
    fn imports_script_files_with_undo() {
        let dump = Arc::new(ApiDump::from_json(&DUMP.replace(r#"{"Name":"Folder""#, r#"{"Name":"Script","Superclass":"Instance","Members":[{"MemberType":"Property","Name":"Source","Category":"Data","ValueType":{"Category":"Primitive","Name":"ProtectedString"},"Security":"None"}]},{"Name":"Folder""#)).unwrap());
        let mut t = Tree::new();
        let ws = t.create("Workspace");
        t.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let mut e = Editor::new(t, dump);
        let f = crate::script_file::parse("hello.server.lua", b"print('hi')
", None).unwrap();
        let id = e.import_script(&f, ws).unwrap();
        assert_eq!(e.tree.name(id), Some("hello"));
        assert_eq!(e.tree.get_prop(id, "Source"), Some(&Value::String("print('hi')\n".into())));
        e.load_source(id, b"print(2)").unwrap();
        assert!(e.load_source(ws, b"x").is_err());
        assert!(e.load_source(id, &[0, 1]).is_err());
        e.undo();
        e.undo();
        assert!(e.tree.node(ws).unwrap().children.is_empty());
    }

    #[test]
    fn copy_paste_cut() {
        let (mut e, ws) = ed();
        let a = e.insert("Part", ws, [0.0; 3]).unwrap();
        e.copy(&[a]).unwrap();
        let pasted = e.paste(ws).unwrap();
        assert_eq!(kids(&e, ws).len(), 2);
        assert_ne!(pasted[0], a);
        e.cut(&[a]).unwrap();
        assert_eq!(kids(&e, ws), pasted);
        e.paste(ws).unwrap();
        assert_eq!(kids(&e, ws).len(), 2);
    }

    #[test]
    fn virtual_position_and_orientation() {
        let (mut e, ws) = ed();
        let p = e.insert("Part", ws, [1.0, 2.0, 3.0]).unwrap();
        assert_eq!(e.get_value(p, "Position"), Some(Value::Vector3([1.0, 2.0, 3.0])));
        e.set_value(&[p], "Orientation", Value::Vector3([0.0, 90.0, 0.0])).unwrap();
        e.set_value(&[p], "Position", Value::Vector3([5.0, 6.0, 7.0])).unwrap();
        let Some(Value::Vector3(o)) = e.get_value(p, "Orientation") else { panic!() };
        assert!((o[1] - 90.0).abs() < 1e-3 && o[0].abs() < 1e-3 && o[2].abs() < 1e-3, "{o:?}");
        assert_eq!(e.get_value(p, "Position"), Some(Value::Vector3([5.0, 6.0, 7.0])));
        e.undo();
        assert_eq!(e.get_value(p, "Position"), Some(Value::Vector3([1.0, 2.0, 3.0])));
        let names: Vec<_> = e.properties(p).into_iter().map(|r| r.name).collect();
        assert_eq!(names.first().map(String::as_str), Some("Transparency"), "Appearance group first: {names:?}");
        assert!(names.contains(&"Position".to_string()) && !names.contains(&"CFrame".to_string()) && !names.contains(&"Parent".to_string()));
    }

    #[test]
    fn multi_select_set_skips_incompatible_and_find_by_name() {
        let (mut e, ws) = ed();
        let p = e.insert("Part", ws, [0.0; 3]).unwrap();
        let m = e.insert("Model", ws, [0.0; 3]).unwrap();
        e.set_value(&[p, m], "Transparency", Value::Float(0.5)).unwrap();
        assert_eq!(e.tree.get_prop(p, "Transparency"), Some(&Value::Float(0.5)));
        assert_eq!(e.tree.get_prop(m, "Transparency"), None);
        e.rename(p, "Brick").unwrap();
        assert_eq!(e.find_by_name("bri", 10), vec![p]);
        assert!(e.find_by_name("", 10).is_empty());
    }
}
