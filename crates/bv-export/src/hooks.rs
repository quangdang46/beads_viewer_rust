//! Export hooks — port of Go `pkg/hooks`: pre/post-export shell commands.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_on_error")]
    pub on_error: OnError,
}

fn default_timeout() -> u64 {
    30
}
fn default_on_error() -> OnError {
    OnError::Fail
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnError {
    #[serde(rename = "fail")]
    Fail,
    #[serde(rename = "continue")]
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_export: Vec<Hook>,
    #[serde(default)]
    pub post_export: Vec<Hook>,
}

/// Environment passed to hook commands.
pub fn hook_env(path: &str, format: &str, count: usize) -> Vec<(String, String)> {
    vec![
        ("BV_EXPORT_PATH".into(), path.into()),
        ("BV_EXPORT_FORMAT".into(), format.into()),
        ("BV_ISSUE_COUNT".into(), count.to_string()),
        ("BV_TIMESTAMP".into(), jiff::Timestamp::now().to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_env_has_four_vars() {
        let env = hook_env("/tmp/out.md", "markdown", 42);
        assert_eq!(env.len(), 4);
        assert!(env
            .iter()
            .any(|(k, v)| k == "BV_EXPORT_PATH" && v == "/tmp/out.md"));
        assert!(env.iter().any(|(k, v)| k == "BV_ISSUE_COUNT" && v == "42"));
    }
}
