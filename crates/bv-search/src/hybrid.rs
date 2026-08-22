//! Hybrid search scorer — port of Go `pkg/search/hybrid_scorer.go` +
//! `presets.go`: text relevance blended with graph metrics.

use serde::Serialize;

/// Preset weight configurations (Go `presets.go` — exact values).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Weights {
    pub text_relevance: f64,
    pub pagerank: f64,
    pub status: f64,
    pub impact: f64,
    pub priority: f64,
    pub recency: f64,
}

pub const PRESETS: &[(&str, Weights)] = &[
    (
        "default",
        Weights {
            text_relevance: 0.40,
            pagerank: 0.20,
            status: 0.15,
            impact: 0.10,
            priority: 0.10,
            recency: 0.05,
        },
    ),
    (
        "bug-hunting",
        Weights {
            text_relevance: 0.30,
            pagerank: 0.15,
            status: 0.15,
            impact: 0.15,
            priority: 0.20,
            recency: 0.05,
        },
    ),
    (
        "sprint-planning",
        Weights {
            text_relevance: 0.30,
            pagerank: 0.20,
            status: 0.25,
            impact: 0.15,
            priority: 0.05,
            recency: 0.05,
        },
    ),
    (
        "impact-first",
        Weights {
            text_relevance: 0.25,
            pagerank: 0.30,
            status: 0.10,
            impact: 0.20,
            priority: 0.10,
            recency: 0.05,
        },
    ),
    (
        "text-only",
        Weights {
            text_relevance: 1.00,
            pagerank: 0.00,
            status: 0.00,
            impact: 0.00,
            priority: 0.00,
            recency: 0.00,
        },
    ),
];

/// Get preset weights by name.
pub fn get_preset(name: &str) -> Option<Weights> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, w)| *w)
}

/// Normalized component scores for a single candidate issue.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ComponentScores {
    pub status: f64,
    pub priority: f64,
    pub recency: f64,
}

impl ComponentScores {
    /// Go normalizers: open=1.0→tombstone=0.0; P0=1.0→P4=0.2; exp(-days/30).
    pub fn new(status_str: &str, prio: i32, days_since_update: f64) -> Self {
        let status = match status_str.trim().to_lowercase().as_str() {
            "open" => 1.0,
            "in_progress" => 0.8,
            "blocked" => 0.6,
            "draft" => 0.4,
            "review" => 0.3,
            "deferred" => 0.3,
            "closed" | "tombstone" => 0.0,
            _ => 0.5,
        };
        let priority = match prio {
            0 => 1.0,
            1 => 0.8,
            2 => 0.6,
            3 => 0.4,
            _ => 0.2,
        };
        let recency = (-days_since_update / 30.0).exp();
        ComponentScores {
            status,
            priority,
            recency,
        }
    }
}

/// Hybrid score for a single result.
#[derive(Debug, Clone, Serialize)]
pub struct HybridResult {
    pub issue_id: String,
    pub score: f64,
    #[serde(rename = "text_score")]
    pub text_score: f64,
    pub component_scores: ComponentScores,
}

/// Compute hybrid score.
pub fn hybrid_score(text_score: f64, weights: &Weights, components: &ComponentScores) -> f64 {
    // Graph metrics are pre-normalized by the caller (pagerank/betweenness/impact).
    // For this module we use placeholder graph scores of 0 — the caller injects
    // real PageRank etc. from bv_analysis stats.
    weights.text_relevance * text_score
        + weights.pagerank * 0.0
        + weights.status * components.status
        + weights.impact * 0.0
        + weights.priority * components.priority
        + weights.recency * components.recency
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_weights() {
        let w = get_preset("default").unwrap();
        assert!((w.text_relevance - 0.40).abs() < 1e-9);
        assert!((w.pagerank - 0.20).abs() < 1e-9);
        assert!((w.status - 0.15).abs() < 1e-9);
        assert!((w.impact - 0.10).abs() < 1e-9);
        assert!((w.priority - 0.10).abs() < 1e-9);
        assert!((w.recency - 0.05).abs() < 1e-9);
    }

    #[test]
    fn all_presets_sum_to_one() {
        for (name, w) in PRESETS {
            let total =
                w.text_relevance + w.pagerank + w.status + w.impact + w.priority + w.recency;
            assert!((total - 1.0).abs() < 1e-6, "preset {name} sums to {total}");
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(get_preset("nonexistent").is_none());
    }

    #[test]
    fn all_five_preset_names_exist() {
        let names: Vec<&str> = PRESETS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), 5);
        for expected in [
            "default",
            "bug-hunting",
            "sprint-planning",
            "impact-first",
            "text-only",
        ] {
            assert!(names.contains(&expected), "missing preset: {expected}");
        }
    }

    #[test]
    fn text_only_weights_zero_out_graph_signals() {
        let w = get_preset("text-only").unwrap();
        assert_eq!(w.pagerank, 0.0);
        assert_eq!(w.status, 0.0);
        assert_eq!(w.impact, 0.0);
    }

    #[test]
    fn hybrid_score_with_text_only_uses_text_exclusively() {
        let w = get_preset("text-only").unwrap();
        let comps = ComponentScores::new("open", 2, 5.0);
        let score = hybrid_score(0.8, &w, &comps);
        assert!((score - 0.8).abs() < 1e-9);
    }

    #[test]
    fn component_normalizers_match_go() {
        let c = ComponentScores::new("open", 0, 0.0);
        assert!((c.status - 1.0).abs() < 1e-9);
        assert!((c.priority - 1.0).abs() < 1e-9);
        assert!((c.recency - 1.0).abs() < 1e-9);

        let c2 = ComponentScores::new("tombstone", 4, 90.0);
        assert!(c2.status.abs() < 1e-9);
        assert!((c2.priority - 0.2).abs() < 1e-9);
        assert!(c2.recency < 0.05);
    }
}
