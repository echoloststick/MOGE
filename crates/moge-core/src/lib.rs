//! MOGE core: value types, API-Dump-driven class metadata, instance tree and asset references.
pub mod api;
pub mod assets;
pub mod json;
pub mod tree;
pub mod value;

pub use api::{ApiDump, ClassInfo, PropInfo};
pub use tree::{Node, Tree, TreeError};
pub use value::{Id, Value};
