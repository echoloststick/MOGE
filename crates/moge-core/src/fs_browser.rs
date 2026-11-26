//! Folder listing for the in-app file picker (Android has no native picker without Java glue).
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// Lists `dir`: folders first, then files whose extension is in `exts` (case-insensitive; empty = all).
/// Hidden entries are skipped.
pub fn list_dir(dir: &Path, exts: &[&str]) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("Cannot open folder '{}': {e}", dir.display()))?;
    let mut out: Vec<Entry> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let is_dir = md.is_dir();
        if !is_dir {
            let ext = Path::new(&name).extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if !exts.is_empty() && !exts.iter().any(|x| x.eq_ignore_ascii_case(&ext)) {
                continue;
            }
        }
        out.push(Entry { name, path: e.path(), is_dir, size: md.len() });
    }
    out.sort_by(|a, b| (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase())));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_sorts_and_hides() {
        let d = std::env::temp_dir().join(format!("moge-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Zeta")).unwrap();
        std::fs::create_dir_all(d.join("alpha")).unwrap();
        for f in ["b.lua", "A.LUAU", "c.png", ".hidden.lua", "n.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let names: Vec<String> = list_dir(&d, &["lua", "luau", "txt"]).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["alpha", "Zeta", "A.LUAU", "b.lua", "n.txt"]);
        assert_eq!(list_dir(&d, &[]).unwrap().len(), 6);
        assert!(list_dir(&d.join("missing"), &[]).unwrap_err().starts_with("Cannot open folder"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
