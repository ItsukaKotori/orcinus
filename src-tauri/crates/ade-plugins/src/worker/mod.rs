//! Plugin worker: the out-of-process Node host. This module owns the wire
//! protocol spoken between the Rust host and the `host-entry.mjs` child
//! (spawn/supervision arrive in a later task).

pub mod protocol;
pub mod spawn;
pub mod supervisor;

pub use protocol::{
    read_frame, write_frame, ChildFrame, LogLevel, ParentFrame, MAX_FRAME_BYTES, TEXT_MAX_BYTES,
};
