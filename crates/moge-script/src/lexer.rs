//! Luau tokenizer. Works on bytes (all syntax is ASCII) and always returns spans on char boundaries.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    Ident,
    Number,
    String,
    Comment,
    Operator,
    Punct,
    /// Colorizer-only kinds
    Builtin,
    Function,
    Property,
    TypeName,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// String/comment that reaches the end of input (or line, for short strings) without closing.
    pub unterminated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then",
    "true", "until", "while", "continue", "export", "type", "typeof",
];

pub const BUILTINS: &[&str] = &[
    "game", "workspace", "script", "print", "warn", "error", "assert", "pcall", "xpcall", "select", "pairs", "ipairs", "next", "tostring", "tonumber",
    "type", "typeof", "require", "wait", "spawn", "delay", "tick", "time", "os", "math", "string", "table", "task", "coroutine", "utf8", "bit32",
    "debug", "Instance", "Vector3", "Vector2", "CFrame", "Color3", "BrickColor", "UDim2", "UDim", "Rect", "Enum", "TweenInfo", "Random", "Ray",
    "NumberRange", "NumberSequence", "ColorSequence", "Region3", "Faces", "Axes", "rawget", "rawset", "rawequal", "rawlen", "setmetatable",
    "getmetatable", "unpack", "shared", "_G", "_VERSION", "plugin", "settings", "newproxy", "gcinfo", "collectgarbage", "loadstring", "buffer",
];

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// If `b[i..]` starts a long bracket `[=*[`, returns the level.
fn long_open(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    if b.get(j) == Some(&b'[') { Some(j - i - 1) } else { None }
}

/// Returns (end, terminated) of a long bracket starting at `i` with `level`.
fn long_end(b: &[u8], i: usize, level: usize) -> (usize, bool) {
    let mut j = i + level + 2;
    while j < b.len() {
        if b[j] == b']' {
            let mut k = j + 1;
            let mut eq = 0;
            while b.get(k) == Some(&b'=') {
                eq += 1;
                k += 1;
            }
            if eq == level && b.get(k) == Some(&b']') {
                return (k + 1, true);
            }
        }
        j += 1;
    }
    (b.len(), false)
}

const OPS3: &[&str] = &["...", "//=", "..="];
const OPS2: &[&str] = &["..", "==", "~=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "^=", "->", "::", "//"];

pub fn tokenize(src: &str) -> Vec<Token> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let mut unterminated = false;
        let kind;
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            kind = Kind::Comment;
            if let Some(l) = long_open(b, i + 2) {
                let (e, ok) = long_end(b, i + 2, l);
                i = e;
                unterminated = !ok;
            } else {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
        } else if let Some(l) = long_open(b, i) {
            kind = Kind::String;
            let (e, ok) = long_end(b, i, l);
            i = e;
            unterminated = !ok;
        } else if c == b'"' || c == b'\'' || c == b'`' {
            kind = Kind::String;
            i += 1;
            let mut closed = false;
            while i < b.len() {
                match b[i] {
                    b'\\' => i += 2,
                    x if x == c => {
                        i += 1;
                        closed = true;
                        break;
                    }
                    b'\n' if c != b'`' => break,
                    _ => i += 1,
                }
            }
            i = i.min(b.len());
            unterminated = !closed;
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).map(|d| d.is_ascii_digit()).unwrap_or(false)) {
            kind = Kind::Number;
            if c == b'0' && matches!(b.get(i + 1), Some(b'x') | Some(b'X') | Some(b'b') | Some(b'B')) {
                i += 2;
                while i < b.len() && (b[i].is_ascii_hexdigit() || b[i] == b'_') {
                    i += 1;
                }
            } else {
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_' || b[i] == b'.' && b.get(i + 1) != Some(&b'.')) {
                    i += 1;
                }
                if matches!(b.get(i), Some(b'e') | Some(b'E')) {
                    let mut j = i + 1;
                    if matches!(b.get(j), Some(b'+') | Some(b'-')) {
                        j += 1;
                    }
                    if b.get(j).map(|d| d.is_ascii_digit()).unwrap_or(false) {
                        i = j;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
            }
        } else if is_ident_start(c) {
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            kind = if KEYWORDS.contains(&&src[start..i]) { Kind::Keyword } else { Kind::Ident };
        } else if let Some(op) = OPS3.iter().chain(OPS2.iter()).find(|o| src[i..].starts_with(**o)) {
            i += op.len();
            kind = Kind::Operator;
        } else if matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'#' | b'=' | b'<' | b'>' | b'~' | b'&' | b'|' | b'?') {
            i += 1;
            kind = Kind::Operator;
        } else if matches!(c, b'(' | b')' | b'{' | b'}' | b'[' | b']' | b';' | b':' | b',' | b'.') {
            i += 1;
            kind = Kind::Punct;
        } else {
            // Advance one full UTF-8 character so spans stay on char boundaries.
            i += 1;
            while i < b.len() && !src.is_char_boundary(i) {
                i += 1;
            }
            kind = Kind::Unknown;
        }
        out.push(Token { kind, start, end: i.min(b.len()), unterminated });
    }
    out
}

/// Spans with editor-oriented kinds (functions, properties, builtins, type names).
pub fn colorize(src: &str) -> Vec<Span> {
    let toks = tokenize(src);
    let mut out = Vec::with_capacity(toks.len());
    for (n, t) in toks.iter().enumerate() {
        let text = &src[t.start..t.end];
        let prev = n.checked_sub(1).map(|p| &toks[p]);
        let next = toks.get(n + 1);
        let prev_text = prev.map(|p| &src[p.start..p.end]);
        let kind = match t.kind {
            Kind::Ident => {
                let after_member = matches!(prev_text, Some(".") | Some(":"));
                let called = next.map(|x| &src[x.start..x.end] == "(" || x.kind == Kind::String).unwrap_or(false);
                if prev_text == Some("function") {
                    Kind::Function
                } else if after_member && called {
                    Kind::Function
                } else if after_member {
                    Kind::Property
                } else if prev_text == Some("::") || (prev_text == Some(":") && false) {
                    Kind::TypeName
                } else if BUILTINS.contains(&text) {
                    Kind::Builtin
                } else if called {
                    Kind::Function
                } else {
                    Kind::Ident
                }
            }
            Kind::Keyword if text == "type" || text == "typeof" => {
                // `type`/`typeof` are builtins unless they start a type alias.
                if text == "type" && matches!(next.map(|x| x.kind), Some(Kind::Ident)) { Kind::Keyword } else { Kind::Builtin }
            }
            k => k,
        };
        out.push(Span { start: t.start, end: t.end, kind });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(String, Kind)> {
        colorize(src).into_iter().map(|s| (src[s.start..s.end].to_string(), s.kind)).collect()
    }

    #[test]
    fn tokenizes_core_syntax() {
        let k = kinds("local x = 0xFF + 1_000.5e-3 -- hi\nprint(\"a\\\"b\", 'c', [==[long]]]==])");
        assert!(k.contains(&("local".into(), Kind::Keyword)));
        assert!(k.contains(&("0xFF".into(), Kind::Number)));
        assert!(k.contains(&("1_000.5e-3".into(), Kind::Number)));
        assert!(k.contains(&("-- hi".into(), Kind::Comment)));
        assert!(k.contains(&("print".into(), Kind::Builtin)));
        assert!(k.contains(&("\"a\\\"b\"".into(), Kind::String)));
        assert!(k.contains(&("[==[long]]]==]".into(), Kind::String)));
    }

    #[test]
    fn member_function_and_operator_kinds() {
        let k = kinds("part.Position = Vector3.new(1, 2, 3) .. x:Destroy()\nfunction foo() end\nx += 1");
        assert!(k.contains(&("Position".into(), Kind::Property)));
        assert!(k.contains(&("new".into(), Kind::Function)));
        assert!(k.contains(&("Destroy".into(), Kind::Function)));
        assert!(k.contains(&("foo".into(), Kind::Function)));
        assert!(k.contains(&("..".into(), Kind::Operator)));
        assert!(k.contains(&("+=".into(), Kind::Operator)));
        assert!(k.contains(&("Vector3".into(), Kind::Builtin)));
    }

    #[test]
    fn comments_strings_and_unterminated() {
        let t = tokenize("--[[ multi\nline ]] x = `a {b}` y = \"open");
        assert_eq!(t[0].kind, Kind::Comment);
        assert!(!t[0].unterminated);
        assert!(t.iter().any(|x| x.kind == Kind::String && !x.unterminated));
        assert!(t.last().unwrap().unterminated);
        assert!(tokenize("--[[ never closed").last().unwrap().unterminated);
        let s = tokenize("a = 'x\ny = 2");
        assert!(s.iter().any(|x| x.kind == Kind::String && x.unterminated), "newline ends a short string");
        assert!(s.iter().any(|x| x.kind == Kind::Number), "lexing continues on the next line");
    }

    #[test]
    fn range_dots_and_unicode_do_not_break_spans() {
        let k = kinds("for i = 1, 10 do local s = 1..2 end");
        assert!(k.contains(&("1".into(), Kind::Number)));
        let src = "x = \"olá\" é ñ -- ção";
        for t in tokenize(src) {
            assert!(src.is_char_boundary(t.start) && src.is_char_boundary(t.end), "{t:?}");
        }
        for s in ["", "\"", "--", "[[", "[=", "0x", "1e", ".", "\u{1F600}"] {
            let _ = tokenize(s);
            let _ = colorize(s);
        }
    }
}
