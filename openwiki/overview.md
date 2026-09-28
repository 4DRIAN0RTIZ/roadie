---
type: Repository Overview
title: roadie repository overview
description: Overview of the roadie Rust TUI that presents GitHub Issues as a Kanban board, shows per-repo summary cards, caches data, and shells out to `gh` and the `roadmap` CLI for reads and mutations.
tags: [rust, tui, github-issues, kanban, caching, testing, releases]
openwiki:
  roles: [repository, architecture, domain]
  source_paths: [Cargo.toml, README.md, src/main.rs, src/lib.rs]
  invariants: [The binary entrypoint wires `GhBackend::from_env()` into `tui::run`., Reads use `gh`; mutations shell out to `roadmap`., The app is issue-only and does not use GitHub Projects v2.]
---

# roadie

`roadie` is a Rust terminal UI for managing work as GitHub Issues through the `roadmap` CLI.
It renders a repo picker, an all-repos overview, and per-repo Kanban boards with the columns `idea`, `planned`, `now`, `working`, and `done`.

## Core behavior

- The binary starts in `src/main.rs` by constructing `GhBackend::from_env()` and passing it to `tui::run`.
- Repository discovery is local: it scans the configured projects directory for git repos with a GitHub `origin` remote.
- Summary cards are loaded lazily in the background and cached for the session and on disk.
- The board view reads issues through `gh` and uses `roadmap` for create, edit, move, comment, close, and reopen actions.
- `done` means a closed issue; there is no GitHub Projects v2 integration.

## User-facing surfaces

- Repo picker grid with an overview card and one card per repo.
- All-repos overview that aggregates counts and shows currently active work.
- Board screen for issue lifecycle management.
- Popups for help, details, create/edit forms, comments, close flows, move flows, search, and WIP warnings.

## Documentation map

- [`Architecture`](./architecture.md): module-level structure and runtime flow.
- [`Backend`](./backend.md): `gh`/`roadmap` command contract and environment configuration.
- [`Cache`](./cache.md): cache location, schema, and persistence behavior.
- [`Domain model`](./domain.md): issues, summaries, columns, and title normalization.
- [`TUI and flows`](./tui.md): screens, popups, and input handling.
- [`Testing`](./testing.md): fake backend, integration harness, and flow tests.
- [`Releases`](./releases.md): git-cliff and GitHub Actions release process.
