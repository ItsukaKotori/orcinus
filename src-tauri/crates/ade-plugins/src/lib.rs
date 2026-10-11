//! Plugin system core: manifest parsing/validation, the capability model,
//! and deterministic content hashing. Pure Rust; no Tauri or `/` store
//! dependencies.

pub mod content_hash;
pub mod manifest;
pub mod types;

pub use content_hash::hash_plugin_tree;
pub use manifest::{engine_satisfies, parse_manifest, Manifest};
pub use types::{Capability, PluginError, PluginScope};
