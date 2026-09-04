//! robot-impact — file-based impact analysis.
//! Port of Go `cmd/bv/robot_registry.go` handleRobotImpact.

use bv_correlation::correlator::CorrelatedCommit;

pub fn compute_file_impact(
    files: &[String],
    report: &std::collections::BTreeMap<String, Vec<CorrelatedCommit>>,
) -> FileImpactResult {
    let mut affected: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for (bead_id, commits) in report {
        for commit in commits {
            for file in &commit.files {
                if files.iter().any(|f| file.contains(f.as_str())) {
                    affected
                        .entry(bead_id.clone())
                        .or_default()
                        .push(file.clone());
                }
            }
        }
    }

    let total_affected = affected.len();
    let risk_level = if total_affected > 10 {
        "high"
    } else if total_affected > 3 {
        "medium"
    } else {
        "low"
    };

    FileImpactResult {
        files: files.to_vec(),
        affected_beads: affected
            .into_iter()
            .map(|(bead_id, overlap_files)| AffectedBead {
                bead_id,
                overlap_files,
            })
            .collect(),
        risk_level: risk_level.to_string(),
        risk_score: (total_affected as f64 / 10.0).min(1.0),
        summary: format!(
            "{} beads affected across {} files",
            total_affected,
            files.len()
        ),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileImpactResult {
    pub files: Vec<String>,
    pub affected_beads: Vec<AffectedBead>,
    pub risk_level: String,
    pub risk_score: f64,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AffectedBead {
    pub bead_id: String,
    pub overlap_files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn impact_empty_files() {
        let report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();
        let result = compute_file_impact(&[], &report);
        assert!(result.affected_beads.is_empty());
    }

    #[test]
    fn impact_finds_affected_beads() {
        let mut report: BTreeMap<String, Vec<CorrelatedCommit>> = BTreeMap::new();
        report.insert(
            "bead-1".into(),
            vec![CorrelatedCommit {
                sha: "abc123".into(),
                bead_id: "bead-1".into(),
                confidence: 0.8,
                methods: vec!["explicit_id"],
                reason: "ID in commit".into(),
                timestamp: "2026-01-01T00:00:00Z".into(),
                author: "dev".into(),
                files: vec!["src/main.rs".into()],
            }],
        );
        let result = compute_file_impact(&["src/main.rs".to_string()], &report);
        assert_eq!(result.affected_beads.len(), 1);
    }
}
