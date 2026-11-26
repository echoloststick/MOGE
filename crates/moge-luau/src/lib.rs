//! Luau runtime for MOGE (mlua). Scripts run against a snapshot of the scene tree; changes are
//! returned as a new tree and never modify the open project.
mod inst;
mod types;

pub use inst::Level;
use mlua::{Lua, VmState};
use moge_core::{ApiDump, Id, Tree, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Script {
    /// Instance the script came from (`script` global); None for loose source text.
    pub id: Option<Id>,
    pub name: String,
    pub source: String,
}

/// Script and LocalScript instances that are enabled and have source, in tree order.
pub fn collect_play_scripts(tree: &Tree) -> Vec<Script> {
    let mut out = Vec::new();
    for id in tree.descendants(Tree::ROOT) {
        let Some(n) = tree.node(id) else { continue };
        if n.class != "Script" && n.class != "LocalScript" {
            continue;
        }
        if matches!(n.props.get("Disabled"), Some(Value::Bool(true))) {
            continue;
        }
        if let Some(Value::String(src)) = n.props.get("Source") {
            if !src.trim().is_empty() {
                out.push(Script { id: Some(id), name: tree.full_name(id).unwrap_or_else(|| n.class.clone()), source: src.clone() });
            }
        }
    }
    out
}

/// Runs `scripts` to completion (every coroutine finished), until `cancel` is set, or `limit` seconds pass.
/// Returns the tree as the scripts left it.
pub fn run(tree: Tree, dump: Arc<ApiDump>, scripts: Vec<Script>, cancel: Arc<AtomicBool>, limit: Option<f64>, out: impl FnMut(Level, String) + 'static) -> Result<Tree, String> {
    let sh: inst::Sh = Rc::new(RefCell::new(inst::Shared::new(tree, dump.clone(), cancel.clone(), Box::new(out))));
    let lua = Lua::new();
    let flag = cancel.clone();
    lua.set_interrupt(move |_| {
        if flag.load(Ordering::Relaxed) {
            Err(mlua::Error::runtime("Execution stopped by the user"))
        } else {
            Ok(VmState::Continue)
        }
    });
    types::register(&lua, &dump).map_err(|e| format!("Failed to initialize the Luau runtime: {e}"))?;
    for s in scripts {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Each script gets its own `script` global through an environment-free reload of the global.
        inst::register(&lua, &sh, s.id).map_err(|e| format!("Failed to initialize the Luau runtime: {e}"))?;
        match lua.load(&s.source).set_name(format!("={}", s.name)).into_function() {
            Ok(f) => match lua.create_thread(f) {
                Ok(th) => inst::step(&sh, th, Vec::new()),
                Err(e) => inst::report(&sh, Level::Error, format!("{}: {e}", s.name)),
            },
            Err(e) => inst::report(&sh, Level::Error, format!("{}: {}", s.name, e.to_string().replace("syntax error: ", ""))),
        }
        // Run this script until it first yields before the next script is started.
    }
    inst::pump(&sh, limit);
    let tree = std::mem::replace(&mut sh.borrow_mut().tree, Tree::new());
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"{"Classes":[
 {"Name":"Instance","Superclass":"<<<ROOT>>>","Tags":["NotCreatable"],"Members":[
   {"MemberType":"Property","Name":"Name","ValueType":{"Category":"Primitive","Name":"string"},"Security":"None"},
   {"MemberType":"Property","Name":"Parent","ValueType":{"Category":"Class","Name":"Instance"},"Security":"None"},
   {"MemberType":"Event","Name":"ChildAdded","Parameters":[]}]},
 {"Name":"BasePart","Superclass":"Instance","Tags":["NotCreatable"],"Members":[
   {"MemberType":"Property","Name":"Position","ValueType":{"Category":"DataType","Name":"Vector3"},"Security":"None"},
   {"MemberType":"Property","Name":"Transparency","ValueType":{"Category":"Primitive","Name":"float"},"Security":"None"},
   {"MemberType":"Property","Name":"Material","ValueType":{"Category":"Enum","Name":"Material"},"Security":"None"}]},
 {"Name":"Part","Superclass":"BasePart","Members":[]},
 {"Name":"Workspace","Superclass":"Instance","Tags":["Service"],"Members":[]}
],"Enums":[{"Name":"Material","Items":[{"Name":"Plastic","Value":256},{"Name":"Neon","Value":288}]}]}"#;

    fn exec(src: &str) -> (Tree, Vec<(Level, String)>) {
        let dump = Arc::new(ApiDump::from_json(DUMP).unwrap());
        let mut tree = Tree::new();
        let ws = tree.create("Workspace");
        tree.set_parent(ws, Some(Tree::ROOT)).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let l2 = log.clone();
        // Level/String are Send, so a plain Rc<RefCell<..>> captured by the 'static closure is fine on this thread.
        let t = run(tree, dump, vec![Script { id: None, name: "Test".into(), source: src.into() }], Arc::new(AtomicBool::new(false)), Some(5.0), move |l, m| l2.borrow_mut().push((l, m))).unwrap();
        let v = log.borrow().clone();
        (t, v)
    }

    #[test]
    fn print_and_math() {
        let (_, log) = exec("print(1 + 2, 'ok', Vector3.new(1, 2, 3) * 2)");
        assert_eq!(log, vec![(Level::Print, "3\tok\t2, 4, 6".to_string())]);
    }

    #[test]
    fn instances_properties_and_enums() {
        let (t, log) = exec(
            "local p = Instance.new('Part'); p.Name = 'Brick'; p.Position = Vector3.new(0, 5, 0); p.Material = Enum.Material.Neon; p.Transparency = 1; p.Parent = workspace; print(workspace.Brick.Material, typeof(p))",
        );
        assert_eq!(log, vec![(Level::Print, "Enum.Material.Neon\tInstance".to_string())]);
        let ws = t.find_first_child_of_class(Tree::ROOT, "Workspace").unwrap();
        let id = t.find_first_child(ws, "Brick").unwrap();
        assert_eq!(t.get_prop(id, "Position"), Some(&Value::Vector3([0.0, 5.0, 0.0])));
        assert_eq!(t.get_prop(id, "Material"), Some(&Value::Enum(288)));
    }

    #[test]
    fn task_wait_and_signals() {
        let (_, log) = exec("workspace.ChildAdded:Connect(function(c) print('added', c.Name) end); task.wait(0.02); Instance.new('Part', workspace)");
        assert_eq!(log, vec![(Level::Print, "added\tPart".to_string())]);
    }

    #[test]
    fn errors_are_reported() {
        let (_, log) = exec("local x = workspace.Nope");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, Level::Error);
        assert!(log[0].1.contains("'Nope' is not a valid member of Workspace"));
    }
}
