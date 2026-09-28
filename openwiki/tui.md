---
type: TUI Reference
title: roadie TUI state and interaction model
description: Reference for the roadie terminal UI screens, popups, forms, keyboard controls, loader flow, and rendering behavior.
tags: [tui, keyboard, state-machine, rendering, ux, rust]
openwiki:
  roles: [architecture, workflow]
  source_paths: [src/tui/app.rs, src/tui/events.rs, src/tui/loader.rs, src/tui/ui.rs, src/tui/flows.rs]
  test_paths: [src/tui/flows.rs]
  invariants: [The UI is driven by pure state transitions plus background loading., Popups are modal state in `App`, not separate processes., Truecolor controls whether fades and flashes animate or render instantly.]
---

# TUI

The TUI is a state machine centered on `App`.
It tracks the current screen, popup, selection, cache, loading state, and transient status messages.

## Screens

- **Repos**: grid of repo cards plus the all-repos overview card.
- **Overview**: flattened list of open issues across repos.
- **Board**: a single repo board with columns and issues.

## Popups and forms

Popups are represented as enum variants inside `App`.
They include:

- help
- issue detail
- create/edit form
- comment editor
- close prompt
- move prompt
- search prompt
- WIP warning

Forms support title, body, status, and milestone fields for creation, and title/body for edit.
The create form uses the `idea`, `planned`, `now`, and `working` statuses only.

## Interaction patterns

- Repo grid navigation uses horizontal and vertical movement with wrap-aware row logic.
- The board lets the user move between columns, move cards between columns, open issue details, create issues, edit issues, comment, close, reopen, search, and filter by milestone.
- `Esc` is context-sensitive: it closes a popup, clears filters, or returns to the repo grid.
- `b` is an alias for returning to the repo grid.
- `q` quits and `?` opens help.

## Loading and activity

Summary cards are loaded lazily in the background.
The footer shows loading progress and status messages while work is in flight.
The UI supports instant rendering from cache, then background refreshes when needed.

## Rendering and animation

Rendering is separated from state mutation.
Visual effects are conditional on truecolor support and never block input.
The renderer uses fade-in and flash effects when cards arrive or issues change, but falls back to immediate drawing on non-truecolor terminals.

## Flow tests

`src/tui/flows.rs` drives the real event handler, loader, and renderer against a fake backend and a `ratatui::TestBackend`.
These tests assert against rendered screen text, not just state transitions.
