//! Session resume preparation for the Agent Session History panel.
//!
//! The reference repins Codex resumes to a managed or per-account Codex home
//! and reports that through `useRealCodexHome`/`substituteCodexHome`. This
//! port keeps no managed Codex homes, so preparation is a constant
//! disclosure: resume always runs against the real `~/.codex` and there is
//! never a substitute home to hand back.

use serde_json::{json, Value};

/// Always `{"useRealCodexHome": true}`; every argument is intentionally
/// ignored because no managed Codex home exists in this port.
pub fn prepare_session_resume(_args: &Value) -> Value {
    json!({ "useRealCodexHome": true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn always_reports_the_real_codex_home() {
        for args in [
            json!(null),
            json!({}),
            json!({
                "agent": "claude",
                "filePath": "/home/.claude/projects/-repo-app/session.jsonl",
                "codexHome": "/managed/codex-accounts/acct/home",
                "sessionId": "session"
            }),
        ] {
            assert_eq!(
                prepare_session_resume(&args),
                json!({ "useRealCodexHome": true })
            );
        }
    }
}
