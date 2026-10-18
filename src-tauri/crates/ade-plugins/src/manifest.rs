//! Plugin manifest v1 parsing and validation.
//!
//! Field rules, cross-checks, and error wording are ported from
//! `orca:src/shared/plugins/plugin-manifest.ts`,
//! `plugin-manifest-fields.ts`, `plugin-id-format.ts`,
//! `plugin-path-safety.ts`, `plugin-manifest-contribution-validation.ts`, and
//! `plugin-content-pack-contributions.ts`. The fork adds `contributes.tabs`
//! (same shape as panels, limit 64).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::types::{
    is_safe_plugin_id, is_safe_plugin_relative_path, Capability, PluginError, INVALID_MANIFEST,
};

pub const MANIFEST_FILENAME: &str = "ade-plugin.json";
pub const LEGACY_MANIFEST_FILENAME: &str = "orca-plugin.json";
/// Enforced by the reading layer, not `parse_manifest`.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

pub const PLUGIN_PANEL_LIMIT: usize = 64;
pub const PLUGIN_TAB_LIMIT: usize = 64;
pub const PLUGIN_COMMAND_LIMIT: usize = 256;
pub const PLUGIN_EVENT_NAMES: [&str; 3] = [
    "worktree.created",
    "worktree.removed",
    "agent.status.changed",
];
pub const PLUGIN_EVENT_SUBSCRIPTION_LIMIT: usize = PLUGIN_EVENT_NAMES.len();
pub const PLUGIN_LANGUAGE_PACK_LIMIT: usize = 16;
pub const PLUGIN_KEYBINDING_LIMIT: usize = 256;
pub const PLUGIN_VM_RECIPE_LIMIT: usize = 64;
pub const PLUGIN_AGENT_PROFILE_LIMIT: usize = 64;
pub const PLUGIN_CAPABILITY_LIMIT: usize = 32;

/// Declarative aliases with a renderer-owned command handler.
pub const PLUGIN_COMMAND_ALIAS_ACTION_IDS: [&str; 15] = [
    "worktree.history.back",
    "worktree.history.forward",
    "sidebar.left.toggle",
    "sidebar.sleepingWorkspaces.toggle",
    "floatingWorkspace.maximize",
    "tab.rename",
    "workspace.rename",
    "workspace.openBoard",
    "view.tasks",
    "sidebar.right.toggle",
    "sidebar.explorer.toggle",
    "sidebar.search.toggle",
    "sidebar.sourceControl.toggle",
    "sidebar.checks.toggle",
    "sidebar.ports.toggle",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "manifestVersion")]
    pub manifest_version: u8,
    pub id: String,
    pub publisher: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub engines: Engines,
    #[serde(rename = "pluginApi")]
    pub plugin_api: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

impl Manifest {
    /// Canonical install identity: `<publisher>.<id>`.
    pub fn qualified_key(&self) -> String {
        format!("{}.{}", self.publisher, self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engines {
    pub orca: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Contributes {
    #[serde(default)]
    pub panels: Vec<PanelContrib>,
    #[serde(default)]
    pub tabs: Vec<TabContrib>,
    #[serde(default)]
    pub commands: Vec<CommandContrib>,
    #[serde(default)]
    pub events: Vec<EventContrib>,
    #[serde(default, rename = "languagePacks")]
    pub language_packs: Vec<LanguagePackContrib>,
    #[serde(default)]
    pub keybindings: Vec<KeybindingContrib>,
    #[serde(default, rename = "vmRecipes")]
    pub vm_recipes: Vec<VmRecipeContrib>,
    #[serde(default)]
    pub agents: Vec<AgentContrib>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelContrib {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub entry: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabContrib {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub entry: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandVisibility {
    #[default]
    Palette,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandContrib {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CommandContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default)]
    pub visibility: CommandVisibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandContext {
    Global,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContrib {
    pub on: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguagePackContrib {
    pub locale: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingContrib {
    pub command: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<CommandContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmRecipeContrib {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContrib {
    pub path: String,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, PluginError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| invalid(format!("(root): {error}")))?;
    reject_nulls(&value, "(root)")?;
    check_container_shapes(&value)?;
    reject_unknown_contributes_keys(&value)?;
    let manifest: Manifest =
        serde_json::from_value(value).map_err(|error| invalid(format!("(root): {error}")))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// v1 engines gate: the `>=x.y.z` grammar only; prerelease/build suffixes on
/// the host version are ignored for ordering.
pub fn engine_satisfies(range: &str, version: &str) -> bool {
    if !is_engine_range(range) {
        return false;
    }
    let host = parse_version_parts(version);
    let minimum = parse_version_parts(&range[2..]);
    for index in 0..3 {
        if host[index] != minimum[index] {
            return host[index] > minimum[index];
        }
    }
    true
}

fn validate_manifest(manifest: &Manifest) -> Result<(), PluginError> {
    if manifest.manifest_version != 1 {
        return Err(invalid("manifestVersion: must be 1"));
    }
    validate_plugin_id("id", &manifest.id)?;
    validate_plugin_id("publisher", &manifest.publisher)?;
    check_str_range("name", &manifest.name, 1, 256)?;
    if !is_semver(&manifest.version) {
        return Err(invalid("version: must be semver"));
    }
    if let Some(description) = &manifest.description {
        check_str_range("description", description, 0, 4096)?;
    }
    if let Some(author) = &manifest.author {
        check_str_range("author.name", &author.name, 1, 256)?;
        if let Some(url) = &author.url {
            check_str_range("author.url", url, 0, 2048)?;
        }
    }
    if let Some(repository) = &manifest.repository {
        check_str_range("repository", repository, 0, 2048)?;
    }
    if let Some(icon) = &manifest.icon {
        validate_relative_path("icon", icon)?;
    }
    let engine_range = &manifest.engines.orca;
    if js_len(engine_range) > 64 {
        return Err(invalid(
            "engines.orca: String must contain at most 64 character(s)",
        ));
    }
    if !is_engine_range(engine_range) {
        return Err(invalid("engines.orca: must be a \">=x.y.z\" version range"));
    }
    if manifest.plugin_api != 1 {
        return Err(invalid("pluginApi: must be 1"));
    }
    if let Some(main) = &manifest.main {
        validate_relative_path("main", main)?;
    }
    validate_contributes(&manifest.contributes)?;
    check_limit(
        "capabilities",
        manifest.capabilities.len(),
        PLUGIN_CAPABILITY_LIMIT,
    )?;
    validate_contribution_cross_checks(manifest)?;
    Ok(())
}

fn validate_contributes(contributes: &Contributes) -> Result<(), PluginError> {
    check_limit(
        "contributes.panels",
        contributes.panels.len(),
        PLUGIN_PANEL_LIMIT,
    )?;
    for (index, panel) in contributes.panels.iter().enumerate() {
        validate_panel_like(
            &format!("contributes.panels.{index}"),
            &panel.id,
            &panel.title,
            panel.icon.as_deref(),
            &panel.entry,
        )?;
    }

    check_limit("contributes.tabs", contributes.tabs.len(), PLUGIN_TAB_LIMIT)?;
    for (index, tab) in contributes.tabs.iter().enumerate() {
        validate_panel_like(
            &format!("contributes.tabs.{index}"),
            &tab.id,
            &tab.title,
            tab.icon.as_deref(),
            &tab.entry,
        )?;
    }

    check_limit(
        "contributes.commands",
        contributes.commands.len(),
        PLUGIN_COMMAND_LIMIT,
    )?;
    for (index, command) in contributes.commands.iter().enumerate() {
        validate_command_id(&format!("contributes.commands.{index}.id"), &command.id)?;
        check_str_range(
            &format!("contributes.commands.{index}.title"),
            &command.title,
            1,
            256,
        )?;
        if let Some(action) = &command.action {
            validate_command_id(&format!("contributes.commands.{index}.action"), action)?;
        }
        if command.visibility == CommandVisibility::Internal && command.action.is_some() {
            return Err(invalid(format!(
                "contributes.commands.{index}: internal commands must not declare an action alias"
            )));
        }
    }

    check_limit(
        "contributes.events",
        contributes.events.len(),
        PLUGIN_EVENT_SUBSCRIPTION_LIMIT,
    )?;
    for (index, event) in contributes.events.iter().enumerate() {
        if !PLUGIN_EVENT_NAMES.contains(&event.on.as_str()) {
            return Err(invalid(format!(
                "contributes.events.{index}.on: Invalid enum value. Expected 'worktree.created' | 'worktree.removed' | 'agent.status.changed', received '{}'",
                event.on
            )));
        }
    }

    check_limit(
        "contributes.languagePacks",
        contributes.language_packs.len(),
        PLUGIN_LANGUAGE_PACK_LIMIT,
    )?;
    for (index, pack) in contributes.language_packs.iter().enumerate() {
        let locale = &pack.locale;
        if !(2..=35).contains(&js_len(locale)) || !is_locale_id(locale) {
            return Err(invalid(format!(
                "contributes.languagePacks.{index}.locale: must be a portable locale identifier"
            )));
        }
        validate_relative_path(
            &format!("contributes.languagePacks.{index}.path"),
            &pack.path,
        )?;
    }

    check_limit(
        "contributes.keybindings",
        contributes.keybindings.len(),
        PLUGIN_KEYBINDING_LIMIT,
    )?;
    for (index, keybinding) in contributes.keybindings.iter().enumerate() {
        validate_command_id(
            &format!("contributes.keybindings.{index}.command"),
            &keybinding.command,
        )?;
        check_str_range(
            &format!("contributes.keybindings.{index}.key"),
            &keybinding.key,
            1,
            128,
        )?;
    }

    check_limit(
        "contributes.vmRecipes",
        contributes.vm_recipes.len(),
        PLUGIN_VM_RECIPE_LIMIT,
    )?;
    for (index, recipe) in contributes.vm_recipes.iter().enumerate() {
        validate_relative_path(&format!("contributes.vmRecipes.{index}.path"), &recipe.path)?;
    }

    check_limit(
        "contributes.agents",
        contributes.agents.len(),
        PLUGIN_AGENT_PROFILE_LIMIT,
    )?;
    for (index, agent) in contributes.agents.iter().enumerate() {
        validate_relative_path(&format!("contributes.agents.{index}.path"), &agent.path)?;
    }
    Ok(())
}

fn validate_contribution_cross_checks(manifest: &Manifest) -> Result<(), PluginError> {
    let contributes = &manifest.contributes;
    reject_duplicate_values(&contributes.panels, "panels", "panels id", |panel| {
        panel.id.clone()
    })?;
    reject_duplicate_values(&contributes.tabs, "tabs", "tabs id", |tab| tab.id.clone())?;
    reject_duplicate_values(
        &contributes.commands,
        "commands",
        "commands id",
        |command| command.id.clone(),
    )?;
    reject_duplicate_values(
        &contributes.language_packs,
        "languagePacks",
        "language pack locale",
        |pack| pack.locale.to_lowercase(),
    )?;
    reject_duplicate_values(
        &contributes.vm_recipes,
        "vmRecipes",
        "vmRecipes path",
        |recipe| recipe.path.clone(),
    )?;
    reject_duplicate_values(&contributes.agents, "agents", "agents path", |agent| {
        agent.path.clone()
    })?;

    for (index, command) in contributes.commands.iter().enumerate() {
        if let Some(action) = &command.action {
            if !PLUGIN_COMMAND_ALIAS_ACTION_IDS.contains(&action.as_str()) {
                return Err(invalid(format!(
                    "contributes.commands.{index}.action: unknown built-in action: {action}"
                )));
            }
        }
    }

    for (index, keybinding) in contributes.keybindings.iter().enumerate() {
        let Some(command) = contributes
            .commands
            .iter()
            .find(|command| command.id == keybinding.command)
        else {
            return Err(invalid(format!(
                "contributes.keybindings.{index}.command: unknown contributed command: {}",
                keybinding.command
            )));
        };
        let command_context = command.context.unwrap_or(CommandContext::Global);
        if command.visibility == CommandVisibility::Internal {
            return Err(invalid(format!(
                "contributes.keybindings.{index}.command: internal commands cannot be bound"
            )));
        }
        if let Some(when) = keybinding.when {
            if when != command_context {
                return Err(invalid(format!(
                    "contributes.keybindings.{index}.when: keybinding context must match its command context"
                )));
            }
        }
    }

    if manifest.main.is_none()
        && contributes
            .commands
            .iter()
            .any(|command| command.action.is_none())
    {
        return Err(invalid(
            "main: required when contributes.commands contains a worker command",
        ));
    }
    if manifest.main.is_none() && !contributes.events.is_empty() {
        return Err(invalid(
            "main: required when contributes.events is non-empty",
        ));
    }
    if !contributes.events.is_empty()
        && !manifest.capabilities.contains(&Capability::EventsSubscribe)
    {
        return Err(invalid(
            "capabilities: events:subscribe capability required when contributes.events is non-empty",
        ));
    }
    Ok(())
}

fn validate_panel_like(
    path: &str,
    id: &str,
    title: &str,
    icon: Option<&str>,
    entry: &str,
) -> Result<(), PluginError> {
    validate_plugin_id(&format!("{path}.id"), id)?;
    check_str_range(&format!("{path}.title"), title, 1, 256)?;
    if let Some(icon) = icon {
        check_str_range(&format!("{path}.icon"), icon, 1, 64)?;
    }
    validate_relative_path(&format!("{path}.entry"), entry)
}

fn validate_plugin_id(path: &str, id: &str) -> Result<(), PluginError> {
    if !is_safe_plugin_id(id) {
        return Err(invalid(format!(
            "{path}: must be kebab-case (a-z, 0-9, dashes) and not a reserved name"
        )));
    }
    Ok(())
}

fn validate_command_id(path: &str, id: &str) -> Result<(), PluginError> {
    check_str_range(path, id, 1, 256)?;
    if !is_command_id(id) {
        return Err(invalid(format!("{path}: must be a portable command id")));
    }
    Ok(())
}

fn validate_relative_path(path: &str, value: &str) -> Result<(), PluginError> {
    check_str_range(path, value, 1, 1024)?;
    if !is_safe_plugin_relative_path(value) {
        return Err(invalid(format!(
            "{path}: must be a portable relative path inside the plugin directory"
        )));
    }
    Ok(())
}

fn check_limit(path: &str, count: usize, limit: usize) -> Result<(), PluginError> {
    if count > limit {
        return Err(invalid(format!(
            "{path}: Array must contain at most {limit} element(s)"
        )));
    }
    Ok(())
}

fn check_str_range(path: &str, value: &str, min: usize, max: usize) -> Result<(), PluginError> {
    let length = js_len(value);
    if length < min {
        return Err(invalid(format!(
            "{path}: String must contain at least {min} character(s)"
        )));
    }
    if length > max {
        return Err(invalid(format!(
            "{path}: String must contain at most {max} character(s)"
        )));
    }
    Ok(())
}

fn reject_duplicate_values<T>(
    entries: &[T],
    path: &str,
    label: &str,
    value_of: impl Fn(&T) -> String,
) -> Result<(), PluginError> {
    let mut seen = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let value = value_of(entry);
        if !seen.insert(value.clone()) {
            return Err(invalid(format!(
                "contributes.{path}.{index}: duplicate {label}: {value}"
            )));
        }
    }
    Ok(())
}

/// Serde structs also deserialize from positional JSON sequences; the Zod
/// schemas only accept objects, so reject sequence-encoded structs up front.
fn check_container_shapes(value: &Value) -> Result<(), PluginError> {
    let Some(root) = value.as_object() else {
        return Err(invalid("(root): expected object"));
    };
    if let Some(author) = root.get("author") {
        expect_object(author, "author")?;
    }
    if let Some(engines) = root.get("engines") {
        expect_object(engines, "engines")?;
    }
    if let Some(capabilities) = root.get("capabilities") {
        for (index, capability) in expect_array(capabilities, "capabilities")?
            .iter()
            .enumerate()
        {
            expect_object(capability, &format!("capabilities.{index}"))?;
        }
    }
    let Some(contributes) = root.get("contributes") else {
        return Ok(());
    };
    expect_object(contributes, "contributes")?;
    let Some(map) = contributes.as_object() else {
        return Ok(());
    };
    for name in CONTRIBUTES_KEYS {
        let Some(entries) = map.get(name) else {
            continue;
        };
        for (index, entry) in expect_array(entries, &format!("contributes.{name}"))?
            .iter()
            .enumerate()
        {
            expect_object(entry, &format!("contributes.{name}.{index}"))?;
        }
    }
    Ok(())
}

fn expect_object(value: &Value, path: &str) -> Result<(), PluginError> {
    if !value.is_object() {
        return Err(invalid(format!("{path}: expected object")));
    }
    Ok(())
}

fn expect_array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, PluginError> {
    value
        .as_array()
        .ok_or_else(|| invalid(format!("{path}: expected array")))
}

fn reject_nulls(value: &Value, path: &str) -> Result<(), PluginError> {
    match value {
        Value::Null => Err(invalid(format!("{path}: must not be null"))),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_nulls(item, &child_path(path, &index.to_string()))?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, item) in map {
                reject_nulls(item, &child_path(path, key))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn reject_unknown_contributes_keys(value: &Value) -> Result<(), PluginError> {
    let Some(contributes) = value.get("contributes") else {
        return Ok(());
    };
    let Some(map) = contributes.as_object() else {
        return Ok(());
    };
    for key in map.keys() {
        if !CONTRIBUTES_KEYS.contains(&key.as_str()) {
            return Err(invalid(format!("contributes.{key}: unrecognized key")));
        }
    }
    Ok(())
}

const CONTRIBUTES_KEYS: [&str; 8] = [
    "panels",
    "tabs",
    "commands",
    "events",
    "languagePacks",
    "keybindings",
    "vmRecipes",
    "agents",
];

fn child_path(path: &str, key: &str) -> String {
    if path == "(root)" {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn js_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn is_engine_range(range: &str) -> bool {
    let Some(rest) = range.strip_prefix(">=") else {
        return false;
    };
    let mut parts = rest.split('.');
    let is_digits = |part: Option<&str>| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    is_digits(parts.next())
        && is_digits(parts.next())
        && is_digits(parts.next())
        && parts.next().is_none()
}

fn parse_version_parts(value: &str) -> [f64; 3] {
    let head = value.split(['-', '+']).next().unwrap_or("");
    let mut parts = [0.0_f64; 3];
    for (index, part) in head.split('.').take(3).enumerate() {
        parts[index] = parse_leading_number(part);
    }
    parts
}

fn parse_leading_number(value: &str) -> f64 {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        0.0
    } else {
        digits.parse().unwrap_or(f64::INFINITY)
    }
}

fn is_semver(value: &str) -> bool {
    let without_build = match value.split_once('+') {
        Some((head, build)) => {
            if !is_dot_identifiers(build) {
                return false;
            }
            head
        }
        None => value,
    };
    let core = match without_build.split_once('-') {
        Some((core, prerelease)) => {
            if !is_prerelease(prerelease) {
                return false;
            }
            core
        }
        None => without_build,
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| is_numeric_identifier(part))
}

fn is_numeric_identifier(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
}

fn is_prerelease(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(is_prerelease_identifier)
}

fn is_prerelease_identifier(part: &str) -> bool {
    if part.is_empty() {
        return false;
    }
    if part.bytes().all(|byte| byte.is_ascii_digit()) {
        return part == "0" || !part.starts_with('0');
    }
    part.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && part
            .bytes()
            .any(|byte| byte.is_ascii_alphabetic() || byte == b'-')
}

fn is_dot_identifiers(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn is_command_id(value: &str) -> bool {
    let mut previous_is_separator = true;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            previous_is_separator = false;
        } else if matches!(character, '.' | '_' | '-') {
            if previous_is_separator {
                return false;
            }
            previous_is_separator = true;
        } else {
            return false;
        }
    }
    !value.is_empty() && !previous_is_separator
}

fn is_locale_id(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(language) = segments.next() else {
        return false;
    };
    if !(2..=3).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    segments.all(|segment| {
        (2..=8).contains(&segment.len()) && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::new(INVALID_MANIFEST, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{INVALID_MANIFEST, PLUGIN_ID_MAX_LENGTH};
    use serde_json::{json, Value};

    fn base_manifest() -> Value {
        json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "contributes": { "panels": [], "commands": [], "events": [] },
            "capabilities": []
        })
    }

    fn parse(value: Value) -> Result<Manifest, PluginError> {
        parse_manifest(serde_json::to_vec(&value).unwrap().as_slice())
    }

    fn assert_invalid(value: Value, fragment: &str) {
        let error = parse(value).expect_err("expected an invalid manifest");
        assert_eq!(
            error.code(),
            INVALID_MANIFEST,
            "code for: {}",
            error.message()
        );
        assert!(
            error.message().contains(fragment),
            "message {:?} does not contain {:?}",
            error.message(),
            fragment
        );
    }

    fn repeat(value: Value, count: usize) -> Value {
        Value::Array(std::iter::repeat_n(value, count).collect())
    }

    #[test]
    fn parses_full_valid_manifest_with_tabs() {
        let value = json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo Plugin",
            "version": "1.2.3-alpha.1+build.5",
            "description": "A demo plugin",
            "author": { "name": "Ada", "url": "https://example.com" },
            "repository": "https://github.com/example/demo",
            "icon": "assets/icon.svg",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "main": "main.mjs",
            "contributes": {
                "panels": [{
                    "id": "dashboard",
                    "title": "Dashboard",
                    "icon": "layout-dashboard",
                    "entry": "panels/dashboard.html"
                }],
                "tabs": [
                    { "id": "console", "title": "Console", "entry": "tabs/console.html" },
                    { "id": "settings-tab", "title": "Settings", "icon": "settings", "entry": "tabs/settings.html" }
                ],
                "commands": [
                    { "id": "jupyter.restartKernel", "title": "Restart kernel" },
                    { "id": "open-board", "title": "Open board", "action": "workspace.openBoard" }
                ],
                "events": [{ "on": "worktree.created" }],
                "languagePacks": [{ "locale": "zh-CN", "path": "locales/zh-CN.json" }],
                "keybindings": [{ "command": "jupyter.restartKernel", "key": "Mod+Shift+R", "when": "global" }],
                "vmRecipes": [{ "path": "vm/recipes/basic.json" }],
                "agents": [{ "path": "agents/reviewer.json" }]
            },
            "capabilities": [
                { "kind": "workspace:read" },
                { "kind": "workspace:open-tab" },
                { "kind": "events:subscribe" }
            ]
        });

        let manifest = parse(value).expect("valid manifest");
        assert_eq!(manifest.qualified_key(), "orca-samples.demo");
        assert_eq!(manifest.manifest_version, 1);
        assert_eq!(manifest.plugin_api, 1);
        assert_eq!(manifest.version, "1.2.3-alpha.1+build.5");
        assert_eq!(manifest.engines.orca, ">=1.0.0");
        assert_eq!(manifest.main.as_deref(), Some("main.mjs"));
        assert_eq!(manifest.author.as_ref().unwrap().name, "Ada");
        assert_eq!(manifest.contributes.panels.len(), 1);
        assert_eq!(manifest.contributes.panels[0].id, "dashboard");
        assert_eq!(manifest.contributes.tabs.len(), 2);
        assert_eq!(manifest.contributes.tabs[1].id, "settings-tab");
        assert_eq!(
            manifest.contributes.tabs[1].icon.as_deref(),
            Some("settings")
        );
        assert_eq!(manifest.contributes.commands.len(), 2);
        assert_eq!(
            manifest.contributes.commands[1].action.as_deref(),
            Some("workspace.openBoard")
        );
        assert_eq!(manifest.contributes.events[0].on, "worktree.created");
        assert_eq!(manifest.contributes.language_packs[0].locale, "zh-CN");
        assert_eq!(manifest.contributes.keybindings[0].key, "Mod+Shift+R");
        assert_eq!(
            manifest.contributes.vm_recipes[0].path,
            "vm/recipes/basic.json"
        );
        assert_eq!(manifest.contributes.agents[0].path, "agents/reviewer.json");
        assert_eq!(manifest.capabilities.len(), 3);
        assert!(manifest
            .capabilities
            .contains(&crate::types::Capability::WorkspaceOpenTab));
    }

    #[test]
    fn defaults_fill_omitted_contributes_and_capabilities() {
        let value = json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1
        });

        let manifest = parse(value).expect("defaults apply");
        assert!(manifest.contributes.panels.is_empty());
        assert!(manifest.contributes.tabs.is_empty());
        assert!(manifest.contributes.commands.is_empty());
        assert!(manifest.contributes.events.is_empty());
        assert!(manifest.contributes.language_packs.is_empty());
        assert!(manifest.contributes.keybindings.is_empty());
        assert!(manifest.contributes.vm_recipes.is_empty());
        assert!(manifest.contributes.agents.is_empty());
        assert!(manifest.capabilities.is_empty());
        assert!(manifest.description.is_none());
        assert!(manifest.author.is_none());
        assert!(manifest.repository.is_none());
        assert!(manifest.icon.is_none());
        assert!(manifest.main.is_none());
    }

    #[test]
    fn rejects_missing_required_fields() {
        for field in [
            "manifestVersion",
            "id",
            "publisher",
            "name",
            "version",
            "engines",
            "pluginApi",
        ] {
            let mut value = base_manifest();
            value.as_object_mut().unwrap().remove(field);
            let error = parse(value).expect_err("missing field");
            assert_eq!(error.code(), INVALID_MANIFEST, "field {field}");
        }

        let mut value = base_manifest();
        value["engines"] = json!({});
        assert_invalid(value, "orca");
    }

    #[test]
    fn rejects_unknown_manifest_version_and_plugin_api() {
        let mut value = base_manifest();
        value["manifestVersion"] = json!(2);
        assert_invalid(value, "manifestVersion");

        let mut value = base_manifest();
        value["pluginApi"] = json!(2);
        assert_invalid(value, "pluginApi");
    }

    #[test]
    fn rejects_invalid_and_reserved_ids() {
        for id in [
            "",
            "Demo",
            "-demo",
            "demo-",
            "de--mo",
            "de mo",
            "de_mo",
            "__proto__",
            "prototype",
            "constructor",
        ] {
            let mut value = base_manifest();
            value["id"] = json!(id);
            assert_invalid(
                value,
                "must be kebab-case (a-z, 0-9, dashes) and not a reserved name",
            );
        }

        let mut value = base_manifest();
        value["id"] = json!("a".repeat(PLUGIN_ID_MAX_LENGTH + 1));
        assert_invalid(
            value,
            "must be kebab-case (a-z, 0-9, dashes) and not a reserved name",
        );

        let mut value = base_manifest();
        value["publisher"] = json!("Bad Publisher");
        assert_invalid(
            value,
            "must be kebab-case (a-z, 0-9, dashes) and not a reserved name",
        );
    }

    #[test]
    fn accepts_valid_semver_versions() {
        for version in [
            "0.0.0",
            "1.0.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0+build.5",
            "1.0.0-alpha.1+build.5",
            "10.20.30",
        ] {
            let mut value = base_manifest();
            value["version"] = json!(version);
            parse(value).unwrap_or_else(|error| panic!("{version}: {}", error.message()));
        }
    }

    #[test]
    fn rejects_invalid_semver_versions() {
        for version in [
            "01.0.0",
            "1.0",
            "1.0.0-01",
            "1.0.0+",
            "1.0.0-",
            "v1.0.0",
            "1.0.0.0",
            "1.0.0-alpha..1",
            "1.0.0+build..5",
            "1.0.0 ",
        ] {
            let mut value = base_manifest();
            value["version"] = json!(version);
            assert_invalid(value, "must be semver");
        }
    }

    #[test]
    fn rejects_invalid_engine_ranges() {
        for range in [
            ">=1.0",
            "^1.0.0",
            ">1.0.0",
            ">=1.0.0 <2.0.0",
            ">=1.0.0.0",
            "1.0.0",
        ] {
            let mut value = base_manifest();
            value["engines"] = json!({ "orca": range });
            assert_invalid(value, "must be a \">=x.y.z\" version range");
        }

        let mut value = base_manifest();
        value["engines"] = json!({ "orca": format!(">={}", "1".repeat(65)) });
        assert_invalid(value, "at most 64");
    }

    #[test]
    fn engine_gate_boundaries() {
        assert!(engine_satisfies(">=1.0.0", "1.0.0"));
        assert!(engine_satisfies(">=1.0.0", "1.0.1"));
        assert!(!engine_satisfies(">=1.0.0", "0.9.9"));
        assert!(engine_satisfies(">=1.2.3", "1.2.3"));
        assert!(!engine_satisfies(">=1.2.3", "1.2.2"));
        assert!(engine_satisfies(">=1.2.3", "1.2.3-beta.1"));
        assert!(engine_satisfies(">=1.2.3", "1.2.4+build.9"));
        assert!(engine_satisfies(">=1.2.3", "1.2.3-0"));
        assert!(engine_satisfies(">=2.0.0", "10.0.0"));
        assert!(!engine_satisfies(">=10.0.0", "2.0.0"));
        assert!(engine_satisfies(">=1.0.0", "2"));
        assert!(engine_satisfies(">=1.0.0", "1.0.0.5"));
        assert!(!engine_satisfies(">=1.0.0", "0"));
        assert!(!engine_satisfies("1.0.0", "1.0.0"));
        assert!(!engine_satisfies(">=x.y.z", "1.0.0"));
        assert!(!engine_satisfies(">=1.0.0", "not-a-version"));
    }

    #[test]
    fn accepts_channel_capability_kinds_and_rejects_unknown() {
        let mut value = base_manifest();
        value["capabilities"] = json!([{ "kind": "worker:call" }, { "kind": "session:push" }]);
        let manifest = parse(value).expect("channel capabilities are valid");
        assert!(manifest.capabilities.contains(&Capability::WorkerCall));
        assert!(manifest.capabilities.contains(&Capability::SessionPush));

        let mut unknown = base_manifest();
        unknown["capabilities"] = json!([{ "kind": "ui:call" }]);
        assert_invalid(unknown, "ui:call");
    }

    #[test]
    fn rejects_unknown_capabilities() {
        let mut value = base_manifest();
        value["capabilities"] = json!([{ "kind": "net:fetch" }]);
        assert_invalid(value, "net:fetch");

        let mut value = base_manifest();
        value["capabilities"] = json!([{ "kind": "storage", "hosts": [] }]);
        let error = parse(value).expect_err("extra capability key");
        assert_eq!(error.code(), INVALID_MANIFEST);

        let mut value = base_manifest();
        value["capabilities"] = json!(["storage"]);
        let error = parse(value).expect_err("bare capability string");
        assert_eq!(error.code(), INVALID_MANIFEST);

        let mut value = base_manifest();
        value["capabilities"] = json!([{ "kind": "storage" }, null]);
        assert_invalid(value, "capabilities.1: must not be null");
    }

    #[test]
    fn rejects_unknown_contributes_keys() {
        let mut value = base_manifest();
        value["contributes"] = json!({ "panels": [], "bogus": [] });
        assert_invalid(value, "contributes.bogus");

        let mut value = base_manifest();
        value["contributes"] = json!({ "panels": [], "tabs": [], "unknownThing": 1 });
        assert_invalid(value, "contributes.unknownThing");
    }

    #[test]
    fn ignores_unknown_top_level_keys() {
        let mut value = base_manifest();
        value["futureField"] = json!({ "anything": true });
        parse(value).expect("unknown top-level keys are stripped");
    }

    #[test]
    fn rejects_null_for_optional_fields() {
        for field in ["description", "author", "repository", "icon", "main"] {
            let mut value = base_manifest();
            value[field] = Value::Null;
            assert_invalid(value, &format!("{field}: must not be null"));
        }

        let mut value = base_manifest();
        value["author"] = json!({ "name": "Ada", "url": null });
        assert_invalid(value, "author.url: must not be null");

        let mut value = base_manifest();
        value["contributes"]["panels"] = json!([{
            "id": "p",
            "title": "P",
            "icon": null,
            "entry": "p.html"
        }]);
        assert_invalid(value, "contributes.panels.0.icon: must not be null");
    }

    #[test]
    fn rejects_malformed_json_and_non_objects() {
        for bytes in [
            b"".as_slice(),
            b"not json".as_slice(),
            b"[]".as_slice(),
            b"{".as_slice(),
        ] {
            let error = parse_manifest(bytes).expect_err("malformed");
            assert_eq!(error.code(), INVALID_MANIFEST);
        }
    }

    #[test]
    fn rejects_wrong_field_types() {
        let mut value = base_manifest();
        value["manifestVersion"] = json!("1");
        assert_eq!(parse(value).unwrap_err().code(), INVALID_MANIFEST);

        let mut value = base_manifest();
        value["name"] = json!(5);
        assert_eq!(parse(value).unwrap_err().code(), INVALID_MANIFEST);

        let mut value = base_manifest();
        value["contributes"] = json!([]);
        assert_eq!(parse(value).unwrap_err().code(), INVALID_MANIFEST);

        let mut value = base_manifest();
        value["capabilities"] = json!({ "kind": "storage" });
        assert_eq!(parse(value).unwrap_err().code(), INVALID_MANIFEST);
    }

    #[test]
    fn rejects_oversized_scalar_fields() {
        let mut value = base_manifest();
        value["name"] = json!("n".repeat(257));
        assert_invalid(value, "at most 256");

        let mut value = base_manifest();
        value["description"] = json!("d".repeat(4097));
        assert_invalid(value, "at most 4096");

        let mut value = base_manifest();
        value["repository"] = json!("r".repeat(2049));
        assert_invalid(value, "at most 2048");

        let mut value = base_manifest();
        value["author"] = json!({ "name": "Ada", "url": "u".repeat(2049) });
        assert_invalid(value, "at most 2048");

        let mut value = base_manifest();
        value["description"] = json!("");
        parse(value).expect("empty description is allowed");
    }

    #[test]
    fn enforces_contribution_limits() {
        let panel = json!({ "id": "p", "title": "P", "entry": "p.html" });
        let mut value = base_manifest();
        value["contributes"]["panels"] = repeat(panel.clone(), PLUGIN_PANEL_LIMIT + 1);
        assert_invalid(value, "at most 64");

        let tab = json!({ "id": "t", "title": "T", "entry": "t.html" });
        let mut value = base_manifest();
        value["contributes"]["tabs"] = repeat(tab, PLUGIN_TAB_LIMIT + 1);
        assert_invalid(value, "at most 64");

        let command = json!({ "id": "c", "title": "C" });
        let mut value = base_manifest();
        value["contributes"]["commands"] = repeat(command, PLUGIN_COMMAND_LIMIT + 1);
        assert_invalid(value, "at most 256");

        let event = json!({ "on": "worktree.created" });
        let mut value = base_manifest();
        value["contributes"]["events"] = repeat(event, PLUGIN_EVENT_SUBSCRIPTION_LIMIT + 1);
        assert_invalid(value, "at most 3");

        let pack = json!({ "locale": "en-US", "path": "locales/en-US.json" });
        let mut value = base_manifest();
        value["contributes"]["languagePacks"] = repeat(pack, PLUGIN_LANGUAGE_PACK_LIMIT + 1);
        assert_invalid(value, "at most 16");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["keybindings"] = repeat(
            json!({ "command": "c", "key": "Mod+K" }),
            PLUGIN_KEYBINDING_LIMIT + 1,
        );
        assert_invalid(value, "at most 256");

        let recipe = json!({ "path": "vm/basic.json" });
        let mut value = base_manifest();
        value["contributes"]["vmRecipes"] = repeat(recipe, PLUGIN_VM_RECIPE_LIMIT + 1);
        assert_invalid(value, "at most 64");

        let agent = json!({ "path": "agents/a.json" });
        let mut value = base_manifest();
        value["contributes"]["agents"] = repeat(agent, PLUGIN_AGENT_PROFILE_LIMIT + 1);
        assert_invalid(value, "at most 64");

        let capability = json!({ "kind": "storage" });
        let mut value = base_manifest();
        value["capabilities"] = repeat(capability, PLUGIN_CAPABILITY_LIMIT + 1);
        assert_invalid(value, "at most 32");
    }

    #[test]
    fn rejects_duplicate_contribution_ids() {
        let mut value = base_manifest();
        value["contributes"]["panels"] = json!([
            { "id": "dashboard", "title": "One", "entry": "one.html" },
            { "id": "dashboard", "title": "Two", "entry": "two.html" }
        ]);
        assert_invalid(value, "duplicate panels id: dashboard");

        let mut value = base_manifest();
        value["contributes"]["tabs"] = json!([
            { "id": "console", "title": "One", "entry": "one.html" },
            { "id": "console", "title": "Two", "entry": "two.html" }
        ]);
        assert_invalid(value, "duplicate tabs id: console");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([
            { "id": "run", "title": "One" },
            { "id": "run", "title": "Two" }
        ]);
        assert_invalid(value, "duplicate commands id: run");
    }

    #[test]
    fn rejects_duplicate_content_packs_case_insensitively() {
        let mut value = base_manifest();
        value["contributes"]["languagePacks"] = json!([
            { "locale": "en-US", "path": "locales/a.json" },
            { "locale": "en-us", "path": "locales/b.json" }
        ]);
        assert_invalid(value, "duplicate language pack locale: en-us");

        let mut value = base_manifest();
        value["contributes"]["vmRecipes"] = json!([
            { "path": "vm/a.json" },
            { "path": "vm/a.json" }
        ]);
        assert_invalid(value, "duplicate vmRecipes path: vm/a.json");

        let mut value = base_manifest();
        value["contributes"]["agents"] = json!([
            { "path": "agents/a.json" },
            { "path": "agents/a.json" }
        ]);
        assert_invalid(value, "duplicate agents path: agents/a.json");
    }

    #[test]
    fn rejects_unsafe_contribution_paths() {
        for entry in [
            "../evil.html",
            "/abs.html",
            "a/../b.html",
            "sub\\..\\x.html",
            "..",
            "a:b.html",
            "con.txt",
        ] {
            let mut value = base_manifest();
            value["contributes"]["panels"] = json!([{
                "id": "p",
                "title": "P",
                "entry": entry
            }]);
            assert_invalid(
                value,
                "must be a portable relative path inside the plugin directory",
            );
        }

        let mut value = base_manifest();
        value["main"] = json!("../main.mjs");
        assert_invalid(
            value,
            "must be a portable relative path inside the plugin directory",
        );

        let mut value = base_manifest();
        value["icon"] = json!("../icon.svg");
        assert_invalid(
            value,
            "must be a portable relative path inside the plugin directory",
        );

        let mut value = base_manifest();
        value["contributes"]["languagePacks"] =
            json!([{ "locale": "en-US", "path": "../locales/en-US.json" }]);
        assert_invalid(
            value,
            "must be a portable relative path inside the plugin directory",
        );

        let mut value = base_manifest();
        value["contributes"]["vmRecipes"] = json!([{ "path": "C:\\vm\\recipe.json" }]);
        assert_invalid(
            value,
            "must be a portable relative path inside the plugin directory",
        );
    }

    #[test]
    fn rejects_invalid_command_fields() {
        for id in ["bad id!", "bad/id", ".leading", "trailing."] {
            let mut value = base_manifest();
            value["main"] = json!("main.mjs");
            value["contributes"]["commands"] = json!([{ "id": id, "title": "X" }]);
            assert_invalid(value, "must be a portable command id");
        }

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "", "title": "X" }]);
        assert_invalid(value, "at least 1");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "run", "title": "" }]);
        assert_invalid(value, "at least 1");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "run", "title": "t".repeat(257) }]);
        assert_invalid(value, "at most 256");

        for context in ["workspace", "GLOBAL"] {
            let mut value = base_manifest();
            value["main"] = json!("main.mjs");
            value["contributes"]["commands"] =
                json!([{ "id": "run", "title": "Run", "context": context }]);
            assert_eq!(parse(value).unwrap_err().code(), INVALID_MANIFEST);
        }

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] =
            json!([{ "id": "run", "title": "Run", "context": "worktree" }]);
        parse(value).expect("worktree context is valid");
    }

    #[test]
    fn command_visibility_defaults_to_palette_and_rejects_conflicts() {
        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "check", "title": "Check" }]);
        let manifest = parse(value).expect("defaults to palette");
        assert_eq!(
            manifest.contributes.commands[0].visibility,
            CommandVisibility::Palette
        );

        let mut internal = base_manifest();
        internal["main"] = json!("main.mjs");
        internal["contributes"]["commands"] =
            json!([{ "id": "snapshot", "title": "Snapshot", "visibility": "internal" }]);
        let manifest = parse(internal).expect("internal is valid");
        assert_eq!(
            manifest.contributes.commands[0].visibility,
            CommandVisibility::Internal
        );

        let mut with_action = base_manifest();
        with_action["contributes"]["commands"] = json!([{
            "id": "snapshot", "title": "Snapshot", "visibility": "internal", "action": "open-tab"
        }]);
        assert_invalid(with_action, "must not declare an action alias");

        let mut bound = base_manifest();
        bound["contributes"]["commands"] =
            json!([{ "id": "snapshot", "title": "Snapshot", "visibility": "internal" }]);
        bound["contributes"]["keybindings"] =
            json!([{ "command": "snapshot", "key": "Cmd+Shift+U" }]);
        assert_invalid(bound, "internal commands cannot be bound");

        let mut unknown = base_manifest();
        unknown["contributes"]["commands"] =
            json!([{ "id": "check", "title": "Check", "visibility": "hidden" }]);
        assert_invalid(unknown, "hidden");
    }

    #[test]
    fn rejects_invalid_event_names() {
        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["capabilities"] = json!([{ "kind": "events:subscribe" }]);
        value["contributes"]["events"] = json!([{ "on": "worktree.renamed" }]);
        assert_invalid(value, "worktree.renamed");
    }

    #[test]
    fn rejects_invalid_language_pack_locales() {
        for locale in ["e", "en_US", "en-US-", "english"] {
            let mut value = base_manifest();
            value["contributes"]["languagePacks"] =
                json!([{ "locale": locale, "path": "locales/x.json" }]);
            assert_invalid(value, "must be a portable locale identifier");
        }

        let mut value = base_manifest();
        value["contributes"]["languagePacks"] =
            json!([{ "locale": "zh-Hant-TW", "path": "locales/zh-Hant-TW.json" }]);
        parse(value).expect("valid locale");
    }

    #[test]
    fn rejects_unknown_builtin_action() {
        let mut value = base_manifest();
        value["contributes"]["commands"] =
            json!([{ "id": "run", "title": "Run", "action": "not.a.real.action" }]);
        assert_invalid(value, "unknown built-in action: not.a.real.action");
    }

    #[test]
    fn accepts_action_alias_command_without_main() {
        let mut value = base_manifest();
        value["contributes"]["commands"] = json!([{
            "id": "open-board",
            "title": "Open board",
            "action": "workspace.openBoard"
        }]);
        parse(value).expect("declarative alias needs no worker");
    }

    #[test]
    fn rejects_worker_command_without_main() {
        let mut value = base_manifest();
        value["contributes"]["commands"] = json!([{ "id": "run", "title": "Run" }]);
        assert_invalid(
            value,
            "required when contributes.commands contains a worker command",
        );
    }

    #[test]
    fn requires_main_and_capability_for_events() {
        let mut value = base_manifest();
        value["contributes"]["events"] = json!([{ "on": "worktree.created" }]);
        assert_invalid(value, "required when contributes.events is non-empty");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["events"] = json!([{ "on": "worktree.created" }]);
        assert_invalid(
            value,
            "events:subscribe capability required when contributes.events is non-empty",
        );

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["capabilities"] = json!([{ "kind": "events:subscribe" }]);
        value["contributes"]["events"] = json!([{ "on": "worktree.created" }]);
        parse(value).expect("event subscription fully declared");
    }

    #[test]
    fn rejects_keybinding_reference_and_context_mismatches() {
        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["keybindings"] = json!([{ "command": "missing", "key": "Mod+K" }]);
        assert_invalid(value, "unknown contributed command: missing");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "run", "title": "Run" }]);
        value["contributes"]["keybindings"] =
            json!([{ "command": "run", "key": "Mod+K", "when": "worktree" }]);
        assert_invalid(value, "keybinding context must match its command context");

        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] =
            json!([{ "id": "run", "title": "Run", "context": "worktree" }]);
        value["contributes"]["keybindings"] =
            json!([{ "command": "run", "key": "Mod+K", "when": "worktree" }]);
        parse(value).expect("matching contexts are valid");
    }

    #[test]
    fn rejects_oversized_keybinding_keys() {
        let mut value = base_manifest();
        value["main"] = json!("main.mjs");
        value["contributes"]["commands"] = json!([{ "id": "run", "title": "Run" }]);
        value["contributes"]["keybindings"] = json!([{ "command": "run", "key": "k".repeat(129) }]);
        assert_invalid(value, "at most 128");
    }

    #[test]
    fn serialization_omits_absent_options_and_round_trips() {
        let manifest = parse(base_manifest()).unwrap();
        let value = serde_json::to_value(&manifest).unwrap();
        assert!(value.get("description").is_none());
        assert!(value.get("author").is_none());
        assert!(value.get("repository").is_none());
        assert!(value.get("icon").is_none());
        assert!(value.get("main").is_none());
        assert_eq!(value["contributes"]["panels"], json!([]));
        assert_eq!(value["capabilities"], json!([]));

        let reparsed = parse(value).expect("round trip");
        assert_eq!(manifest, reparsed);
    }

    #[test]
    fn manifest_constants_match_oracle() {
        assert_eq!(MANIFEST_FILENAME, "ade-plugin.json");
        assert_eq!(LEGACY_MANIFEST_FILENAME, "orca-plugin.json");
        assert_eq!(MAX_MANIFEST_BYTES, 1024 * 1024);
    }
}
