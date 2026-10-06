use std::io;
use std::path::{Path, PathBuf};

pub fn managed_hooks_dir(home: &str) -> PathBuf {
    Path::new(home).join(".ade").join("agent-hooks")
}

pub fn managed_script_path(home: &str) -> PathBuf {
    managed_hooks_dir(home).join("claude-hook.sh")
}

pub fn managed_script_contents() -> String {
    r#"#!/bin/sh
printf '{}\n'
payload=$( { command -p cat 2>/dev/null || cat; } )
if [ -z "$payload" ]; then
  exit 0
fi
spool_hook_event() {
  case "$payload" in
    *'"PreToolUse"'*|*'"PostToolUse"'*|*'"PostToolUseFailure"'*) return 0 ;;
  esac
  [ -n "${ORCA_AGENT_HOOK_ENDPOINT:-}" ] || return 0
  [ -n "${ORCA_PANE_KEY:-}" ] || return 0
  [ -r "$ORCA_AGENT_HOOK_ENDPOINT" ] || return 0
  spool_base=${ORCA_AGENT_HOOK_ENDPOINT%/*}
  spool_dir="$spool_base/spool"
  mkdir -p "$spool_dir" 2>/dev/null || return 0
  chmod 700 "$spool_dir" 2>/dev/null || :
  spool_id=$(printf %s "${ORCA_PANE_KEY:-unknown}" | tail -c 36 | tr '/:' '__')
  spool_file="$spool_dir/pane-$spool_id.jsonl"
  if [ -f "$spool_file" ] && find "$spool_file" -mtime +7 -print -quit 2>/dev/null | grep -q .; then : > "$spool_file"; fi
  [ -f "$spool_file" ] || : > "$spool_file"
  spool_size=$(wc -c < "$spool_file" 2>/dev/null || printf 0)
  [ "$spool_size" -lt 5242880 ] || return 0
  spool_now=$(date +%s 2>/dev/null || printf 0)
  spool_now=$((spool_now * 1000))
  spool_json_escape() { printf %s "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/[[:cntrl:]]/ /g'; }
  { printf '\n{"paneKey":"%s","tabId":"%s","worktreeId":"%s","env":"%s","version":"%s","launchToken":"%s","source":"claude","receivedAt":%s,"payload":%s}\n' \
    "$(spool_json_escape "${ORCA_PANE_KEY:-}")" \
    "$(spool_json_escape "${ORCA_TAB_ID:-}")" \
    "$(spool_json_escape "${ORCA_WORKTREE_ID:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_HOOK_ENV:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_HOOK_VERSION:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_LAUNCH_TOKEN:-}")" \
    "$spool_now" "$payload"; } >> "$spool_file" 2>/dev/null || :
  chmod 600 "$spool_file" 2>/dev/null || :
}
if [ -n "${ORCA_AGENT_HOOK_ENDPOINT:-}" ] && [ -r "$ORCA_AGENT_HOOK_ENDPOINT" ]; then
  unset ORCA_AGENT_HOOK_TRANSPORT
  . "$ORCA_AGENT_HOOK_ENDPOINT" 2>/dev/null || :
fi
if [ -z "${ORCA_AGENT_HOOK_PORT:-}" ] || [ -z "${ORCA_AGENT_HOOK_TOKEN:-}" ] || [ -z "${ORCA_PANE_KEY:-}" ]; then
  spool_hook_event
  exit 0
fi
if [ "${ORCA_AGENT_HOOK_TRANSPORT:-}" = "raw-json-v1" ] && command -v base64 >/dev/null 2>&1 && command -v tr >/dev/null 2>&1; then
  orca_hook_metadata=$(printf '%s\037%s\037%s\037%s\037%s\037%s' "$ORCA_PANE_KEY" "$ORCA_TAB_ID" "$ORCA_AGENT_LAUNCH_TOKEN" "$ORCA_WORKTREE_ID" "$ORCA_AGENT_HOOK_ENV" "$ORCA_AGENT_HOOK_VERSION" | base64 | tr -d '\n') && \
  [ -n "$orca_hook_metadata" ] && \
  printf '%s' "$payload" | curl -sS -X POST "http://127.0.0.1:${ORCA_AGENT_HOOK_PORT}/hook/claude" \
    --connect-timeout "${connect_timeout:-0.5}" --max-time "${max_time:-1.5}" \
    --noproxy "127.0.0.1" \
    -H "Content-Type: application/json" \
    -H "X-Orca-Agent-Hook-Token: ${ORCA_AGENT_HOOK_TOKEN}" \
    -H "X-Orca-Agent-Hook-Meta-Encoding: base64" \
    -H "X-Orca-Agent-Hook-Meta: ${orca_hook_metadata}" \
    --data-binary @- >/dev/null 2>&1 || spool_hook_event
else
  printf '%s' "$payload" | curl -sS -X POST "http://127.0.0.1:${ORCA_AGENT_HOOK_PORT}/hook/claude" \
    --connect-timeout "${connect_timeout:-0.5}" --max-time "${max_time:-1.5}" \
    --noproxy "127.0.0.1" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -H "X-Orca-Agent-Hook-Token: ${ORCA_AGENT_HOOK_TOKEN}" \
    --data-urlencode "paneKey=${ORCA_PANE_KEY}" \
    --data-urlencode "tabId=${ORCA_TAB_ID}" \
    --data-urlencode "launchToken=${ORCA_AGENT_LAUNCH_TOKEN}" \
    --data-urlencode "worktreeId=${ORCA_WORKTREE_ID}" \
    --data-urlencode "env=${ORCA_AGENT_HOOK_ENV}" \
    --data-urlencode "version=${ORCA_AGENT_HOOK_VERSION}" \
    --data-urlencode "payload@-" >/dev/null 2>&1 || spool_hook_event
fi
exit 0
"#
    .to_string()
}

pub fn write_managed_script(home: &str) -> io::Result<PathBuf> {
    let dir = managed_hooks_dir(home);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = managed_script_path(home);
    let contents = managed_script_contents();
    if std::fs::read_to_string(&path).map(|existing| existing == contents).unwrap_or(false) {
        return Ok(path);
    }
    let tmp = dir.join(format!(".claude-hook-{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn script_starts_with_neutral_stdout_and_captures_stdin_before_anything_else() {
        let script = managed_script_contents();
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines[0], "#!/bin/sh");
        assert_eq!(lines[1], "printf '{}\\n'");
        assert!(script.contains("payload=$( { command -p cat 2>/dev/null || cat; } )"));
        assert!(script.contains("if [ -z \"$payload\" ]; then"));
    }

    #[test]
    fn script_posts_raw_json_with_base64_meta_and_form_fallback() {
        let script = managed_script_contents();
        assert!(script.contains("ORCA_AGENT_HOOK_TRANSPORT:-}\" = \"raw-json-v1\""));
        assert!(script.contains("printf '%s\\037%s\\037%s\\037%s\\037%s\\037%s'"));
        assert!(script.contains("X-Orca-Agent-Hook-Meta-Encoding: base64"));
        assert!(script.contains("--data-urlencode \"payload@-\""));
        assert!(script.contains("unset ORCA_AGENT_HOOK_TRANSPORT"));
        assert!(script.contains(". \"$ORCA_AGENT_HOOK_ENDPOINT\""));
    }

    #[test]
    fn script_spool_skips_tool_progress_and_bounds_the_file() {
        let script = managed_script_contents();
        assert!(script.contains("*'\"PreToolUse\"'*"));
        assert!(script.contains("spool_dir=\"$spool_base/spool\""));
        assert!(script.contains("5242880"));
        assert!(script.contains("-mtime +7"));
        assert!(script.contains("chmod 600 \"$spool_file\""));
    }

    #[test]
    fn write_managed_script_is_executable_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let first = write_managed_script(home).unwrap();
        assert_eq!(first, managed_script_path(home));
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let before = std::fs::read_to_string(&first).unwrap();
        write_managed_script(home).unwrap();
        assert_eq!(std::fs::read_to_string(&first).unwrap(), before);
    }
}
