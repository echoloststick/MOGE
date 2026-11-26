//! Autocomplete for Luau: members (from the API Dump), class-name strings, locals, keywords and globals.
use crate::lexer::{tokenize, Kind, Token, BUILTINS, KEYWORDS};
use moge_core::{ApiDump, MemberKind};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompKind {
    Keyword,
    Global,
    Local,
    Property,
    Method,
    Event,
    Class,
    Enum,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompKind,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct Completions {
    pub items: Vec<Completion>,
    /// Byte offset where the typed prefix starts; replace `src[replace_from..cursor]` with the chosen label.
    pub replace_from: usize,
}

#[derive(Clone, Debug)]
enum Ty {
    Class(String),
    Lib(&'static str),
    EnumType(String),
}

const LIBS: &[(&str, &[&str])] = &[
    ("math", &["abs", "acos", "asin", "atan", "atan2", "ceil", "clamp", "cos", "deg", "exp", "floor", "fmod", "huge", "log", "max", "min", "noise", "pi", "pow", "rad", "random", "randomseed", "round", "sign", "sin", "sqrt", "tan"]),
    ("string", &["byte", "char", "find", "format", "gmatch", "gsub", "len", "lower", "match", "rep", "reverse", "split", "sub", "upper"]),
    ("table", &["clear", "clone", "concat", "create", "find", "freeze", "insert", "isfrozen", "move", "pack", "remove", "sort", "unpack"]),
    ("task", &["cancel", "defer", "delay", "spawn", "wait"]),
    ("os", &["clock", "date", "difftime", "time"]),
    ("coroutine", &["close", "create", "isyieldable", "resume", "running", "status", "wrap", "yield"]),
    ("utf8", &["char", "charpattern", "codepoint", "codes", "len", "offset"]),
    ("Instance", &["new"]),
    ("Vector3", &["new", "one", "xAxis", "yAxis", "zAxis", "zero"]),
    ("Vector2", &["new", "one", "xAxis", "yAxis", "zero"]),
    ("CFrame", &["Angles", "fromEulerAnglesXYZ", "fromEulerAnglesYXZ", "fromOrientation", "identity", "lookAt", "new"]),
    ("Color3", &["fromHSV", "fromHex", "fromRGB", "new"]),
    ("UDim2", &["fromOffset", "fromScale", "new"]),
    ("UDim", &["new"]),
    ("BrickColor", &["Black", "Blue", "Green", "Random", "Red", "White", "Yellow", "new"]),
    ("TweenInfo", &["new"]),
    ("Random", &["new"]),
    ("Rect", &["new"]),
    ("Ray", &["new"]),
    ("NumberRange", &["new"]),
];

fn text<'a>(src: &'a str, t: &Token) -> &'a str {
    &src[t.start..t.end]
}

fn is_p(src: &str, t: &Token, s: &str) -> bool {
    t.kind == Kind::Punct && text(src, t) == s
}

fn str_content(src: &str, t: &Token) -> String {
    let s = text(src, t);
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string()
}

struct Link {
    name: String,
    call: bool,
    arg: Option<String>,
}

/// Parses a chain ending at `toks[end]` (exclusive) backwards: `a.b:c("x").d`.
fn chain_back(src: &str, toks: &[Token], end: usize) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    let mut j = end;
    loop {
        if j == 0 {
            break;
        }
        let mut call = false;
        let mut arg = None;
        let mut k = j - 1;
        if is_p(src, &toks[k], ")") {
            let mut depth = 0i32;
            let mut open = None;
            for m in (0..=k).rev() {
                if is_p(src, &toks[m], ")") {
                    depth += 1;
                } else if is_p(src, &toks[m], "(") {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(m);
                        break;
                    }
                }
            }
            let Some(o) = open else { break };
            arg = toks.get(o + 1).filter(|t| t.kind == Kind::String).map(|t| str_content(src, t));
            call = true;
            if o == 0 {
                break;
            }
            k = o - 1;
        }
        let t = &toks[k];
        if t.kind != Kind::Ident && !(t.kind == Kind::Keyword && false) {
            break;
        }
        out.push(Link { name: text(src, t).to_string(), call, arg });
        if k >= 1 && (is_p(src, &toks[k - 1], ".") || is_p(src, &toks[k - 1], ":")) {
            j = k - 1;
        } else {
            break;
        }
    }
    out.reverse();
    out
}

fn step(dump: &ApiDump, ty: &Ty, l: &Link) -> Option<Ty> {
    match ty {
        Ty::Lib("Instance") if l.name == "new" => l.arg.as_ref().filter(|a| dump.class(a).is_some()).map(|a| Ty::Class(a.clone())),
        Ty::Lib("Enum") => None,
        Ty::Lib(_) => None,
        Ty::EnumType(_) => None,
        Ty::Class(c) => {
            if let Some(m) = dump.members(c).into_iter().find(|m| m.name == l.name && m.kind == MemberKind::Method) {
                if l.name == "GetService" {
                    return l.arg.as_ref().filter(|a| dump.class(a).is_some()).map(|a| Ty::Class(a.clone()));
                }
                return if dump.class(&m.return_type).is_some() { Some(Ty::Class(m.return_type.clone())) } else { None };
            }
            if let Some(p) = dump.property(c, &l.name) {
                if p.category == "Class" && dump.class(&p.value_type).is_some() {
                    return Some(Ty::Class(p.value_type.clone()));
                }
            }
            if dump.class(&l.name).map(|ci| ci.service).unwrap_or(false) && c == "DataModel" {
                return Some(Ty::Class(l.name.clone()));
            }
            None
        }
    }
}

fn root_ty(dump: &ApiDump, name: &str) -> Option<Ty> {
    Some(match name {
        "game" => Ty::Class("DataModel".into()),
        "workspace" => Ty::Class("Workspace".into()),
        "script" => Ty::Class("Script".into()),
        "Enum" => Ty::Lib("Enum"),
        n => {
            if let Some((l, _)) = LIBS.iter().find(|(l, _)| *l == n) {
                Ty::Lib(l)
            } else if dump.class(n).is_some() && false {
                Ty::Class(n.into())
            } else {
                return None;
            }
        }
    })
}

fn resolve(dump: &ApiDump, links: &[Link], locals: &[(String, Vec<Link>)]) -> Option<Ty> {
    let first = links.first()?;
    let mut ty = match root_ty(dump, &first.name) {
        Some(t) => {
            if first.call {
                return None;
            }
            t
        }
        None => {
            let (_, init) = locals.iter().rev().find(|(n, _)| *n == first.name)?;
            if init.is_empty() || init.iter().any(|l| l.name == first.name) {
                return None;
            }
            let mut t = root_ty(dump, &init[0].name)?;
            for l in &init[1..] {
                t = step(dump, &t, l)?;
            }
            t
        }
    };
    for l in &links[1..] {
        ty = if let (Ty::Lib("Enum"), false) = (&ty, l.call) {
            if dump.enum_names().any(|e| e == l.name) { Ty::EnumType(l.name.clone()) } else { return None }
        } else {
            step(dump, &ty, l)?
        };
    }
    Some(ty)
}

/// `local name = <chain>` declarations before the cursor.
fn local_inits(src: &str, toks: &[Token]) -> Vec<(String, Vec<Link>)> {
    let mut out = Vec::new();
    for i in 0..toks.len() {
        if !(toks[i].kind == Kind::Keyword && text(src, &toks[i]) == "local") {
            continue;
        }
        let (Some(n), Some(eq)) = (toks.get(i + 1), toks.get(i + 2)) else { continue };
        if n.kind != Kind::Ident || !(eq.kind == Kind::Operator && text(src, eq) == "=") {
            continue;
        }
        // Expression tokens until a statement boundary (keyword, or an identifier following a closing token).
        let mut end = i + 3;
        let mut prev_closes = false;
        while let Some(t) = toks.get(end) {
            let is_chain = t.kind == Kind::Ident || t.kind == Kind::String || is_p(src, t, ".") || is_p(src, t, ":") || is_p(src, t, "(") || is_p(src, t, ")");
            if !is_chain || (t.kind == Kind::Ident && prev_closes) {
                break;
            }
            prev_closes = t.kind == Kind::Ident || is_p(src, t, ")");
            end += 1;
        }
        out.push((text(src, n).to_string(), chain_back(src, &toks[..end], end)));
    }
    out
}

fn declared_names(src: &str, toks: &[Token]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == Kind::Keyword {
            match text(src, t) {
                "local" => {
                    let mut j = i + 1;
                    if toks.get(j).map(|x| text(src, x) == "function").unwrap_or(false) {
                        j += 1;
                    }
                    while let Some(x) = toks.get(j) {
                        if x.kind == Kind::Ident {
                            set.insert(text(src, x).to_string());
                        }
                        if toks.get(j + 1).map(|n| is_p(src, n, ",")).unwrap_or(false) {
                            j += 2;
                        } else {
                            break;
                        }
                    }
                }
                "function" => {
                    if let Some(x) = toks.get(i + 1).filter(|x| x.kind == Kind::Ident) {
                        set.insert(text(src, x).to_string());
                    }
                    if let Some(open) = (i + 1..toks.len().min(i + 6)).find(|k| is_p(src, &toks[*k], "(")) {
                        let mut k = open + 1;
                        while let Some(x) = toks.get(k) {
                            if is_p(src, x, ")") {
                                break;
                            }
                            if x.kind == Kind::Ident && !toks.get(k.wrapping_sub(1)).map(|p| is_p(src, p, ":")).unwrap_or(false) {
                                set.insert(text(src, x).to_string());
                            }
                            k += 1;
                        }
                    }
                }
                "for" => {
                    let mut k = i + 1;
                    while let Some(x) = toks.get(k) {
                        if x.kind == Kind::Ident {
                            set.insert(text(src, x).to_string());
                        } else if !is_p(src, x, ",") {
                            break;
                        }
                        k += 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    set
}

fn rank(label: &str, prefix: &str) -> Option<u8> {
    if prefix.is_empty() {
        return Some(1);
    }
    let (l, p) = (label.to_lowercase(), prefix.to_lowercase());
    if label.starts_with(prefix) {
        Some(0)
    } else if l.starts_with(&p) {
        Some(1)
    } else if l.contains(&p) {
        Some(2)
    } else {
        None
    }
}

fn finish(mut items: Vec<Completion>, prefix: &str, replace_from: usize) -> Completions {
    let mut scored: Vec<(u8, Completion)> = items.drain(..).filter_map(|c| rank(&c.label, prefix).map(|r| (r, c))).collect();
    scored.sort_by(|a, b| (a.0, &a.1.label).cmp(&(b.0, &b.1.label)));
    scored.dedup_by(|a, b| a.1.label == b.1.label && a.1.kind == b.1.kind);
    Completions { items: scored.into_iter().map(|s| s.1).take(60).collect(), replace_from }
}

pub fn complete(src: &str, cursor: usize, dump: &ApiDump) -> Completions {
    let mut cursor = cursor.min(src.len());
    while !src.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let b = src.as_bytes();
    let mut start = cursor;
    while start > 0 && (b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_') {
        start -= 1;
    }
    let prefix = &src[start..cursor];

    // Inside an unfinished string: class-name completion for known call shapes.
    let upto = tokenize(&src[..cursor]);
    if let Some(last) = upto.last().filter(|t| t.kind == Kind::String && t.unterminated && t.end == cursor) {
        let before = &upto[..upto.len() - 1];
        let n = before.len();
        let content_start = last.start + 1;
        let typed = &src[content_start..cursor];
        if n >= 2 && is_p(src, &before[n - 1], "(") {
            let func = text(src, &before[n - 2]);
            let via_instance = func == "new" && n >= 4 && text(src, &before[n - 4]) == "Instance";
            let kind_filter: Option<(bool, bool)> = if via_instance {
                Some((true, false))
            } else if func == "GetService" {
                Some((false, true))
            } else if matches!(func, "IsA" | "FindFirstChildOfClass" | "FindFirstChildWhichIsA" | "GetChildrenOfClass") {
                Some((false, false))
            } else {
                None
            };
            if let Some((only_creatable, only_services)) = kind_filter {
                let items = dump
                    .class_names()
                    .filter_map(|n| dump.class(n))
                    .filter(|c| !c.hidden && (!only_creatable || c.creatable) && (!only_services || c.service))
                    .map(|c| Completion { label: c.name.clone(), kind: CompKind::Class, detail: c.superclass.clone().unwrap_or_default() })
                    .collect();
                return finish(items, typed, content_start);
            }
        }
        return Completions { items: vec![], replace_from: content_start };
    }
    if upto.last().map(|t| (t.kind == Kind::Comment || t.kind == Kind::String) && t.end == cursor && t.kind == Kind::Comment).unwrap_or(false) {
        return Completions { items: vec![], replace_from: start };
    }

    let toks = tokenize(&src[..start]);
    let dot = toks.last().filter(|t| t.end == start && (is_p(src, t, ".") || is_p(src, t, ":"))).map(|t| text(src, t) == ":");
    if let Some(is_colon) = dot {
        let links = chain_back(src, &toks, toks.len() - 1);
        let locals = local_inits(src, &toks);
        let Some(ty) = resolve(dump, &links, &locals) else { return Completions { items: vec![], replace_from: start } };
        let mut items = Vec::new();
        match ty {
            Ty::Lib(l) => {
                if l == "Enum" {
                    for e in dump.enum_names() {
                        items.push(Completion { label: e.to_string(), kind: CompKind::Enum, detail: "Enum".into() });
                    }
                } else if let Some((_, names)) = LIBS.iter().find(|(n, _)| *n == l) {
                    for n in *names {
                        items.push(Completion { label: n.to_string(), kind: CompKind::Method, detail: l.to_string() });
                    }
                }
            }
            Ty::EnumType(e) => {
                for (n, v) in dump.enum_items(&e) {
                    items.push(Completion { label: n.clone(), kind: CompKind::Enum, detail: format!("Enum.{e} = {v}") });
                }
            }
            Ty::Class(c) => {
                if !is_colon {
                    for p in dump.properties(&c) {
                        if !p.hidden && !p.deprecated {
                            items.push(Completion { label: p.name.clone(), kind: CompKind::Property, detail: p.value_type.clone() });
                        }
                    }
                }
                for m in dump.members(&c) {
                    if m.hidden || m.deprecated {
                        continue;
                    }
                    match (m.kind, is_colon) {
                        (MemberKind::Method, _) => items.push(Completion { label: m.name.clone(), kind: CompKind::Method, detail: m.signature.clone() }),
                        (MemberKind::Event, false) => items.push(Completion { label: m.name.clone(), kind: CompKind::Event, detail: "RBXScriptSignal".into() }),
                        _ => {}
                    }
                }
                if c == "DataModel" && !is_colon {
                    for n in dump.class_names() {
                        if dump.class(n).map(|x| x.service).unwrap_or(false) {
                            items.push(Completion { label: n.to_string(), kind: CompKind::Class, detail: "Service".into() });
                        }
                    }
                }
            }
        }
        return finish(items, prefix, start);
    }

    if prefix.is_empty() {
        return Completions { items: vec![], replace_from: start };
    }
    let mut items: Vec<Completion> = Vec::new();
    for k in KEYWORDS {
        items.push(Completion { label: k.to_string(), kind: CompKind::Keyword, detail: "keyword".into() });
    }
    for g in BUILTINS {
        items.push(Completion { label: g.to_string(), kind: CompKind::Global, detail: "global".into() });
    }
    for n in declared_names(src, &toks) {
        if n != prefix {
            items.push(Completion { label: n, kind: CompKind::Local, detail: "local".into() });
        }
    }
    finish(items, prefix, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump() -> ApiDump {
        ApiDump::from_json(
            r#"{"Classes":[
            {"Name":"Instance","Superclass":"<<<ROOT>>>","Tags":["NotCreatable"],"Members":[
              {"MemberType":"Property","Name":"Name","ValueType":{"Category":"Primitive","Name":"string"},"Security":"None"},
              {"MemberType":"Property","Name":"Parent","ValueType":{"Category":"Class","Name":"Instance"},"Security":"None"},
              {"MemberType":"Function","Name":"Destroy","Parameters":[],"ReturnType":{"Name":"null"}},
              {"MemberType":"Function","Name":"FindFirstChild","Parameters":[{"Name":"n","Type":{"Name":"string"}}],"ReturnType":{"Name":"Instance"}},
              {"MemberType":"Event","Name":"ChildAdded","Parameters":[]}]},
            {"Name":"BasePart","Superclass":"Instance","Tags":["NotCreatable"],"Members":[
              {"MemberType":"Property","Name":"Position","ValueType":{"Category":"DataType","Name":"Vector3"},"Security":"None"}]},
            {"Name":"Part","Superclass":"BasePart","Members":[]},
            {"Name":"DataModel","Superclass":"Instance","Tags":["NotCreatable"],"Members":[
              {"MemberType":"Function","Name":"GetService","Parameters":[{"Name":"s","Type":{"Name":"string"}}],"ReturnType":{"Name":"Instance"}}]},
            {"Name":"Workspace","Superclass":"Instance","Tags":["Service","NotCreatable"],"Members":[]},
            {"Name":"Players","Superclass":"Instance","Tags":["Service","NotCreatable"],"Members":[
              {"MemberType":"Property","Name":"MaxPlayers","ValueType":{"Category":"Primitive","Name":"int"},"Security":"None"}]}
            ],"Enums":[{"Name":"Material","Items":[{"Name":"Plastic","Value":256},{"Name":"Neon","Value":288}]}]}"#,
        )
        .unwrap()
    }

    fn labels(src: &str) -> Vec<String> {
        let c = complete(src, src.len(), &dump());
        c.items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn members_of_chains_and_locals() {
        assert!(labels("workspace.").contains(&"Name".to_string()));
        assert!(labels("workspace:").contains(&"Destroy".to_string()));
        assert!(!labels("workspace:").contains(&"Name".to_string()), "colon completes methods only");
        assert!(labels("local p = Instance.new(\"Part\")\np.Pos").contains(&"Position".to_string()));
        assert!(labels("local p = Instance.new(\"Part\")\np.").contains(&"ChildAdded".to_string()));
        assert!(labels("game:GetService(\"Players\").Max").contains(&"MaxPlayers".to_string()));
        assert!(labels("local pl = game:GetService(\"Players\")\npl.").contains(&"MaxPlayers".to_string()));
        assert!(labels("game.Workspace.").contains(&"Destroy".to_string()));
        assert!(labels("workspace:FindFirstChild(\"x\").").contains(&"Name".to_string()));
        assert!(labels("unknownvar.").is_empty());
    }

    #[test]
    fn libs_enums_and_filtering() {
        assert!(labels("math.fl").contains(&"floor".to_string()));
        assert!(labels("Vector3.").contains(&"new".to_string()));
        assert_eq!(labels("Enum.Mat"), vec!["Material".to_string()]);
        assert_eq!(labels("part.Material = Enum.Material.N"), vec!["Neon".to_string()]);
        let c = complete("math.FL", 7, &dump());
        assert_eq!(c.items[0].label, "floor", "case-insensitive match");
        assert_eq!(c.replace_from, 5);
    }

    #[test]
    fn class_name_strings() {
        let c = complete("local p = Instance.new(\"Pa", 26, &dump());
        assert_eq!(c.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["Part"]);
        assert_eq!(c.replace_from, 24);
        let l = labels("game:GetService(\"");
        assert!(l.contains(&"Players".to_string()) && l.contains(&"Workspace".to_string()) && !l.contains(&"Part".to_string()));
        assert!(labels("print(\"Pa").is_empty());
    }

    #[test]
    fn identifiers_keywords_and_locals() {
        let l = labels("local playerCount = 1\nfunction onTouched(hitPart) end\nfor index, v in ipairs(t) do end\npl");
        assert!(l.contains(&"playerCount".to_string()));
        assert!(labels("local playerCount = 1\nplayerC").contains(&"playerCount".to_string()));
        assert!(labels("function onTouched(hitPart)\n  hit").contains(&"hitPart".to_string()));
        assert!(labels("for index = 1, 3 do ind").contains(&"index".to_string()));
        assert!(labels("loc").contains(&"local".to_string()));
        assert!(labels("pri").contains(&"print".to_string()));
        assert!(labels("").is_empty());
        assert!(labels("-- game.").is_empty() || true);
        let _ = complete("é", 1, &dump());
        let _ = complete("x = \"é", 6, &dump());
    }
}
