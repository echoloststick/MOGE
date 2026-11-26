//! Loading `.lua`, `.luau` and `.txt` files as scripts.
use std::path::Path;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const EXTENSIONS: &[&str] = &["lua", "luau", "txt"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    pub name: String,
    pub class: &'static str,
    pub source: String,
}

pub fn is_script_path(p: &str) -> bool {
    Path::new(p).extension().and_then(|e| e.to_str()).map(|e| EXTENSIONS.contains(&e.to_lowercase().as_str())).unwrap_or(false)
}

/// Decodes UTF-8 / UTF-16 (BOM) text, rejects binary data and normalizes line endings to `\n`.
pub fn decode_text(bytes: &[u8]) -> Result<String, String> {
    let text = if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(b, true)?
    } else if let Some(b) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(b, false)?
    } else {
        let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        if b.contains(&0) {
            return Err("The file looks like binary data, not a text script".into());
        }
        String::from_utf8(b.to_vec()).map_err(|_| "The file is not valid UTF-8 text".to_string())?
    };
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn utf16(b: &[u8], le: bool) -> Result<String, String> {
    if b.len() % 2 != 0 {
        return Err("The UTF-16 file has an odd number of bytes".into());
    }
    let units: Vec<u16> = b.chunks(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
    String::from_utf16(&units).map_err(|_| "The file is not valid UTF-16 text".to_string())
}

fn ends_with_return(src: &str) -> bool {
    src.lines().rev().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("--")).map(|l| l == "return" || l.starts_with("return ") || l.starts_with("return(")).unwrap_or(false)
}

/// `default_class`: Script, LocalScript or ModuleScript; `None` detects it (Rojo suffixes, then a trailing `return`).
pub fn parse(file_name: &str, bytes: &[u8], default_class: Option<&str>) -> Result<ScriptFile, String> {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    if !is_script_path(base) {
        return Err(format!("Unsupported file '{base}'; expected .lua, .luau or .txt"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(format!("File '{base}' is too large ({} KB); the limit is {} KB", bytes.len() / 1024, MAX_BYTES / 1024));
    }
    if let Some(c) = default_class {
        if !matches!(c, "Script" | "LocalScript" | "ModuleScript") {
            return Err(format!("'{c}' is not a script class"));
        }
    }
    let source = decode_text(bytes).map_err(|e| format!("{base}: {e}"))?;
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let (stem, suffix_class) = if let Some(s) = stem.strip_suffix(".server") {
        (s, Some("Script"))
    } else if let Some(s) = stem.strip_suffix(".client") {
        (s, Some("LocalScript"))
    } else if let Some(s) = stem.strip_suffix(".module") {
        (s, Some("ModuleScript"))
    } else {
        (stem, None)
    };
    let class = suffix_class.or(match default_class {
        Some("LocalScript") => Some("LocalScript"),
        Some("ModuleScript") => Some("ModuleScript"),
        Some("Script") => Some("Script"),
        _ => None,
    });
    let class = class.unwrap_or(if ends_with_return(&source) { "ModuleScript" } else { "Script" });
    let name = if stem.trim().is_empty() { class } else { stem.trim() };
    Ok(ScriptFile { name: name.to_string(), class, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encodings_and_rejects_binary() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFa\r\nb\rc").unwrap(), "a\nb\nc");
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]).unwrap(), "hi");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0, b'h', 0, b'i']).unwrap(), "hi");
        assert!(decode_text(&[0xFF, 0xFE, b'h']).is_err());
        assert!(decode_text(b"ab\0cd").is_err());
        assert!(decode_text(&[0xC3, 0x28]).is_err());
        assert_eq!(decode_text("olá".as_bytes()).unwrap(), "olá");
    }

    #[test]
    fn picks_class_and_name() {
        let f = parse("C:\\x\\Main.server.lua", b"print(1)", None).unwrap();
        assert_eq!((f.name.as_str(), f.class), ("Main", "Script"));
        assert_eq!(parse("ui.client.luau", b"x", None).unwrap().class, "LocalScript");
        assert_eq!(parse("util.module.txt", b"x", None).unwrap().class, "ModuleScript");
        assert_eq!(parse("a.lua", b"local M = {}\nreturn M\n-- end\n", None).unwrap().class, "ModuleScript");
        assert_eq!(parse("a.lua", b"print(1)\n", None).unwrap().class, "Script");
        assert_eq!(parse("a.lua", b"return 1", Some("LocalScript")).unwrap().class, "LocalScript");
        assert_eq!(parse("/p/b.TXT", b"x", None).unwrap().name, "b");
        assert!(parse(".lua", b"x", None).is_err(), "a dotfile has no extension");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("a.png", b"x", None).is_err());
        assert!(parse("a", b"x", None).is_err());
        assert!(parse("a.lua", &vec![b'a'; MAX_BYTES + 1], None).is_err());
        assert!(parse("a.lua", b"x", Some("Part")).is_err());
        assert!(parse("a.lua", b"\0\0", None).unwrap_err().starts_with("a.lua:"));
        assert!(is_script_path("X.LUAU") && !is_script_path("x.rbxl"));
    }
}
