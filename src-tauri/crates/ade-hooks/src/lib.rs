//! Agent hook receiver (spec §2.1/§3): HTTP ingest, attribution, endpoint
//! publication, managed claude settings/script, and the last-status cache.
//! 归一化不在此层——宿主只搬运原始 hook JSON（规格 §3.2）。

pub mod cache;
pub mod endpoint;
pub mod script;

pub use cache::CachedHookEvent;
