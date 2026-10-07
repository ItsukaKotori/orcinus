//! Re-export of the shared JSON wrapper.
//!
//! The wrapper (and its recursive named specta definition) lives in
//! `ade_core::json` so sized payload types there can project JSON fields as
//! `Json` too; the bridge command surface keeps referring to
//! `crate::json::Json` unchanged.
pub use ade_core::json::Json;
