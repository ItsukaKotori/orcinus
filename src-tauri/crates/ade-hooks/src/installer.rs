use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::script;

pub const MANAGED_HOOK_TIMEOUT_SECONDS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallState {
    Installed,
    Skipped(HookInstallSkipReason),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallSkipReason {
    HooksDisabled,
    CliNotFound,
}

pub fn claude_events() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("SessionStart", None),
        ("UserPromptSubmit", None),
        ("Stop", None),
        ("StopFailure", None),
        ("SubagentStart", None),
        ("SubagentStop", None),
        ("TeammateIdle", None),
        ("PreToolUse", Some("*")),
        ("PostToolUse", Some("*")),
        ("PostToolUseFailure", Some("*")),
        ("PermissionRequest", Some("*")),
        ("PostCompact", None),
    ]
}

pub fn claude_settings_path(home: &str) -> PathBuf {
    Path::new(home).join(".claude").join("settings.json")
}

/// 托管条目的调用串：脚本存在 → `/bin/sh` 执行；缺失/不可读 → 排空 stdin
/// 后回 `{}`（PermissionRequest fail-closed 防线，规格 §3.3/§3.4）。
pub fn managed_command() -> String {
    "if [ -f \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ] && [ -r \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ] && [ -x \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ]; then /bin/sh \"${HOME-}/.ade/agent-hooks/claude-hook.sh\"; else { command -p cat 2>/dev/null || cat; } >/dev/null 2>&1 || :; printf '{}\\n'; fi".to_string()
}

pub fn managed_command_matcher(command: &str) -> bool {
    command.replace('\\', "/").contains("agent-hooks/claude-hook.sh")
}

fn managed_hook_definition(matcher: Option<&str>) -> Value {
    let hook = json!({
        "type": "command",
        "command": managed_command(),
        "timeout": MANAGED_HOOK_TIMEOUT_SECONDS,
    });
    match matcher {
        Some(matcher) => json!({ "matcher": matcher, "hooks": [hook] }),
        None => json!({ "hooks": [hook] }),
    }
}

fn definition_is_managed(definition: &Value) -> bool {
    if let Some(command) = definition.get("command").and_then(Value::as_str) {
        if managed_command_matcher(command) {
            return true;
        }
    }
    definition["hooks"]
        .as_array()
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook["command"]
                    .as_str()
                    .map(managed_command_matcher)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn apply_managed_hooks(config: &Value) -> Value {
    let mut next = config.clone();
    let root = next.as_object_mut().expect("settings object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("hooks object");
    for (event, matcher) in claude_events() {
        let existing = hooks
            .get(event)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cleaned: Vec<Value> = existing
            .into_iter()
            .filter(|definition| !definition_is_managed(definition))
            .collect();
        cleaned.push(managed_hook_definition(matcher));
        hooks.insert(event.to_string(), Value::Array(cleaned));
    }
    next
}

pub fn remove_managed_hooks(config: &Value) -> (Value, bool) {
    let mut next = config.clone();
    let mut changed = false;
    let Some(root) = next.as_object_mut() else {
        return (next, false);
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return (next, false);
    };
    let event_names: Vec<String> = hooks.keys().cloned().collect();
    for event in event_names {
        let Some(definitions) = hooks.get(&event).and_then(Value::as_array).cloned() else {
            continue;
        };
        let before = definitions.len();
        let cleaned: Vec<Value> = definitions
            .into_iter()
            .filter(|definition| !definition_is_managed(definition))
            .collect();
        if cleaned.len() != before {
            changed = true;
        }
        if cleaned.is_empty() {
            hooks.remove(&event);
        } else {
            hooks.insert(event, Value::Array(cleaned));
        }
    }
    (next, changed)
}

fn read_settings_json(path: &Path) -> io::Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            let value: Value = serde_json::from_str(&raw)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if value.is_object() {
                Ok(value)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "claude settings must be a JSON object",
                ))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error),
    }
}

/// 写入路径解引用 symlink（dotfiles 管理器断链防线，规格 §3.3）。
fn resolve_write_path(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            std::fs::canonicalize(path).map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))
        }
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(error),
    }
}

fn write_settings_json(path: &Path, value: &Value) -> io::Result<()> {
    let write_path = resolve_write_path(path)?;
    if let Some(parent) = write_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut serialized = serde_json::to_string_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    serialized.push('\n');
    if std::fs::read_to_string(&write_path)
        .map(|existing| existing == serialized)
        .unwrap_or(false)
    {
        return Ok(());
    }
    let tmp = write_path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, serialized.as_bytes())?;
    if write_path.exists() {
        let backup = PathBuf::from(format!("{}.bak", write_path.to_string_lossy()));
        let _ = std::fs::copy(&write_path, &backup);
    }
    std::fs::rename(&tmp, &write_path)?;
    Ok(())
}

pub fn install_claude_hooks(home: &str, enabled: bool, cli_present: bool) -> HookInstallState {
    if !enabled {
        return HookInstallState::Skipped(HookInstallSkipReason::HooksDisabled);
    }
    if !cli_present {
        return HookInstallState::Skipped(HookInstallSkipReason::CliNotFound);
    }
    if let Err(error) = script::write_managed_script(home) {
        return HookInstallState::Error(format!("failed to write managed hook script: {error}"));
    }
    let path = claude_settings_path(home);
    let config = match read_settings_json(&path) {
        Ok(config) => config,
        Err(error) => return HookInstallState::Error(format!("failed to read claude settings: {error}")),
    };
    let next = apply_managed_hooks(&config);
    match write_settings_json(&path, &next) {
        Ok(()) => HookInstallState::Installed,
        Err(error) => HookInstallState::Error(format!("failed to write claude settings: {error}")),
    }
}

/// 显式关闭开关 = 移除托管条目（脚本保留）；启动期关闭 = 从不调用本函数
/// （规格 §3.3 与 §4 裁定：启动 skip 不删防多 profile 互删，显式 toggle 才删）。
pub fn remove_claude_hooks(home: &str) -> HookInstallState {
    let path = claude_settings_path(home);
    if !path.exists() {
        return HookInstallState::Installed;
    }
    let config = match read_settings_json(&path) {
        Ok(config) => config,
        Err(error) => return HookInstallState::Error(format!("failed to read claude settings: {error}")),
    };
    let (next, _) = remove_managed_hooks(&config);
    match write_settings_json(&path, &next) {
        Ok(()) => HookInstallState::Installed,
        Err(error) => HookInstallState::Error(format!("failed to write claude settings: {error}")),
    }
}

pub fn is_claude_cli_available(home: &str) -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for extra in [".local/bin", ".claude/local", ".bun/bin", ".npm-global/bin"] {
        dirs.push(Path::new(home).join(extra));
    }
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    dirs.into_iter().any(|dir| is_executable_file(&dir.join("claude")))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed_definitions(value: &Value, event: &str) -> usize {
        value["hooks"][event]
            .as_array()
            .map(|definitions| {
                definitions
                    .iter()
                    .filter(|definition| {
                        definition["hooks"]
                            .as_array()
                            .map(|hooks| {
                                hooks.iter().any(|hook| {
                                    hook["command"]
                                        .as_str()
                                        .map(managed_command_matcher)
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn managed_command_survives_missing_script_with_neutral_json() {
        let command = managed_command();
        assert!(command.contains("agent-hooks/claude-hook.sh"));
        assert!(command.contains("/bin/sh"));
        assert!(command.contains("printf '{}\\n'"));
    }

    #[test]
    fn apply_installs_all_twelve_events_and_preserves_user_entries() {
        let config = json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "/usr/local/bin/user-stop" } ] } ],
                "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "echo hi" } ] } ]
            },
            "permissions": { "allow": ["Bash(ls:*)"] }
        });
        let next = apply_managed_hooks(&config);
        for (event, _) in claude_events() {
            assert_eq!(managed_definitions(&next, event), 1, "{event}");
        }
        assert!(next["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|definition| definition["hooks"][0]["command"] == "/usr/local/bin/user-stop"));
        assert_eq!(next["permissions"], config["permissions"]);
        // 幂等：重复 apply 不叠加托管条目。
        assert_eq!(managed_definitions(&apply_managed_hooks(&next), "Stop"), 1);
    }

    #[test]
    fn apply_uses_star_matcher_only_on_tool_and_permission_events() {
        let next = apply_managed_hooks(&json!({}));
        for (event, matcher) in claude_events() {
            let managed = next["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .find(|definition| {
                    definition["hooks"][0]["command"]
                        .as_str()
                        .map(managed_command_matcher)
                        .unwrap_or(false)
                })
                .unwrap();
            assert_eq!(managed.get("matcher").and_then(Value::as_str), matcher, "{event}");
            assert_eq!(managed["hooks"][0]["timeout"], 10);
            assert_eq!(managed["hooks"][0]["type"], "command");
        }
    }

    #[test]
    fn remove_strips_managed_entries_but_keeps_user_entries_and_other_keys() {
        let config = apply_managed_hooks(&json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "/usr/local/bin/user-stop" } ] } ]
            },
            "theme": "dark"
        }));
        let (next, changed) = remove_managed_hooks(&config);
        assert!(changed);
        for (event, _) in claude_events() {
            assert_eq!(managed_definitions(&next, event), 0, "{event}");
        }
        assert!(next["hooks"]["Stop"].as_array().unwrap().iter().any(|definition| {
            definition["hooks"][0]["command"] == "/usr/local/bin/user-stop"
        }));
        assert_eq!(next["theme"], "dark");
        let (_, changed_again) = remove_managed_hooks(&next);
        assert!(!changed_again);
    }

    #[test]
    fn install_skip_paths_never_touch_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let settings_path = claude_settings_path(home);
        assert!(matches!(
            install_claude_hooks(home, false, true),
            HookInstallState::Skipped(HookInstallSkipReason::HooksDisabled)
        ));
        assert!(!settings_path.exists());
        assert!(matches!(
            install_claude_hooks(home, true, false),
            HookInstallState::Skipped(HookInstallSkipReason::CliNotFound)
        ));
        assert!(!settings_path.exists());
    }

    #[test]
    fn install_writes_settings_and_script_with_rolling_backup() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(
            install_claude_hooks(home, true, true),
            HookInstallState::Installed
        );
        let stored: Value =
            serde_json::from_str(&std::fs::read_to_string(claude_settings_path(home)).unwrap())
                .unwrap();
        assert_eq!(stored["theme"], "dark");
        assert_eq!(managed_definitions(&stored, "Stop"), 1);
        assert!(claude_settings_path(home).with_extension("json.bak").exists());
        assert!(script::managed_script_path(home).exists());
    }

    #[test]
    fn install_dereferences_symlinked_settings_target() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let real_dir = dir.path().join("dotfiles");
        std::fs::create_dir_all(&real_dir).unwrap();
        let real = real_dir.join("settings.json");
        std::fs::write(&real, "{}").unwrap();
        let link_dir = Path::new(home).join(".claude");
        std::fs::create_dir_all(&link_dir).unwrap();
        let link = link_dir.join("settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            install_claude_hooks(home, true, true),
            HookInstallState::Installed
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
        assert_eq!(managed_definitions(&stored, "Stop"), 1);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn install_refuses_to_clobber_malformed_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), "{ not json").unwrap();
        assert!(matches!(
            install_claude_hooks(home, true, true),
            HookInstallState::Error(_)
        ));
        assert_eq!(
            std::fs::read_to_string(claude_settings_path(home)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn install_reports_error_for_non_object_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), "[]").unwrap();
        assert!(matches!(
            install_claude_hooks(home, true, true),
            HookInstallState::Error(_)
        ));
        assert_eq!(
            std::fs::read_to_string(claude_settings_path(home)).unwrap(),
            "[]"
        );
    }

    #[test]
    fn remove_reports_error_for_non_object_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), "[]").unwrap();
        assert!(matches!(
            remove_claude_hooks(home),
            HookInstallState::Error(_)
        ));
        assert_eq!(
            std::fs::read_to_string(claude_settings_path(home)).unwrap(),
            "[]"
        );
    }

    #[test]
    fn remove_without_settings_file_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        assert!(!matches!(
            remove_claude_hooks(home),
            HookInstallState::Error(_)
        ));
        assert!(!claude_settings_path(home).exists());
    }
}
