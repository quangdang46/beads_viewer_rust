//! Agent file detection — port of Go `pkg/agents/detect.go`.
//!
//! Detects AGENTS.md/CLAUDE.md in the current directory or parent directories,
//! and checks for existing blurb content.

use super::{contains_any_blurb, contains_legacy_blurb, get_blurb_version, SUPPORTED_AGENT_FILES};

/// Detection result for an agent configuration file.
#[derive(Debug, Default)]
pub struct AgentFileDetection {
    pub file_path: String,
    pub file_type: String,
    pub has_blurb: bool,
    pub has_legacy_blurb: bool,
    pub blurb_version: i32,
}

impl AgentFileDetection {
    pub fn found(&self) -> bool {
        !self.file_path.is_empty()
    }

    pub fn needs_blurb(&self) -> bool {
        self.found() && !self.has_blurb
    }

    pub fn needs_upgrade(&self) -> bool {
        self.has_legacy_blurb || (self.has_blurb && self.blurb_version < super::BLURB_VERSION)
    }
}

/// Detect agent file in current directory.
pub fn detect_agent_file(work_dir: &std::path::Path) -> AgentFileDetection {
    // Try uppercase variants first (AGENTS.md, CLAUDE.md)
    for filename in SUPPORTED_AGENT_FILES.iter() {
        if !filename.starts_with(|c: char| c.is_uppercase()) {
            continue;
        }
        let file_path = work_dir.join(filename);
        if let Some(detection) = check_agent_file(&file_path, filename) {
            return detection;
        }
    }
    // Try lowercase variants as fallback
    for filename in SUPPORTED_AGENT_FILES.iter() {
        if filename.starts_with(|c: char| c.is_uppercase()) {
            continue;
        }
        let file_path = work_dir.join(filename);
        if let Some(detection) = check_agent_file(&file_path, filename) {
            return detection;
        }
    }
    AgentFileDetection::default()
}

/// Detect agent file walking up parent directories (max 3 levels).
pub fn detect_agent_file_in_parents(
    work_dir: &std::path::Path,
    max_levels: usize,
) -> AgentFileDetection {
    let mut current_dir = work_dir.to_path_buf();
    for _ in 0..=max_levels {
        let detection = detect_agent_file(&current_dir);
        if detection.found() {
            return detection;
        }
        if !current_dir.pop() {
            break;
        }
    }
    AgentFileDetection::default()
}

fn check_agent_file(file_path: &std::path::Path, file_type: &str) -> Option<AgentFileDetection> {
    let metadata = std::fs::metadata(file_path).ok()?;
    if metadata.is_dir() {
        return None;
    }
    let content = std::fs::read_to_string(file_path).ok()?;
    let has_legacy = contains_legacy_blurb(&content);
    Some(AgentFileDetection {
        file_path: file_path.to_string_lossy().to_string(),
        file_type: file_type.to_string(),
        has_blurb: contains_any_blurb(&content),
        has_legacy_blurb: has_legacy,
        blurb_version: get_blurb_version(&content),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn detect_nonexistent_returns_empty() {
        let detection = detect_agent_file(&PathBuf::from("/nonexistent/path"));
        assert!(!detection.found());
    }

    #[test]
    fn detect_agents_md_if_exists() {
        let tmp = std::env::temp_dir().join("bvr_test_agents_detect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("AGENTS.md"), "# Project\n\n").unwrap();
        let detection = detect_agent_file(&tmp);
        assert!(detection.found());
        assert_eq!(detection.file_type, "AGENTS.md");
        assert!(!detection.has_blurb);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_agents_md_with_blurb() {
        let tmp = std::env::temp_dir().join("bvr_test_agents_blurb");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let content = "# Project\n\n<!-- bv-agent-instructions-v3 -->\nsome content\n<!-- end-bv-agent-instructions -->";
        fs::write(tmp.join("AGENTS.md"), content).unwrap();
        let detection = detect_agent_file(&tmp);
        assert!(detection.found());
        assert!(detection.has_blurb);
        assert_eq!(detection.blurb_version, 3);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_in_parents_finds_root() {
        let root = std::env::temp_dir().join("bvr_test_parent_root");
        let child = root.join("a").join("b").join("c");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&child).unwrap();
        fs::write(root.join("AGENTS.md"), "# Project\n\n").unwrap();
        let detection = detect_agent_file_in_parents(&child, 5);
        assert!(detection.found());
        let _ = fs::remove_dir_all(&root);
    }
}
