//! Explicit-ID correlation — port of Go `pkg/correlation/explicit.go`:
//! builtin ID patterns, match classification, confidence calculation.

use regex::Regex;

/// Match classification (Go matchType strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Closes,
    Fixes,
    Resolves,
    Bracket,
    Refs,
    Bead,
}

/// Go: `CalculateConfidence` — exact bonus/penalty table.
pub fn calculate_confidence(match_kind: Option<MatchKind>, total_matches: usize) -> f64 {
    let mut base = 0.90f64;
    match match_kind {
        Some(MatchKind::Closes) | Some(MatchKind::Fixes) | Some(MatchKind::Resolves) => {
            base += 0.05
        }
        Some(MatchKind::Bracket) => base += 0.02,
        Some(MatchKind::Refs) => base += 0.01,
        Some(MatchKind::Bead) => base += 0.03,
        None => {}
    }
    if total_matches > 1 {
        base -= 0.02 * (total_matches - 1) as f64;
    }
    base.clamp(0.70, 0.99)
}

/// Builtin bead-ID patterns mirroring Go's explicit.go set.
pub struct IdPatterns {
    /// [PREFIX-123] bracket form
    pub bracket: Regex,
    /// closes/fixes/resolves/refs + optional # + ID
    pub action: Regex,
    /// beads[-_]N legacy
    pub bead: Regex,
    /// bv[-_]N project form
    pub bv: Regex,
    /// generic UPPERCASE-PREFIX-N
    pub generic: Regex,
}

impl Default for IdPatterns {
    fn default() -> Self {
        IdPatterns {
            bracket: Regex::new(r"\[([A-Za-z]+-\d+)\]").unwrap(),
            action: Regex::new(
                r"(?i)(closes?|closed|fixes|fixed|resolves?|resolved|refs?)[:\s]+#?([A-Za-z]+-\d+)",
            )
            .unwrap(),
            bead: Regex::new(r"(?i)\bbeads[-_](\d+)\b").unwrap(),
            bv: Regex::new(r"(?i)\bbv[-_](\d+)\b").unwrap(),
            generic: Regex::new(r"\b([A-Z]{2,10}-\d+)\b").unwrap(),
        }
    }
}

/// One detected ID mention in a commit message.
#[derive(Debug, Clone)]
pub struct IdMention {
    pub bead_id: String,
    pub kind: MatchKind,
}

/// Scan a commit message for bead-ID mentions.
pub fn find_mentions(message: &str, patterns: &IdPatterns) -> Vec<IdMention> {
    let mut out = Vec::new();

    // Action keywords first (highest signal).
    for cap in patterns.action.captures_iter(message) {
        if let Some(id) = cap.get(2) {
            let verb = cap.get(1).unwrap().as_str().to_lowercase();
            let kind = if verb.starts_with("close") {
                MatchKind::Closes
            } else if verb.starts_with("fix") {
                MatchKind::Fixes
            } else if verb.starts_with("resolv") {
                MatchKind::Resolves
            } else {
                MatchKind::Refs
            };
            out.push(IdMention {
                bead_id: id.as_str().to_string(),
                kind,
            });
        }
    }

    // Bracket form.
    for cap in patterns.bracket.captures_iter(message) {
        out.push(IdMention {
            bead_id: cap[1].to_string(),
            kind: MatchKind::Bracket,
        });
    }

    // bv-N / beads-N forms.
    for cap in patterns.bv.captures_iter(message) {
        out.push(IdMention {
            bead_id: format!("bv-{}", &cap[1]),
            kind: MatchKind::Bead,
        });
    }
    for cap in patterns.generic.captures_iter(message) {
        out.push(IdMention {
            bead_id: cap[1].to_string(),
            kind: MatchKind::Refs,
        });
    }

    // Dedup by (id), keeping the strongest kind per id.
    let mut seen: std::collections::HashMap<String, MatchKind> = std::collections::HashMap::new();
    let mut deduped = Vec::new();
    for m in out {
        let better = seen
            .get(&m.bead_id)
            .map(|existing| kind_rank(m.kind) > kind_rank(*existing))
            .unwrap_or(true);
        if better {
            seen.insert(m.bead_id.clone(), m.kind);
        }
    }
    for (id, kind) in seen {
        deduped.push(IdMention { bead_id: id, kind });
    }
    deduped.sort_by(|a, b| a.bead_id.cmp(&b.bead_id));
    deduped
}

fn kind_rank(k: MatchKind) -> u8 {
    match k {
        MatchKind::Closes | MatchKind::Fixes | MatchKind::Resolves => 4,
        MatchKind::Bracket => 3,
        MatchKind::Bead => 2,
        MatchKind::Refs => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn confidence_base_and_clamps() {
        assert!(close(calculate_confidence(None, 1), 0.90));
        assert!(close(calculate_confidence(Some(MatchKind::Fixes), 1), 0.95));
        assert!(close(
            calculate_confidence(Some(MatchKind::Bracket), 1),
            0.92
        ));
        assert!(close(calculate_confidence(Some(MatchKind::Refs), 1), 0.91));
        assert!(close(calculate_confidence(Some(MatchKind::Bead), 1), 0.93));
        // multi-ID penalty: 0.95 - 0.02 = 0.93
        assert!(close(calculate_confidence(Some(MatchKind::Fixes), 2), 0.93));
        // clamp low bound: base .90 - .02*14 = .62 -> clamped to .70
        assert_eq!(calculate_confidence(None, 15), 0.70);
    }

    #[test]
    fn finds_action_keywords() {
        let p = IdPatterns::default();
        let mentions = find_mentions("fixes BV-123 and closes AUTH-45", &p);
        assert!(mentions.iter().any(|m| m.bead_id == "BV-123"));
        assert!(mentions.iter().any(|m| m.bead_id == "AUTH-45"));
    }

    #[test]
    fn finds_bracket_and_generic_forms() {
        let p = IdPatterns::default();
        let mentions = find_mentions("[CORE-7] refactor per PROJ-42", &p);
        assert!(mentions
            .iter()
            .any(|m| m.bead_id == "CORE-7" && m.kind == MatchKind::Bracket));
        assert!(mentions.iter().any(|m| m.bead_id == "PROJ-42"));
    }

    #[test]
    fn no_mentions_clean_message() {
        let p = IdPatterns::default();
        assert!(find_mentions("just a regular commit", &p).is_empty());
    }
}
