//! `orcinus status` (spec §3.6): never launches the app. Reports the local
//! runtime metadata file state. The runtime RPC was removed with the
//! computer-use trim — nothing serves `runtime.sock` anymore — so `status` no
//! longer observes a live runtime: `reachable` is always false.

use serde_json::{json, Value};

use crate::client::{metadata_path, read_metadata, MetadataRead};

/// Local runtime-metadata snapshot. `not_running` = no metadata file;
/// `stale_bootstrap` = metadata present but no observable runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    pub runtime_state: &'static str,
    pub runtime_id: String,
}

impl StatusSnapshot {
    /// `--json` result payload.
    pub fn to_result(&self) -> Value {
        json!({
            "runtime": {
                "state": self.runtime_state,
                "reachable": false,
                "runtimeId": if self.runtime_id.is_empty() { Value::Null } else { Value::String(self.runtime_id.clone()) },
            },
        })
    }

    /// Human-readable form.
    pub fn to_human(&self) -> String {
        [
            format!("runtimeState: {}", self.runtime_state),
            "runtimeReachable: false".to_string(),
            format!(
                "runtimeId: {}",
                if self.runtime_id.is_empty() {
                    "none".to_string()
                } else {
                    self.runtime_id.clone()
                }
            ),
        ]
        .join("\n")
    }
}

/// File-derived state: missing metadata is `not_running`; a present file that
/// cannot be observed is `stale_bootstrap`. The metadata id is reported when
/// the file parses.
pub fn read_status() -> StatusSnapshot {
    match read_metadata(&metadata_path()) {
        MetadataRead::Missing => StatusSnapshot {
            runtime_state: "not_running",
            runtime_id: String::new(),
        },
        MetadataRead::Unreadable => StatusSnapshot {
            runtime_state: "stale_bootstrap",
            runtime_id: String::new(),
        },
        MetadataRead::Ready(metadata) => StatusSnapshot {
            runtime_state: "stale_bootstrap",
            runtime_id: metadata.runtime_id,
        },
    }
}
