//! Verification normalization (spec §3.2), ported from
//! `orca:src/main/computer/computer-action-verification-normalization.ts`:
//! action results without a `verification` field get an `unverified` state
//! naming the action path that produced them.

use serde_json::{json, Value};

/// Fill in the `unverified` verification for action results that do not carry
/// one, naming the action path that cannot assert its own effect. Results
/// without a truthy `action` or with an unknown `action.path` are untouched,
/// as is any existing truthy `action.verification`.
pub fn normalize_action_result(result: &mut Value) {
    let Some(action) = result.get("action") else {
        return;
    };
    if !is_truthy(action) {
        return;
    }
    let reason = match action.get("path").and_then(Value::as_str) {
        Some("synthetic") => "synthetic_input",
        Some("clipboard") => "clipboard_paste",
        Some("accessibility") => "accessibility_action_unasserted",
        _ => return,
    };
    if action.get("verification").is_some_and(is_truthy) {
        return;
    }
    if let Some(action) = result.get_mut("action").and_then(Value::as_object_mut) {
        action.insert(
            "verification".to_string(),
            json!({"state": "unverified", "reason": reason}),
        );
    }
}

/// JS truthiness, used by the oracle's `if (!action)` / `action.verification`
/// guards.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_none_or(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_each_known_path_to_its_reason() {
        for (path, reason) in [
            ("synthetic", "synthetic_input"),
            ("clipboard", "clipboard_paste"),
            ("accessibility", "accessibility_action_unasserted"),
        ] {
            let mut result = json!({"action": {"path": path, "elementIndex": 1}});
            normalize_action_result(&mut result);
            assert_eq!(
                result["action"]["verification"],
                json!({"state": "unverified", "reason": reason}),
                "path {path}"
            );
            assert_eq!(result["action"]["path"], json!(path));
        }
    }

    #[test]
    fn leaves_results_without_a_normalizable_action_untouched() {
        for original in [
            json!({"snapshot": {}}),
            json!({"action": null}),
            json!({"action": {}}),
            json!({"action": {"path": "unknown"}}),
            json!({"action": "synthetic"}),
            json!("not an object"),
        ] {
            let mut result = original.clone();
            normalize_action_result(&mut result);
            assert_eq!(result, original);
        }
    }

    #[test]
    fn does_not_overwrite_an_existing_truthy_verification() {
        let existing = json!({"state": "verified", "reason": "manual"});
        let mut result = json!({"action": {"path": "synthetic", "verification": existing.clone()}});
        normalize_action_result(&mut result);
        assert_eq!(result["action"]["verification"], existing);
    }

    #[test]
    fn replaces_a_falsy_verification_like_the_oracle() {
        for falsy in [json!(null), json!(false)] {
            let mut result = json!({"action": {"path": "clipboard", "verification": falsy}});
            normalize_action_result(&mut result);
            assert_eq!(
                result["action"]["verification"],
                json!({"state": "unverified", "reason": "clipboard_paste"})
            );
        }
    }
}
