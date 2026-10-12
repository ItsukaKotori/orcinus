//! Host-side plugin invocation surface.
//!
//! The [`gate::decide`] capability gate, the [`api`] method-spec table with
//! parameter/result validation, the [`methods::execute`] chokepoint every
//! plugin-originated call runs through, and the [`services::PluginHostServices`]
//! seam the Tauri bridge implements. Pure Rust; no Tauri dependencies.

pub mod api;
pub mod gate;
pub mod methods;
pub mod services;
