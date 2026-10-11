//! Provider action pre-validation (spec §3.2), ported verbatim from
//! `orca:src/main/computer/computer-provider-action-validation.ts`,
//! `orca:src/shared/computer-use-key-spec.ts` and
//! `orca:src/main/computer/computer-clipboard-paste-validation.ts`. Every
//! validation failure is `invalid_argument` and every message must stay
//! byte-for-byte identical to the oracle.

use serde_json::Value;

use crate::protocol::{ComputerError, ComputerErrorCode};

/// UTF-8 byte limit for `pasteText` payloads (`CLIPBOARD_TEXT_WRITE_MAX_BYTES`).
pub const CLIPBOARD_TEXT_WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Verbatim `CLIPBOARD_TEXT_WRITE_TOO_LARGE_ERROR` from
/// `orca:src/shared/clipboard-text.ts`.
pub const CLIPBOARD_TEXT_WRITE_TOO_LARGE_ERROR: &str =
    "Clipboard text is too large to copy safely.";

const HOTKEY_HINT: &str =
    "Hotkey requires a modifier and one key, e.g. CmdOrCtrl+A. Use press-key for a single key.";
const PRESS_KEY_HINT: &str = "Press-key accepts one key only, e.g. Return, Escape, Tab, or +. Use hotkey for modifier combinations.";
const CLICK_MODIFIERS_HINT: &str =
    "Click modifiers accept modifier keys only, e.g. CmdOrCtrl or CmdOrCtrl+Shift.";

/// Hotkey modifier vocabulary (oracle `HOTKEY_MODIFIERS`).
const HOTKEY_MODIFIERS: [&str; 12] = [
    "alt",
    "cmd",
    "cmdorctrl",
    "command",
    "commandorcontrol",
    "control",
    "ctrl",
    "meta",
    "option",
    "shift",
    "super",
    "win",
];

/// Methods pre-validated by the provider client (task contract; `getAppState`
/// is included because snapshots target an app/window like the actions do).
const ACTION_METHODS: [&str; 10] = [
    "getAppState",
    "click",
    "performSecondaryAction",
    "scroll",
    "drag",
    "typeText",
    "pressKey",
    "hotkey",
    "pasteText",
    "setValue",
];

/// Whether `method` is an action method subject to pre-validation and
/// verification normalization.
pub fn is_action_method(method: &str) -> bool {
    ACTION_METHODS.contains(&method)
}

/// Validate action-method params before they reach the helper. Non-action
/// methods are let through (their params are validated by their own surfaces).
pub fn validate_computer_action(method: &str, params: &Value) -> Result<(), ComputerError> {
    if !is_action_method(method) {
        return Ok(());
    }
    require_non_empty_string(params, "app")?;
    validate_window_target(params)?;
    match method {
        "getAppState" => Ok(()),
        "click" => {
            validate_element_or_coordinates("Click", params)?;
            validate_positive_integer(params, "clickCount")?;
            validate_mouse_button(params)?;
            validate_click_modifiers(params)
        }
        "performSecondaryAction" => {
            require_non_negative_integer(params, "elementIndex")?;
            require_non_empty_string(params, "action").map(|_| ())
        }
        "scroll" => {
            validate_element_or_coordinates("Scroll", params)?;
            validate_scroll_direction(params)?;
            validate_positive_number(params, "pages")
        }
        "drag" => validate_drag_target(params),
        "typeText" => require_non_empty_string(params, "text").map(|_| ()),
        "pasteText" => validate_paste_text(params),
        "pressKey" => validate_press_key(params),
        "hotkey" => validate_hotkey(params),
        "setValue" => {
            require_non_negative_integer(params, "elementIndex")?;
            require_string_allowing_empty(params, "value").map(|_| ())
        }
        _ => Ok(()),
    }
}

/// `computerUseHotkeyValidationMessage`: `None` when the key is a modifier
/// combination with exactly one non-modifier key.
pub fn hotkey_validation_message(key: &str) -> Option<&'static str> {
    let parts: Vec<&str> = key.split('+').map(str::trim).collect();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return Some(HOTKEY_HINT);
    }
    let key_part_count = parts
        .iter()
        .filter(|part| !is_hotkey_modifier(normalize_hotkey_part(part)))
        .count();
    if key_part_count != 1 {
        return Some(HOTKEY_HINT);
    }
    None
}

/// `computerUsePressKeyValidationMessage`: a single key only; `+` is allowed
/// as a literal key.
pub fn press_key_validation_message(key: &str) -> Option<&'static str> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Some(PRESS_KEY_HINT);
    }
    if trimmed != "+" && trimmed.contains('+') {
        return Some(PRESS_KEY_HINT);
    }
    None
}

/// `computerUseClickModifiersValidationMessage`: up to four modifier keys.
pub fn click_modifiers_validation_message(modifiers: &str) -> Option<&'static str> {
    let parts: Vec<&str> = modifiers.split('+').map(str::trim).collect();
    if parts.len() > 4
        || parts
            .iter()
            .any(|part| part.is_empty() || !is_hotkey_modifier(part.to_lowercase()))
    {
        return Some(CLICK_MODIFIERS_HINT);
    }
    None
}

fn invalid_argument(message: impl Into<String>) -> ComputerError {
    ComputerError::new(ComputerErrorCode::InvalidArgument, message)
}

fn has(params: &Value, key: &str) -> bool {
    params.get(key).is_some()
}

fn normalize_hotkey_part(part: &str) -> String {
    part.to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '_' && *character != '-')
        .collect()
}

fn is_hotkey_modifier(part: impl AsRef<str>) -> bool {
    HOTKEY_MODIFIERS.contains(&part.as_ref())
}

fn validate_window_target(params: &Value) -> Result<(), ComputerError> {
    if has(params, "windowId") && has(params, "windowIndex") {
        return Err(invalid_argument(
            "Window targeting accepts either windowId or windowIndex, not both",
        ));
    }
    Ok(())
}

fn validate_element_or_coordinates(
    action_name: &'static str,
    params: &Value,
) -> Result<(), ComputerError> {
    let has_element = has(params, "elementIndex");
    let has_x = has(params, "x");
    let has_y = has(params, "y");
    if !has_element && !(has_x && has_y) {
        return Err(invalid_argument(format!(
            "{action_name} requires elementIndex or both x and y"
        )));
    }
    if has_x != has_y {
        return Err(invalid_argument(format!(
            "{action_name} coordinates require both x and y"
        )));
    }
    if has_element && (has_x || has_y) {
        return Err(invalid_argument(format!(
            "{action_name} accepts either elementIndex or coordinate fields, not both"
        )));
    }
    if has_element {
        require_non_negative_integer(params, "elementIndex")?;
    }
    require_finite_number(params, "x")?;
    require_finite_number(params, "y")
}

fn validate_drag_target(params: &Value) -> Result<(), ComputerError> {
    let has_from_element = has(params, "fromElementIndex");
    let has_to_element = has(params, "toElementIndex");
    let has_element_pair = has_from_element && has_to_element;
    let has_partial_element_pair = has_from_element || has_to_element;
    let coordinates = ["fromX", "fromY", "toX", "toY"];
    let has_coordinate_pair = coordinates.iter().all(|key| has(params, key));
    let has_partial_coordinate_pair = coordinates.iter().any(|key| has(params, key));

    if has_element_pair && has_coordinate_pair {
        return Err(invalid_argument(
            "Drag accepts either element indexes or coordinate fields, not both",
        ));
    }
    if has_partial_element_pair && !has_element_pair {
        return Err(invalid_argument(
            "Drag element targeting requires both fromElementIndex and toElementIndex",
        ));
    }
    if has_partial_coordinate_pair && !has_coordinate_pair {
        return Err(invalid_argument(
            "Drag coordinates require fromX, fromY, toX, and toY",
        ));
    }
    if !has_element_pair && !has_coordinate_pair {
        return Err(invalid_argument(
            "Drag requires fromElementIndex and toElementIndex, or all coordinate fields",
        ));
    }
    if has_element_pair {
        require_non_negative_integer(params, "fromElementIndex")?;
        require_non_negative_integer(params, "toElementIndex")?;
    }
    if has_coordinate_pair {
        for key in coordinates {
            require_finite_number(params, key)?;
        }
    }
    Ok(())
}

fn validate_mouse_button(params: &Value) -> Result<(), ComputerError> {
    let Some(mouse_button) = params.get("mouseButton") else {
        return Ok(());
    };
    if matches!(mouse_button.as_str(), Some("left" | "right" | "middle")) {
        return Ok(());
    }
    Err(invalid_argument(
        "Unsupported mouseButton; expected left, right, or middle",
    ))
}

fn validate_click_modifiers(params: &Value) -> Result<(), ComputerError> {
    if !has(params, "modifiers") {
        return Ok(());
    }
    let modifiers = require_non_empty_string(params, "modifiers")?;
    match click_modifiers_validation_message(&modifiers) {
        Some(message) => Err(invalid_argument(message)),
        None => Ok(()),
    }
}

fn validate_scroll_direction(params: &Value) -> Result<(), ComputerError> {
    let direction = require_non_empty_string(params, "direction")?;
    if matches!(direction.as_str(), "up" | "down" | "left" | "right") {
        return Ok(());
    }
    Err(invalid_argument(
        "Unsupported direction; expected up, down, left, or right",
    ))
}

fn validate_press_key(params: &Value) -> Result<(), ComputerError> {
    let key = require_non_empty_string(params, "key")?;
    match press_key_validation_message(&key) {
        Some(message) => Err(invalid_argument(message)),
        None => Ok(()),
    }
}

fn validate_hotkey(params: &Value) -> Result<(), ComputerError> {
    let key = require_non_empty_string(params, "key")?;
    match hotkey_validation_message(&key) {
        Some(message) => Err(invalid_argument(message)),
        None => Ok(()),
    }
}

fn validate_paste_text(params: &Value) -> Result<(), ComputerError> {
    let text = require_non_empty_string(params, "text")?;
    if text.len() > CLIPBOARD_TEXT_WRITE_MAX_BYTES {
        return Err(invalid_argument(CLIPBOARD_TEXT_WRITE_TOO_LARGE_ERROR));
    }
    Ok(())
}

fn require_string_allowing_empty(params: &Value, key: &str) -> Result<String, ComputerError> {
    match params.get(key).and_then(Value::as_str) {
        Some(value) => Ok(value.to_string()),
        None => Err(invalid_argument(format!("Missing {key}"))),
    }
}

fn require_non_empty_string(params: &Value, key: &str) -> Result<String, ComputerError> {
    let value = require_string_allowing_empty(params, key)?;
    if value.is_empty() {
        return Err(invalid_argument(format!("Missing {key}")));
    }
    Ok(value)
}

/// JS `Number.isInteger`: an f64 without a fractional part.
fn json_integer(value: &Value) -> Option<f64> {
    let number = value.as_f64()?;
    (number.fract() == 0.0).then_some(number)
}

fn require_non_negative_integer(params: &Value, key: &str) -> Result<(), ComputerError> {
    match params.get(key) {
        Some(value) if json_integer(value).is_some_and(|number| number >= 0.0) => Ok(()),
        _ => Err(invalid_argument(format!(
            "{key} must be a non-negative integer"
        ))),
    }
}

fn require_finite_number(params: &Value, key: &str) -> Result<(), ComputerError> {
    match params.get(key) {
        None => Ok(()),
        Some(value) if value.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Some(_) => Err(invalid_argument(format!("{key} must be a finite number"))),
    }
}

fn validate_positive_integer(params: &Value, key: &str) -> Result<(), ComputerError> {
    match params.get(key) {
        None => Ok(()),
        Some(value) if json_integer(value).is_some_and(|number| number > 0.0) => Ok(()),
        Some(_) => Err(invalid_argument(format!(
            "{key} must be a positive integer"
        ))),
    }
}

fn validate_positive_number(params: &Value, key: &str) -> Result<(), ComputerError> {
    match params.get(key) {
        None => Ok(()),
        Some(value) if value.as_f64().is_some_and(|number| number > 0.0) => Ok(()),
        Some(_) => Err(invalid_argument(format!("{key} must be a positive number"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ACTION_METHODS_UNDER_TEST: [&str; 10] = [
        "getAppState",
        "click",
        "performSecondaryAction",
        "scroll",
        "drag",
        "typeText",
        "pressKey",
        "hotkey",
        "pasteText",
        "setValue",
    ];

    const HOTKEY_HINT: &str =
        "Hotkey requires a modifier and one key, e.g. CmdOrCtrl+A. Use press-key for a single key.";
    const PRESS_KEY_HINT: &str = "Press-key accepts one key only, e.g. Return, Escape, Tab, or +. Use hotkey for modifier combinations.";
    const CLICK_MODIFIERS_HINT: &str =
        "Click modifiers accept modifier keys only, e.g. CmdOrCtrl or CmdOrCtrl+Shift.";
    const CLIPBOARD_TOO_LARGE: &str = "Clipboard text is too large to copy safely.";

    fn assert_invalid(method: &str, params: Value, expected_message: &str) {
        let error = validate_computer_action(method, &params)
            .expect_err(&format!("{method} accepted invalid params {params}"));
        assert_eq!(
            error.code,
            ComputerErrorCode::InvalidArgument,
            "method {method} params {params}"
        );
        assert_eq!(
            error.message, expected_message,
            "method {method} params {params}"
        );
    }

    fn assert_valid(method: &str, params: Value) {
        if let Err(error) = validate_computer_action(method, &params) {
            panic!(
                "{method} rejected valid params {params}: {} {}",
                error.code.as_str(),
                error.message
            );
        }
    }

    #[test]
    fn action_method_set_matches_the_client_contract() {
        for method in ACTION_METHODS_UNDER_TEST {
            assert!(
                is_action_method(method),
                "{method} must be an action method"
            );
        }
        for method in [
            "handshake",
            "listApps",
            "listWindows",
            "terminate",
            "permissionsStatus",
        ] {
            assert!(
                !is_action_method(method),
                "{method} must not be an action method"
            );
        }
    }

    #[test]
    fn action_methods_require_a_non_empty_string_app() {
        for method in ACTION_METHODS_UNDER_TEST {
            assert_invalid(method, json!({}), "Missing app");
            assert_invalid(method, json!({"app": ""}), "Missing app");
            assert_invalid(method, json!({"app": 3}), "Missing app");
            assert_invalid(method, json!({"app": null}), "Missing app");
        }
    }

    #[test]
    fn utility_methods_are_out_of_scope_for_action_validation() {
        for method in [
            "handshake",
            "listApps",
            "listWindows",
            "terminate",
            "permissionsStatus",
        ] {
            assert_valid(method, json!({}));
        }
    }

    #[test]
    fn window_targeting_accepts_either_window_id_or_window_index_but_not_both() {
        let message = "Window targeting accepts either windowId or windowIndex, not both";
        for method in ACTION_METHODS_UNDER_TEST {
            assert_invalid(
                method,
                json!({"app": "Finder", "windowId": 1, "windowIndex": 0}),
                message,
            );
        }
        assert_invalid(
            "getAppState",
            json!({"app": "Finder", "windowId": null, "windowIndex": 0}),
            message,
        );
        assert_valid("getAppState", json!({"app": "Finder"}));
        assert_valid("getAppState", json!({"app": "Finder", "windowId": 1}));
        assert_valid("getAppState", json!({"app": "Finder", "windowIndex": 0}));
    }

    #[test]
    fn click_requires_element_index_or_both_coordinates() {
        assert_invalid(
            "click",
            json!({"app": "X"}),
            "Click requires elementIndex or both x and y",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "x": 1.0}),
            "Click requires elementIndex or both x and y",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "y": 2.0}),
            "Click requires elementIndex or both x and y",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 0, "x": 1.0}),
            "Click coordinates require both x and y",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 0, "y": 2.0}),
            "Click coordinates require both x and y",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 1, "x": 1.0, "y": 2.0}),
            "Click accepts either elementIndex or coordinate fields, not both",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": -1}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 0.5}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": "0"}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "x": 1.0, "y": "2"}),
            "y must be a finite number",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "x": "1", "y": 2}),
            "x must be a finite number",
        );
        assert_valid("click", json!({"app": "X", "elementIndex": 0}));
        assert_valid("click", json!({"app": "X", "x": 1.5, "y": 10}));
    }

    #[test]
    fn click_count_must_be_a_positive_integer_when_present() {
        assert_valid("click", json!({"app": "X", "elementIndex": 0}));
        assert_valid(
            "click",
            json!({"app": "X", "elementIndex": 0, "clickCount": 2}),
        );
        for value in [json!(0), json!(-1), json!(1.5), json!("2"), json!(true)] {
            assert_invalid(
                "click",
                json!({"app": "X", "elementIndex": 0, "clickCount": value}),
                "clickCount must be a positive integer",
            );
        }
    }

    #[test]
    fn mouse_button_must_be_left_right_or_middle_when_present() {
        const MESSAGE: &str = "Unsupported mouseButton; expected left, right, or middle";
        for button in ["left", "right", "middle"] {
            assert_valid(
                "click",
                json!({"app": "X", "elementIndex": 0, "mouseButton": button}),
            );
        }
        for button in [json!("back"), json!(1), json!(null)] {
            assert_invalid(
                "click",
                json!({"app": "X", "elementIndex": 0, "mouseButton": button}),
                MESSAGE,
            );
        }
    }

    #[test]
    fn click_modifiers_must_be_modifier_keys() {
        assert_valid(
            "click",
            json!({"app": "X", "elementIndex": 0, "modifiers": "CmdOrCtrl"}),
        );
        assert_valid(
            "click",
            json!({"app": "X", "elementIndex": 0, "modifiers": " CmdOrCtrl + Shift "}),
        );
        for modifiers in ["A", "Cmd+A", "Cmd+Ctrl+Alt+Shift+X", "CmdOrCtrl+"] {
            assert_invalid(
                "click",
                json!({"app": "X", "elementIndex": 0, "modifiers": modifiers}),
                CLICK_MODIFIERS_HINT,
            );
        }
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 0, "modifiers": ""}),
            "Missing modifiers",
        );
        assert_invalid(
            "click",
            json!({"app": "X", "elementIndex": 0, "modifiers": 4}),
            "Missing modifiers",
        );
    }

    #[test]
    fn perform_secondary_action_requires_element_index_and_action() {
        assert_valid(
            "performSecondaryAction",
            json!({"app": "X", "elementIndex": 0, "action": "AXPress"}),
        );
        assert_invalid(
            "performSecondaryAction",
            json!({"app": "X", "action": "AXPress"}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "performSecondaryAction",
            json!({"app": "X", "elementIndex": -1, "action": "AXPress"}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "performSecondaryAction",
            json!({"app": "X", "elementIndex": 0}),
            "Missing action",
        );
        assert_invalid(
            "performSecondaryAction",
            json!({"app": "X", "elementIndex": 0, "action": ""}),
            "Missing action",
        );
    }

    #[test]
    fn scroll_requires_element_index_or_coordinates_direction_and_positive_pages() {
        assert_invalid(
            "scroll",
            json!({"app": "X", "direction": "up"}),
            "Scroll requires elementIndex or both x and y",
        );
        assert_invalid(
            "scroll",
            json!({"app": "X", "elementIndex": 0}),
            "Missing direction",
        );
        assert_invalid(
            "scroll",
            json!({"app": "X", "elementIndex": 0, "direction": ""}),
            "Missing direction",
        );
        assert_invalid(
            "scroll",
            json!({"app": "X", "elementIndex": 0, "direction": "diagonal"}),
            "Unsupported direction; expected up, down, left, or right",
        );
        for direction in ["up", "down", "left", "right"] {
            assert_valid(
                "scroll",
                json!({"app": "X", "elementIndex": 0, "direction": direction}),
            );
        }
        assert_valid(
            "scroll",
            json!({"app": "X", "x": 1, "y": 2, "direction": "down", "pages": 1.5}),
        );
        for pages in [json!(0), json!(-1), json!("1"), json!(null)] {
            assert_invalid(
                "scroll",
                json!({"app": "X", "elementIndex": 0, "direction": "up", "pages": pages}),
                "pages must be a positive number",
            );
        }
    }

    #[test]
    fn drag_requires_matching_element_pair_or_all_four_coordinates() {
        const BOTH: &str = "Drag accepts either element indexes or coordinate fields, not both";
        const PARTIAL_ELEMENT: &str =
            "Drag element targeting requires both fromElementIndex and toElementIndex";
        const PARTIAL_COORDINATES: &str = "Drag coordinates require fromX, fromY, toX, and toY";
        const MISSING: &str =
            "Drag requires fromElementIndex and toElementIndex, or all coordinate fields";

        assert_invalid("drag", json!({"app": "X"}), MISSING);
        assert_invalid(
            "drag",
            json!({"app": "X", "fromElementIndex": 0}),
            PARTIAL_ELEMENT,
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "toElementIndex": 1}),
            PARTIAL_ELEMENT,
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "fromX": 1.0}),
            PARTIAL_COORDINATES,
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "fromX": 1.0, "fromY": 2.0, "toX": 3.0}),
            PARTIAL_COORDINATES,
        );
        assert_invalid(
            "drag",
            json!({
                "app": "X",
                "fromElementIndex": 0,
                "toElementIndex": 1,
                "fromX": 1.0,
                "fromY": 2.0,
                "toX": 3.0,
                "toY": 4.0,
            }),
            BOTH,
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "fromElementIndex": -1, "toElementIndex": 1}),
            "fromElementIndex must be a non-negative integer",
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "fromElementIndex": 0, "toElementIndex": 1.5}),
            "toElementIndex must be a non-negative integer",
        );
        assert_invalid(
            "drag",
            json!({"app": "X", "fromX": "1", "fromY": 2.0, "toX": 3.0, "toY": 4.0}),
            "fromX must be a finite number",
        );
        assert_valid(
            "drag",
            json!({"app": "X", "fromElementIndex": 0, "toElementIndex": 1}),
        );
        assert_valid(
            "drag",
            json!({"app": "X", "fromX": 1.0, "fromY": 2.0, "toX": 3.0, "toY": 4.0}),
        );
    }

    #[test]
    fn type_text_requires_non_empty_text() {
        assert_valid("typeText", json!({"app": "X", "text": "hello"}));
        for text in [json!(null), json!(""), json!(42)] {
            assert_invalid(
                "typeText",
                json!({"app": "X", "text": text}),
                "Missing text",
            );
        }
        assert_invalid("typeText", json!({"app": "X"}), "Missing text");
    }

    #[test]
    fn press_key_accepts_one_key_only_including_a_literal_plus() {
        assert_valid("pressKey", json!({"app": "X", "key": "Return"}));
        assert_valid("pressKey", json!({"app": "X", "key": "+"}));
        assert_valid("pressKey", json!({"app": "X", "key": " + "}));
        assert_invalid("pressKey", json!({"app": "X"}), "Missing key");
        assert_invalid("pressKey", json!({"app": "X", "key": ""}), "Missing key");
        assert_invalid(
            "pressKey",
            json!({"app": "X", "key": "Cmd+A"}),
            PRESS_KEY_HINT,
        );
        assert_invalid(
            "pressKey",
            json!({"app": "X", "key": "A+B"}),
            PRESS_KEY_HINT,
        );
    }

    #[test]
    fn hotkey_requires_at_least_one_modifier_and_exactly_one_key() {
        assert_valid("hotkey", json!({"app": "X", "key": "CmdOrCtrl+A"}));
        assert_valid("hotkey", json!({"app": "X", "key": "meta + a"}));
        assert_valid("hotkey", json!({"app": "X", "key": "cmd_or_ctrl+a"}));
        assert_invalid("hotkey", json!({"app": "X"}), "Missing key");
        for key in ["A", "A+B", "Cmd+Shift", "Cmd+", "+A"] {
            assert_invalid("hotkey", json!({"app": "X", "key": key}), HOTKEY_HINT);
        }
    }

    #[test]
    fn paste_text_requires_text_and_enforces_the_sixteen_mib_utf8_limit() {
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        assert_invalid("pasteText", json!({"app": "X"}), "Missing text");
        assert_invalid("pasteText", json!({"app": "X", "text": ""}), "Missing text");
        assert_valid("pasteText", json!({"app": "X", "text": "hi"}));

        let at_limit = "a".repeat(MAX_BYTES);
        assert_valid("pasteText", json!({"app": "X", "text": at_limit}));
        assert_invalid(
            "pasteText",
            json!({"app": "X", "text": format!("{at_limit}b")}),
            CLIPBOARD_TOO_LARGE,
        );
        let multibyte_at_limit = format!("{}é", "a".repeat(MAX_BYTES - 2));
        assert_valid("pasteText", json!({"app": "X", "text": multibyte_at_limit}));
        let multibyte_over_limit = format!("{}é", "a".repeat(MAX_BYTES - 1));
        assert_invalid(
            "pasteText",
            json!({"app": "X", "text": multibyte_over_limit}),
            CLIPBOARD_TOO_LARGE,
        );
    }

    #[test]
    fn set_value_requires_element_index_and_allows_an_empty_value() {
        assert_valid(
            "setValue",
            json!({"app": "X", "elementIndex": 0, "value": ""}),
        );
        assert_valid(
            "setValue",
            json!({"app": "X", "elementIndex": 2, "value": "text"}),
        );
        assert_invalid(
            "setValue",
            json!({"app": "X", "value": "text"}),
            "elementIndex must be a non-negative integer",
        );
        assert_invalid(
            "setValue",
            json!({"app": "X", "elementIndex": 0}),
            "Missing value",
        );
        assert_invalid(
            "setValue",
            json!({"app": "X", "elementIndex": 0, "value": null}),
            "Missing value",
        );
    }

    #[test]
    fn key_spec_helpers_expose_the_oracle_hints_verbatim() {
        assert_eq!(hotkey_validation_message("A"), Some(HOTKEY_HINT));
        assert_eq!(hotkey_validation_message(""), Some(HOTKEY_HINT));
        assert_eq!(hotkey_validation_message("cmdorctrl+a"), None);
        assert_eq!(hotkey_validation_message(" meta + a "), None);
        assert_eq!(press_key_validation_message("Cmd+A"), Some(PRESS_KEY_HINT));
        assert_eq!(press_key_validation_message(""), Some(PRESS_KEY_HINT));
        assert_eq!(press_key_validation_message("+"), None);
        assert_eq!(press_key_validation_message(" + "), None);
        assert_eq!(
            click_modifiers_validation_message("A"),
            Some(CLICK_MODIFIERS_HINT)
        );
        assert_eq!(click_modifiers_validation_message("CmdOrCtrl+Shift"), None);
        assert_eq!(
            click_modifiers_validation_message("cmd_or_ctrl"),
            Some(CLICK_MODIFIERS_HINT)
        );
    }
}
