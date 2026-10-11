//! Plugin system core: manifest parsing/validation, the capability model,
//! deterministic content hashing, dual-scope paths, declared-artifact
//! validation, and discovery. Pure Rust; no Tauri or `/` store dependencies.

pub mod artifact;
pub mod content_hash;
pub mod discovery;
pub mod manifest;
pub mod paths;
pub mod types;

pub use artifact::{resolve_declared_artifact, validate_declared_artifacts, ArtifactSet};
pub use content_hash::hash_plugin_tree;
pub use discovery::{discover, DiscoveredPlugin, DiscoveryProjection, PluginSourceKind};
pub use manifest::{engine_satisfies, parse_manifest, Manifest};
pub use paths::{global_roots, project_roots, validate_containment, ScopeRoots};
pub use types::{Capability, PluginError, PluginScope};
