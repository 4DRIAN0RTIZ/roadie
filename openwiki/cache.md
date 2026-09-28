---
type: Cache Reference
title: roadie cache model and persistence
description: Reference for the session and disk cache used by roadie to store repo summaries and board data, including schema versioning, atomic writes, and cache invalidation rules.
tags: [cache, persistence, rust, json]
openwiki:
  roles: [operations, architecture, repository]
  source_paths: [src/cache.rs]
  invariants: [Unreadable, corrupt, missing, or mismatched-schema cache files are ignored., Cache writes are atomic and use a temporary file plus rename., Summary and board snapshots are timestamped with Unix seconds.]
---

# Cache

`roadie` keeps a cache of repository summaries and board data to make the UI responsive and to avoid unnecessary repeated `gh` calls.

## Stored data

The serialized cache stores a map keyed by repo name.
Each repo entry may contain:

- a summary snapshot
- a board snapshot

A snapshot is wrapped in `Stamped<T>` with a Unix-second timestamp.

## Schema and compatibility

The cache file includes a top-level schema version.
The current on-disk schema is `2`.
If the cache file is missing, unreadable, corrupt, empty, or has a different schema version, roadie falls back to an empty cache.

## Location

Cache location is resolved in this order:

1. `ROADMAP_CACHE_DIR/cache.json`
2. `XDG_CACHE_HOME/roadie/cache.json`
3. `HOME/.cache/roadie/cache.json`

## Write behavior

Writes are atomic:

- create parent directories if needed
- write to a temp file next to the target file
- rename the temp file into place
- remove the temp file on failure

A background writer thread coalesces bursts of updates and writes only the latest snapshot it receives.

## Cache mutation rules

- `put_summary` stores a repo summary with a timestamp.
- `put_board` stores issues and milestones with a timestamp.
- `drop_board` clears only the board for a repo.
- `invalidate` removes the whole repo entry.

The UI updates the cache after user mutations so local state stays coherent even before the next fetch.
