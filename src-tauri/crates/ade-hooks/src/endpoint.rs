use std::collections::HashMap;
use std::path::Path;

use ade_core::ids;

pub const ENDPOINT_FILE_NAME: &str = "endpoint.env";
pub const HOOK_PROTOCOL_VERSION: &str = "1";
pub const HOOK_RAW_JSON_TRANSPORT: &str = "raw-json-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFields {
    pub port: u16,
    pub token: String,
    pub env: String,
}

/// 值会被 shell `source`；正则等价 fork `isShellSafeEndpointValue`
/// （规格 §3.5）：空白、引号、`;`、`$` 等一律拒绝，空串也拒绝
/// （防止 `KEY=` 清空已存在的变量）。
pub fn is_shell_safe_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '/' | '-'))
}

pub fn endpoint_env_lines(fields: &EndpointFields) -> Vec<(String, String)> {
    vec![
        ("ORCA_AGENT_HOOK_PORT".to_string(), fields.port.to_string()),
        ("ORCA_AGENT_HOOK_TOKEN".to_string(), fields.token.clone()),
        ("ORCA_AGENT_HOOK_ENV".to_string(), fields.env.clone()),
        (
            "ORCA_AGENT_HOOK_VERSION".to_string(),
            HOOK_PROTOCOL_VERSION.to_string(),
        ),
        (
            "ORCA_AGENT_HOOK_TRANSPORT".to_string(),
            HOOK_RAW_JSON_TRANSPORT.to_string(),
        ),
    ]
}

/// 0600 原子写（oracle `writeEndpointFile` 语义，规格 §5.1）：目录 0700、
/// 临时文件 `create_new` + 0600、rename 落位、失败清 tmp。
pub fn write_endpoint_file(dir: &Path, fields: &EndpointFields) -> std::io::Result<bool> {
    let lines = endpoint_env_lines(fields);
    for (key, value) in &lines {
        if !is_shell_safe_value(value) {
            eprintln!(
                "[ade-hooks] refusing to write endpoint file: {key} contains characters unsafe for shell sourcing"
            );
            return Ok(false);
        }
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let final_path = dir.join(ENDPOINT_FILE_NAME);
    let tmp_path = dir.join(format!(
        ".endpoint-{}-{}.tmp",
        std::process::id(),
        ids::new_uuid()
    ));
    let mut contents = lines
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    let result = (|| -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(contents.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&tmp_path, contents.as_bytes())?;
        }
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result.map(|_| true)
}

pub fn pty_env(fields: &EndpointFields, endpoint_path: &Path) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = endpoint_env_lines(fields).into_iter().collect();
    env.insert(
        "ORCA_AGENT_HOOK_ENDPOINT".to_string(),
        endpoint_path.to_string_lossy().into_owned(),
    );
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fields() -> EndpointFields {
        EndpointFields {
            port: 43123,
            token: "a1b2-c3".to_string(),
            env: "development".to_string(),
        }
    }

    #[test]
    fn shell_safe_values_reject_metacharacters_and_empty() {
        assert!(is_shell_safe_value("a1B2-c3._:/x"));
        assert!(!is_shell_safe_value(""));
        assert!(!is_shell_safe_value("tok en"));
        assert!(!is_shell_safe_value("tok\n"));
        assert!(!is_shell_safe_value("tok;rm"));
    }

    #[test]
    fn endpoint_lines_carry_port_token_env_version_and_transport() {
        assert_eq!(
            endpoint_env_lines(&fields()),
            vec![
                ("ORCA_AGENT_HOOK_PORT".to_string(), "43123".to_string()),
                ("ORCA_AGENT_HOOK_TOKEN".to_string(), "a1b2-c3".to_string()),
                (
                    "ORCA_AGENT_HOOK_ENV".to_string(),
                    "development".to_string()
                ),
                (
                    "ORCA_AGENT_HOOK_VERSION".to_string(),
                    "1".to_string()
                ),
                (
                    "ORCA_AGENT_HOOK_TRANSPORT".to_string(),
                    "raw-json-v1".to_string()
                ),
            ]
        );
    }

    #[test]
    fn write_endpoint_file_is_atomic_0600_and_shell_sourceable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("agent-hooks");
        assert!(write_endpoint_file(&target, &fields()).unwrap());
        let path = target.join(ENDPOINT_FILE_NAME);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "ORCA_AGENT_HOOK_PORT=43123\n\
             ORCA_AGENT_HOOK_TOKEN=a1b2-c3\n\
             ORCA_AGENT_HOOK_ENV=development\n\
             ORCA_AGENT_HOOK_VERSION=1\n\
             ORCA_AGENT_HOOK_TRANSPORT=raw-json-v1\n"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let leftovers = std::fs::read_dir(&target)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".endpoint-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn write_endpoint_file_refuses_unsafe_values_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut unsafe_fields = fields();
        unsafe_fields.token = "bad token".to_string();
        assert!(!write_endpoint_file(dir.path(), &unsafe_fields).unwrap());
        assert!(!dir.path().join(ENDPOINT_FILE_NAME).exists());
    }

    #[test]
    fn pty_env_supersets_endpoint_lines_with_the_endpoint_path() {
        let env = pty_env(&fields(), Path::new("/data/agent-hooks/endpoint.env"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_PORT").map(String::as_str), Some("43123"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_TOKEN").map(String::as_str), Some("a1b2-c3"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_ENV").map(String::as_str), Some("development"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_VERSION").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("ORCA_AGENT_HOOK_TRANSPORT").map(String::as_str),
            Some("raw-json-v1")
        );
        assert_eq!(
            env.get("ORCA_AGENT_HOOK_ENDPOINT").map(String::as_str),
            Some("/data/agent-hooks/endpoint.env")
        );
    }
}
