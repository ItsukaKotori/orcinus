use serde::{Deserialize, Serialize};

use crate::errors::BridgeError;

/// PTY 数据面端点（规格 §4.7）：WS 环回服务的端口与一次性下发 token。
/// Task 3 期间由 orcinus-app setup 的 echo server 填充；Task 9 起
/// `PtyHost::start` 接管（端口与 token 在进程生命周期内不变）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DataEndpointPayload {
    pub port: u16,
    pub token: String,
}

/// 下发 WS 数据面端点（`src/bridge/real/pty-socket.ts` 的
/// `fetchPtyDataEndpoint` 消费；服务未起时报错，渲染层缓存成功结果）。
#[tauri::command]
#[specta::specta]
pub fn pty_data_endpoint(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<DataEndpointPayload, BridgeError> {
    state
        .pty_data_endpoint()
        .map(|(port, token)| DataEndpointPayload { port, token })
        .ok_or_else(|| BridgeError::message("pty data server is not running"))
}

#[cfg(test)]
mod tests {
    use super::DataEndpointPayload;

    #[test]
    fn payload_serializes_camel_case() {
        let payload = DataEndpointPayload {
            port: 51234,
            token: "abcd".to_string(),
        };
        let value: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "port": 51234, "token": "abcd" })
        );
    }
}
