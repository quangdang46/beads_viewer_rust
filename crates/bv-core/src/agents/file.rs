//! Agent file operations — port of Go `pkg/agents/file.go`.
//!
//! Append, update, and remove blurb content in agent configuration files.

use super::{AGENT_BLURB, BLURB_END_MARKER, BLURB_START_MARKER};

/// Append blurb to file (creates if missing).
pub fn append_blurb_to_file(path: &std::path::Path) -> Result<(), String> {
    let content = if path.exists() {
        std::fs::read_to_string(path).map_err(|e| format!("read file: {e}"))?
    } else {
        String::new()
    };
    let new_content = super::append_blurb(&content);
    atomic_write(path, new_content.as_bytes())
        .map_err(|e| format!("write file: {e}"))
}

/// Update blurb in file (replace existing with current version).
pub fn update_blurb_in_file(path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read file: {e}"))?;
    let new_content = super::update_blurb(&content);
    atomic_write(path, new_content.as_bytes())
        .map_err(|e| format!("write file: {e}"))
}

/// Remove blurb from file.
pub fn remove_blurb_from_file(path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read file: {e}"))?;
    let new_content = super::remove_blurb(&content);
    atomic_write(path, new_content.as_bytes())
        .map_err(|e| format!("write file: {e}"))
}

/// Create a new agent file with the blurb.
pub fn create_agent_file(path: &std::path::Path) -> Result<(), String> {
    let content = super::append_blurb("# Agent Configuration\n\n");
    atomic_write(path, content.as_bytes())
        .map_err(|e| format!("create file: {e}"))
}

/// Verify blurb is present in file.
pub fn verify_blurb_present(path: &std::path::Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read file: {e}"))?;
    Ok(super::verify_blurb_present(&content))
}

/// Atomic write: write to temp file, then rename. Prevents corruption on crash.
fn atomic_write(path: &std::path::Path, data: &[u8]) -> Result<(), std::io::Error> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn create_and_verify() {
        let tmp = PathBuf::from(std::env::temp_dir()).join("bvr_test_create_agent.md");
        let _ = std::fs::remove_file(&tmp);
        create_agent_file(&tmp).unwrap();
        assert!(verify_blurb_present(&tmp).unwrap());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn append_and_remove_roundtrip() {
        let tmp = PathBuf::from(std::env::temp_dir()).join("bvr_test_roundtrip.md");
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(&tmp, "# My Project\n\n").unwrap();
        append_blurb_to_file(&tmp).unwrap();
        assert!(verify_blurb_present(&tmp).unwrap());
        remove_blurb_from_file(&tmp).unwrap();
        assert!(!verify_blurb_present(&tmp).unwrap());
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("# My Project"));
        let _ = std::fs::remove_file(&tmp);
    }
}
