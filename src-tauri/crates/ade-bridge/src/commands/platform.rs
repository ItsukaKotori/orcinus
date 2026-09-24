use serde::{Deserialize, Serialize};

use crate::errors::BridgeError;

/// Platform contract (`src/shared/preload-api/api/app-api.ts` `PlatformApi.get`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub platform: String,
    pub os_release: String,
    pub arch: String,
    pub shell: String,
    pub display_server: Option<String>,
}

/// Map Rust's `std::env::consts::OS` to the Node `process.platform` vocabulary
/// the renderer contract uses.
pub fn map_platform(os: &str) -> &str {
    match os {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Linux display server detection; `None` off Linux and when neither socket
/// variable is set.
pub fn detect_display_server(
    os: &str,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> Option<String> {
    if os != "linux" {
        return None;
    }
    if wayland_display.is_some_and(|value| !value.is_empty()) {
        return Some("wayland".to_string());
    }
    if display.is_some_and(|value| !value.is_empty()) {
        return Some("x11".to_string());
    }
    None
}

fn uname_release() -> String {
    std::process::Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn login_shell() -> String {
    std::env::var("SHELL")
        .or_else(|_| std::env::var("COMSPEC"))
        .unwrap_or_default()
}

pub fn platform_info() -> PlatformInfo {
    let os = std::env::consts::OS;
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    PlatformInfo {
        platform: map_platform(os).to_string(),
        os_release: uname_release(),
        arch: std::env::consts::ARCH.to_string(),
        shell: login_shell(),
        display_server: detect_display_server(os, wayland_display.as_deref(), display.as_deref()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn platform_get() -> Result<PlatformInfo, BridgeError> {
    Ok(platform_info())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rust_os_to_node_platform() {
        assert_eq!(map_platform("macos"), "darwin");
        assert_eq!(map_platform("windows"), "win32");
        assert_eq!(map_platform("linux"), "linux");
    }

    #[test]
    fn display_server_only_reported_on_linux() {
        assert_eq!(
            detect_display_server("macos", Some("wayland-0"), Some(":0")),
            None
        );
        assert_eq!(detect_display_server("windows", None, None), None);
        assert_eq!(
            detect_display_server("linux", Some("wayland-0"), Some(":0")),
            Some("wayland".to_string())
        );
        assert_eq!(
            detect_display_server("linux", None, Some(":0")),
            Some("x11".to_string())
        );
        assert_eq!(detect_display_server("linux", None, None), None);
        assert_eq!(detect_display_server("linux", Some(""), Some("")), None);
    }

    #[test]
    fn platform_info_serializes_camel_case() {
        let info = PlatformInfo {
            platform: "darwin".to_string(),
            os_release: "Darwin 25.0.0".to_string(),
            arch: "aarch64".to_string(),
            shell: "/bin/zsh".to_string(),
            display_server: None,
        };
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["platform"], "darwin");
        assert_eq!(value["osRelease"], "Darwin 25.0.0");
        assert_eq!(value["arch"], "aarch64");
        assert_eq!(value["shell"], "/bin/zsh");
        assert!(value["displayServer"].is_null());
    }

    #[test]
    fn platform_get_matches_host_constants() {
        let info = platform_get().unwrap();
        assert_eq!(info.platform, map_platform(std::env::consts::OS));
        assert_eq!(info.arch, std::env::consts::ARCH);
    }
}
