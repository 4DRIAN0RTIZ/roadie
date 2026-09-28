---
type: Domain Model Reference
title: roadie domain model
description: Reference for the roadie data model, including issues, columns, repo summaries, label parsing, title normalization, and all-repos aggregation.
tags: [domain-model, issues, labels, rust]
openwiki:
  roles: [domain, architecture]
  source_paths: [src/domain/mod.rs, src/domain/summary.rs]
  invariants: [A roadmap issue is identified by the `roadmap` label and one optional status label., Closed issues are excluded from repo summary counts., `working` takes precedence over `now` when selecting the active issue.]
---

# Domain model

The domain layer is the shared vocabulary used by the backend, cache, and UI.
It turns raw GitHub issue JSON into typed values and computes summary statistics for repo cards.

## Key concepts

### Column

The UI and issue state are organized around the roadmap columns:

- `idea`
- `planned`
- `now`
- `working`
- `done`

`done` corresponds to closed issues.

### Issue

Issues carry:

- number
- title
- body
- state
- labels
- milestone
- URL
- update time

The code derives the logical column from issue labels and state.

### RepoSummary

A repo summary contains:

- repo description
- open issue counts for the four badge columns
- the active issue title
- the latest release tag, when present

`RepoSummary::from_issues` skips closed issues, counts the open issues per column, and selects the active issue by preferring `working` over the lowest-numbered `now` issue.

### Aggregation

`aggregate` sums the per-column counts across multiple repo summaries for the all-repos overview card.

## Title normalization

Issue titles are normalized so the roadie prefix stays consistent.
Create flows reject blank titles after removing the optional prefix and trim whitespace before storing or sending data to the backend.
