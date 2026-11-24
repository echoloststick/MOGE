//! Roblox file importers: rbxlx (XML), rbxl/rbxm (binary).
mod binary;
mod error;
mod lz4;
mod rbxlx;
mod xml;

pub use error::FormatError;
use moge_core::{Id, Tree};

/// Result of an import. `roots` are detached top-level instances (a DataModel for binary places,
/// services for rbxlx, models for rbxm); attach them with `Tree::merge_place`.
#[derive(Debug, Default)]
pub struct Import {
    pub roots: Vec<Id>,
    pub warnings: Vec<String>,
}

pub fn load_binary(data: &[u8], tree: &mut Tree) -> Result<Import, FormatError> {
    binary::load(data, tree)
}

pub fn load_xml(text: &str, tree: &mut Tree) -> Result<Import, FormatError> {
    rbxlx::load(text, tree)
}

/// Detects the container (binary magic or XML) and imports it.
pub fn load(data: &[u8], tree: &mut Tree) -> Result<Import, FormatError> {
    if data.starts_with(binary::MAGIC_PREFIX) {
        return load_binary(data, tree);
    }
    let text = std::str::from_utf8(data).map_err(|_| FormatError::Malformed("File is neither a Roblox binary nor valid UTF-8 XML".into()))?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with('<') {
        load_xml(text, tree)
    } else {
        Err(FormatError::BadMagic)
    }
}
