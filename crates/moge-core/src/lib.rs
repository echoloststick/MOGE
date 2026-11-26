//! MOGE core: value types, API-Dump-driven class metadata, instance tree and asset references.
pub mod api;
pub mod assets;
pub mod b64;
pub mod color;
pub mod editor;
pub mod fs_browser;
pub mod script_file;
pub mod json;
pub mod project;
pub mod template;
pub mod tree;
pub mod value;

pub use api::{ApiDump, ClassInfo, MemberInfo, MemberKind, PropInfo};
pub use tree::{Node, Tree, TreeError};
pub use value::{Id, Value};
