//! Scoring constants + composite impact scoring — port of Go
//! `pkg/analysis/priority.go` (weights) and triage factors.

use serde::Serialize;

// === Impact weights (Go priority.go:55-62) — DO NOT ALTER ===
pub const WEIGHT_PAGE_RANK: f64 = 0.22;
pub const WEIGHT_BETWEENNESS: f64 = 0.20;
pub const WEIGHT_BLOCKER_RATIO: f64 = 0.13;
pub const WEIGHT_STALENESS: f64 = 0.05;
pub const WEIGHT_PRIORITY_BOOST: f64 = 0.10;
pub const WEIGHT_TIME_TO_IMPACT: f64 = 0.10;
pub const WEIGHT_URGENCY: f64 = 0.10;
pub const WEIGHT_RISK: f64 = 0.10;

/// Weighted contribution of each component (Go `ScoreBreakdown`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScoreBreakdown {
    pub pagerank: f64,
    pub betweenness: f64,
    pub blocker_ratio: f64,
    pub staleness: f64,
    pub priority_boost: f64,
    pub time_to_impact: f64,
    pub urgency: f64,
    pub risk: f64,
}

/// Composite impact score (Go `ImpactScore`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImpactScore {
    pub score: f64,
    pub breakdown: ScoreBreakdown,
}

/// Compute weighted impact from normalized component values (each 0..1).
#[allow(clippy::too_many_arguments)]
pub fn compute_impact(
    pagerank_norm: f64,
    betweenness_norm: f64,
    blocker_ratio_norm: f64,
    staleness_norm: f64,
    priority_boost_norm: f64,
    time_to_impact_norm: f64,
    urgency_norm: f64,
    risk_norm: f64,
) -> ImpactScore {
    let b = ScoreBreakdown {
        pagerank: pagerank_norm * WEIGHT_PAGE_RANK,
        betweenness: betweenness_norm * WEIGHT_BETWEENNESS,
        blocker_ratio: blocker_ratio_norm * WEIGHT_BLOCKER_RATIO,
        staleness: staleness_norm * WEIGHT_STALENESS,
        priority_boost: priority_boost_norm * WEIGHT_PRIORITY_BOOST,
        time_to_impact: time_to_impact_norm * WEIGHT_TIME_TO_IMPACT,
        urgency: urgency_norm * WEIGHT_URGENCY,
        risk: risk_norm * WEIGHT_RISK,
    };
    ImpactScore {
        score: b.pagerank
            + b.betweenness
            + b.blocker_ratio
            + b.staleness
            + b.priority_boost
            + b.time_to_impact
            + b.urgency
            + b.risk,
        breakdown: b,
    }
}

/// Go: `scoreToPriority` — impact score (0-1) -> P0..P4.
pub fn score_to_priority(score: f64) -> i32 {
    if score >= 0.7 {
        0
    } else if score >= 0.5 {
        1
    } else if score >= 0.3 {
        2
    } else if score >= 0.15 {
        3
    } else {
        4
    }
}

/// Go: `priorityToScore`.
pub fn priority_to_score(priority: i32) -> f64 {
    match priority {
        0 => 0.8,
        1 => 0.6,
        2 => 0.4,
        3 => 0.2,
        _ => 0.1,
    }
}

/// Triage combination weights (Go triage.go ~1204): base + boosts.
pub const TRIAGE_BASE_WEIGHT: f64 = 0.70;
pub const TRIAGE_UNBLOCK_BOOST_WEIGHT: f64 = 0.15;
pub const TRIAGE_QUICK_WIN_WEIGHT: f64 = 0.15;
/// unblocks threshold for the unblock boost.
pub const TRIAGE_UNBLOCK_THRESHOLD: usize = 5;

/// QuickWin sub-score (Go triage.go ~899): unblock .4 + simplicity .4 + prio .2.
pub fn quickwin_score(unblocks: usize, blocker_ratio_norm: f64, priority: i32) -> f64 {
    let unblock_impact = (unblocks as f64 + 1.0).log2().min(1.0);
    let simplicity = if blocker_ratio_norm < 0.2 {
        1.0
    } else if blocker_ratio_norm < 0.4 {
        0.5
    } else {
        0.0
    };
    let prio_bonus = match priority {
        0 | 1 => 1.0,
        2 => 0.5,
        _ => 0.0,
    };
    (unblock_impact * 0.4) + (simplicity * 0.4) + (prio_bonus * 0.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn weights_sum_to_one() {
        let total = WEIGHT_PAGE_RANK
            + WEIGHT_BETWEENNESS
            + WEIGHT_BLOCKER_RATIO
            + WEIGHT_STALENESS
            + WEIGHT_PRIORITY_BOOST
            + WEIGHT_TIME_TO_IMPACT
            + WEIGHT_URGENCY
            + WEIGHT_RISK;
        assert!(close(total, 1.0), "weights sum to {total}");
    }

    #[test]
    fn compute_impact_weighting_exact() {
        let r = compute_impact(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        assert!(close(r.score, 1.0));
        assert!(close(r.breakdown.pagerank, 0.22));
        assert!(close(r.breakdown.betweenness, 0.20));
        assert!(close(r.breakdown.blocker_ratio, 0.13));
        assert!(close(r.breakdown.staleness, 0.05));
        // all-zero input
        let z = compute_impact(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!(close(z.score, 0.0));
    }

    #[test]
    fn score_to_priority_thresholds() {
        assert_eq!(score_to_priority(0.70), 0);
        assert_eq!(score_to_priority(0.9), 0);
        assert_eq!(score_to_priority(0.5), 1);
        assert_eq!(score_to_priority(0.3), 2);
        assert_eq!(score_to_priority(0.15), 3);
        assert_eq!(score_to_priority(0.14), 4);
        assert_eq!(score_to_priority(0.0), 4);
    }

    #[test]
    fn priority_to_score_monotone() {
        for p in 0..4 {
            assert!(priority_to_score(p) > priority_to_score(p + 1));
        }
    }

    #[test]
    fn quickwin_formula_matches_go() {
        // log2(unblocks+1)*.4 with unblocks=0 -> 0; simplicity=1; prio P0 -> 1.0
        // qw = 0*.4 + 1*.4 + 1*.2 = 0.6
        let qw = quickwin_score(0, 0.1, 0);
        assert!(close(qw, 0.6));
        // unblocks=15 -> log2(16)=4 capped at 1 → 0.4 + 0.5*0.4 + 0 = 0.6
        let qw2 = quickwin_score(15, 0.3, 3);
        assert!(close(qw2, 0.6));
    }
}
