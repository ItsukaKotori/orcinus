//! `orcinus status` (spec §3.6): never launches the app. Missing metadata
//! synthesizes `{app:{running:false},runtime:{state:"not_running",reachable:
//! false}}`; readable-but-unreachable metadata is `stale_bootstrap`. `--json`
//! always exits 0; human mode exits 1 when `!reachable` (same as the oracle).

use serde_json::{json, Value};

use crate::client::{metadata_path, read_metadata, MetadataRead, RuntimeClient, STATUS_TIMEOUT};
use crate::errors::CliError;

/// One status snapshot in the CLI result shape (spec §3.6/§3.5).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    pub app_running: bool,
    pub pid: Option<u32>,
    pub desktop_window_status: Option<String>,
    pub runtime_state: String,
    pub reachable: bool,
    pub runtime_id: String,
    pub app_version: Option<String>,
    pub graph_state: String,
}

impl StatusSnapshot {
    /// `not_running` (no metadata) or `stale_bootstrap` (metadata present but
    /// unreadable/incomplete or unreachable), both unreachable.
    pub fn unreachable(app_running: bool, state: &str) -> Self {
        Self {
            app_running,
            pid: None,
            desktop_window_status: None,
            runtime_state: state.to_string(),
            reachable: false,
            runtime_id: String::new(),
            app_version: None,
            graph_state: "not_running".to_string(),
        }
    }

    /// `status.get` projection for a reachable runtime.
    pub fn from_status_get(result: &Value, fallback_runtime_id: &str, pid: Option<u32>) -> Self {
        let app = &result["app"];
        let runtime = &result["runtime"];
        Self {
            app_running: app["running"].as_bool().unwrap_or(false),
            pid: app["pid"].as_u64().map(|pid| pid as u32).or(pid),
            desktop_window_status: app["desktopWindowStatus"].as_str().map(str::to_string),
            runtime_state: runtime["state"].as_str().unwrap_or("ready").to_string(),
            reachable: true,
            runtime_id: runtime["runtimeId"]
                .as_str()
                .unwrap_or(fallback_runtime_id)
                .to_string(),
            app_version: runtime["appVersion"].as_str().map(str::to_string),
            graph_state: result["graph"]["state"]
                .as_str()
                .unwrap_or("not_ready")
                .to_string(),
        }
    }

    /// `--json` result payload.
    pub fn to_result(&self) -> Value {
        json!({
            "app": {
                "running": self.app_running,
                "pid": self.pid,
                "desktopWindowStatus": self.desktop_window_status,
            },
            "runtime": {
                "state": self.runtime_state,
                "reachable": self.reachable,
                "runtimeId": if self.runtime_id.is_empty() { Value::Null } else { Value::String(self.runtime_id.clone()) },
                "appVersion": self.app_version,
            },
            "graph": {"state": self.graph_state},
        })
    }

    /// Human-readable form, mirroring the oracle `formatCliStatus` lines.
    pub fn to_human(&self) -> String {
        [
            format!("appRunning: {}", self.app_running),
            format!(
                "pid: {}",
                self.pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            format!(
                "desktopWindowStatus: {}",
                self.desktop_window_status.as_deref().unwrap_or("unknown")
            ),
            format!("runtimeState: {}", self.runtime_state),
            format!("runtimeReachable: {}", self.reachable),
            format!(
                "runtimeId: {}",
                if self.runtime_id.is_empty() {
                    "none".to_string()
                } else {
                    self.runtime_id.clone()
                }
            ),
            format!("graphState: {}", self.graph_state),
        ]
        .join("\n")
    }
}

/// Why a probe did not reach a live runtime.
pub enum ProbeFailure {
    /// No metadata file: the app has never started.
    NotRunning,
    /// Metadata present but the runtime is down / stale (or metadata itself is
    /// unreadable): callers synthesize a snapshot.
    Unreachable,
    /// The runtime answered but rejected the auth token; commands that require
    /// the runtime surface this instead of pretending the app is absent.
    AuthFailed(CliError),
}

/// Probe the runtime once. `Ok` is a reachable snapshot; `Err` distinguishes
/// stale/unreadable/missing metadata from an authentication failure.
pub fn probe_status() -> Result<(StatusSnapshot, String), ProbeFailure> {
    let path = metadata_path();
    let metadata = match read_metadata(&path) {
        Ok(MetadataRead::Ready(metadata)) => metadata,
        Ok(MetadataRead::Missing) => return Err(ProbeFailure::NotRunning),
        Ok(MetadataRead::Unreadable(_)) => return Err(ProbeFailure::Unreachable),
        Err(_) => return Err(ProbeFailure::Unreachable),
    };
    let runtime_id = metadata.runtime_id.clone();
    let client = RuntimeClient::connect_at(&path).map_err(|error| {
        if error.code == "unauthorized" {
            ProbeFailure::AuthFailed(error)
        } else {
            ProbeFailure::Unreachable
        }
    })?;
    match client.call_with_timeout("status.get", json!({}), STATUS_TIMEOUT) {
        Ok(result) => Ok((
            StatusSnapshot::from_status_get(&result, &runtime_id, Some(metadata.pid)),
            runtime_id,
        )),
        Err(error) => {
            if error.code == "unauthorized" {
                Err(ProbeFailure::AuthFailed(error))
            } else {
                Err(ProbeFailure::Unreachable)
            }
        }
    }
}

/// `status` semantics (spec §3.6): never fails hard. Missing metadata
/// synthesizes `not_running`; unreadable metadata and unreachable runtimes
/// synthesize `stale_bootstrap`; the returned runtime id is whatever metadata
/// could be read.
pub fn read_status() -> (StatusSnapshot, String) {
    let path = metadata_path();
    let runtime_id = match read_metadata(&path) {
        Ok(MetadataRead::Ready(metadata)) => metadata.runtime_id.clone(),
        _ => String::new(),
    };
    match probe_status() {
        Ok((snapshot, id)) => (snapshot, id),
        Err(ProbeFailure::NotRunning) => (
            StatusSnapshot::unreachable(false, "not_running"),
            String::new(),
        ),
        Err(_) => (
            StatusSnapshot::unreachable(false, "stale_bootstrap"),
            runtime_id,
        ),
    }
}

/// True when a `status.get` probe reaches a live runtime (used by `open`).
pub fn probe_reachable() -> bool {
    probe_status().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_snapshot_json_matches_the_synthetic_shape() {
        let snapshot = StatusSnapshot::unreachable(false, "not_running");
        assert_eq!(
            snapshot.to_result(),
            json!({
                "app": {"running": false, "pid": null, "desktopWindowStatus": null},
                "runtime": {
                    "state": "not_running",
                    "reachable": false,
                    "runtimeId": null,
                    "appVersion": null,
                },
                "graph": {"state": "not_running"},
            })
        );
    }

    #[test]
    fn status_get_projection_reads_app_runtime_and_graph() {
        let result = json!({
            "app": {"running": true, "pid": 99, "desktopWindowStatus": "available"},
            "runtime": {"state": "ready", "reachable": true, "runtimeId": "RID", "appVersion": "1.2.3"},
            "graph": {"state": "not_ready"},
        });
        let snapshot = StatusSnapshot::from_status_get(&result, "fallback", None);
        assert_eq!(snapshot.pid, Some(99));
        assert_eq!(snapshot.runtime_id, "RID");
        assert_eq!(snapshot.app_version.as_deref(), Some("1.2.3"));
        assert_eq!(snapshot.runtime_state, "ready");
        assert!(snapshot.reachable);
    }

    #[test]
    fn human_lines_match_the_oracle_labels() {
        let mut snapshot = StatusSnapshot::unreachable(false, "not_running");
        snapshot.desktop_window_status = Some("available".to_string());
        let human = snapshot.to_human();
        assert!(human.contains("appRunning: false"));
        assert!(human.contains("pid: none"));
        assert!(human.contains("desktopWindowStatus: available"));
        assert!(human.contains("runtimeState: not_running"));
        assert!(human.contains("runtimeReachable: false"));
        assert!(human.contains("runtimeId: none"));
        assert!(human.contains("graphState: not_running"));
    }
}
