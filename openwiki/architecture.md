---
type: Architecture Overview
title: roadie architecture overview
description: High-level architecture of the roadie application, including the binary entrypoint, backend trait, cache layer, domain model, TUI state machine, loaders, and test strategy.
tags: [architecture, rust, tui, backend, cache, domain, testing]
openwiki:
  roles: [architecture, repository]
  source_paths: [src/main.rs, src/lib.rs, src/backend.rs, src/cache.rs, src/domain/mod.rs, src/domain/summary.rs, src/tui/app.rs, src/tui/loader.rs, src/tui/events.rs, src/tui/ui.rs]
  invariants: [The UI state is kept in `App` and is testable without I/O., Background summary loading is separate from rendering and key handling., The cache is advisory: failures or schema mismatches fall back to empty state.]
---

# Architecture

`roadie` is split into four layers:

1. **Binary bootstrap** in `src/main.rs`.
2. **Backend and cache infrastructure** in `src/backend.rs` and `src/cache.rs`.
3. **Domain model** in `src/domain/`.
4. **TUI state, events, loaders, and rendering** in `src/tui/`.

## Runtime flow

```mermaid
flowchart TD
    A[main.rs] --> B[GhBackend::from_env()]
    B --> C[tui::run]
    C --> D[App state]
    D --> E[load_repos]
    E --> F[SummaryLoader]
    F --> G[GhBackend summary calls]
    D --> H[handle_key]
    H --> I[Backend mutations via roadmap]
    H --> J[Backend reads via gh]
    D --> K[cache::spawn_writer]
    K --> L[cache.json]
```

## Major responsibilities

### `backend`

Defines the `Backend` trait and the `GhBackend` implementation.
`GhBackend` knows how to locate repositories, read issues and repository metadata, and call the external `roadmap` script for mutations.

### `cache`

Implements the persistent session cache for repo summaries and board data.
The cache stores timestamps, serializes to JSON, and writes atomically through a background writer thread.

### `domain`

Contains the data model and parsing/normalization logic shared by the backend, cache, and UI.

### `tui`

Holds the application state machine and the rendering pipeline.
Important submodules include:

- `app.rs`: mutable UI state, popups, forms, selections, and message handling.
- `events.rs`: key handling and screen transitions.
- `loader.rs`: background summary loading.
- `ui.rs`: drawing logic.
- `flows.rs`: end-to-end UI tests built on the real state machine.

## Design constraints

- Pure state transitions are kept out of I/O so they can be tested directly.
- Background summary loading never blocks the UI thread.
- Cache data is replaceable and can be discarded safely when schema or contents are invalid.
- Mutations go through the `roadmap` CLI so label rotation and close semantics remain consistent with the script.
