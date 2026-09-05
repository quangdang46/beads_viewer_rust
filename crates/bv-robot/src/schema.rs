//! JSON Schema definitions for robot command outputs — port of Go
//! `generateRobotSchemas` (main.go:9011). Detailed schemas for the primary
//! commands; generic fallback (Go `genericRobotCommandSchema`) for the rest.

use serde_json::{json, Map, Value};

use crate::docs;

/// Go `robotContractVersion`.
pub const SCHEMA_VERSION: &str = "1.0.0";

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

fn s(t: &str) -> Value {
    json!({"type": t})
}

fn array_of(items: Value) -> Value {
    json!({"type": "array", "items": items})
}

fn string_array() -> Value {
    array_of(s("string"))
}

fn format_enum() -> Value {
    json!({"type": "string", "enum": ["json", "toon"]})
}

/// Go envelope schema (shared preamble in every robot output).
fn envelope_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time", "description": "ISO 8601 timestamp when output was generated"},
            "data_hash": {"type": "string", "description": "Fingerprint of source beads.jsonl for cache validation"},
            "output_format": {"type": "string", "enum": ["json", "toon"], "description": "Output format used (json or toon)"},
            "version": {"type": "string", "description": "bv version that generated this output"},
        },
        "required": ["generated_at", "data_hash"],
    })
}

fn recommendation_def() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": s("string"),
            "title": s("string"),
            "type": s("string"),
            "status": s("string"),
            "priority": s("integer"),
            "labels": string_array(),
            "score": s("number"),
            "reasons": string_array(),
            "unblocks": s("integer"),
        },
        "required": ["id", "title", "score"],
    })
}

fn alert_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": s("string"),
            "severity": s("string"),
            "message": s("string"),
            "baseline_val": s("number"),
            "current_val": s("number"),
            "delta": s("number"),
            "details": string_array(),
            "issue_id": s("string"),
            "label": s("string"),
            "unblocks_count": s("integer"),
            "downstream_priority_sum": s("integer"),
        },
        "required": ["type", "severity", "message"],
    })
}

fn recipe_summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": s("string"),
            "description": s("string"),
            "source": s("string"),
        },
        "required": ["name", "description", "source"],
    })
}

fn suggestion_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "suggestions": array_of(json!({
                "type": "object",
                "properties": {
                    "type": s("string"),
                    "target_bead": s("string"),
                    "related_bead": s("string"),
                    "summary": s("string"),
                    "reason": s("string"),
                    "confidence": s("number"),
                    "action_command": s("string"),
                    "generated_at": {"type": "string", "format": "date-time"},
                    "metadata": {"type": "object", "additionalProperties": true},
                },
                "required": ["type", "target_bead", "summary", "reason", "confidence", "generated_at"],
            })),
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "stats": {
                "type": "object",
                "properties": {
                    "total": s("integer"),
                    "by_type": {"type": "object", "additionalProperties": s("integer")},
                    "by_confidence": {"type": "object", "additionalProperties": s("integer")},
                    "high_confidence_count": s("integer"),
                    "actionable_count": s("integer"),
                },
                "required": ["total"],
            },
        },
        "required": ["suggestions", "generated_at", "stats"],
    })
}

fn label_health_config_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stale_threshold_days": s("integer"),
            "velocity_weight": s("number"),
            "freshness_weight": s("number"),
            "flow_weight": s("number"),
            "criticality_weight": s("number"),
            "min_issues_for_health": s("integer"),
            "include_closed_in_flow": s("boolean"),
        },
    })
}

fn label_analysis_result_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "total_labels": s("integer"),
            "healthy_count": s("integer"),
            "warning_count": s("integer"),
            "critical_count": s("integer"),
            "labels": array_of(s("object")),
            "summaries": array_of(s("object")),
            "attention_needed": string_array(),
        },
    })
}

fn cross_label_flow_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "labels": string_array(),
            "flow_matrix": array_of(array_of(s("integer"))),
            "dependencies": array_of(s("object")),
            "critical_paths": array_of(s("object")),
            "bottleneck_labels": string_array(),
            "total_cross_label_deps": s("integer"),
        },
    })
}

fn label_attention_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "label": s("string"),
            "attention_score": s("number"),
            "normalized_score": s("number"),
            "rank": s("integer"),
            "pagerank_sum": s("number"),
            "staleness_factor": s("number"),
            "block_impact": s("number"),
            "velocity_factor": s("number"),
            "open_count": s("integer"),
            "blocked_count": s("integer"),
            "stale_count": s("integer"),
        },
        "required": ["label", "attention_score", "rank"],
    })
}

fn sprint_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": s("string"),
            "name": s("string"),
            "start_date": {"type": "string", "format": "date-time"},
            "end_date": {"type": "string", "format": "date-time"},
            "bead_ids": string_array(),
            "velocity_target": s("number"),
            "created_at": {"type": "string", "format": "date-time"},
            "updated_at": {"type": "string", "format": "date-time"},
        },
        "required": ["id", "name"],
    })
}

fn capacity_bottleneck_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": s("string"),
            "title": s("string"),
            "blocks_count": s("integer"),
            "blocks": string_array(),
        },
        "required": ["id", "blocks_count"],
    })
}

fn burndown_point_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "date": {"type": "string", "format": "date-time"},
            "remaining": s("integer"),
            "completed": s("integer"),
        },
        "required": ["date", "remaining", "completed"],
    })
}

fn timing_metric_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": s("string"),
            "count": s("integer"),
            "total_ms": s("number"),
            "avg_ms": s("number"),
            "max_ms": s("number"),
            "min_ms": s("number"),
        },
        "required": ["name", "count"],
    })
}

fn cache_metric_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": s("string"),
            "hits": s("integer"),
            "misses": s("integer"),
            "total": s("integer"),
            "hit_rate": s("number"),
        },
        "required": ["name", "hits", "misses", "total", "hit_rate"],
    })
}

fn memory_metric_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "heap_alloc_mb": {"type": ["number", "null"]},
            "heap_sys_mb": {"type": ["number", "null"]},
            "heap_objects_k": {"type": ["number", "null"]},
            "gc_cycles": {"type": ["integer", "null"]},
            "gc_pause_ms": {"type": ["number", "null"]},
            "goroutine_count": {"type": ["integer", "null"]},
        },
    })
}

/// Go `titleCaseRobotCommand` — "robot-triage" -> "Robot Triage".
fn title_case_robot_command(name: &str) -> String {
    name.split('-')
        .map(|part| {
            if part.is_empty() {
                part.to_string()
            } else {
                let mut c = part.chars();
                format!("{}{}", c.next().unwrap().to_uppercase(), c.as_str())
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Go `genericRobotCommandSchema` — envelope + description fallback.
fn generic_command_schema(name: &str, doc: &Value) -> Value {
    let mut properties = Map::new();
    properties.insert(
        "generated_at".into(),
        json!({"type": "string", "format": "date-time"}),
    );
    if doc["needs_issues"].as_bool().unwrap_or(true) {
        properties.insert("data_hash".into(), s("string"));
    }
    properties.insert("output_format".into(), format_enum());
    properties.insert("version".into(), s("string"));
    json!({
        "$schema": DRAFT,
        "title": format!("{} Output", title_case_robot_command(name)),
        "description": doc["description"],
        "type": "object",
        "properties": Value::Object(properties),
        "additionalProperties": true,
    })
}

fn triage_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Triage Output",
        "description": "Unified triage recommendations with quick picks, blockers, and project health",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "triage": {
                "type": "object",
                "properties": {
                    "meta": {
                        "type": "object",
                        "properties": {
                            "version": s("string"),
                            "generated_at": s("string"),
                            "phase2_ready": s("boolean"),
                            "issue_count": s("integer"),
                            "history_status": {"type": "string", "enum": ["ok", "error", "timeout"], "description": "Outcome of the git-history correlation prologue; omitted when history was not attempted (#166)"},
                        },
                    },
                    "quick_ref": {
                        "type": "object",
                        "properties": {
                            "open_count": {"type": "integer", "description": "Strict count of issues with status == open (equals project_health.counts.by_status.open)"},
                            "actionable_count": {"type": "integer", "description": "Non-closed issues ready to work on (no open blocking dependencies)"},
                            "blocked_count": {"type": "integer", "description": "Strict count of issues with status == blocked (equals project_health.counts.by_status.blocked)"},
                            "in_progress_count": {"type": "integer", "description": "Strict count of issues with status == in_progress"},
                            "not_closed_count": {"type": "integer", "description": "All non-closed issues (open+in_progress+blocked+deferred); equals actionable_count + not_actionable_count"},
                            "not_actionable_count": {"type": "integer", "description": "Non-closed issues blocked by open dependencies, regardless of status"},
                            "top_picks": array_of(json!({"$ref": "#/$defs/recommendation"})),
                        },
                    },
                    "recommendations": array_of(json!({"$ref": "#/$defs/recommendation"})),
                    "quick_wins": array_of(s("object")),
                    "blockers_to_clear": array_of(s("object")),
                    "project_health": s("object"),
                    "commands": s("object"),
                },
            },
            "usage_hints": array_of(s("string")),
        },
        "$defs": {"recommendation": recommendation_def()},
    })
}

fn next_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Next Output",
        "description": "Single top pick recommendation with claim command",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "id": s("string"),
            "title": s("string"),
            "score": s("number"),
            "reasons": string_array(),
            "unblocks": s("integer"),
            "claim_command": s("string"),
            "show_command": s("string"),
        },
        "required": ["generated_at", "data_hash", "id", "title", "score"],
    })
}

fn plan_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Plan Output",
        "description": "Dependency-respecting execution plan with parallel tracks",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "plan": {
                "type": "object",
                "properties": {
                    "phases": array_of(json!({
                        "type": "object",
                        "properties": {
                            "phase": s("integer"),
                            "issues": array_of(s("object")),
                        },
                    })),
                    "summary": s("object"),
                },
            },
            "status": s("object"),
            "usage_hints": array_of(s("object")),
        },
    })
}

fn insights_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Insights Output",
        "description": "Full graph analysis metrics including PageRank, betweenness, HITS, cycles",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "Stats": s("object"),
            "Cycles": array_of(s("object")),
            "Keystones": array_of(s("object")),
            "Bottlenecks": array_of(s("object")),
            "Influencers": array_of(s("object")),
            "Hubs": array_of(s("object")),
            "Authorities": array_of(s("object")),
            "Orphans": array_of(s("object")),
            "Cores": s("object"),
            "Articulation": array_of(s("object")),
            "Slack": s("object"),
            "Velocity": s("object"),
            "status": s("object"),
            "advanced_insights": s("object"),
            "usage_hints": array_of(s("object")),
        },
    })
}

fn priority_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Priority Output",
        "description": "Priority misalignment detection with recommendations",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "as_of": s("string"),
            "as_of_commit": s("string"),
            "analysis_config": s("object"),
            "status": s("object"),
            "label_scope": s("string"),
            "label_context": s("object"),
            "recommendations": array_of(s("object")),
            "field_descriptions": {"type": "object", "additionalProperties": s("string")},
            "filters": {
                "type": "object",
                "properties": {
                    "min_confidence": s("number"),
                    "max_results": s("integer"),
                    "by_label": s("string"),
                    "by_assignee": s("string"),
                },
            },
            "summary": {
                "type": "object",
                "properties": {
                    "total_issues": s("integer"),
                    "recommendations": s("integer"),
                    "high_confidence": s("integer"),
                },
            },
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "analysis_config", "status", "recommendations", "field_descriptions", "filters", "summary", "usage_hints"],
    })
}

fn graph_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Graph Output",
        "description": "Dependency graph in JSON/DOT/Mermaid format",
        "type": "object",
        "properties": {
            "format": {"type": "string", "enum": ["json", "dot", "mermaid"]},
            "graph": s("string"),
            "nodes": s("integer"),
            "edges": s("integer"),
            "filters_applied": {"type": "object", "additionalProperties": s("string")},
            "explanation": {
                "type": "object",
                "properties": {
                    "what": s("string"),
                    "how_to_render": s("string"),
                    "when_to_use": s("string"),
                },
                "required": ["what", "when_to_use"],
            },
            "data_hash": s("string"),
            "adjacency": {
                "type": "object",
                "properties": {
                    "nodes": array_of(s("object")),
                    "edges": array_of(s("object")),
                },
            },
        },
        "required": ["format", "nodes", "edges", "explanation"],
    })
}

fn diff_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Diff Output",
        "description": "Changes since a historical point (commit, branch, date)",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "resolved_revision": s("string"),
            "as_of": s("string"),
            "as_of_commit": s("string"),
            "from_data_hash": s("string"),
            "to_data_hash": s("string"),
            "diff": {
                "type": "object",
                "properties": {
                    "from_timestamp": {"type": "string", "format": "date-time"},
                    "to_timestamp": {"type": "string", "format": "date-time"},
                    "from_revision": s("string"),
                    "to_revision": s("string"),
                    "new_issues": array_of(s("object")),
                    "closed_issues": array_of(s("object")),
                    "removed_issues": array_of(s("object")),
                    "reopened_issues": array_of(s("object")),
                    "modified_issues": array_of(s("object")),
                    "new_cycles": array_of(s("object")),
                    "resolved_cycles": array_of(s("object")),
                    "metric_deltas": s("object"),
                    "summary": s("object"),
                },
            },
        },
        "required": ["generated_at", "resolved_revision", "from_data_hash", "to_data_hash", "diff"],
    })
}

fn alerts_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Alerts Output",
        "description": "Stale issues, blocking cascades, priority mismatches",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "alerts": array_of(alert_schema()),
            "summary": {
                "type": "object",
                "properties": {
                    "total": s("integer"),
                    "critical": s("integer"),
                    "warning": s("integer"),
                    "info": s("integer"),
                },
            },
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "output_format", "version", "alerts", "summary", "usage_hints"],
    })
}

fn recipes_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Recipes Output",
        "description": "Recipe names, descriptions, and sources for pre-filtering work",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "output_format": format_enum(),
            "version": s("string"),
            "recipes": array_of(recipe_summary_schema()),
        },
        "required": ["generated_at", "output_format", "version", "recipes"],
    })
}

fn metrics_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Metrics Output",
        "description": "Performance metrics: timing, cache hit rates, and memory usage",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "timing": array_of(timing_metric_schema()),
            "cache": array_of(cache_metric_schema()),
            "memory": memory_metric_schema(),
        },
        "required": ["generated_at", "data_hash", "output_format", "version", "memory"],
    })
}

fn suggest_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Suggest Output",
        "description": "Smart suggestions for duplicates, dependencies, labels, cycle breaks",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "filters": {
                "type": "object",
                "properties": {
                    "type": s("string"),
                    "min_confidence": s("number"),
                    "bead_id": s("string"),
                },
            },
            "suggestions": suggestion_set_schema(),
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "filters", "suggestions", "usage_hints"],
    })
}

fn label_health_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Label Health Output",
        "description": "Per-label health metrics with analysis config and usage hints",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "analysis_config": label_health_config_schema(),
            "results": label_analysis_result_schema(),
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "analysis_config", "results", "usage_hints"],
    })
}

fn label_flow_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Label Flow Output",
        "description": "Cross-label dependency flow analysis with analysis config and usage hints",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "flow": cross_label_flow_schema(),
            "analysis_config": label_health_config_schema(),
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "flow", "analysis_config", "usage_hints"],
    })
}

fn label_attention_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Label Attention Output",
        "description": "Attention-ranked labels requiring focus",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "limit": s("integer"),
            "total_labels": s("integer"),
            "labels": array_of(label_attention_item_schema()),
            "usage_hints": string_array(),
        },
        "required": ["generated_at", "data_hash", "limit", "total_labels", "labels", "usage_hints"],
    })
}

fn sprint_list_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Sprint List Output",
        "description": "All configured sprints with the standard robot envelope",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "sprint_count": s("integer"),
            "sprints": array_of(sprint_schema()),
        },
        "required": ["generated_at", "data_hash", "output_format", "version", "sprint_count", "sprints"],
    })
}

fn sprint_show_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Sprint Show Output",
        "description": "A single configured sprint with the standard robot envelope",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "sprint": sprint_schema(),
        },
        "required": ["generated_at", "data_hash", "output_format", "version", "sprint"],
    })
}

fn capacity_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Capacity Output",
        "description": "Capacity simulation and dependency-aware completion projection",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "agents": s("integer"),
            "label": s("string"),
            "open_issue_count": s("integer"),
            "total_minutes": s("integer"),
            "total_days": s("number"),
            "serial_minutes": s("integer"),
            "parallel_minutes": s("integer"),
            "parallelizable_pct": s("number"),
            "estimated_days": s("number"),
            "critical_path_length": s("integer"),
            "critical_path": string_array(),
            "actionable_count": s("integer"),
            "actionable": string_array(),
            "bottlenecks": array_of(capacity_bottleneck_schema()),
        },
        "required": [
            "generated_at", "data_hash", "output_format", "version", "agents",
            "open_issue_count", "total_minutes", "total_days", "serial_minutes",
            "parallel_minutes", "parallelizable_pct", "estimated_days",
            "critical_path_length", "critical_path", "actionable_count", "actionable",
        ],
    })
}

fn burndown_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Burndown Output",
        "description": "Sprint burndown data with issue counts, burn rates, daily points, and optional scope changes",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "sprint_id": s("string"),
            "sprint_name": s("string"),
            "start_date": {"type": "string", "format": "date-time"},
            "end_date": {"type": "string", "format": "date-time"},
            "total_days": s("integer"),
            "elapsed_days": s("integer"),
            "remaining_days": s("integer"),
            "total_issues": s("integer"),
            "completed_issues": s("integer"),
            "remaining_issues": s("integer"),
            "ideal_burn_rate": s("number"),
            "actual_burn_rate": s("number"),
            "projected_complete": {"type": "string", "format": "date-time"},
            "on_track": s("boolean"),
            "daily_points": {"type": ["array", "null"], "items": burndown_point_schema()},
            "ideal_line": {"type": ["array", "null"], "items": burndown_point_schema()},
            "scope_changes": {
                "type": ["array", "null"],
                "items": {
                    "type": "object",
                    "properties": {
                        "date": {"type": "string", "format": "date-time"},
                        "issue_id": s("string"),
                        "issue_title": s("string"),
                        "action": {"type": "string", "enum": ["added", "removed"]},
                    },
                },
            },
        },
        "required": [
            "generated_at", "data_hash", "output_format", "version",
            "sprint_id", "sprint_name", "start_date", "end_date",
            "total_days", "elapsed_days", "remaining_days",
            "total_issues", "completed_issues", "remaining_issues",
            "ideal_burn_rate", "actual_burn_rate", "on_track",
            "daily_points", "ideal_line",
        ],
    })
}

fn forecast_schema() -> Value {
    json!({
        "$schema": DRAFT,
        "title": "Robot Forecast Output",
        "description": "ETA predictions with dependency-aware scheduling",
        "type": "object",
        "properties": {
            "generated_at": {"type": "string", "format": "date-time"},
            "data_hash": s("string"),
            "output_format": format_enum(),
            "version": s("string"),
            "agents": s("integer"),
            "filters": {"type": "object", "additionalProperties": s("string")},
            "forecast_count": s("integer"),
            "forecasts": array_of(s("object")),
            "summary": {
                "type": "object",
                "properties": {
                    "total_minutes": s("integer"),
                    "total_days": s("number"),
                    "avg_confidence": s("number"),
                    "earliest_eta": {"type": "string", "format": "date-time"},
                    "latest_eta": {"type": "string", "format": "date-time"},
                },
            },
        },
        "required": ["generated_at", "data_hash", "output_format", "version", "agents", "forecast_count", "forecasts"],
    })
}

/// Go `generateRobotSchemas` — full payload with per-command schemas.
/// Detailed schemas for primary commands; generic fallback for the rest.
pub fn generate_robot_schemas(now: &str) -> Value {
    let mut commands = Map::new();
    let detailed: &[(&str, Value)] = &[
        ("robot-triage", triage_schema()),
        ("robot-next", next_schema()),
        ("robot-plan", plan_schema()),
        ("robot-insights", insights_schema()),
        ("robot-priority", priority_schema()),
        ("robot-graph", graph_schema()),
        ("robot-diff", diff_schema()),
        ("robot-alerts", alerts_schema()),
        ("robot-recipes", recipes_schema()),
        ("robot-metrics", metrics_schema()),
        ("robot-suggest", suggest_schema()),
        ("robot-label-health", label_health_schema()),
        ("robot-label-flow", label_flow_schema()),
        ("robot-label-attention", label_attention_schema()),
        ("robot-sprint-list", sprint_list_schema()),
        ("robot-sprint-show", sprint_show_schema()),
        ("robot-capacity", capacity_schema()),
        ("robot-burndown", burndown_schema()),
        ("robot-forecast", forecast_schema()),
    ];
    for (name, schema) in detailed {
        commands.insert(name.to_string(), schema.clone());
    }

    // Generic fallback for every remaining documented command (Go behavior).
    let cmds = docs::commands_doc();
    if let Some(obj) = cmds.as_object() {
        for (name, doc) in obj {
            commands
                .entry(name.clone())
                .or_insert_with(|| generic_command_schema(name, doc));
        }
    }

    // Sort command keys for stable output (Go map serialization sorts).
    let mut keys: Vec<&String> = commands.keys().collect();
    keys.sort();
    let mut sorted = Map::new();
    for k in keys {
        sorted.insert(k.clone(), commands[k].clone());
    }

    json!({
        "schema_version": SCHEMA_VERSION,
        "generated_at": now,
        "envelope": envelope_schema(),
        "commands": Value::Object(sorted),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_contract_version() {
        assert_eq!(SCHEMA_VERSION, "1.0.0");
    }

    #[test]
    fn triage_schema_has_recommendation_defs() {
        let out = generate_robot_schemas("2026-01-01T00:00:00Z");
        let triage = &out["commands"]["robot-triage"];
        assert_eq!(
            triage["$defs"]["recommendation"]["required"],
            json!(["id", "title", "score"])
        );
        assert_eq!(
            triage["properties"]["triage"]["properties"]["quick_ref"]["properties"]["open_count"]
                ["type"],
            json!("integer")
        );
    }

    #[test]
    fn every_documented_command_has_a_schema() {
        let out = generate_robot_schemas("2026-01-01T00:00:00Z");
        let cmds = out["commands"].as_object().unwrap();
        let doc_value = docs::commands_doc();
        let doc_cmds = doc_value.as_object().unwrap();
        for name in doc_cmds.keys() {
            assert!(cmds.contains_key(name), "missing schema for {name}");
        }
    }

    #[test]
    fn generic_fallback_omits_data_hash_when_no_issues_needed() {
        let out = generate_robot_schemas("2026-01-01T00:00:00Z");
        let docs_schema = &out["commands"]["robot-docs"];
        assert!(docs_schema["properties"].get("data_hash").is_none());
        assert_eq!(docs_schema["title"], json!("Robot Docs Output"));
        assert_eq!(docs_schema["$schema"], json!(DRAFT));
    }
}
