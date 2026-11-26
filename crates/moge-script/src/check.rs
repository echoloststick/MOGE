//! Block-structure diagnostics (unclosed/unmatched blocks, unterminated strings and comments).
use crate::lexer::{tokenize, Kind, Token};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 1-based line and column (in characters).
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Block {
    Function,
    If,
    /// while/for waiting for its `do`; becomes active once `do` is seen.
    Loop(bool),
    Do,
    Repeat,
}

fn position(src: &str, byte: usize) -> (usize, usize) {
    let before = &src[..byte.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

/// Statement-start keywords after which an `if` begins a statement (otherwise it is an if-expression).
fn if_is_expression(src: &str, prev: Option<&Token>) -> bool {
    let Some(p) = prev else { return false };
    let t = &src[p.start..p.end];
    match p.kind {
        Kind::Operator => true,
        Kind::Punct => matches!(t, "(" | "{" | "," | "[" | ":"),
        Kind::Keyword => matches!(t, "return" | "and" | "or" | "not" | "in" | "else" | "elseif" | "then") && matches!(t, "return" | "and" | "or" | "not" | "in"),
        _ => false,
    }
}

pub fn check(src: &str) -> Vec<Diagnostic> {
    let toks = tokenize(src);
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut add = |tok: &Token, message: String| {
        let (line, col) = position(src, tok.start);
        diags.push(Diagnostic { line, col, message, start: tok.start, end: tok.end });
    };
    let mut stack: Vec<(Block, usize)> = Vec::new();
    let mut expr_ifs = 0usize;
    for (n, t) in toks.iter().enumerate() {
        if t.unterminated {
            match t.kind {
                Kind::String => add(t, "Unfinished string literal".into()),
                Kind::Comment => add(t, "Unfinished multi-line comment".into()),
                _ => {}
            }
            continue;
        }
        if t.kind != Kind::Keyword {
            continue;
        }
        let text = &src[t.start..t.end];
        let prev = n.checked_sub(1).map(|p| &toks[p]);
        let after_member = prev.map(|p| &src[p.start..p.end] == "." || &src[p.start..p.end] == ":").unwrap_or(false);
        if after_member {
            continue; // a field named like a keyword, e.g. `obj.end`
        }
        match text {
            "function" => stack.push((Block::Function, n)),
            "if" => {
                if if_is_expression(src, prev) {
                    expr_ifs += 1;
                } else {
                    stack.push((Block::If, n));
                }
            }
            "while" | "for" => stack.push((Block::Loop(false), n)),
            "do" => match stack.last_mut() {
                Some((Block::Loop(seen @ false), _)) => *seen = true,
                _ => stack.push((Block::Do, n)),
            },
            "repeat" => stack.push((Block::Repeat, n)),
            "elseif" | "else" => {
                if !matches!(stack.last(), Some((Block::If, _))) && expr_ifs == 0 {
                    add(t, format!("'{text}' without a matching 'if'"));
                }
            }
            "until" => match stack.last() {
                Some((Block::Repeat, _)) => {
                    stack.pop();
                }
                _ => add(t, "'until' without a matching 'repeat'".into()),
            },
            "end" => match stack.last() {
                Some((Block::Repeat, o)) => {
                    let o = &toks[*o];
                    let msg = format!("Expected 'until' to close 'repeat' opened at line {}", position(src, o.start).0);
                    add(t, msg);
                    stack.pop();
                }
                Some((Block::Loop(false), _)) => {
                    add(t, "Expected 'do' before 'end'".into());
                    stack.pop();
                }
                Some(_) => {
                    stack.pop();
                }
                None => add(t, "'end' without a matching block".into()),
            },
            _ => {}
        }
    }
    for (b, n) in stack {
        let t = &toks[n];
        let msg = match b {
            Block::Function => "Unclosed 'function' (expected 'end')",
            Block::If => "Unclosed 'if' (expected 'end')",
            Block::Loop(_) => "Unclosed loop (expected 'end')",
            Block::Do => "Unclosed 'do' (expected 'end')",
            Block::Repeat => "Unclosed 'repeat' (expected 'until')",
        };
        add(t, msg.into());
    }
    diags.sort_by_key(|d| d.start);
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(src: &str) -> Vec<String> {
        check(src).into_iter().map(|d| format!("{}:{} {}", d.line, d.col, d.message)).collect()
    }

    #[test]
    fn valid_programs_have_no_diagnostics() {
        let ok = [
            "local function f(a)\n  if a then return 1 elseif a == 2 then return 2 else return 3 end\nend\nfor i = 1, 3 do print(i) end\nwhile true do break end",
            "repeat local x = 1 until x == 1\nlocal t = {}\nfor k, v in pairs(t) do end\ndo local y = 2 end",
            "local x = if a then 1 else 2\nlocal y = (if a then 1 elseif b then 2 else 3)\nreturn if x then y else x",
            "local t = { end_ = 1 }\nobj.end = 2\nobj:end()",
            "",
        ];
        for s in ok {
            assert!(check(s).is_empty(), "{s:?} -> {:?}", msgs(s));
        }
    }

    #[test]
    fn reports_unclosed_and_extra_blocks() {
        assert_eq!(msgs("function f()\n  if x then\nend"), vec!["1:1 Unclosed 'function' (expected 'end')".to_string()]);
        assert_eq!(msgs("if x then end end"), vec!["1:15 'end' without a matching block".to_string()]);
        assert_eq!(msgs("for i = 1, 2 do\n"), vec!["1:1 Unclosed loop (expected 'end')".to_string()]);
        assert_eq!(msgs("repeat x = 1 end"), vec!["1:14 Expected 'until' to close 'repeat' opened at line 1".to_string()]);
        assert_eq!(msgs("x = 1 else"), vec!["1:7 'else' without a matching 'if'".to_string()]);
        assert_eq!(msgs("until x"), vec!["1:1 'until' without a matching 'repeat'".to_string()]);
        assert_eq!(msgs("while x end"), vec!["1:9 Expected 'do' before 'end'".to_string()]);
    }

    #[test]
    fn reports_unfinished_strings_and_comments() {
        let m = msgs("print(\"abc\nx = 1 --[[ open");
        assert_eq!(m, vec!["1:7 Unfinished string literal".to_string(), "2:7 Unfinished multi-line comment".to_string()]);
    }
}
