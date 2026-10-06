use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

use crate::errors::BridgeError;

const MAX_SOUND_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED_EXTENSIONS: &[(&str, &str)] = &[
    ("ogg", "audio/ogg"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("flac", "audio/flac"),
];

pub fn sound_mime_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    ALLOWED_EXTENSIONS
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSoundReadResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn failure(reason: &str) -> NotificationSoundReadResult {
    NotificationSoundReadResult {
        ok: false,
        data_base64: None,
        mime_type: None,
        path: None,
        reason: Some(reason.to_string()),
    }
}

pub fn load_sound(path: &Path) -> NotificationSoundReadResult {
    let Some(mime) = sound_mime_type(&path.to_string_lossy()) else {
        return failure("unsupported-type");
    };
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return failure("missing-path"),
        Err(_) => return failure("read-failed"),
    };
    if metadata.len() > MAX_SOUND_BYTES {
        return failure("too-large");
    }
    match std::fs::read(path) {
        Ok(bytes) => NotificationSoundReadResult {
            ok: true,
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            mime_type: Some(mime.to_string()),
            path: Some(path.to_string_lossy().into_owned()),
            reason: None,
        },
        Err(_) => failure("read-failed"),
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsReadSoundArgs {
    pub path: String,
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_read_sound(
    args: NotificationsReadSoundArgs,
) -> Result<NotificationSoundReadResult, BridgeError> {
    let path = PathBuf::from(&args.path);
    crate::commands::run_blocking(move || Ok(load_sound(&path))).await
}

/// 打开 macOS 通知系统设置（blocked-by-system 回退入口）。
#[tauri::command]
#[specta::specta]
pub async fn notifications_open_system_settings() -> Result<(), BridgeError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.notifications")
            .spawn()
            .map_err(|error| {
                BridgeError::message(format!("failed to open notification settings: {error}"))
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_type_supports_the_fork_extensions_only() {
        assert_eq!(sound_mime_type("a.wav"), Some("audio/wav"));
        assert_eq!(sound_mime_type("a.MP3"), Some("audio/mpeg"));
        assert_eq!(sound_mime_type("a.ogg"), Some("audio/ogg"));
        assert_eq!(sound_mime_type("a.flac"), Some("audio/flac"));
        assert_eq!(sound_mime_type("a.txt"), None);
        assert_eq!(sound_mime_type("noext"), None);
    }

    #[test]
    fn read_sound_reports_missing_and_unsupported_without_touching_fs() {
        assert_eq!(
            load_sound(Path::new("/definitely/missing.wav")).reason.as_deref(),
            Some("missing-path")
        );
        assert_eq!(
            load_sound(Path::new("/tmp/song.txt")).reason.as_deref(),
            Some("unsupported-type")
        );
    }

    #[test]
    fn read_sound_returns_base64_for_a_small_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ding.wav");
        std::fs::write(&path, b"RIFF").unwrap();
        let result = load_sound(&path);
        assert!(result.ok);
        assert_eq!(result.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(result.data_base64.as_deref(), Some("UklGRg=="));
    }
}
