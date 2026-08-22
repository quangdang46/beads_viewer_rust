# UX Golden Checklist — TUI behavioral parity

> Executed at each TUI milestone. Items must pass on this repo.
> Source: plan §5.7 + README Keyboard Control Map.

## M1: Core Journey (items 1-15)
- [ ] 01. Open app → default list view with split detail pane (>100 cols)
- [ ] 02. j/k moves cursor down/up through issues
- [ ] 03. g jumps to top, G to bottom
- [ ] 04. Ctrl+D/U pages down/up by half screen
- [ ] 05. / enters search mode, typing filters list
- [ ] 06. Enter exits search and applies filter
- [ ] 07. Esc cancels search/filter
- [ ] 08. Enter opens detail pane for selected issue
- [ ] 09. Tab toggles focus between list and detail
- [ ] 10. o filters open-only; c closed-only; r ready; a all
- [ ] 11. s cycles sort modes (Default→Created↑→Created↓→Priority→Updated)
- [ ] 12. Status bar shows current sort mode badge
- [ ] 13. Issue count in header matches loaded beads
- [ ] 14. q at top level shows quit confirmation
- [ ] 15. Esc dismisses quit confirm

## M2: Structural Views (items 16-22)
- [ ] 16. b opens kanban board with status swimlanes
- [ ] 17. s cycles swimlane mode (status→priority→type)
- [ ] 18. E opens tree view showing parent-child hierarchy
- [ ] 19. g opens graph canvas with nodes and edges
- [ ] 20. Graph edges use manhattan routing (orthogonal paths)
- [ ] 21. Actionable view shows tracks with unblocks counts
- [ ] 22. Label dashboard shows health scores per label

## M3: Analytics Views (items 23-30)
- [ ] 23. i opens insights with 6 metric panels
- [ ] 24. Insights proof panel shows calculation details
- [ ] 25. f opens flow matrix showing cross-label deps
- [ ] 26. ] shows attention-ranked labels table
- [ ] 27. ! opens alerts panel when alerts exist
- [ ] 28. h opens history view w/ bead-commit correlations
- [ ] 29. History confidence threshold c cycles 0→0.3→0.5→0.7
- [ ] 30. t enters time-travel mode with revision prompt

## M4-M5: Infrastructure + Chrome (items 31-40)
- [ ] 31. External edit of .beads/issues.jsonl triggers live reload <1s
- [ ] 32. Freshness indicator appears when snapshot >30s old
- [ ] 33. Ctrl+R forces refresh
- [ ] 34. ? shows help overlay with context-aware shortcuts
- [ ] 35. ; toggles shortcuts sidebar (width 34 cols)
- [ ] 36. ` opens interactive tutorial with page navigation
- [ ] 37. Tutorial progress persists across sessions
- [ ] 38. V opens cass session modal (when cass installed)
- [ ] 39. Mouse wheel scrolls focused panel
- [ ] 40. Left-click selects issue row under cursor

## Scoring
Each item: PASS/FAIL/PARTIAL. Any FAIL blocks the milestone gate (CP-E).
Cosmetic diffs (color shade) are accepted; behavioral diffs block.
