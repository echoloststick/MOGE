//! Small non-validating XML parser sufficient for rbxlx (elements, attributes, text, CDATA, entities).
use crate::error::FormatError;

const MAX_DEPTH: usize = 600;

#[derive(Debug, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }
}

fn err<T>(m: &str) -> Result<T, FormatError> {
    Err(FormatError::Malformed(format!("Invalid XML: {m}")))
}

pub fn parse(src: &str) -> Result<Element, FormatError> {
    let mut x = X { s: src, b: src.as_bytes(), i: 0 };
    x.skip_misc()?;
    if x.b.get(x.i) != Some(&b'<') {
        return err("expected a root element");
    }
    x.run()
}

struct X<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> X<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn starts(&self, p: &str) -> bool {
        self.b[self.i..].starts_with(p.as_bytes())
    }
    fn skip_to(&mut self, end: &str) -> Result<(), FormatError> {
        match self.s[self.i..].find(end) {
            Some(p) => {
                self.i += p + end.len();
                Ok(())
            }
            None => err("unterminated markup"),
        }
    }
    fn skip_misc(&mut self) -> Result<(), FormatError> {
        loop {
            self.ws();
            if self.starts("<?") {
                self.skip_to("?>")?;
            } else if self.starts("<!--") {
                self.skip_to("-->")?;
            } else if self.starts("<!DOCTYPE") {
                self.skip_to(">")?;
            } else {
                return Ok(());
            }
        }
    }
    fn name(&mut self) -> Result<String, FormatError> {
        let st = self.i;
        while self.i < self.b.len() && !matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/' | b'=') {
            self.i += 1;
        }
        if st == self.i {
            return err("empty name");
        }
        Ok(self.s[st..self.i].to_string())
    }
    /// Parses an opening tag at '<'. Returns the element and whether it was self-closing.
    fn open_tag(&mut self) -> Result<(Element, bool), FormatError> {
        self.i += 1; // '<'
        let name = self.name()?;
        let mut el = Element { name, ..Default::default() };
        loop {
            self.ws();
            match self.b.get(self.i) {
                None => return err("unexpected end of input"),
                Some(b'/') => {
                    if self.b.get(self.i + 1) == Some(&b'>') {
                        self.i += 2;
                        return Ok((el, true));
                    }
                    return err("stray '/' in tag");
                }
                Some(b'>') => {
                    self.i += 1;
                    return Ok((el, false));
                }
                _ => {
                    let k = self.name()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b'=') {
                        return err("expected '=' after attribute name");
                    }
                    self.i += 1;
                    self.ws();
                    let q = match self.b.get(self.i) {
                        Some(q @ (b'"' | b'\'')) => *q as char,
                        _ => return err("expected quoted attribute value"),
                    };
                    self.i += 1;
                    let p = match self.s[self.i..].find(q) {
                        Some(p) => p,
                        None => return err("unterminated attribute value"),
                    };
                    el.attrs.push((k, unescape(&self.s[self.i..self.i + p])));
                    self.i += p + 1;
                }
            }
        }
    }

    /// Iterative (explicit stack) so deeply nested files cannot overflow the call stack.
    fn run(&mut self) -> Result<Element, FormatError> {
        let (root, closed) = self.open_tag()?;
        if closed {
            return Ok(root);
        }
        let mut stack = vec![root];
        loop {
            let p = match self.s[self.i..].find('<') {
                Some(p) => p,
                None => return err("unexpected end of input"),
            };
            if p > 0 {
                let t = unescape(&self.s[self.i..self.i + p]);
                stack.last_mut().expect("stack is never empty here").text.push_str(&t);
                self.i += p;
            }
            if self.starts("</") {
                self.i += 2;
                let close = self.name()?;
                let top = stack.pop().expect("stack is never empty here");
                if close != top.name {
                    return err(&format!("mismatched closing tag </{close}> for <{}>", top.name));
                }
                self.skip_to(">")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(top),
                    None => return Ok(top),
                }
            } else if self.starts("<!--") {
                self.skip_to("-->")?;
            } else if self.starts("<![CDATA[") {
                self.i += 9;
                let p = match self.s[self.i..].find("]]>") {
                    Some(p) => p,
                    None => return err("unterminated CDATA"),
                };
                let raw = &self.s[self.i..self.i + p];
                stack.last_mut().expect("stack is never empty here").text.push_str(raw);
                self.i += p + 3;
            } else if self.starts("<?") {
                self.skip_to("?>")?;
            } else {
                let (el, closed) = self.open_tag()?;
                if closed {
                    stack.last_mut().expect("stack is never empty here").children.push(el);
                } else {
                    if stack.len() >= MAX_DEPTH {
                        return err("nesting too deep");
                    }
                    stack.push(el);
                }
            }
        }
    }
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let decoded = rest.find(';').filter(|e| *e <= 12).and_then(|e| {
            let ent = &rest[1..e];
            let ch = match ent {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => {
                    let n = if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        ent.strip_prefix('#').and_then(|d| d.parse().ok())
                    };
                    n.and_then(char::from_u32)
                }
            };
            ch.map(|c| (c, e + 1))
        });
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structure_entities_cdata() {
        let e = parse("<?xml version=\"1.0\"?><!-- c --><a x='1' y=\"&lt;\"><b>t&amp;&#65;&#x42;</b><c/><d><![CDATA[<raw>]]></d></a>").unwrap();
        assert_eq!(e.attr("x"), Some("1"));
        assert_eq!(e.attr("y"), Some("<"));
        assert_eq!(e.child("b").unwrap().text, "t&AB");
        assert!(e.child("c").is_some());
        assert_eq!(e.child("d").unwrap().text, "<raw>");
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a").is_err());
        assert!(parse("<a x=1/>").is_err());
        assert!(parse("").is_err());
        let deep = "<a>".repeat(3000);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn unknown_entities_are_kept() {
        assert_eq!(unescape("a &foo; &"), "a &foo; &");
    }
}
