//! Correlation feedback store — port of Go `pkg/correlation/feedback.go`:
//! JSONL append-only at <beadsDir>/correlation_feedback.jsonl, key
//! (commit_sha, bead_id), last-write-wins.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrelationFeedback {
    pub commit_sha: String,
    pub bead_id: String,
    #[serde(rename = "feedback_at")]
    pub feedback_at: String,
    /// Go's tag is `json:"feedback_by"` with no `omitempty`, so the key is
    /// always present even when empty — it is the default `--correlation-by`
    /// fallback's companion in `--robot-explain-correlation`'s `feedback` object.
    #[serde(default)]
    pub feedback_by: String,
    /// confirm | reject | ignore
    #[serde(rename = "type")]
    pub feedback_type: String,
    /// Go's tag is `json:"reason"` with no `omitempty` either.
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub original_conf: f64,
}

pub const FEEDBACK_FILE: &str = "correlation_feedback.jsonl";

/// Go `FeedbackStats` — aggregate statistics about correlation feedback.
///
/// Field order matches Go's struct declaration (types.go:304-312), which is the
/// wire order of `--robot-correlation-stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct FeedbackStats {
    pub total_feedback: usize,
    pub confirmed: usize,
    pub rejected: usize,
    pub ignored: usize,
    /// `confirmed / (confirmed + rejected)`; `0.0` when no decision was made.
    pub accuracy_rate: f64,
    /// Mean `original_conf` over confirmations; `0.0` when there are none.
    pub avg_confirm_conf: f64,
    /// Mean `original_conf` over rejections; `0.0` when there are none.
    pub avg_reject_conf: f64,
}

/// Append-only feedback store.
pub struct FeedbackStore {
    path: PathBuf,
}

impl FeedbackStore {
    pub fn new(beads_dir: &Path) -> Self {
        FeedbackStore {
            path: beads_dir.join(FEEDBACK_FILE),
        }
    }

    pub fn record(&self, fb: &CorrelationFeedback) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(fb)?;
        line.push('\n');
        f.write_all(line.as_bytes())
    }

    /// Load all feedback; last-write-wins per (sha, bead).
    pub fn load_all(&self) -> HashMap<(String, String), CorrelationFeedback> {
        let mut map = HashMap::new();
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return map;
        };
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(fb) = serde_json::from_str::<CorrelationFeedback>(line) {
                map.insert((fb.commit_sha.clone(), fb.bead_id.clone()), fb);
            }
        }
        map
    }

    /// Go `FeedbackStats`: accuracy = confirmed / (confirmed + rejected).
    pub fn stats(&self) -> (usize, usize, usize, Option<f64>) {
        let all = self.load_all();
        let mut confirmed = 0;
        let mut rejected = 0;
        let mut ignored = 0;
        for fb in all.values() {
            match fb.feedback_type.as_str() {
                "confirm" => confirmed += 1,
                "reject" => rejected += 1,
                _ => ignored += 1,
            }
        }
        let accuracy = if confirmed + rejected > 0 {
            Some(confirmed as f64 / (confirmed + rejected) as f64)
        } else {
            None
        };
        (confirmed, rejected, ignored, accuracy)
    }

    /// Go `sortedFeedbackLocked` — the store's values in semantic key order
    /// `(commit_sha, bead_id)`. Go sorts before aggregating so the float sums in
    /// [`Self::get_stats`] do not depend on Go map iteration order; this is the
    /// same guarantee.
    pub fn sorted_feedback(&self) -> Vec<CorrelationFeedback> {
        let mut all: Vec<CorrelationFeedback> = self.load_all().into_values().collect();
        all.sort_by(|a, b| {
            a.commit_sha
                .cmp(&b.commit_sha)
                .then_with(|| a.bead_id.cmp(&b.bead_id))
        });
        all
    }

    /// Go `FeedbackStore.GetStats` — the whole `--robot-correlation-stats` body.
    ///
    /// Go reports `0.0`, not "absent", for the rate and the two averages when
    /// their denominators are zero, so every field is a plain `f64` here too.
    pub fn get_stats(&self) -> FeedbackStats {
        let mut stats = FeedbackStats::default();
        let mut confirm_sum = 0.0f64;
        let mut reject_sum = 0.0f64;

        for fb in self.sorted_feedback() {
            stats.total_feedback += 1;
            match fb.feedback_type.as_str() {
                "confirm" => {
                    stats.confirmed += 1;
                    confirm_sum += fb.original_conf;
                }
                "reject" => {
                    stats.rejected += 1;
                    reject_sum += fb.original_conf;
                }
                "ignore" => stats.ignored += 1,
                // Go's switch has no default: an unrecognised type still counts
                // toward TotalFeedback and lands in none of the three buckets.
                _ => {}
            }
        }

        let decisions = stats.confirmed + stats.rejected;
        if decisions > 0 {
            stats.accuracy_rate = stats.confirmed as f64 / decisions as f64;
        }
        if stats.confirmed > 0 {
            stats.avg_confirm_conf = confirm_sum / stats.confirmed as f64;
        }
        if stats.rejected > 0 {
            stats.avg_reject_conf = reject_sum / stats.rejected as f64;
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_load_last_write_wins() {
        let dir = std::env::temp_dir().join(format!("bvr-fb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = FeedbackStore::new(&dir);

        store
            .record(&CorrelationFeedback {
                commit_sha: "abc".into(),
                bead_id: "X-1".into(),
                feedback_at: "2026-01-01T00:00:00Z".into(),
                feedback_by: "tester".into(),
                feedback_type: "reject".into(),
                reason: String::new(),
                original_conf: 0.8,
            })
            .unwrap();
        store
            .record(&CorrelationFeedback {
                commit_sha: "abc".into(),
                bead_id: "X-1".into(),
                feedback_at: "2026-01-02T00:00:00Z".into(),
                feedback_by: "tester".into(),
                feedback_type: "confirm".into(),
                reason: "verified".into(),
                original_conf: 0.8,
            })
            .unwrap();

        let all = store.load_all();
        assert_eq!(all.len(), 1); // deduped by (sha,bead)
        assert_eq!(all[&("abc".into(), "X-1".into())].feedback_type, "confirm");

        let (c, r, _i, acc) = store.stats();
        assert_eq!((c, r), (1, 0));
        assert_eq!(acc, Some(1.0));

        std::fs::remove_dir_all(&dir).ok();
    }
}
