//! `orcinus skills get <topic>` (spec §3.6/§3.7): serve the embedded,
//! fork-owned guide. `--json` returns `{name, full, markdown}`; an unknown
//! topic is `invalid_argument` listing the available topics.

use serde_json::{json, Value};

use crate::errors::CliError;

/// Build-time embedded guide (spec §3.7); the crate lives at
/// `src-tauri/crates/orcinus-cli/src/`, so the repo root is four levels up.
pub const COMPUTER_USE_GUIDE: &str = include_str!("../../../../skill-guides/computer-use.md");

const COMPUTER_USE_TOPIC: &str = "computer-use";

/// The guide body for a topic. `--full` is accepted for oracle parity; the
/// embedded fork guide has no separate reference table, so both forms return
/// the same markdown with `full: true`.
#[derive(Debug)]
pub struct Guide {
    pub name: &'static str,
    pub markdown: &'static str,
}

impl Guide {
    pub fn json(&self) -> Value {
        json!({
            "name": self.name,
            "full": true,
            "markdown": self.markdown,
        })
    }
}

/// Resolve `topic` to its guide; unknown topics list what is available.
pub fn get(topic: &str) -> Result<Guide, CliError> {
    match topic {
        COMPUTER_USE_TOPIC => Ok(Guide {
            name: COMPUTER_USE_TOPIC,
            markdown: COMPUTER_USE_GUIDE,
        }),
        other => Err(CliError::new(
            "invalid_argument",
            format!("Unknown skill \"{other}\". Available skills: {COMPUTER_USE_TOPIC}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computer_use_guide_is_embedded_and_non_empty() {
        let guide = get("computer-use").expect("guide resolves");
        assert!(!guide.markdown.trim().is_empty());
        assert!(guide.markdown.contains("# Computer Use"));
        assert!(!guide.markdown.contains("ORCA "));
    }

    #[test]
    fn json_shape_is_name_full_markdown() {
        let value = get("computer-use").expect("guide resolves").json();
        assert_eq!(value["name"], "computer-use");
        assert_eq!(value["full"], true);
        assert!(value["markdown"].as_str().is_some_and(|md| !md.is_empty()));
    }

    #[test]
    fn unknown_topic_lists_available_topics() {
        let error = get("browsing").unwrap_err();
        assert_eq!(error.code, "invalid_argument");
        assert!(error.message.contains("browsing"));
        assert!(error.message.contains("computer-use"));
    }
}
