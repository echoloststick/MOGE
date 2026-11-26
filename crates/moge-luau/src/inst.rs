//! Instance userdata, signals, the task scheduler and the Roblox-style globals.
use crate::types::*;
use mlua::{Function, Lua, MetaMethod, MultiValue, Result, Thread, ThreadStatus, UserData, UserDataMethods, Value as LV};
use moge_core::{ApiDump, Id, MemberKind, PropInfo, Tree, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Print,
    Info,
    Warn,
    Error,
}

struct Task {
    at: f64,
    th: Thread,
    args: Vec<LV>,
}

pub struct Shared {
    pub tree: Tree,
    pub dump: Arc<ApiDump>,
    pub out: Box<dyn FnMut(Level, String)>,
    pub cancel: Arc<AtomicBool>,
    handlers: HashMap<(Id, String), Vec<(u64, Function, bool)>>,
    next_conn: u64,
    queue: Vec<Task>,
    start: Instant,
    modules: HashMap<Id, LV>,
    warned: Vec<String>,
}
pub type Sh = Rc<RefCell<Shared>>;

impl Shared {
    pub fn new(tree: Tree, dump: Arc<ApiDump>, cancel: Arc<AtomicBool>, out: Box<dyn FnMut(Level, String)>) -> Shared {
        Shared { tree, dump, out, cancel, handlers: HashMap::new(), next_conn: 1, queue: Vec::new(), start: Instant::now(), modules: HashMap::new(), warned: Vec::new() }
    }
}

pub fn report(sh: &Sh, level: Level, msg: String) {
    (sh.borrow_mut().out)(level, msg);
}

fn err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(e.to_string())
}

fn elapsed(sh: &Sh) -> f64 {
    sh.borrow().start.elapsed().as_secs_f64()
}

#[derive(Clone)]
pub struct Inst {
    pub id: Id,
    pub sh: Sh,
}
#[derive(Clone)]
struct Signal {
    sh: Sh,
    id: Id,
    name: String,
}
#[derive(Clone)]
struct Connection {
    sh: Sh,
    key: (Id, String),
    cid: u64,
}

pub fn userdata_type_name(v: &LV) -> &'static str {
    if ud::<Inst>(v).is_some() {
        "Instance"
    } else if ud::<Signal>(v).is_some() {
        "RBXScriptSignal"
    } else if ud::<Connection>(v).is_some() {
        "RBXScriptConnection"
    } else {
        "userdata"
    }
}

pub fn inst(lua: &Lua, sh: &Sh, id: Id) -> Result<LV> {
    wrap(lua, Inst { id, sh: sh.clone() })
}

fn opt_inst(lua: &Lua, sh: &Sh, id: Option<Id>) -> Result<LV> {
    match id {
        Some(id) => inst(lua, sh, id),
        None => Ok(LV::Nil),
    }
}

fn list(lua: &Lua, sh: &Sh, ids: Vec<Id>) -> Result<LV> {
    let t = lua.create_table()?;
    for (i, id) in ids.into_iter().enumerate() {
        t.set(i + 1, inst(lua, sh, id)?)?;
    }
    Ok(LV::Table(t))
}

fn string(lua: &Lua, s: &str) -> Result<LV> {
    Ok(LV::String(lua.create_string(s)?))
}

// ---------------------------------------------------------------- value conversion

fn to_lua(lua: &Lua, sh: &Sh, v: Value, info: Option<&PropInfo>) -> Result<LV> {
    Ok(match v {
        Value::Bool(b) => LV::Boolean(b),
        Value::Int(i) => LV::Number(i as f64),
        Value::Float(f) => LV::Number(f),
        Value::String(s) => string(lua, &s)?,
        Value::Vector2(v) => wrap(lua, Vector2(v))?,
        Value::Vector3(v) => wrap(lua, Vector3(v))?,
        Value::Color3(v) => wrap(lua, Color3(v))?,
        Value::UDim(s, o) => wrap(lua, UDim(s, o))?,
        Value::UDim2 { sx, ox, sy, oy } => wrap(lua, UDim2 { sx, ox, sy, oy })?,
        Value::CFrame { pos, rot } => wrap(lua, CFrame { pos, rot })?,
        Value::BrickColor(n) => LV::Number(n as f64),
        Value::Enum(u) => {
            let ty = info.map(|i| i.value_type.clone()).unwrap_or_default();
            let name = sh.borrow().dump.enum_item_name(&ty, u).unwrap_or("").to_string();
            wrap(lua, EnumItem { ty, name, value: u })?
        }
        Value::Ref(Some(id)) if sh.borrow().tree.is_alive(id) => inst(lua, sh, id)?,
        _ => LV::Nil,
    })
}

fn from_lua(v: &LV, info: Option<&PropInfo>) -> Result<Value> {
    let int_prop = info.map(|i| i.value_type == "int" || i.value_type == "int64").unwrap_or(false);
    Ok(match v {
        LV::Boolean(b) => Value::Bool(*b),
        LV::Integer(_) | LV::Number(_) => {
            let n = num(v).unwrap_or(0.0);
            if int_prop {
                Value::Int(n.round() as i64)
            } else {
                Value::Float(n)
            }
        }
        LV::String(s) => Value::String(s.to_str()?.to_string()),
        LV::Nil => Value::Ref(None),
        LV::UserData(_) => {
            if let Some(x) = ud::<Vector3>(v) {
                Value::Vector3(x.0)
            } else if let Some(x) = ud::<Vector2>(v) {
                Value::Vector2(x.0)
            } else if let Some(x) = ud::<Color3>(v) {
                Value::Color3(x.0)
            } else if let Some(x) = ud::<UDim2>(v) {
                Value::UDim2 { sx: x.sx, ox: x.ox, sy: x.sy, oy: x.oy }
            } else if let Some(x) = ud::<UDim>(v) {
                Value::UDim(x.0, x.1)
            } else if let Some(x) = ud::<CFrame>(v) {
                Value::CFrame { pos: x.pos, rot: x.rot }
            } else if let Some(x) = ud::<EnumItem>(v) {
                Value::Enum(x.value)
            } else if let Some(x) = ud::<Inst>(v) {
                Value::Ref(Some(x.id))
            } else {
                return Err(mlua::Error::runtime("Unsupported value type"));
            }
        }
        _ => return Err(mlua::Error::runtime(format!("Unsupported value type '{}'", v.type_name()))),
    })
}

// ---------------------------------------------------------------- scheduler and signals

fn enqueue(sh: &Sh, th: Thread, delay: f64, args: Vec<LV>) {
    let at = elapsed(sh) + delay.max(0.0);
    sh.borrow_mut().queue.push(Task { at, th, args });
}

fn clean_error(e: &mlua::Error) -> String {
    let s = e.to_string();
    let s = s.split("stack traceback:").next().unwrap_or(&s).trim();
    s.strip_prefix("runtime error: ").unwrap_or(s).to_string()
}

/// Resumes a coroutine; a yielded number re-queues it as a wait.
pub fn step(sh: &Sh, th: Thread, args: Vec<LV>) {
    match th.resume::<MultiValue>(MultiValue::from_vec(args)) {
        Ok(ret) => {
            if th.status() == ThreadStatus::Resumable {
                let d = ret.iter().next().and_then(num).unwrap_or(0.0);
                enqueue(sh, th, d, Vec::new());
            }
        }
        Err(e) => {
            if !sh.borrow().cancel.load(Ordering::Relaxed) {
                report(sh, Level::Error, clean_error(&e));
            }
        }
    }
}

fn spawn_fn(lua: &Lua, sh: &Sh, f: Function, args: Vec<LV>) {
    match lua.create_thread(f) {
        Ok(th) => step(sh, th, args),
        Err(e) => report(sh, Level::Error, clean_error(&e)),
    }
}

pub fn fire(lua: &Lua, sh: &Sh, id: Id, ev: &str, args: Vec<LV>) {
    let key = (id, ev.to_string());
    let hs: Vec<(u64, Function, bool)> = sh.borrow().handlers.get(&key).cloned().unwrap_or_default();
    for (cid, f, once) in hs {
        if once {
            if let Some(v) = sh.borrow_mut().handlers.get_mut(&key) {
                v.retain(|h| h.0 != cid);
            }
        }
        spawn_fn(lua, sh, f, args.clone());
    }
}

/// Runs queued tasks until the queue is empty, the run is cancelled or `limit` seconds passed.
pub fn pump(sh: &Sh, limit: Option<f64>) {
    loop {
        if sh.borrow().cancel.load(Ordering::Relaxed) {
            break;
        }
        let now = elapsed(sh);
        if let Some(l) = limit {
            if now > l {
                report(sh, Level::Warn, format!("Execution stopped: the time limit of {l} seconds was reached"));
                break;
            }
        }
        let mut due: Vec<Task> = {
            let mut s = sh.borrow_mut();
            let all = std::mem::take(&mut s.queue);
            let (d, rest): (Vec<Task>, Vec<Task>) = all.into_iter().partition(|t| t.at <= now);
            s.queue = rest;
            d
        };
        due.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        for t in due {
            step(sh, t.th, t.args);
        }
        let next = sh.borrow().queue.iter().map(|t| t.at).fold(f64::INFINITY, f64::min);
        if next.is_infinite() {
            break;
        }
        let wait = (next - elapsed(sh)).clamp(0.0, 0.008);
        std::thread::sleep(std::time::Duration::from_secs_f64(wait));
    }
}

fn set_parent_fire(lua: &Lua, sh: &Sh, id: Id, new: Option<Id>) -> Result<()> {
    let old = sh.borrow().tree.node(id).and_then(|n| n.parent);
    sh.borrow_mut().tree.set_parent(id, new).map_err(err)?;
    if old != new {
        if let Some(o) = old {
            fire(lua, sh, o, "ChildRemoved", vec![inst(lua, sh, id)?]);
        }
        if let Some(n) = new {
            fire(lua, sh, n, "ChildAdded", vec![inst(lua, sh, id)?]);
        }
    }
    Ok(())
}

impl UserData for Signal {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        fn connect(lua: &Lua, s: &Signal, f: Function, once: bool) -> Result<LV> {
            let (cid, warn) = {
                let mut sh = s.sh.borrow_mut();
                let cid = sh.next_conn;
                sh.next_conn += 1;
                sh.handlers.entry((s.id, s.name.clone())).or_default().push((cid, f, once));
                let supported = matches!(s.name.as_str(), "ChildAdded" | "ChildRemoved" | "Event");
                let warn = !supported && !sh.warned.contains(&s.name);
                if warn {
                    sh.warned.push(s.name.clone());
                }
                (cid, warn)
            };
            if warn {
                report(&s.sh, Level::Warn, format!("Signal '{}' is accepted but is not fired by the MOGE runtime yet", s.name));
            }
            wrap(lua, Connection { sh: s.sh.clone(), key: (s.id, s.name.clone()), cid })
        }
        m.add_method("Connect", |lua, s, f: Function| connect(lua, s, f, false));
        m.add_method("Once", |lua, s, f: Function| connect(lua, s, f, true));
        m.add_method("Wait", |_, _s, ()| -> Result<()> { Err(mlua::Error::runtime("RBXScriptSignal:Wait() is not supported by the MOGE runtime yet; use :Connect() or :Once()")) });
        m.add_meta_method(MetaMethod::ToString, |_, s, ()| Ok(format!("Signal {}", s.name)));
    }
}

impl UserData for Connection {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Disconnect", |_, c, ()| {
            if let Some(v) = c.sh.borrow_mut().handlers.get_mut(&c.key) {
                v.retain(|h| h.0 != c.cid);
            }
            Ok(())
        });
        m.add_field_method_get("Connected", |_, c| Ok(c.sh.borrow().handlers.get(&c.key).map(|v| v.iter().any(|h| h.0 == c.cid)).unwrap_or(false)));
    }
}

// ---------------------------------------------------------------- Instance

const METHODS: &[&str] = &[
    "FindFirstChild", "FindFirstChildOfClass", "FindFirstChildWhichIsA", "FindFirstAncestor", "WaitForChild", "GetChildren", "GetDescendants", "IsA",
    "Destroy", "ClearAllChildren", "Clone", "GetFullName", "SetAttribute", "GetAttribute", "IsDescendantOf", "IsAncestorOf", "GetService", "Fire",
];

fn dead() -> mlua::Error {
    mlua::Error::runtime("The Instance has been destroyed")
}

fn class_of(i: &Inst) -> Result<String> {
    i.sh.borrow().tree.node(i.id).map(|n| n.class.clone()).ok_or_else(dead)
}

impl UserData for Inst {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| index(lua, this, &key));
        m.add_meta_method(MetaMethod::NewIndex, |lua, this, (key, v): (String, LV)| newindex(lua, this, &key, v));
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.sh.borrow().tree.name(this.id).unwrap_or("<destroyed>").to_string()));
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (LV, LV)| Ok(matches!((ud::<Inst>(&a), ud::<Inst>(&b)), (Some(x), Some(y)) if x.id == y.id)));
    }
}

fn index(lua: &Lua, this: &Inst, key: &str) -> Result<LV> {
    let class = class_of(this)?;
    match key {
        "ClassName" => return string(lua, &class),
        "Name" => return string(lua, &this.sh.borrow().tree.name(this.id).unwrap_or("").to_string()),
        "Parent" => {
            let p = this.sh.borrow().tree.node(this.id).and_then(|n| n.parent);
            return opt_inst(lua, &this.sh, p);
        }
        _ => {}
    }
    if let Some(m) = METHODS.iter().find(|m| **m == key) {
        let (this, m) = (this.clone(), *m);
        return Ok(LV::Function(lua.create_function(move |lua, args: MultiValue| call_method(lua, &this, m, args.into_iter().skip(1).collect()))?));
    }
    let dump = this.sh.borrow().dump.clone();
    if let Some(info) = dump.property(&class, key) {
        let v = this.sh.borrow().tree.get_or_default(&dump, this.id, key);
        return match v {
            Some(v) => to_lua(lua, &this.sh, v, Some(info)),
            None => Ok(LV::Nil),
        };
    }
    if dump.members(&class).iter().any(|m| m.name == key && m.kind == MemberKind::Event) {
        return wrap(lua, Signal { sh: this.sh.clone(), id: this.id, name: key.to_string() });
    }
    let child = this.sh.borrow().tree.find_first_child(this.id, key);
    if let Some(c) = child {
        return inst(lua, &this.sh, c);
    }
    let full = this.sh.borrow().tree.full_name(this.id).unwrap_or_default();
    Err(mlua::Error::runtime(format!("'{key}' is not a valid member of {class} \"{full}\"")))
}

fn newindex(lua: &Lua, this: &Inst, key: &str, v: LV) -> Result<()> {
    let class = class_of(this)?;
    match key {
        "Parent" => {
            let p = match &v {
                LV::Nil => None,
                other => Some(ud::<Inst>(other).ok_or_else(|| mlua::Error::runtime("Parent must be an Instance or nil"))?.id),
            };
            return set_parent_fire(lua, &this.sh, this.id, p);
        }
        "Name" => {
            return match &v {
                LV::String(s) => this.sh.borrow_mut().tree.set_prop(this.id, "Name", Value::String(s.to_str()?.to_string())).map_err(err),
                _ => Err(mlua::Error::runtime("Name must be a string")),
            };
        }
        _ => {}
    }
    let dump = this.sh.borrow().dump.clone();
    let Some(info) = dump.property(&class, key) else {
        return Err(mlua::Error::runtime(format!("'{key}' is not a valid property of {class}")));
    };
    let val = from_lua(&v, Some(info))?;
    this.sh.borrow_mut().tree.set_prop_checked(&dump, this.id, key, val).map_err(err)
}

fn arg_str(args: &[LV], i: usize, what: &str) -> Result<String> {
    match args.get(i) {
        Some(LV::String(s)) => Ok(s.to_str()?.to_string()),
        _ => Err(mlua::Error::runtime(format!("Argument {} ({what}) must be a string", i + 1))),
    }
}

fn arg_inst(args: &[LV], i: usize) -> Result<Inst> {
    args.get(i).and_then(ud::<Inst>).ok_or_else(|| mlua::Error::runtime(format!("Argument {} must be an Instance", i + 1)))
}

fn one(v: LV) -> Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![v]))
}

fn call_method(lua: &Lua, this: &Inst, name: &str, args: Vec<LV>) -> Result<MultiValue> {
    let sh = &this.sh;
    let id = this.id;
    let class = class_of(this)?;
    let dump = sh.borrow().dump.clone();
    match name {
        "FindFirstChild" => {
            let n = arg_str(&args, 0, "name")?;
            let recursive = matches!(args.get(1), Some(LV::Boolean(true)));
            let s = sh.borrow();
            let r = if recursive { s.tree.descendants(id).into_iter().find(|d| s.tree.name(*d) == Some(n.as_str())) } else { s.tree.find_first_child(id, &n) };
            drop(s);
            one(opt_inst(lua, sh, r)?)
        }
        "WaitForChild" => {
            let n = arg_str(&args, 0, "name")?;
            let r = sh.borrow().tree.find_first_child(id, &n);
            if r.is_none() {
                report(sh, Level::Warn, format!("WaitForChild('{n}') did not find the child; the MOGE runtime does not wait for new children yet"));
            }
            one(opt_inst(lua, sh, r)?)
        }
        "FindFirstChildOfClass" => {
            let c = arg_str(&args, 0, "className")?;
            let r = sh.borrow().tree.find_first_child_of_class(id, &c);
            one(opt_inst(lua, sh, r)?)
        }
        "FindFirstChildWhichIsA" => {
            let c = arg_str(&args, 0, "className")?;
            let s = sh.borrow();
            let r = s.tree.node(id).and_then(|n| n.children.iter().copied().find(|ch| s.tree.node(*ch).map(|x| dump.is_a(&x.class, &c)).unwrap_or(false)));
            drop(s);
            one(opt_inst(lua, sh, r)?)
        }
        "FindFirstAncestor" => {
            let n = arg_str(&args, 0, "name")?;
            let s = sh.borrow();
            let mut cur = s.tree.node(id).and_then(|x| x.parent);
            let mut found = None;
            while let Some(c) = cur {
                if s.tree.name(c) == Some(n.as_str()) {
                    found = Some(c);
                    break;
                }
                cur = s.tree.node(c).and_then(|x| x.parent);
            }
            drop(s);
            one(opt_inst(lua, sh, found)?)
        }
        "GetChildren" => {
            let ids = sh.borrow().tree.node(id).map(|n| n.children.clone()).unwrap_or_default();
            one(list(lua, sh, ids)?)
        }
        "GetDescendants" => {
            let ids = sh.borrow().tree.descendants(id);
            one(list(lua, sh, ids)?)
        }
        "IsA" => one(LV::Boolean(dump.is_a(&class, &arg_str(&args, 0, "className")?))),
        "GetFullName" => one(string(lua, &sh.borrow().tree.full_name(id).unwrap_or_default())?),
        "IsDescendantOf" | "IsAncestorOf" => {
            let other = arg_inst(&args, 0)?;
            let (a, b) = if name == "IsDescendantOf" { (id, other.id) } else { (other.id, id) };
            let s = sh.borrow();
            let mut cur = s.tree.node(a).and_then(|n| n.parent);
            let mut r = false;
            while let Some(c) = cur {
                if c == b {
                    r = true;
                    break;
                }
                cur = s.tree.node(c).and_then(|n| n.parent);
            }
            one(LV::Boolean(r))
        }
        "Destroy" => {
            if sh.borrow().tree.node(id).and_then(|n| n.parent).is_some() {
                set_parent_fire(lua, sh, id, None)?;
            }
            sh.borrow_mut().tree.destroy(id).map_err(err)?;
            Ok(MultiValue::new())
        }
        "ClearAllChildren" => {
            let kids = sh.borrow().tree.node(id).map(|n| n.children.clone()).unwrap_or_default();
            for k in kids {
                let _ = set_parent_fire(lua, sh, k, None);
                let _ = sh.borrow_mut().tree.destroy(k);
            }
            Ok(MultiValue::new())
        }
        "Clone" => {
            let n = sh.borrow_mut().tree.clone_subtree(id).map_err(err)?;
            one(inst(lua, sh, n)?)
        }
        "SetAttribute" => {
            let k = arg_str(&args, 0, "attribute")?;
            let v = args.get(1).cloned().unwrap_or(LV::Nil);
            let mut s = sh.borrow_mut();
            let node = s.tree.node_mut(id).ok_or_else(dead)?;
            if matches!(v, LV::Nil) {
                node.attributes.remove(&k);
            } else {
                node.attributes.insert(k, from_lua(&v, None)?);
            }
            Ok(MultiValue::new())
        }
        "GetAttribute" => {
            let k = arg_str(&args, 0, "attribute")?;
            let v = sh.borrow().tree.node(id).and_then(|n| n.attributes.get(&k).cloned());
            one(match v {
                Some(v) => to_lua(lua, sh, v, None)?,
                None => LV::Nil,
            })
        }
        "GetService" => {
            let c = arg_str(&args, 0, "service")?;
            let r = sh.borrow().tree.find_first_child_of_class(Tree::ROOT, &c);
            match r {
                Some(r) => one(inst(lua, sh, r)?),
                None => Err(mlua::Error::runtime(format!("'{c}' is not a valid Service name"))),
            }
        }
        "Fire" => {
            fire(lua, sh, id, "Event", args);
            Ok(MultiValue::new())
        }
        _ => Err(mlua::Error::runtime(format!("Unknown method '{name}'"))),
    }
}

// ---------------------------------------------------------------- globals

pub fn register(lua: &Lua, sh: &Sh, script: Option<Id>) -> Result<()> {
    let g = lua.globals();
    g.set("game", inst(lua, sh, Tree::ROOT)?)?;
    let ws = sh.borrow().tree.find_first_child_of_class(Tree::ROOT, "Workspace");
    g.set("workspace", opt_inst(lua, sh, ws)?)?;
    g.set("script", opt_inst(lua, sh, script)?)?;

    let s2 = sh.clone();
    let instance = lua.create_table()?;
    instance.set(
        "new",
        lua.create_function(move |lua, (class, parent): (String, Option<Inst>)| {
            let dump = s2.borrow().dump.clone();
            let info = dump.class(&class).ok_or_else(|| mlua::Error::runtime(format!("Unable to create an Instance of type \"{class}\"")))?;
            if !info.creatable {
                return Err(mlua::Error::runtime(format!("Unable to create an Instance of type \"{class}\"")));
            }
            let id = s2.borrow_mut().tree.create(&class);
            if let Some(p) = parent {
                set_parent_fire(lua, &s2, id, Some(p.id))?;
            }
            inst(lua, &s2, id)
        })?,
    )?;
    g.set("Instance", instance)?;

    let s2 = sh.clone();
    g.set(
        "typeof",
        lua.create_function(|_, v: LV| Ok(type_name_of(&v)))?,
    )?;
    let print = |level: Level, s: Sh| {
        lua.create_function(move |lua, args: MultiValue| {
            let tostring: Function = lua.globals().get("tostring")?;
            let mut parts = Vec::new();
            for a in args {
                parts.push(tostring.call::<String>(a)?);
            }
            report(&s, level, parts.join("\t"));
            Ok(())
        })
    };
    g.set("print", print(Level::Print, s2.clone())?)?;
    g.set("warn", print(Level::Warn, s2)?)?;
    g.set(
        "tick",
        lua.create_function(|_, ()| Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)))?,
    )?;

    // task library
    let task = lua.create_table()?;
    let wait: Function = lua.load("return function(t) coroutine.yield(t or 0) return t or 0 end").eval()?;
    task.set("wait", wait.clone())?;
    let s2 = sh.clone();
    let spawn = lua.create_function(move |lua, (f, rest): (LV, MultiValue)| {
        let args: Vec<LV> = rest.into_iter().collect();
        let th = match f {
            LV::Function(f) => lua.create_thread(f)?,
            LV::Thread(t) => t,
            _ => return Err(mlua::Error::runtime("task.spawn expects a function or a thread")),
        };
        step(&s2, th.clone(), args);
        Ok(th)
    })?;
    task.set("spawn", spawn.clone())?;
    let s2 = sh.clone();
    task.set(
        "defer",
        lua.create_function(move |lua, (f, rest): (Function, MultiValue)| {
            let th = lua.create_thread(f)?;
            enqueue(&s2, th.clone(), 0.0, rest.into_iter().collect());
            Ok(th)
        })?,
    )?;
    let s2 = sh.clone();
    let delay = lua.create_function(move |lua, (t, f, rest): (Option<f64>, Function, MultiValue)| {
        let th = lua.create_thread(f)?;
        enqueue(&s2, th.clone(), t.unwrap_or(0.0), rest.into_iter().collect());
        Ok(th)
    })?;
    task.set("delay", delay.clone())?;
    let s2 = sh.clone();
    task.set(
        "cancel",
        lua.create_function(move |_, th: Thread| {
            s2.borrow_mut().queue.retain(|t| t.th != th);
            Ok(())
        })?,
    )?;
    g.set("task", task)?;
    g.set("wait", wait)?;
    g.set("spawn", spawn)?;
    g.set("delay", delay)?;

    let s2 = sh.clone();
    g.set(
        "require",
        lua.create_function(move |lua, m: Inst| {
            if let Some(v) = s2.borrow().modules.get(&m.id) {
                return Ok(v.clone());
            }
            let (class, src, name) = {
                let s = s2.borrow();
                let n = s.tree.node(m.id).ok_or_else(dead)?;
                let src = match n.props.get("Source") {
                    Some(Value::String(x)) => x.clone(),
                    _ => String::new(),
                };
                (n.class.clone(), src, s.tree.name(m.id).unwrap_or("Module").to_string())
            };
            if class != "ModuleScript" {
                return Err(mlua::Error::runtime(format!("Attempted to call require with an invalid argument: {name} is a {class}, not a ModuleScript")));
            }
            let f = lua.load(&src).set_name(format!("={name}")).into_function()?;
            let v: LV = f.call(())?;
            let v = if matches!(v, LV::Nil) { LV::Boolean(true) } else { v };
            s2.borrow_mut().modules.insert(m.id, v.clone());
            Ok(v)
        })?,
    )?;
    Ok(())
}
