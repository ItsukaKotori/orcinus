//! `ade-computer` — computer-use provider core (spec §3.2): protocol types,
//! helper path resolution, and availability. Pure Rust: no tauri/specta/store
//! dependencies so the CLI and tests can use it directly.

pub mod availability;
pub mod client;
pub mod paths;
pub mod permissions;
pub mod protocol;
pub mod provider;
pub mod socket;
pub mod transport;
pub mod validation;
pub mod verification;
