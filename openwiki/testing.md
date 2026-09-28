---
type: Testing Guide
title: roadie testing strategy
description: Guide to the hermetic unit and integration tests used by roadie, including fake backends, stub executables, temp sandboxes, and end-to-end TUI flow tests.
tags: [testing, integration-tests, unit-tests, rust, hermetic]
openwiki:
  roles: [testing, workflow]
  source_paths: [tests/common/mod.rs, tests/gh_backend.rs, tests/create_flow.rs, src/tui/flows.rs]
  validation_commands: [cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test]
  invariants: [Tests do not require network access., Tests do not use the real `HOME` or process-wide `PATH`., Tests assert rendered TUI output as well as backend command logs.]
---

# Testing

The repository is designed to be tested hermetically.
That means the suite avoids the network, avoids the user's real home directory, and avoids mutating the process-wide `PATH`.

## Test layers

### Unit tests

Unit tests live next to the code.
They cover cache behavior, domain parsing, UI state transitions, and other pure logic.

### TUI flow tests

`src/tui/flows.rs` exercises the real event handler, loader, and renderer with:

- a fake backend
- a `ratatui::TestBackend`
- injected time

The tests assert on rendered screen text, footer content, and state transitions.

### Backend integration tests

`tests/common/mod.rs` builds a temp sandbox with:

- real temporary git repositories
- stub `gh` and `roadmap` executables
- argv logs for every stub invocation

Those helpers let the integration tests verify exact command shapes without external dependencies.

## Common fixtures

The sandbox can swap canned stub outputs from `tests/fixtures/` into the stub directory.
That is how tests simulate:

- issue lists
- repository descriptions
- milestones
- release lookups
- roadmap create output
- failing `gh` invocations

## Development check

The standard local validation command is:

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```
