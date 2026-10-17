//! `skills` command surface (spec §3.9): a minimal, home-root-only
//! implementation of `skills.discover`.
//!
//! Scope, as fixed by the plan: the fixed home table from oracle
//! `src/main/skills/skill-discovery-sources.ts` (`buildSkillDiscoverySources`),
//! one `SKILL.md` level below each root, `.system` first-segment skills marked
//! `bundled`, and a 10s in-memory scan cache (`refresh: true` bypasses it). No
//! repo roots, no WSL/remote targets, no plugin-root expansion, no freshness /
//! install / remove surface — every other `skills.*` method stays on the
//! renderer-side rejecting fallback.
//!
//! Field decisions (documented for the reviewer):
//! - `scannedAt` crosses the bridge as an ISO-8601 UTC string (matching the
//!   aiVault surface); `real/skills.ts` parses it to epoch millis for
//!   the shared `SkillDiscoveryResult` contract.
//! - `description` and `updatedAt` are minimal legal values (`null`): the hook
//!   read surface (`useInstalledAgentSkills`) matches on name / directory path /
//!   `sourceKind` only.
//! - `id` is the skill file path: stable, unique after path dedup, and only
//!   identity consumers in this slice (React keys) need it.
//! - Environment root overrides mirror oracle `resolveEnvironmentSkillProviderRoots`
//!   plus `HERMES_HOME`: `CLAUDE_CONFIG_DIR`, `GROK_HOME`, `HERMES_HOME`. No
//!   `CLAUDE_SKILLS_ROOT` exists in the reference; it is not invented here.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::lock;

/// The file that marks a directory as one skill.
pub const SKILL_FILE_NAME: &str = "SKILL.md";
/// Shared scan TTL (oracle `SKILL_ROOT_SCAN_TTL_MS`).
pub const SKILL_DISCOVERY_TTL: Duration = Duration::from_secs(10);
/// Frontmatter `name:` is read from this many leading lines at most.
const SKILL_NAME_SCAN_LINES: usize = 20;
/// Markdown head size bound (oracle `MAX_MARKDOWN_BYTES`).
const MAX_SKILL_MARKDOWN_BYTES: u64 = 256 * 1024;
/// Provider root override maximum length (oracle `PROVIDER_ROOT_MAX_LENGTH`).
const PROVIDER_ROOT_MAX_LENGTH: usize = 32_768;

/// One fixed discovery root. `providers`/`owner`/`source_kind` are `'static`
/// because every entry comes from the hard-coded table below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRoot {
    pub id: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
    pub source_kind: &'static str,
    pub providers: &'static [&'static str],
    pub owner: Option<&'static str>,
}

/// Environment-driven replacements for provider roots, mirroring
/// `resolveEnvironmentSkillProviderRoots` plus the Hermes home override.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillRootOverrides {
    pub claude: Option<PathBuf>,
    pub grok: Option<PathBuf>,
    pub hermes: Option<PathBuf>,
}

/// The `{ refresh? }` command args.
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillDiscoverArgs {
    #[serde(default)]
    pub refresh: Option<bool>,
}

/// `skills.discover` result. `skills`/`sources` stay opaque JSON (the shared
/// contract lives in `src/shared/skills.ts`), like `aiVault.listSessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillDiscoveryResultDto {
    pub skills: Json,
    pub sources: Json,
    pub scanned_at: String,
}

/// The oracle home table order and labels (`buildSkillDiscoverySources`
/// lines 104-236), including the `codex-plugin-cache` plugin root.
pub fn home_skill_roots(home: &Path, overrides: &SkillRootOverrides) -> Vec<SkillRoot> {
    let root = |id, label, path: PathBuf, source_kind, providers, owner| SkillRoot {
        id,
        label,
        path,
        source_kind,
        providers,
        owner,
    };
    let claude = overrides
        .claude
        .clone()
        .unwrap_or_else(|| home.join(".claude").join("skills"));
    let grok = overrides
        .grok
        .clone()
        .unwrap_or_else(|| home.join(".grok").join("skills"));
    let hermes = overrides
        .hermes
        .clone()
        .unwrap_or_else(|| home.join(".hermes").join("skills"));
    vec![
        root(
            "home-codex",
            "Codex home",
            home.join(".codex").join("skills"),
            "home",
            &["codex"],
            Some("codex"),
        ),
        root(
            "home-agents",
            "Agent skills home",
            home.join(".agents").join("skills"),
            "home",
            &["agent-skills"],
            None,
        ),
        root(
            "home-claude",
            "Claude home",
            claude,
            "home",
            &["claude"],
            Some("claude"),
        ),
        root(
            "codex-plugin-cache",
            "Codex plugin cache",
            home.join(".codex").join("plugins").join("cache"),
            "plugin",
            &["codex", "agent-skills"],
            Some("codex"),
        ),
        root(
            "home-grok",
            "Grok home",
            grok,
            "home",
            &["agent-skills"],
            Some("grok"),
        ),
        root(
            "home-opencode",
            "OpenCode home",
            home.join(".config").join("opencode").join("skills"),
            "home",
            &["agent-skills"],
            Some("opencode"),
        ),
        root(
            "home-pi",
            "Pi home",
            home.join(".pi").join("agent").join("skills"),
            "home",
            &["agent-skills"],
            Some("pi"),
        ),
        root(
            "home-omp",
            "OMP home",
            home.join(".omp").join("agent").join("skills"),
            "home",
            &["agent-skills"],
            Some("omp"),
        ),
        root(
            "home-hermes",
            "Hermes home",
            hermes,
            "home",
            &["agent-skills"],
            Some("hermes"),
        ),
        root(
            "home-prime-agent",
            "Prime Agent home",
            home.join(".prime").join("agent").join("skills"),
            "home",
            &["agent-skills"],
            Some("prime-agent"),
        ),
        root(
            "home-gemini",
            "Gemini home",
            home.join(".gemini").join("skills"),
            "home",
            &["agent-skills"],
            Some("gemini"),
        ),
        root(
            "home-antigravity",
            "Antigravity home",
            home.join(".gemini").join("antigravity").join("skills"),
            "home",
            &["agent-skills"],
            Some("antigravity"),
        ),
        root(
            "home-cursor",
            "Cursor home",
            home.join(".cursor").join("skills"),
            "home",
            &["agent-skills"],
            Some("cursor"),
        ),
        root(
            "home-droid",
            "Droid home",
            home.join(".factory").join("skills"),
            "home",
            &["agent-skills"],
            Some("droid"),
        ),
        root(
            "home-continue",
            "Continue home",
            home.join(".continue").join("skills"),
            "home",
            &["agent-skills"],
            Some("continue"),
        ),
        root(
            "home-trae",
            "Trae home",
            home.join(".trae-cn").join("skills"),
            "home",
            &["agent-skills"],
            Some("trae"),
        ),
        root(
            "home-aug",
            "Augment home",
            home.join(".augment").join("skills"),
            "home",
            &["agent-skills"],
            Some("aug"),
        ),
    ]
}

/// Resolve `CLAUDE_CONFIG_DIR` / `GROK_HOME` / `HERMES_HOME` into root
/// overrides with the oracle normalization: trimmed, non-empty, absolute, NUL
/// free, length bounded.
pub fn resolve_provider_root_overrides(
    get_env: impl Fn(&str) -> Option<String>,
) -> SkillRootOverrides {
    let normalized = |key: &str| {
        let value = get_env(key)?;
        let candidate = value.trim();
        if candidate.is_empty()
            || candidate.len() > PROVIDER_ROOT_MAX_LENGTH
            || candidate.contains('\0')
            || !Path::new(candidate).is_absolute()
        {
            return None;
        }
        Some(PathBuf::from(candidate))
    };
    SkillRootOverrides {
        claude: normalized("CLAUDE_CONFIG_DIR").map(|path| path.join("skills")),
        grok: normalized("GROK_HOME").map(|path| path.join("skills")),
        hermes: normalized("HERMES_HOME").map(|path| path.join("skills")),
    }
}

/// Scan every fixed home root: `<root>/<dir>/SKILL.md` (dot-directories
/// skipped) plus `<root>/.system/<dir>/SKILL.md` as `bundled` skills.
pub fn discover(home: &Path, overrides: &SkillRootOverrides) -> SkillDiscoveryResultDto {
    let roots = home_skill_roots(home, overrides);
    let mut skills: Vec<ScannedSkill> = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut sources: Vec<(&'static str, &'static str, Value)> = Vec::with_capacity(roots.len());
    for root in &roots {
        let metadata = fs::metadata(&root.path).ok();
        let exists = metadata.is_some();
        if metadata.is_some_and(|metadata| metadata.is_dir()) {
            for skill in scan_root(root) {
                if seen_paths.insert(skill.skill_file_path.clone()) {
                    skills.push(skill);
                }
            }
        }
        sources.push((root.label, root.id, source_json(root, exists)));
    }
    skills.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.skill_file_path.cmp(&right.skill_file_path))
    });
    sources.sort_by(|left, right| {
        left.0
            .to_lowercase()
            .cmp(&right.0.to_lowercase())
            .then_with(|| left.1.cmp(right.1))
    });
    SkillDiscoveryResultDto {
        skills: Json::new(Value::Array(
            skills.into_iter().map(|skill| skill.to_json()).collect(),
        )),
        sources: Json::new(Value::Array(
            sources.into_iter().map(|(_, _, source)| source).collect(),
        )),
        scanned_at: iso8601_utc(SystemTime::now()),
    }
}

/// One scanned skill, before JSON projection.
struct ScannedSkill {
    id: String,
    name: String,
    directory_path: PathBuf,
    skill_file_path: PathBuf,
    source_kind: &'static str,
    source_label: String,
    providers: &'static [&'static str],
    root_path: PathBuf,
}

impl ScannedSkill {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "description": Value::Null,
            "providers": self.providers,
            "sourceKind": self.source_kind,
            "sourceLabel": self.source_label,
            "rootPath": self.root_path.to_string_lossy(),
            "directoryPath": self.directory_path.to_string_lossy(),
            "skillFilePath": self.skill_file_path.to_string_lossy(),
            "installed": true,
            "updatedAt": Value::Null,
        })
    }
}

fn source_json(root: &SkillRoot, exists: bool) -> Value {
    let mut source = serde_json::Map::new();
    source.insert("id".to_string(), json!(root.id));
    source.insert("label".to_string(), json!(root.label));
    source.insert("path".to_string(), json!(root.path.to_string_lossy()));
    source.insert("sourceKind".to_string(), json!(root.source_kind));
    source.insert("providers".to_string(), json!(root.providers));
    source.insert("owner".to_string(), json!(root.owner));
    source.insert("exists".to_string(), json!(exists));
    if !exists {
        source.insert("skippedReason".to_string(), json!("missing"));
    }
    Value::Object(source)
}

fn scan_root(root: &SkillRoot) -> Vec<ScannedSkill> {
    let Ok(entries) = fs::read_dir(&root.path) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".system" {
            // Why: `.system` is the one dot-directory that is entered; its
            // children are the bundled skills (oracle `sourceKindForSkill`).
            if let Some(skill) = scanned_skill(root, &path, true) {
                skills.push(skill);
            }
            if let Ok(system_entries) = fs::read_dir(&path) {
                for system_entry in system_entries.flatten() {
                    let child = system_entry.path();
                    let child_name = system_entry.file_name();
                    let child_name = child_name.to_string_lossy();
                    if child_name.starts_with('.') || !child.is_dir() {
                        continue;
                    }
                    if let Some(skill) = scanned_skill(root, &child, true) {
                        skills.push(skill);
                    }
                }
            }
            continue;
        }
        if name.starts_with('.') {
            continue;
        }
        if let Some(skill) = scanned_skill(root, &path, false) {
            skills.push(skill);
        }
    }
    skills
}

fn scanned_skill(root: &SkillRoot, directory: &Path, bundled: bool) -> Option<ScannedSkill> {
    let skill_file_path = directory.join(SKILL_FILE_NAME);
    if !skill_file_path.is_file() {
        return None;
    }
    let fallback_name = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = read_skill_markdown(&skill_file_path)
        .and_then(|markdown| parse_skill_name(&markdown))
        .unwrap_or(fallback_name);
    let source_kind = if bundled { "bundled" } else { root.source_kind };
    let source_label = if bundled {
        format!("{} bundled", root.label)
    } else {
        root.label.to_string()
    };
    Some(ScannedSkill {
        id: skill_file_path.to_string_lossy().into_owned(),
        name,
        directory_path: directory.to_path_buf(),
        skill_file_path,
        source_kind,
        source_label,
        providers: root.providers,
        root_path: root.path.clone(),
    })
}

/// Frontmatter `name:` from the first [`SKILL_NAME_SCAN_LINES`] lines, inside a
/// leading `---` fence. Quoted scalars are unwrapped; a missing/empty name is
/// `None` so the caller falls back to the directory name.
pub fn parse_skill_name(markdown: &str) -> Option<String> {
    let normalized = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let mut lines = normalized.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines.take(SKILL_NAME_SCAN_LINES - 1) {
        if line.trim_end() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let name = strip_quote_pair(value.trim());
            return (!name.is_empty()).then(|| name.to_string());
        }
    }
    None
}

fn strip_quote_pair(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn read_skill_markdown(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_SKILL_MARKDOWN_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The 10s result cache (spec §3.9): repeated reads inside the TTL serve the
/// previous scan, `refresh: true` scans again and replaces the entry.
#[derive(Debug)]
pub struct SkillDiscoveryCache {
    ttl: Duration,
    entry: Option<(Instant, SkillDiscoveryResultDto)>,
}

impl SkillDiscoveryCache {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, entry: None }
    }

    pub fn get_or_scan(
        &mut self,
        refresh: bool,
        now: Instant,
        scan: impl FnOnce() -> SkillDiscoveryResultDto,
    ) -> SkillDiscoveryResultDto {
        if !refresh {
            if let Some((scanned_at, cached)) = &self.entry {
                if now.saturating_duration_since(*scanned_at) < self.ttl {
                    return cached.clone();
                }
            }
        }
        let fresh = scan();
        self.entry = Some((now, fresh.clone()));
        fresh
    }
}

static DISCOVERY_CACHE: LazyLock<Mutex<SkillDiscoveryCache>> =
    LazyLock::new(|| Mutex::new(SkillDiscoveryCache::new(SKILL_DISCOVERY_TTL)));

/// Production scan: injects the process home and environment overrides.
pub fn discover_cached(refresh: bool) -> Result<SkillDiscoveryResultDto, BridgeError> {
    let home = process_home()?;
    let overrides = resolve_provider_root_overrides(|key| std::env::var(key).ok());
    let mut cache = lock(&DISCOVERY_CACHE);
    Ok(cache.get_or_scan(refresh, Instant::now(), || discover(&home, &overrides)))
}

fn process_home() -> Result<PathBuf, BridgeError> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| BridgeError::message("failed to resolve home directory: HOME is not set"))
}

/// UTC ISO-8601 with millisecond precision, matching the other bridge
/// timestamps (`ade-ai-vault`) without a time crate.
fn iso8601_utc(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs() as i64;
    let millis = duration.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// Howard Hinnant's `civil_from_days` (days since the Unix epoch → y/m/d).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day as u32)
}

#[tauri::command]
#[specta::specta]
pub async fn skills_discover(
    args: Option<SkillDiscoverArgs>,
) -> Result<SkillDiscoveryResultDto, BridgeError> {
    let refresh = args.and_then(|args| args.refresh).unwrap_or(false);
    crate::commands::run_blocking(move || discover_cached(refresh)).await
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, UNIX_EPOCH};

    use serde_json::{json, Value};

    use super::*;

    fn write_skill(home: &Path, root: &str, dir: &str, content: &str) -> PathBuf {
        let path = home.join(root).join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(SKILL_FILE_NAME), content).unwrap();
        path
    }

    fn skills_of(result: &SkillDiscoveryResultDto) -> Vec<Value> {
        result.skills.0.as_array().unwrap().clone()
    }

    fn sources_of(result: &SkillDiscoveryResultDto) -> Vec<Value> {
        result.sources.0.as_array().unwrap().clone()
    }

    fn find_skill<'a>(skills: &'a [Value], name: &str) -> &'a Value {
        skills
            .iter()
            .find(|skill| skill["name"] == name)
            .unwrap_or_else(|| panic!("skill {name} not found"))
    }

    fn find_source<'a>(sources: &'a [Value], id: &str) -> &'a Value {
        sources
            .iter()
            .find(|source| source["id"] == id)
            .unwrap_or_else(|| panic!("source {id} not found"))
    }

    fn empty_result() -> SkillDiscoveryResultDto {
        SkillDiscoveryResultDto {
            skills: Json::new(json!([])),
            sources: Json::new(json!([])),
            scanned_at: "1970-01-01T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn home_roots_match_the_oracle_home_table() {
        type OracleRootRow = (
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            &'static [&'static str],
            Option<&'static str>,
        );
        let home = Path::new("/home/tester");
        let roots = home_skill_roots(home, &SkillRootOverrides::default());
        let table: &[OracleRootRow] = &[
            (
                "home-codex",
                "Codex home",
                ".codex/skills",
                "home",
                &["codex"],
                Some("codex"),
            ),
            (
                "home-agents",
                "Agent skills home",
                ".agents/skills",
                "home",
                &["agent-skills"],
                None,
            ),
            (
                "home-claude",
                "Claude home",
                ".claude/skills",
                "home",
                &["claude"],
                Some("claude"),
            ),
            (
                "codex-plugin-cache",
                "Codex plugin cache",
                ".codex/plugins/cache",
                "plugin",
                &["codex", "agent-skills"],
                Some("codex"),
            ),
            (
                "home-grok",
                "Grok home",
                ".grok/skills",
                "home",
                &["agent-skills"],
                Some("grok"),
            ),
            (
                "home-opencode",
                "OpenCode home",
                ".config/opencode/skills",
                "home",
                &["agent-skills"],
                Some("opencode"),
            ),
            (
                "home-pi",
                "Pi home",
                ".pi/agent/skills",
                "home",
                &["agent-skills"],
                Some("pi"),
            ),
            (
                "home-omp",
                "OMP home",
                ".omp/agent/skills",
                "home",
                &["agent-skills"],
                Some("omp"),
            ),
            (
                "home-hermes",
                "Hermes home",
                ".hermes/skills",
                "home",
                &["agent-skills"],
                Some("hermes"),
            ),
            (
                "home-prime-agent",
                "Prime Agent home",
                ".prime/agent/skills",
                "home",
                &["agent-skills"],
                Some("prime-agent"),
            ),
            (
                "home-gemini",
                "Gemini home",
                ".gemini/skills",
                "home",
                &["agent-skills"],
                Some("gemini"),
            ),
            (
                "home-antigravity",
                "Antigravity home",
                ".gemini/antigravity/skills",
                "home",
                &["agent-skills"],
                Some("antigravity"),
            ),
            (
                "home-cursor",
                "Cursor home",
                ".cursor/skills",
                "home",
                &["agent-skills"],
                Some("cursor"),
            ),
            (
                "home-droid",
                "Droid home",
                ".factory/skills",
                "home",
                &["agent-skills"],
                Some("droid"),
            ),
            (
                "home-continue",
                "Continue home",
                ".continue/skills",
                "home",
                &["agent-skills"],
                Some("continue"),
            ),
            (
                "home-trae",
                "Trae home",
                ".trae-cn/skills",
                "home",
                &["agent-skills"],
                Some("trae"),
            ),
            (
                "home-aug",
                "Augment home",
                ".augment/skills",
                "home",
                &["agent-skills"],
                Some("aug"),
            ),
        ];
        assert_eq!(roots.len(), table.len());
        for (root, (id, label, suffix, source_kind, providers, owner)) in roots.iter().zip(table) {
            assert_eq!(root.id, *id);
            assert_eq!(root.label, *label);
            assert_eq!(root.path, home.join(suffix));
            assert_eq!(root.source_kind, *source_kind);
            assert_eq!(root.providers, *providers);
            assert_eq!(root.owner, *owner);
        }
    }

    #[test]
    fn provider_root_overrides_replace_their_roots() {
        let home = Path::new("/home/tester");
        let overrides = SkillRootOverrides {
            claude: Some(PathBuf::from("/custom/claude/skills")),
            grok: Some(PathBuf::from("/custom/grok/skills")),
            hermes: None,
        };
        let roots = home_skill_roots(home, &overrides);
        let path_of = |id: &str| {
            roots
                .iter()
                .find(|root| root.id == id)
                .unwrap()
                .path
                .clone()
        };
        assert_eq!(
            path_of("home-claude"),
            PathBuf::from("/custom/claude/skills")
        );
        assert_eq!(path_of("home-grok"), PathBuf::from("/custom/grok/skills"));
        assert_eq!(path_of("home-hermes"), home.join(".hermes/skills"));
        assert_eq!(path_of("home-codex"), home.join(".codex/skills"));
    }

    #[test]
    fn environment_root_overrides_require_absolute_non_empty_values() {
        let overrides = resolve_provider_root_overrides(|key| match key {
            "CLAUDE_CONFIG_DIR" => Some("/config/claude".to_string()),
            "GROK_HOME" => Some("relative/ignored".to_string()),
            "HERMES_HOME" => Some("/hermes home".to_string()),
            _ => None,
        });
        assert_eq!(
            overrides.claude.as_deref(),
            Some(Path::new("/config/claude/skills"))
        );
        assert!(overrides.grok.is_none());
        assert_eq!(
            overrides.hermes.as_deref(),
            Some(Path::new("/hermes home/skills"))
        );

        let blank = resolve_provider_root_overrides(|_| Some("   ".to_string()));
        assert_eq!(blank, SkillRootOverrides::default());
    }

    #[test]
    fn skill_names_use_frontmatter_with_directory_name_fallback() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".claude/skills",
            "alpha",
            "---\nname: Alpha\n---\n# Alpha\n",
        );
        write_skill(
            home.path(),
            ".codex/skills",
            "beta",
            "---\ndescription: no name here\n---\n",
        );
        write_skill(home.path(), ".agents/skills", "gamma", "");

        let result = discover(home.path(), &SkillRootOverrides::default());
        let skills = skills_of(&result);
        assert_eq!(skills.len(), 3);
        let alpha = find_skill(&skills, "Alpha");
        assert_eq!(alpha["sourceKind"], "home");
        assert_eq!(alpha["sourceLabel"], "Claude home");
        assert_eq!(find_skill(&skills, "beta")["name"], "beta");
        assert_eq!(find_skill(&skills, "gamma")["name"], "gamma");
    }

    #[test]
    fn frontmatter_name_must_appear_within_the_first_twenty_lines() {
        assert_eq!(
            parse_skill_name("---\nname: \"Quoted Name\"\n---\n"),
            Some("Quoted Name".to_string())
        );
        assert_eq!(parse_skill_name("name: Not Frontmatter\n"), None);
        assert_eq!(parse_skill_name("# Heading\n"), None);
        assert_eq!(parse_skill_name("---\nname:\n---\n"), None);
        assert_eq!(parse_skill_name("---\n---\nname: After fence\n"), None);

        let mut late = String::from("---\n");
        for index in 0..19 {
            late.push_str(&format!("key{index}: value\n"));
        }
        late.push_str("name: Late\n---\n");
        assert_eq!(parse_skill_name(&late), None);

        let mut early = String::from("---\nname: Early\n");
        for index in 0..25 {
            early.push_str(&format!("key{index}: value\n"));
        }
        early.push_str("---\n");
        assert_eq!(parse_skill_name(&early), Some("Early".to_string()));
    }

    #[test]
    fn dot_directories_are_skipped_and_system_skills_are_bundled() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".claude/skills",
            ".hidden",
            "---\nname: Hidden\n---\n",
        );
        write_skill(
            home.path(),
            ".claude/skills/.system",
            "review",
            "---\nname: Review\n---\n",
        );
        write_skill(
            home.path(),
            ".claude/skills",
            "visible",
            "---\nname: Visible\n---\n",
        );

        let result = discover(home.path(), &SkillRootOverrides::default());
        let skills = skills_of(&result);
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().all(|skill| skill["name"] != "Hidden"));
        let review = find_skill(&skills, "Review");
        assert_eq!(review["sourceKind"], "bundled");
        assert_eq!(review["sourceLabel"], "Claude home bundled");
        assert_eq!(
            PathBuf::from(review["directoryPath"].as_str().unwrap()),
            home.path().join(".claude/skills/.system/review")
        );
        assert_eq!(find_skill(&skills, "Visible")["sourceKind"], "home");
    }

    #[test]
    fn missing_roots_report_exists_false_and_emit_no_skills() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".codex/skills",
            "alpha",
            "---\nname: Alpha\n---\n",
        );

        let result = discover(home.path(), &SkillRootOverrides::default());
        let sources = sources_of(&result);
        assert_eq!(sources.len(), 17);
        let codex = find_source(&sources, "home-codex");
        assert_eq!(codex["exists"], true);
        assert!(codex.get("skippedReason").is_none());
        let claude = find_source(&sources, "home-claude");
        assert_eq!(claude["exists"], false);
        assert_eq!(claude["skippedReason"], "missing");
        assert_eq!(skills_of(&result).len(), 1);
    }

    #[test]
    fn skills_sort_by_name_case_insensitively() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".codex/skills",
            "zulu",
            "---\nname: Zebra\n---\n",
        );
        write_skill(
            home.path(),
            ".claude/skills",
            "first",
            "---\nname: apple\n---\n",
        );
        write_skill(
            home.path(),
            ".agents/skills",
            "second",
            "---\nname: Banana\n---\n",
        );

        let result = discover(home.path(), &SkillRootOverrides::default());
        let skills = skills_of(&result);
        let names: Vec<&str> = skills
            .iter()
            .map(|skill| skill["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["apple", "Banana", "Zebra"]);
    }

    #[test]
    fn shared_skill_files_are_deduplicated_across_roots() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".codex/skills",
            "alpha",
            "---\nname: Alpha\n---\n",
        );
        let overrides = SkillRootOverrides {
            claude: Some(home.path().join(".codex/skills")),
            ..SkillRootOverrides::default()
        };

        let result = discover(home.path(), &overrides);
        let skills = skills_of(&result);
        assert_eq!(skills.len(), 1);
        // Root order decides the surviving row: codex precedes claude.
        assert_eq!(skills[0]["sourceLabel"], "Codex home");
        assert_eq!(skills[0]["sourceKind"], "home");
    }

    #[test]
    fn cache_serves_repeats_within_the_ttl_and_rescans_after_expiry() {
        let scans = Cell::new(0);
        let mut cache = SkillDiscoveryCache::new(SKILL_DISCOVERY_TTL);
        let now = Instant::now();
        let scan = || {
            scans.set(scans.get() + 1);
            empty_result()
        };

        cache.get_or_scan(false, now, scan);
        cache.get_or_scan(false, now + Duration::from_secs(9), scan);
        assert_eq!(scans.get(), 1, "a repeat inside the TTL must not rescan");

        cache.get_or_scan(false, now + SKILL_DISCOVERY_TTL, scan);
        assert_eq!(scans.get(), 2, "the entry expires at the TTL boundary");
    }

    #[test]
    fn refresh_bypasses_a_fresh_cache_entry() {
        let scans = Cell::new(0);
        let mut cache = SkillDiscoveryCache::new(SKILL_DISCOVERY_TTL);
        let now = Instant::now();
        let scan = || {
            scans.set(scans.get() + 1);
            empty_result()
        };

        cache.get_or_scan(false, now, scan);
        cache.get_or_scan(true, now + Duration::from_millis(1), scan);
        assert_eq!(scans.get(), 2, "refresh must scan even inside the TTL");

        cache.get_or_scan(false, now + Duration::from_millis(2), scan);
        assert_eq!(scans.get(), 2, "the refreshed entry becomes the cached one");
    }

    #[test]
    fn discovery_serializes_the_renderer_contract_fields() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".claude/skills",
            "alpha",
            "---\nname: Alpha\ndescription: Test\n---\n",
        );

        let result = discover(home.path(), &SkillRootOverrides::default());
        let value = serde_json::to_value(&result).unwrap();
        let scanned_at = value["scannedAt"].as_str().unwrap();
        assert!(
            scanned_at.ends_with('Z'),
            "scannedAt must be an ISO-8601 UTC string"
        );
        let skill = &value["skills"][0];
        assert!(skill["id"].is_string());
        assert_eq!(skill["name"], "Alpha");
        assert!(skill["description"].is_null());
        assert_eq!(skill["providers"], json!(["claude"]));
        assert_eq!(skill["sourceKind"], "home");
        assert_eq!(skill["sourceLabel"], "Claude home");
        assert_eq!(skill["installed"], true);
        assert!(skill["updatedAt"].is_null());
        assert_eq!(
            PathBuf::from(skill["directoryPath"].as_str().unwrap()),
            home.path().join(".claude/skills/alpha")
        );
        assert_eq!(
            PathBuf::from(skill["skillFilePath"].as_str().unwrap()),
            home.path().join(".claude/skills/alpha/SKILL.md")
        );
        assert_eq!(
            PathBuf::from(skill["rootPath"].as_str().unwrap()),
            home.path().join(".claude/skills")
        );
        assert_eq!(value["sources"].as_array().unwrap().len(), 17);
    }

    #[test]
    fn iso8601_formats_known_instants() {
        assert_eq!(iso8601_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso8601_utc(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123)),
            "2023-11-14T22:13:20.123Z"
        );
    }
}
