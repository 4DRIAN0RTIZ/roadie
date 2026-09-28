---
type: Release Process
title: roadie release process
description: Release automation for roadie, including git-cliff changelog generation, GitHub Actions workflows, version bumps, tag creation, and published artifacts.
tags: [releases, changelog, github-actions, git-cliff]
openwiki:
  roles: [delivery, workflow]
  source_paths: [README.md, CHANGELOG.md, cliff.toml]
  invariants: [Pushes to `develop` regenerate the unreleased changelog section., Pushes to `main` produce a tagged GitHub Release and fast-forward `develop` to `main`., Conventional commits drive version bumps.]
---

# Releases

Release behavior is documented in the repository README and changelog configuration.

## Branch model

- Day-to-day work lands on `develop`.
- Releases are cut from `main`.

## Automation

- Pushes to `develop` regenerate the `[Unreleased]` section of `CHANGELOG.md`.
- Pushes to `main` compute the next version, bump `Cargo.toml` and `Cargo.lock`, regenerate `CHANGELOG.md`, create a release commit and tag, publish a GitHub Release, and fast-forward `develop` to `main`.

## Versioning

Conventional commits drive the bump level:

- `feat` → minor
- `fix` and other releasable types → patch
- breaking changes → major
- `chore`, `test`, `ci`, `style`, and `build` do not appear in the changelog

## Artifacts

The published release includes the Linux x86_64 binary and a `.sha256` file.

## Operational requirements

- a GitHub remote with both `develop` and `main`
- GitHub Actions permissions that allow writing repository contents
