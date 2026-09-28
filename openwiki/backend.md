---
type: Backend Reference
title: roadie backend and command contract
description: Reference for the `Backend` trait, the `GhBackend` implementation, environment variables, repository discovery, and the external `gh` and `roadmap` commands used by roadie.
tags: [backend, github-cli, roadmap-cli, rust, environment]
openwiki:
  roles: [integration, repository, architecture]
  source_paths: [src/backend.rs, src/main.rs]
  invariants: [Reads use `gh` inside the repo directory., Mutations shell out to `roadmap`., The backend rejects invalid repo names and non-GitHub `origin` remotes.]
---

# Backend

The application uses a `Backend` trait so the TUI can be driven by a real GitHub-backed implementation or a fake backend in tests.

## `Backend` trait

The trait exposes the operations the UI needs:

- list repos
- resolve a repo slug from git remote metadata
- list issues
- list milestones
- build repo summaries
- create, edit, move, comment, close, reopen issues
- open URLs

The trait is `Send + Sync` so background summary workers can share one backend across threads.

## `GhBackend`

`GhBackend` is the production implementation.
It resolves program paths from the environment and then shells out to external commands.

### Environment variables

- `ROADMAP_PROJECTS_DIR`: directory containing local git repositories.
- `ROADMAP_BIN`: explicit path to the `roadmap` executable.
- `ROADMAP_GH`: explicit path to the `gh` executable.
- `HOME`: used as a fallback for `ROADMAP_PROJECTS_DIR` and `ROADMAP_BIN` resolution.

If `ROADMAP_BIN` is unset, the backend prefers `~/bin/roadmap` when that file exists, otherwise it falls back to `roadmap` on `PATH`.

### Repository discovery

Repository discovery is local and file-system based:

- scan `ROADMAP_PROJECTS_DIR`
- keep entries with a `.git` directory
- keep entries whose `origin` remote parses as a GitHub remote
- sort repo names alphabetically

### Read commands

`gh` is used for read paths:

- `gh issue list --label roadmap --state all --json number,title,body,labels,state,milestone,assignees,url,updatedAt`
- `gh issue list --label roadmap --state open --json number,title,labels`
- `gh repo view --json description --jq .description // ""`
- `gh api repos/<slug>/milestones --jq .[].title`
- `gh api repos/<slug>/releases/latest --jq .tag_name`

A 404 from the latest-release query is treated as `None` instead of an error.

### Mutation commands

Mutations shell out to `roadmap`:

- create
- edit
- status changes
- comment
- close
- reopen

The helper that builds create arguments strips a `[roadmap]` prefix before passing the title to the script, because the script adds it itself.

## Testability hooks

`GhBackend::with_paths` lets tests inject stub executables instead of mutating `PATH`.
That is the mechanism used by the integration tests and sandbox helpers.
