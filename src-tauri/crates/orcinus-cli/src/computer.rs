//! `orcinus computer *` (spec §3.6, plan task 12): the 14 computer-use
//! subcommands. Flag-level validation mirrors
//! `orca:src/cli/handlers/computer-action-flag-validation.ts` and
//! `computer-action-flags.ts` byte-for-byte; assembled params are
//! re-validated through `ade_computer::validation` (cheap double insurance)
//! before one RPC call. `--json` spills `result.screenshot.data` to a 0600
//! temp file with `path`/`dataOmitted`/`expiresAt`; human mode prints one
//! summary and never a nextSteps table (controller ruling).

use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ade_computer::validation::{
    click_modifiers_validation_message, hotkey_validation_message, press_key_validation_message,
    validate_computer_action,
};
use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::client::RuntimeClient;
use crate::errors::{success_envelope, CliError};

/// Screenshot spillover directory override (spec §3.6).
pub const SCREENSHOT_TMPDIR_ENV: &str = "ORCINUS_COMPUTER_SCREENSHOT_TMPDIR";
const SCREENSHOT_SUBDIR: &str = "orcinus-computer-use";
const SCREENSHOT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SCREENSHOT_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SCREENSHOT_CLEANUP_MARKER: &str = ".last-cleanup";

/// Target flags shared by observation and action commands.
#[derive(Debug, Args)]
pub struct TargetArgs {
    /// App selector: name, bundle id, or `pid:<n>`.
    #[arg(long)]
    app: Option<String>,
    /// Explicit session id (mutually exclusive with --worktree).
    #[arg(long)]
    session: Option<String>,
    /// Explicit worktree selector, passed through (mutually exclusive with --session).
    #[arg(long)]
    worktree: Option<String>,
    /// Window id (mutually exclusive with --window-index).
    #[arg(long = "window-id", allow_negative_numbers = true)]
    window_id: Option<String>,
    /// Zero-based window index (mutually exclusive with --window-id).
    #[arg(long = "window-index", allow_negative_numbers = true)]
    window_index: Option<String>,
    /// Restore the target window before observing or acting.
    #[arg(long = "restore-window")]
    restore_window: bool,
    /// Skip the screenshot in the result.
    #[arg(long = "no-screenshot")]
    no_screenshot: bool,
}

#[derive(Debug, Subcommand)]
pub enum ComputerCommand {
    /// Show computer-use provider capabilities.
    Capabilities {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// List running apps available to computer-use.
    ListApps {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Open computer-use permission setup.
    Permissions {
        /// Permission to open setup for: accessibility or screenshots.
        #[arg(long)]
        id: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// List windows for an app available to computer-use.
    ListWindows {
        /// App selector: name, bundle id, or `pid:<n>`.
        #[arg(long)]
        app: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Capture a compact accessibility snapshot of an app.
    GetAppState {
        #[command(flatten)]
        target: TargetArgs,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Click an app element or window coordinate, optionally with modifiers.
    Click {
        #[command(flatten)]
        target: TargetArgs,
        /// Zero-based element index from the last snapshot.
        #[arg(long = "element-index", allow_negative_numbers = true)]
        element_index: Option<String>,
        /// Window x coordinate (with --y).
        #[arg(long, allow_negative_numbers = true)]
        x: Option<String>,
        /// Window y coordinate (with --x).
        #[arg(long, allow_negative_numbers = true)]
        y: Option<String>,
        /// Click count; defaults to a single click.
        #[arg(long = "click-count", allow_negative_numbers = true)]
        click_count: Option<String>,
        /// Mouse button: left, right, or middle.
        #[arg(long = "mouse-button")]
        mouse_button: Option<String>,
        /// Modifier chord, e.g. CmdOrCtrl+Shift.
        #[arg(long)]
        modifiers: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Perform an advertised secondary accessibility action.
    PerformSecondaryAction {
        #[command(flatten)]
        target: TargetArgs,
        /// Zero-based element index from the last snapshot.
        #[arg(long = "element-index", allow_negative_numbers = true)]
        element_index: Option<String>,
        /// Accessibility action name, e.g. AXPress.
        #[arg(long)]
        action: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Scroll an app element or window coordinate.
    Scroll {
        #[command(flatten)]
        target: TargetArgs,
        /// Zero-based element index from the last snapshot.
        #[arg(long = "element-index", allow_negative_numbers = true)]
        element_index: Option<String>,
        /// Window x coordinate (with --y).
        #[arg(long, allow_negative_numbers = true)]
        x: Option<String>,
        /// Window y coordinate (with --x).
        #[arg(long, allow_negative_numbers = true)]
        y: Option<String>,
        /// Scroll direction: up, down, left, or right.
        #[arg(long)]
        direction: Option<String>,
        /// Number of pages to scroll.
        #[arg(long, allow_negative_numbers = true)]
        pages: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Drag between app elements or window coordinates.
    Drag {
        #[command(flatten)]
        target: TargetArgs,
        /// Source element index (with --to-element-index).
        #[arg(long = "from-element-index", allow_negative_numbers = true)]
        from_element_index: Option<String>,
        /// Destination element index (with --from-element-index).
        #[arg(long = "to-element-index", allow_negative_numbers = true)]
        to_element_index: Option<String>,
        /// Source x coordinate (with the other three coordinates).
        #[arg(long = "from-x", allow_negative_numbers = true)]
        from_x: Option<String>,
        /// Source y coordinate (with the other three coordinates).
        #[arg(long = "from-y", allow_negative_numbers = true)]
        from_y: Option<String>,
        /// Destination x coordinate (with the other three coordinates).
        #[arg(long = "to-x", allow_negative_numbers = true)]
        to_x: Option<String>,
        /// Destination y coordinate (with the other three coordinates).
        #[arg(long = "to-y", allow_negative_numbers = true)]
        to_y: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Type literal text at the current app focus.
    TypeText {
        #[command(flatten)]
        target: TargetArgs,
        /// Text to type.
        #[arg(long)]
        text: Option<String>,
        /// Read the text from stdin instead of --text.
        #[arg(long = "text-stdin")]
        text_stdin: bool,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Press a single key such as Return or Escape.
    PressKey {
        #[command(flatten)]
        target: TargetArgs,
        /// Key to press.
        #[arg(long)]
        key: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Press a platform-aware key combination.
    Hotkey {
        #[command(flatten)]
        target: TargetArgs,
        /// Key combination, e.g. CmdOrCtrl+A.
        #[arg(long)]
        key: Option<String>,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Paste exact text at the current app focus.
    PasteText {
        #[command(flatten)]
        target: TargetArgs,
        /// Text to paste.
        #[arg(long)]
        text: Option<String>,
        /// Read the text from stdin instead of --text.
        #[arg(long = "text-stdin")]
        text_stdin: bool,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Set the value of a settable app element.
    SetValue {
        #[command(flatten)]
        target: TargetArgs,
        /// Zero-based element index from the last snapshot.
        #[arg(long = "element-index", allow_negative_numbers = true)]
        element_index: Option<String>,
        /// Value to set; an empty string is valid.
        #[arg(long)]
        value: Option<String>,
        /// Read the value from stdin instead of --value.
        #[arg(long = "value-stdin")]
        value_stdin: bool,
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
}

impl ComputerCommand {
    /// Whether the command asked for the JSON envelope.
    pub fn json(&self) -> bool {
        match self {
            Self::Capabilities { json }
            | Self::ListApps { json }
            | Self::Permissions { json, .. }
            | Self::ListWindows { json, .. }
            | Self::GetAppState { json, .. }
            | Self::Click { json, .. }
            | Self::PerformSecondaryAction { json, .. }
            | Self::Scroll { json, .. }
            | Self::Drag { json, .. }
            | Self::TypeText { json, .. }
            | Self::PressKey { json, .. }
            | Self::Hotkey { json, .. }
            | Self::PasteText { json, .. }
            | Self::SetValue { json, .. } => *json,
        }
    }
}

/// Human-readable summary selector; the formatter reads the raw RPC result.
enum HumanView {
    Capabilities,
    ListApps,
    Permissions,
    ListWindows,
    GetAppState,
    Action,
}

impl HumanView {
    fn format(&self, result: &Value) -> String {
        match self {
            Self::Capabilities => format_capabilities(result),
            Self::ListApps => format_list_apps(result),
            Self::Permissions => format_permissions(result),
            Self::ListWindows => format_list_windows(result),
            Self::GetAppState => format_get_app_state(result),
            Self::Action => format_action(result),
        }
    }
}

struct Prepared {
    method: &'static str,
    params: Value,
    human: HumanView,
}

/// Run one `computer` subcommand: validate flags, assemble params, call the
/// runtime once and print the envelope (or the human summary).
pub fn run(command: ComputerCommand) -> Result<(), CliError> {
    let json_output = command.json();
    let prepared = prepare(command)?;

    let client = RuntimeClient::connect()?;
    let mut result = client.call(prepared.method, prepared.params)?;

    if json_output {
        let id = Uuid::new_v4().to_string();
        prepare_screenshot_json(&mut result, &id);
        println!("{}", success_envelope(&id, client.runtime_id(), result));
    } else {
        println!("{}", prepared.human.format(&result));
    }
    Ok(())
}

fn prepare(command: ComputerCommand) -> Result<Prepared, CliError> {
    match command {
        ComputerCommand::Capabilities { .. } => Ok(Prepared {
            method: "computer.capabilities",
            params: json!({}),
            human: HumanView::Capabilities,
        }),
        ComputerCommand::ListApps { .. } => Ok(Prepared {
            method: "computer.listApps",
            params: json!({}),
            human: HumanView::ListApps,
        }),
        ComputerCommand::Permissions { id, .. } => {
            let id = optional_string(id.as_deref());
            if let Some(id) = &id {
                if id != "accessibility" && id != "screenshots" {
                    return Err(invalid_argument(
                        "--id must be \"accessibility\" or \"screenshots\"",
                    ));
                }
            }
            let params = match id {
                Some(id) => json!({"id": id}),
                None => json!({}),
            };
            Ok(Prepared {
                method: "computer.permissions",
                params,
                human: HumanView::Permissions,
            })
        }
        ComputerCommand::ListWindows { app, .. } => {
            let app = require_string(app.as_deref(), "app")?;
            Ok(Prepared {
                method: "computer.listWindows",
                params: json!({"app": app}),
                human: HumanView::ListWindows,
            })
        }
        ComputerCommand::GetAppState { target, .. } => {
            let params = Value::Object(target_map(&target)?);
            validate_provider_params("getAppState", &params)?;
            Ok(Prepared {
                method: "computer.getAppState",
                params,
                human: HumanView::GetAppState,
            })
        }
        ComputerCommand::Click {
            target,
            element_index,
            x,
            y,
            click_count,
            mouse_button,
            modifiers,
            ..
        } => {
            let mut params = target_map(&target)?;
            let element_index =
                optional_non_negative_integer(element_index.as_deref(), "element-index")?;
            let x = optional_number(x.as_deref(), "x")?;
            let y = optional_number(y.as_deref(), "y")?;
            let click_count = optional_positive_integer(click_count.as_deref(), "click-count")?;
            let mouse_button = optional_string(mouse_button.as_deref());
            validate_element_or_coordinates("Click", element_index, x, y)?;
            if let Some(button) = &mouse_button {
                if !matches!(button.as_str(), "left" | "right" | "middle") {
                    return Err(invalid_argument(
                        "Unsupported --mouse-button; expected left, right, or middle",
                    ));
                }
            }
            if let Some(modifiers) = &modifiers {
                if let Some(message) = click_modifiers_validation_message(modifiers) {
                    return Err(invalid_argument(message));
                }
            }
            insert_integer(&mut params, "elementIndex", element_index);
            insert_number(&mut params, "x", x);
            insert_number(&mut params, "y", y);
            insert_integer(&mut params, "clickCount", click_count);
            if let Some(button) = mouse_button {
                params.insert("mouseButton".to_string(), json!(button));
            }
            if let Some(modifiers) = modifiers {
                params.insert("modifiers".to_string(), json!(modifiers));
            }
            let params = Value::Object(params);
            validate_provider_params("click", &params)?;
            Ok(Prepared {
                method: "computer.click",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::PerformSecondaryAction {
            target,
            element_index,
            action,
            ..
        } => {
            let mut params = target_map(&target)?;
            let element_index =
                require_non_negative_integer(element_index.as_deref(), "element-index")?;
            let action = require_string(action.as_deref(), "action")?;
            params.insert("elementIndex".to_string(), json!(element_index));
            params.insert("action".to_string(), json!(action));
            let params = Value::Object(params);
            validate_provider_params("performSecondaryAction", &params)?;
            Ok(Prepared {
                method: "computer.performSecondaryAction",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::Scroll {
            target,
            element_index,
            x,
            y,
            direction,
            pages,
            ..
        } => {
            let mut params = target_map(&target)?;
            let element_index =
                optional_non_negative_integer(element_index.as_deref(), "element-index")?;
            let x = optional_number(x.as_deref(), "x")?;
            let y = optional_number(y.as_deref(), "y")?;
            let direction = require_string(direction.as_deref(), "direction")?;
            let pages = optional_positive_number(pages.as_deref(), "pages")?;
            validate_element_or_coordinates("Scroll", element_index, x, y)?;
            if !matches!(direction.as_str(), "up" | "down" | "left" | "right") {
                return Err(invalid_argument(
                    "Unsupported --direction; expected up, down, left, or right",
                ));
            }
            insert_integer(&mut params, "elementIndex", element_index);
            insert_number(&mut params, "x", x);
            insert_number(&mut params, "y", y);
            params.insert("direction".to_string(), json!(direction));
            insert_number(&mut params, "pages", pages);
            let params = Value::Object(params);
            validate_provider_params("scroll", &params)?;
            Ok(Prepared {
                method: "computer.scroll",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::Drag {
            target,
            from_element_index,
            to_element_index,
            from_x,
            from_y,
            to_x,
            to_y,
            ..
        } => {
            let mut params = target_map(&target)?;
            let from_element_index =
                optional_non_negative_integer(from_element_index.as_deref(), "from-element-index")?;
            let to_element_index =
                optional_non_negative_integer(to_element_index.as_deref(), "to-element-index")?;
            let from_x = optional_number(from_x.as_deref(), "from-x")?;
            let from_y = optional_number(from_y.as_deref(), "from-y")?;
            let to_x = optional_number(to_x.as_deref(), "to-x")?;
            let to_y = optional_number(to_y.as_deref(), "to-y")?;
            validate_drag_target(
                from_element_index,
                to_element_index,
                from_x,
                from_y,
                to_x,
                to_y,
            )?;
            insert_integer(&mut params, "fromElementIndex", from_element_index);
            insert_integer(&mut params, "toElementIndex", to_element_index);
            insert_number(&mut params, "fromX", from_x);
            insert_number(&mut params, "fromY", from_y);
            insert_number(&mut params, "toX", to_x);
            insert_number(&mut params, "toY", to_y);
            let params = Value::Object(params);
            validate_provider_params("drag", &params)?;
            Ok(Prepared {
                method: "computer.drag",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::TypeText {
            target,
            text,
            text_stdin,
            ..
        } => {
            let mut params = target_map(&target)?;
            let text = text_payload(text.as_deref(), text_stdin, "text")?;
            params.insert("text".to_string(), json!(text));
            let params = Value::Object(params);
            validate_provider_params("typeText", &params)?;
            Ok(Prepared {
                method: "computer.typeText",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::PressKey { target, key, .. } => {
            let mut params = target_map(&target)?;
            let key = require_string(key.as_deref(), "key")?;
            if let Some(message) = press_key_validation_message(&key) {
                return Err(invalid_argument(message));
            }
            params.insert("key".to_string(), json!(key));
            let params = Value::Object(params);
            validate_provider_params("pressKey", &params)?;
            Ok(Prepared {
                method: "computer.pressKey",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::Hotkey { target, key, .. } => {
            let mut params = target_map(&target)?;
            let key = require_string(key.as_deref(), "key")?;
            if let Some(message) = hotkey_validation_message(&key) {
                return Err(invalid_argument(message));
            }
            params.insert("key".to_string(), json!(key));
            let params = Value::Object(params);
            validate_provider_params("hotkey", &params)?;
            Ok(Prepared {
                method: "computer.hotkey",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::PasteText {
            target,
            text,
            text_stdin,
            ..
        } => {
            let mut params = target_map(&target)?;
            let text = text_payload(text.as_deref(), text_stdin, "text")?;
            params.insert("text".to_string(), json!(text));
            let params = Value::Object(params);
            validate_provider_params("pasteText", &params)?;
            Ok(Prepared {
                method: "computer.pasteText",
                params,
                human: HumanView::Action,
            })
        }
        ComputerCommand::SetValue {
            target,
            element_index,
            value,
            value_stdin,
            ..
        } => {
            let mut params = target_map(&target)?;
            let element_index =
                require_non_negative_integer(element_index.as_deref(), "element-index")?;
            let value = text_payload(value.as_deref(), value_stdin, "value")?;
            params.insert("elementIndex".to_string(), json!(element_index));
            params.insert("value".to_string(), json!(value));
            let params = Value::Object(params);
            validate_provider_params("setValue", &params)?;
            Ok(Prepared {
                method: "computer.setValue",
                params,
                human: HumanView::Action,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// flag helpers (oracle messages verbatim)

fn invalid_argument(message: impl Into<String>) -> CliError {
    CliError::new("invalid_argument", message)
}

fn optional_string(value: Option<&str>) -> Option<String> {
    value
        .filter(|raw| !raw.is_empty())
        .map(|raw| raw.to_string())
}

fn require_string(value: Option<&str>, name: &str) -> Result<String, CliError> {
    match value {
        Some(raw) if !raw.is_empty() => Ok(raw.to_string()),
        _ => Err(invalid_argument(format!("Missing required --{name}"))),
    }
}

fn require_string_allowing_empty(value: Option<&str>, name: &str) -> Result<String, CliError> {
    match value {
        Some(raw) => Ok(raw.to_string()),
        None => Err(invalid_argument(format!("Missing required --{name}"))),
    }
}

fn optional_number(value: Option<&str>, name: &str) -> Result<Option<f64>, CliError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed = raw
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite());
    match parsed {
        Some(number) => Ok(Some(number)),
        None => Err(invalid_argument(format!(
            "Invalid numeric value for --{name}"
        ))),
    }
}

fn optional_non_negative_integer(value: Option<&str>, name: &str) -> Result<Option<i64>, CliError> {
    let Some(number) = optional_number(value, name)? else {
        return Ok(None);
    };
    if number.fract() == 0.0 && number >= 0.0 {
        Ok(Some(number as i64))
    } else {
        Err(invalid_argument(format!(
            "Invalid non-negative integer for --{name}"
        )))
    }
}

fn require_non_negative_integer(value: Option<&str>, name: &str) -> Result<i64, CliError> {
    match optional_non_negative_integer(value, name)? {
        Some(number) => Ok(number),
        None => Err(invalid_argument(format!("Missing required --{name}"))),
    }
}

fn optional_positive_integer(value: Option<&str>, name: &str) -> Result<Option<i64>, CliError> {
    let Some(number) = optional_number(value, name)? else {
        return Ok(None);
    };
    if number.fract() == 0.0 && number > 0.0 {
        Ok(Some(number as i64))
    } else {
        Err(invalid_argument(format!(
            "Invalid positive integer for --{name}"
        )))
    }
}

fn optional_positive_number(value: Option<&str>, name: &str) -> Result<Option<f64>, CliError> {
    let Some(number) = optional_number(value, name)? else {
        return Ok(None);
    };
    if number > 0.0 {
        Ok(Some(number))
    } else {
        Err(invalid_argument(format!(
            "Invalid positive number for --{name}"
        )))
    }
}

fn number_value(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_992.0 {
        json!(number as i64)
    } else {
        serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn insert_integer(params: &mut Map<String, Value>, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        params.insert(key.to_string(), json!(value));
    }
}

fn insert_number(params: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        params.insert(key.to_string(), number_value(value));
    }
}

fn target_map(target: &TargetArgs) -> Result<Map<String, Value>, CliError> {
    let app = require_string(target.app.as_deref(), "app")?;
    let session = optional_string(target.session.as_deref());
    let worktree = optional_string(target.worktree.as_deref());
    if session.is_some() && worktree.is_some() {
        return Err(invalid_argument(
            "Computer-use targeting accepts either --session or --worktree, not both",
        ));
    }
    let window_id = optional_non_negative_integer(target.window_id.as_deref(), "window-id")?;
    let window_index =
        optional_non_negative_integer(target.window_index.as_deref(), "window-index")?;
    validate_exclusive_window_target(window_id, window_index)?;

    let mut params = Map::new();
    params.insert("app".to_string(), json!(app));
    if let Some(session) = session {
        params.insert("session".to_string(), json!(session));
    }
    if let Some(worktree) = worktree {
        params.insert("worktree".to_string(), json!(worktree));
    }
    insert_integer(&mut params, "windowId", window_id);
    insert_integer(&mut params, "windowIndex", window_index);
    if target.restore_window {
        params.insert("restoreWindow".to_string(), json!(true));
    }
    if target.no_screenshot {
        params.insert("noScreenshot".to_string(), json!(true));
    }
    Ok(params)
}

fn validate_exclusive_window_target(
    window_id: Option<i64>,
    window_index: Option<i64>,
) -> Result<(), CliError> {
    if window_id.is_some() && window_index.is_some() {
        return Err(invalid_argument(
            "Window targeting accepts either --window-id or --window-index, not both",
        ));
    }
    Ok(())
}

fn validate_element_or_coordinates(
    action_name: &str,
    element_index: Option<i64>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<(), CliError> {
    let has_element = element_index.is_some();
    let has_x = x.is_some();
    let has_y = y.is_some();
    if !has_element && !(has_x && has_y) {
        return Err(invalid_argument(format!(
            "{action_name} requires --element-index or both --x and --y"
        )));
    }
    if has_x != has_y {
        return Err(invalid_argument(format!(
            "{action_name} coordinates require both --x and --y"
        )));
    }
    if has_element && (has_x || has_y) {
        return Err(invalid_argument(format!(
            "{action_name} accepts either --element-index or coordinate flags, not both"
        )));
    }
    Ok(())
}

fn validate_drag_target(
    from_element_index: Option<i64>,
    to_element_index: Option<i64>,
    from_x: Option<f64>,
    from_y: Option<f64>,
    to_x: Option<f64>,
    to_y: Option<f64>,
) -> Result<(), CliError> {
    let has_element_pair = from_element_index.is_some() && to_element_index.is_some();
    let has_partial_element_pair = from_element_index.is_some() || to_element_index.is_some();
    let coordinates = [from_x, from_y, to_x, to_y];
    let has_coordinate_pair = coordinates.iter().all(Option::is_some);
    let has_partial_coordinate_pair = coordinates.iter().any(Option::is_some);
    if has_element_pair && has_coordinate_pair {
        return Err(invalid_argument(
            "Drag accepts either element indexes or coordinate flags, not both",
        ));
    }
    if has_partial_element_pair && !has_element_pair {
        return Err(invalid_argument(
            "Drag element targeting requires both --from-element-index and --to-element-index",
        ));
    }
    if has_partial_coordinate_pair && !has_coordinate_pair {
        return Err(invalid_argument(
            "Drag coordinates require --from-x, --from-y, --to-x, and --to-y",
        ));
    }
    if !has_element_pair && !has_coordinate_pair {
        return Err(invalid_argument(
            "Drag requires --from-element-index and --to-element-index, or all coordinate flags",
        ));
    }
    Ok(())
}

fn text_payload(text: Option<&str>, stdin_flag: bool, name: &str) -> Result<String, CliError> {
    if stdin_flag {
        if text.is_some() {
            return Err(invalid_argument(format!(
                "Use either --{name} or --{name}-stdin, not both"
            )));
        }
        let payload = read_stdin_text(is_stdin_tty())?;
        if name == "text" && payload.is_empty() {
            return Err(invalid_argument("Missing text from stdin"));
        }
        return Ok(payload);
    }
    if name == "value" {
        require_string_allowing_empty(text, name)
    } else {
        require_string(text, name)
    }
}

fn is_stdin_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn read_stdin_text(is_tty: bool) -> Result<String, CliError> {
    if is_tty {
        return Err(invalid_argument(
            "stdin payload requested but stdin is a TTY",
        ));
    }
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .map_err(|error| {
            CliError::new("runtime_error", format!("Could not read stdin: {error}"))
        })?;
    Ok(payload)
}

/// Double validation through the provider contract (spec §3.2); the CLI text
/// above fires first, this catches anything the flag layer missed.
fn validate_provider_params(method: &str, params: &Value) -> Result<(), CliError> {
    validate_computer_action(method, params).map_err(|error| {
        let mut cli_error = CliError::new(error.code.as_str(), error.message);
        cli_error.data = error.data;
        cli_error
    })
}

// ---------------------------------------------------------------------------
// screenshot spillover (spec §3.6, port of `computer-format.ts`)

/// Replace `result.screenshot.data` with `path`/`dataOmitted`/`expiresAt`
/// after writing the decoded bytes to a 0600 temp file. Any failure keeps the
/// inline payload untouched.
fn prepare_screenshot_json(result: &mut Value, request_id: &str) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    if !object.contains_key("screenshotStatus") {
        return;
    }
    let Some(data) = object
        .get("screenshot")
        .and_then(Value::as_object)
        .and_then(|screenshot| screenshot.get("data"))
        .and_then(Value::as_str)
        .filter(|data| !data.is_empty())
        .map(|data| data.to_string())
    else {
        return;
    };
    let extension = match object
        .get("screenshot")
        .and_then(|screenshot| screenshot.get("format"))
        .and_then(Value::as_str)
    {
        Some("png") => "png",
        _ => "img",
    };
    let file_id = object
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .unwrap_or(request_id)
        .to_string();

    let Some(directory) = screenshot_temp_dir() else {
        return;
    };
    cleanup_stale_screenshots(&directory);
    let Some(bytes) = decode_base64(&data) else {
        return;
    };
    let path = directory.join(format!(
        "{}-screenshot.{extension}",
        safe_file_stem(&file_id)
    ));
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
    else {
        return;
    };
    if file.write_all(&bytes).is_err() {
        let _ = std::fs::remove_file(&path);
        return;
    }

    let expires_at = rfc3339_millis(now_epoch_millis() + SCREENSHOT_TTL.as_millis());
    let Some(screenshot) = object.get_mut("screenshot").and_then(Value::as_object_mut) else {
        return;
    };
    screenshot.remove("data");
    screenshot.insert("path".to_string(), json!(path.to_string_lossy()));
    screenshot.insert("dataOmitted".to_string(), json!(true));
    screenshot.insert("expiresAt".to_string(), json!(expires_at));
}

fn screenshot_temp_dir() -> Option<PathBuf> {
    let directory = match std::env::var(SCREENSHOT_TMPDIR_ENV) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => std::env::temp_dir().join(SCREENSHOT_SUBDIR),
    };
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(&directory).ok()?;
    let metadata = std::fs::symlink_metadata(&directory).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(directory)
}

fn cleanup_stale_screenshots(directory: &Path) {
    let marker = directory.join(SCREENSHOT_CLEANUP_MARKER);
    let now = SystemTime::now();
    if let Ok(metadata) = std::fs::metadata(&marker) {
        if let Ok(modified) = metadata.modified() {
            if now
                .duration_since(modified)
                .map(|age| age < SCREENSHOT_CLEANUP_INTERVAL)
                .unwrap_or(true)
            {
                return;
            }
        }
    }

    if let Some(cutoff) = now.checked_sub(SCREENSHOT_TTL) {
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.ends_with("-screenshot.png") && !name.ends_with("-screenshot.img") {
                    continue;
                }
                let stale = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .map(|modified| modified < cutoff)
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&marker)
    {
        let _ = file.write_all(now_epoch_millis().to_string().as_bytes());
    }
}

fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        buffer = (buffer << 6) | u32::from(base64_value(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }
    Some(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

/// RFC3339 UTC with millisecond precision (oracle `toISOString` shape).
fn rfc3339_millis(epoch_millis: u128) -> String {
    let seconds = (epoch_millis / 1_000) as i64;
    let millis = (epoch_millis % 1_000) as u32;
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch to y/m/d.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---------------------------------------------------------------------------
// human summaries

fn format_capabilities(value: &Value) -> String {
    let supports = &value["supports"];
    let actions = supports["actions"]
        .as_object()
        .map(|actions| {
            actions
                .iter()
                .filter(|(_, enabled)| enabled.as_bool() == Some(true))
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    [
        format!(
            "{} ({}, protocol {})",
            value["provider"].as_str().unwrap_or("unknown"),
            value["platform"].as_str().unwrap_or("unknown"),
            value["protocolVersion"]
                .as_u64()
                .map(|version| version.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        format!(
            "  Apps: list={} bundleIds={} pids={}",
            flag(&supports["apps"]["list"]),
            flag(&supports["apps"]["bundleIds"]),
            flag(&supports["apps"]["pids"])
        ),
        format!(
            "  Windows: list={} targetById={} targetByIndex={}",
            flag(&supports["windows"]["list"]),
            flag(&supports["windows"]["targetById"]),
            flag(&supports["windows"]["targetByIndex"])
        ),
        format!(
            "  Observation: screenshot={} elementFrames={} annotatedScreenshot={}",
            flag(&supports["observation"]["screenshot"]),
            flag(&supports["observation"]["elementFrames"]),
            flag(&supports["observation"]["annotatedScreenshot"])
        ),
        format!("  Actions: {actions}"),
    ]
    .join("\n")
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap_or(false)
}

fn format_list_apps(value: &Value) -> String {
    let Some(apps) = value["apps"].as_array() else {
        return "No apps found.".to_string();
    };
    if apps.is_empty() {
        return "No apps found.".to_string();
    }
    apps.iter()
        .map(|app| {
            let bundle = app["bundleId"]
                .as_str()
                .map(|bundle| format!("  {bundle}"))
                .unwrap_or_default();
            format!(
                "{}  pid:{}{bundle}",
                app["name"].as_str().unwrap_or("unknown"),
                app["pid"]
                    .as_u64()
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "?".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_list_windows(value: &Value) -> String {
    let windows = value["windows"].as_array();
    let Some(windows) = windows else {
        return "No windows found for unknown.".to_string();
    };
    if windows.is_empty() {
        return format!(
            "No windows found for {}.",
            value["app"]["name"].as_str().unwrap_or("unknown")
        );
    }
    windows
        .iter()
        .map(|window| {
            let id = window["id"]
                .as_u64()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_string());
            let origin = match (window["x"].as_i64(), window["y"].as_i64()) {
                (Some(x), Some(y)) => format!(" @ {x},{y}"),
                _ => String::new(),
            };
            let screen = window["screenIndex"]
                .as_u64()
                .map(|index| format!(" screen:{index}"))
                .unwrap_or_default();
            let mut state: Vec<&str> = Vec::new();
            if window["isMinimized"].as_bool() == Some(true) {
                state.push("minimized");
            }
            if window["isOffscreen"].as_bool() == Some(true) {
                state.push("offscreen");
            }
            let state_text = if state.is_empty() {
                String::new()
            } else {
                format!(" {}", state.join(","))
            };
            format!(
                "[{}] id:{id} \"{}\" ({}x{}{origin}){screen}{state_text}",
                window["index"].as_u64().unwrap_or(0),
                window["title"].as_str().unwrap_or(""),
                window["width"].as_u64().unwrap_or(0),
                window["height"].as_u64().unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_permissions(value: &Value) -> String {
    if value["platform"].as_str() != Some("darwin") {
        return "Computer-use permission setup is only required on macOS.".to_string();
    }
    let permissions = value["permissions"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    format!(
                        "{}={}",
                        entry["id"].as_str().unwrap_or("unknown"),
                        entry["status"].as_str().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "unknown".to_string());
    let mut lines = vec![
        if value["launchedHelper"].as_bool().unwrap_or(false) {
            "Opened Orcinus Computer Use permission setup.".to_string()
        } else {
            "Computer Use permissions checked.".to_string()
        },
        format!(
            "Helper app: {}",
            value["helperAppPath"].as_str().unwrap_or("unavailable")
        ),
        format!("Permissions: {permissions}"),
    ];
    match value["nextStep"].as_str() {
        Some(next_step) if !next_step.is_empty() => lines.push(format!("Next: {next_step}")),
        _ => lines.push("Computer Use permissions are already granted.".to_string()),
    }
    lines.join("\n")
}

fn format_get_app_state(value: &Value) -> String {
    let snapshot = &value["snapshot"];
    let app = &snapshot["app"];
    let bundle = app["bundleId"]
        .as_str()
        .map(|bundle| format!(", {bundle}"))
        .unwrap_or_default();
    let window = &snapshot["window"];
    let window_id = window["id"]
        .as_u64()
        .map(|id| format!(" id:{id}"))
        .unwrap_or_default();
    let window_index = window["index"]
        .as_u64()
        .map(|index| format!(" index:{index}"))
        .unwrap_or_default();
    let origin = match (window["x"].as_i64(), window["y"].as_i64()) {
        (Some(x), Some(y)) => format!(" @ {x},{y}"),
        _ => String::new(),
    };
    format!(
        "{} (pid {}{bundle})\n  Window:{window_id}{window_index} \"{}\" ({}x{}{origin})\n  Visible elements: {}\n{}",
        app["name"].as_str().unwrap_or("unknown"),
        app["pid"]
            .as_u64()
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "?".to_string()),
        window["title"].as_str().unwrap_or(""),
        window["width"].as_u64().unwrap_or(0),
        window["height"].as_u64().unwrap_or(0),
        snapshot["elementCount"].as_u64().unwrap_or(0),
        snapshot["treeText"].as_str().unwrap_or("")
    )
}

fn format_action(value: &Value) -> String {
    let action = &value["action"];
    format!(
        "action={} verification={}",
        action["path"].as_str().unwrap_or("unknown"),
        action["verification"]["state"]
            .as_str()
            .unwrap_or("unverified")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_payloads_requested_on_a_tty_are_rejected() {
        let error = read_stdin_text(true).expect_err("TTY stdin must be rejected");
        assert_eq!(error.code, "invalid_argument");
        assert_eq!(error.message, "stdin payload requested but stdin is a TTY");
    }

    #[test]
    fn text_payload_rejects_text_and_stdin_together() {
        let error =
            text_payload(Some("hi"), true, "text").expect_err("both payload flags must conflict");
        assert_eq!(error.code, "invalid_argument");
        assert_eq!(error.message, "Use either --text or --text-stdin, not both");
    }

    #[test]
    fn base64_decoder_handles_standard_payloads() {
        assert_eq!(
            decode_base64("aGVsbG8=").as_deref(),
            Some(b"hello".as_slice())
        );
        assert_eq!(decode_base64("").as_deref(), Some(b"".as_slice()));
        assert_eq!(decode_base64("cG5n").as_deref(), Some(b"png".as_slice()));
        assert!(decode_base64("!!!").is_none());
    }

    #[test]
    fn rfc3339_formats_epoch_millis_in_utc() {
        assert_eq!(rfc3339_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339_millis(1_000), "1970-01-01T00:00:01.000Z");
        assert_eq!(
            rfc3339_millis(1_700_000_000_000),
            "2023-11-14T22:13:20.000Z"
        );
    }

    #[test]
    fn safe_file_stem_replaces_unsafe_characters() {
        assert_eq!(safe_file_stem("snap/42"), "snap_42");
        assert_eq!(safe_file_stem("snap-42.png"), "snap-42.png");
    }

    #[test]
    fn target_map_reports_missing_app_and_exclusive_targets() {
        let missing_app = TargetArgs {
            app: None,
            session: None,
            worktree: None,
            window_id: None,
            window_index: None,
            restore_window: false,
            no_screenshot: false,
        };
        assert_eq!(
            target_map(&missing_app).unwrap_err().message,
            "Missing required --app"
        );

        let exclusive = TargetArgs {
            app: Some("X".to_string()),
            session: Some("a".to_string()),
            worktree: Some("b".to_string()),
            window_id: None,
            window_index: None,
            restore_window: false,
            no_screenshot: false,
        };
        assert_eq!(
            target_map(&exclusive).unwrap_err().message,
            "Computer-use targeting accepts either --session or --worktree, not both"
        );
    }
}
