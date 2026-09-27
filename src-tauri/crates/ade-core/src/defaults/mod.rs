use serde_json::Value;

const GENERATED: &str = include_str!("ade-defaults.generated.json");

fn substitute_home(value: &mut Value, home: &str) {
    match value {
        Value::String(s) => {
            if s.contains("{{HOME}}") {
                *s = s.replace("{{HOME}}", home);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute_home(item, home)),
        Value::Object(map) => map.values_mut().for_each(|item| substitute_home(item, home)),
        _ => {}
    }
}

/// The stock workspace directory (`getDefaultWorkspaceDir`): `{{HOME}}/orca/workspaces`
/// with the host separator, trailing separators on `home` trimmed.
pub fn default_workspace_dir(home: &str) -> String {
    let separator = if home.contains('\\') { '\\' } else { '/' };
    let trimmed = home.trim_end_matches(['\\', '/']);
    format!("{trimmed}{separator}orca{separator}workspaces")
}

fn apply_platform_defaults(settings: &mut Value, home: &str, platform: &str) {
    let Some(map) = settings.as_object_mut() else {
        return;
    };
    let is_windows = platform == "windows";
    let is_linux = platform == "linux";
    let is_macos = platform == "macos";
    let primary_selection = is_linux || is_macos;
    let font_family = if is_windows {
        "Cascadia Mono"
    } else if is_linux {
        "DejaVu Sans Mono"
    } else {
        "SF Mono"
    };
    map.insert("primarySelectionMiddleClickPaste".into(), Value::Bool(primary_selection));
    map.insert(
        "primarySelectionMiddleClickPasteDefaultedForLinux".into(),
        Value::Bool(is_linux),
    );
    map.insert(
        "primarySelectionMiddleClickPasteDefaultedForTerminalDefaults".into(),
        Value::Bool(primary_selection),
    );
    map.insert("terminalFontFamily".into(), Value::String(font_family.into()));
    map.insert("terminalRightClickToPaste".into(), Value::Bool(is_windows));
    map.insert(
        "workspaceDir".into(),
        Value::String(default_workspace_dir(home)),
    );
}

pub fn settings_defaults(home: &str) -> Value {
    let mut payload: Value = serde_json::from_str(GENERATED).expect("generated defaults are valid JSON");
    let mut settings = payload["settings"].take();
    substitute_home(&mut settings, home);
    apply_platform_defaults(&mut settings, home, std::env::consts::OS);
    settings
}

pub fn ui_state_defaults() -> Value {
    let mut payload: Value = serde_json::from_str(GENERATED).expect("generated defaults are valid JSON");
    payload["uiState"].take()
}

pub fn onboarding_defaults() -> Value {
    let mut payload: Value = serde_json::from_str(GENERATED).expect("generated defaults are valid JSON");
    payload["onboarding"].take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_defaults_returns_generated_defaults() {
        let defaults = onboarding_defaults();
        assert_eq!(defaults["flowVersion"], 4);
        assert_eq!(defaults["lastCompletedStep"], -1);
        assert_eq!(defaults["closedAt"], Value::Null);
        assert_eq!(defaults["outcome"], Value::Null);
        assert_eq!(defaults["checklist"]["addedRepo"], false);
        assert_eq!(defaults["checklist"]["dismissed"], false);
    }

    #[test]
    fn substitutes_home_placeholder() {
        let defaults = settings_defaults("/Users/tester");
        let text = defaults.to_string();
        assert!(!text.contains("{{HOME}}"));
    }

    #[test]
    fn substitute_home_replaces_every_occurrence() {
        let mut value = serde_json::json!({
            "a": "{{HOME}}/x",
            "b": ["{{HOME}}"],
            "c": { "d": "{{HOME}}" }
        });
        substitute_home(&mut value, "/Users/tester");
        assert_eq!(
            value,
            serde_json::json!({
                "a": "/Users/tester/x",
                "b": ["/Users/tester"],
                "c": { "d": "/Users/tester" }
            })
        );
    }

    #[test]
    fn settings_defaults_follow_host_platform_and_home() {
        let home = "/Users/tester/";
        let settings = settings_defaults(home);
        let expected_font = if cfg!(windows) {
            "Cascadia Mono"
        } else if cfg!(target_os = "linux") {
            "DejaVu Sans Mono"
        } else {
            "SF Mono"
        };
        assert_eq!(settings["terminalFontFamily"], expected_font);
        assert_eq!(settings["workspaceDir"], "/Users/tester/orca/workspaces");
        assert!(settings["workspaceDir"].as_str().unwrap().starts_with("/Users/tester"));
    }

    fn platform_settings(home: &str, platform: &str) -> Value {
        let mut settings = settings_defaults(home);
        apply_platform_defaults(&mut settings, home, platform);
        settings
    }

    #[test]
    fn macos_platform_defaults_match_typescript() {
        let settings = platform_settings("/Users/alice", "macos");
        assert_eq!(settings["primarySelectionMiddleClickPaste"], true);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForLinux"], false);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForTerminalDefaults"], true);
        assert_eq!(settings["terminalFontFamily"], "SF Mono");
        assert_eq!(settings["terminalRightClickToPaste"], false);
        assert_eq!(settings["workspaceDir"], "/Users/alice/orca/workspaces");
    }

    #[test]
    fn linux_platform_defaults_match_typescript() {
        let settings = platform_settings("/home/alice", "linux");
        assert_eq!(settings["primarySelectionMiddleClickPaste"], true);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForLinux"], true);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForTerminalDefaults"], true);
        assert_eq!(settings["terminalFontFamily"], "DejaVu Sans Mono");
        assert_eq!(settings["terminalRightClickToPaste"], false);
        assert_eq!(settings["workspaceDir"], "/home/alice/orca/workspaces");
    }

    #[test]
    fn windows_platform_defaults_match_typescript() {
        let settings = platform_settings("C:\\Users\\alice", "windows");
        assert_eq!(settings["primarySelectionMiddleClickPaste"], false);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForLinux"], false);
        assert_eq!(settings["primarySelectionMiddleClickPasteDefaultedForTerminalDefaults"], false);
        assert_eq!(settings["terminalFontFamily"], "Cascadia Mono");
        assert_eq!(settings["terminalRightClickToPaste"], true);
        assert_eq!(settings["workspaceDir"], "C:\\Users\\alice\\orca\\workspaces");
    }

    #[test]
    fn default_workspace_dir_matches_generated_default() {
        let home = "/Users/tester";
        let settings = settings_defaults(home);
        assert_eq!(settings["workspaceDir"], default_workspace_dir(home));
        assert_eq!(default_workspace_dir("/Users/tester/"), default_workspace_dir(home));
        assert_eq!(
            default_workspace_dir("C:\\Users\\tester\\"),
            "C:\\Users\\tester\\orca\\workspaces"
        );
    }

    #[test]
    fn workspace_dir_trims_trailing_separators_like_typescript() {
        assert_eq!(platform_settings("/Users/alice/", "macos")["workspaceDir"], "/Users/alice/orca/workspaces");
        assert_eq!(platform_settings("C:\\Users\\alice\\\\", "windows")["workspaceDir"], "C:\\Users\\alice\\orca\\workspaces");
    }
}
