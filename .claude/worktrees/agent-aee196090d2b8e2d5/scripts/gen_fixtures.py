#!/usr/bin/env python3
"""Generate synthetic beads JSONL fixtures for differential testing.

Fixture classes (plan §6 Phase 0):
  small_chain      12 nodes, linear chain A1->A2->...->A12
  medium_tree      60 nodes, branching factor 3, depth 4
  large_cyclic_600 ~600 nodes with a few deliberate cycles
  xl_2500          2500 nodes, sparse DAG
"""
import json
import os
import random
from datetime import datetime, timedelta, timezone
from pathlib import Path

BASE = datetime(2026, 1, 1, tzinfo=timezone.utc)


def make_issue(id: str, title: str, deps: list, status="open", priority=2,
               issue_type="task", created_offset_days=0):
    created = BASE + timedelta(days=created_offset_days)
    return {
        "id": id,
        "title": title,
        "description": f"Synthetic fixture issue {id}",
        "status": status,
        "priority": priority,
        "issue_type": issue_type,
        "assignee": "",
        "labels": [f"fixture"],
        "dependencies": [
            {"issue_id": id, "depends_on_id": d, "type": "blocks",
             "created_at": created.isoformat().replace("+00:00", "Z"),
             "created_by": "fixture-gen"}
            for d in deps
        ],
        "created_at": created.isoformat().replace("+00:00", "Z"),
        "updated_at": (created + timedelta(hours=1)).isoformat().replace("+00:00", "Z"),
    }


def write(path: Path, issues: list):
    (path / ".beads").mkdir(parents=True, exist_ok=True)
    with open(path / ".beads" / "issues.jsonl", "w") as f:
        for i in issues:
            f.write(json.dumps(i) + "\n")
    print(f"wrote {len(issues):5d} issues -> {path}/.beads/issues.jsonl")


def small_chain(root: Path):
    n = 12
    issues = []
    for i in range(1, n + 1):
        # chain: FIX-i blocks on FIX-(i-1)
        deps = [f"FIX-{i-1}"] if i > 1 else []
        issues.append(make_issue(f"FIX-{i}", f"Chain step {i}", deps))
    write(root / "small_chain", issues)


def medium_tree(root: Path):
    issues = []
    counter = [0]

    def nid():
        counter[0] += 1
        return f"TREE-{counter[0]}"

    root_id = nid()
    issues.append(make_issue(root_id, "Tree root", []))
    frontier = [root_id]
    for depth in range(4):
        next_frontier = []
        for parent in frontier:
            for _ in range(3):
                child = nid()
                issues.append(make_issue(child, f"Depth-{depth+1} node", [parent]))
                next_frontier.append(child)
        frontier = next_frontier
    write(root / "medium_tree", issues)


def large_cyclic_600(root: Path):
    rng = random.Random(42)
    n = 600
    issues = []
    ids = [f"Cyc-{i}" for i in range(1, n + 1)]
    for idx, id in enumerate(ids):
        deps = []
        if idx > 0 and rng.random() < 0.7:
            deps.append(ids[idx - 1])
        if rng.random() < 0.15:
            deps.append(ids[rng.randrange(n)])
        issues.append(make_issue(id, f"Cyclic graph node {idx+1}", deps[:3]))
    # Deliberate small cycles: 3 cycles of length 4
    for c in range(3):
        base = c * 4
        for k in range(4):
            issues[base + k]["dependencies"].append(
                {"issue_id": ids[base + k], "depends_on_id": ids[base + (k + 1) % 4],
                 "type": "blocks", "created_by": "fixture-gen",
                 "created_at": issues[base + k]["created_at"]})
    write(root / "large_cyclic_600", issues)


def xl_2500(root: Path):
    rng = random.Random(7)
    n = 2500
    issues = []
    ids = [f"XL-{i}" for i in range(1, n + 1)]
    for idx, id in enumerate(ids):
        deps = []
        if idx > 0 and rng.random() < 0.25:
            deps.append(ids[rng.randrange(idx)])  # backward edges only => DAG
        issues.append(make_issue(id, f"XL node {idx+1}", deps[:2],
                                 priority=rng.randint(0, 4)))
    write(root / "xl_2500", issues)


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent / "tests" / "fixtures"
    small_chain(root)
    medium_tree(root)
    large_cyclic_600(root)
    xl_2500(root)
