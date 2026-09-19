# roadie

Kanban TUI over GitHub Issues, built on the `roadmap` CLI (`~/bin/roadmap`).
Columns: `idea | planned | now | working | done` (done = closed issue).
Issue-only: no GitHub Projects v2. WIP limit is 1 for `working` per repo.

## Install

Requires `gh` (authenticated), `git` and the `roadmap` script.

```sh
cargo install --path .
roadie
```

Env: `ROADMAP_PROJECTS_DIR` (default `~/Projects`), `ROADMAP_BIN`
(default `~/bin/roadmap`, then `roadmap` in `PATH`).

## Keys

Repo picker (card grid, first screen):

| Key | Action |
| --- | --- |
| `h`/`l`, arrows | previous / next card (no-op on the all-repos card) |
| `j`/`k`, arrows | row down / up; `j` from the all-repos card goes to the first repo, `k` from the first repo row returns to it; a shorter last row lands on its last card |
| `g` / `G` | all-repos card / last repo card |
| `Enter` | open the repo's board (first card is the all-repos overview) |
| `r` | refetch every card (otherwise cards come from the cache) |
| `Esc` | nothing (`q` quits) |
| `?` / `q` | help / quit |

The all-repos card spans the full width in the first row (a normal card on a
single-column terminal); repo cards start on the next row.

Cards show the repo description, open-issue counts per column and the
active issue. The list appears instantly (local scan); stats load lazily in
the background (up to 5 `gh` workers, one issues call and one description
call per repo). A repo's card is not drawn until its load finishes (its
slot stays empty so the layout never reflows); a failed load shows a red
`err` card. Repos without roadmap issues are dimmed. On truecolor
terminals (`COLORTERM=truecolor|24bit`) each whole card (border and text)
fades in over ~700ms (smoothstep, small stagger between cards that arrive together) when its data arrives; elsewhere it appears instantly.
The all-repos card is always drawn and shows `N/M repos loaded`. The UI
redraws fast (~33ms) only while loads or fades are active. Enter works on
a card that has not loaded yet.

The footer is a log-style line (like hteam-cli's status bar): while cards
load it reads `fetching N/M repos...`, then `done, M repos loaded` (or
`done, K errors` in red) and clears itself after 3s back to the key hints.
On the board, reads show `fetching issues...` then `done`. Errors stay red
until the next key press.

Board animations (truecolor only, visual only, never delay input): columns
and cards fade in left to right / top to bottom when a board opens or is
refreshed; cards that moved, were created, edited, commented or closed
flash an accent that eases back over ~500ms; the selection highlight blends
in over ~120ms; a column's count flashes when it changes; popups fade in
over ~150ms. Without truecolor everything is drawn instantly. The fast redraw
tick runs only while something animates.

Board:

| Key | Action |
| --- | --- |
| `h`/`l`, arrows | change column |
| `j`/`k` | move in column (`g`/`G` top/bottom) |
| `H` / `L` | move card to previous / next column |
| `m` | move menu (`1`-`5` or `j`/`k` + Enter) |
| `Enter` | issue detail (scroll `j`/`k`, `o` browser, `e` edit) |
| `n` | new issue (Tab fields, Ctrl+S save) |
| `e` | edit title and body (Ctrl+E for `$EDITOR`) |
| `c` | comment |
| `x` / `u` | close with optional comment / reopen |
| `o` | open in browser |
| `f` | cycle milestone filter |
| `/` | text search |
| `Esc` | close popup, else clear filters, else back to the repo grid |
| `b` | back to the repo grid (alias of `Esc`) |
| `r` | refetch this board (otherwise served from the cache) |
| `?` / `q` | help / quit |

The repo picker's first card is the "all repos" overview (like `roadmap list`).
Moving a second card to `working` asks for confirmation (`y`).

## Cache

Card summaries and boards are cached for the session and on disk, so nothing
is refetched unless you ask. On startup cards render from the cache (footer:
`loaded from cache (3h ago), r to refresh`) and only repos without a cache
entry are fetched. Re-entering a repo shows its board instantly from the
cache. `r` forces a refetch (all cards on the grid, one board on a board);
on failure the old cached data stays and the error shows in the footer.
Your own actions (move, create, edit, comment, close, reopen) update the
cached board and card, so the cache never goes stale after them.

Location: `$ROADMAP_CACHE_DIR/cache.json`, else
`$XDG_CACHE_HOME/roadie/cache.json`, else
`~/.cache/roadie/cache.json`. Writes are atomic and happen on a
background thread; a missing, corrupt or old-schema file is ignored.
To clear it: `rm -r ~/.cache/roadie` (or press `r` on the grid).

## How it maps to `roadmap`

| TUI action | Command |
| --- | --- |
| repo list | local scan of git `origin` remotes |
| card stats | `gh issue list --label roadmap --state open --json number,title,labels` + `gh repo view --json description` |
| load board | `gh issue list --label roadmap --state all --json ...` |
| create | `roadmap create <repo> <title> --body --status --milestone` |
| edit | `roadmap edit <repo> <n> --title --body` |
| move to idea/planned/now/working | `roadmap status <repo> <n> <status>` |
| move to done / `x` | `roadmap close <repo> <n> [--comment]` |
| reopen / move out of done | `roadmap reopen <repo> <n>` (+ `status`) |
| comment | `roadmap comment <repo> <n> <text>` |

Mutations always shell out to `roadmap`, so label rotation and close
semantics stay identical. `gh` errors show in the status bar.

Body convention: short prose, 2-4 lines, no bold+colon bullet lists.

## Development

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

Logic is behind a `Backend` trait (`src/backend.rs`); tests use a fake.

### Tests

The suite is hermetic (no network, no real `gh`/`roadmap`, no real `HOME` or
cache) and runs in about a second.

- Unit tests live next to the code. `src/tui/flows.rs` holds end-to-end flow
  tests: scripted keys go through the real event handler, background loader
  and renderer (ratatui `TestBackend`) over `FakeBackend`, and assert on the
  rendered screen text. Time is injected (`Flow::advance`); animations are off
  unless a test enables them. The fake applies mutations to its issue list and
  can simulate GitHub list lag (`list_lag`).
- `tests/gh_backend.rs` and `tests/create_flow.rs` run the real `GhBackend`
  against stub executables. `tests/common/mod.rs` creates a temp sandbox with
  shell-script stubs for `gh` and `roadmap` (passed via
  `GhBackend::with_paths`, so PATH is never mutated) and real `git init` repos
  with GitHub-looking origins. Every stub call is appended to a log
  (`sandbox.gh_log()` / `roadmap_log()`, args tab-separated) so tests assert
  the exact commands.
- Add a fixture: drop the canned output in `tests/fixtures/` and load it with
  `sandbox.use_fixture("<stub file>", "<fixture>")`. Stub files: `issue_list.json`
  (`gh issue list`; `issue_list.<n>.json` overrides the n-th call to simulate
  lag), `repo_view.txt` (description), `milestones.txt`, `roadmap_create.txt`.
  `sandbox.fail_gh("stderr")` makes every `gh` call exit 1.
- Env overrides for manual runs: `ROADMAP_BIN`, `ROADMAP_GH`,
  `ROADMAP_PROJECTS_DIR`, `ROADMAP_CACHE_DIR`.

## Releases

Releases are automated with [git-cliff](https://git-cliff.org) and GitHub Actions.

- Branch model: work lands on `develop`, releases are cut from `main`.
- Push to `develop`: `changelog.yml` regenerates the `[Unreleased]` section of `CHANGELOG.md`.
- Push to `main`: `release.yml` computes the next version, bumps `Cargo.toml`/`Cargo.lock`, regenerates `CHANGELOG.md`, commits `chore(release): vX.Y.Z`, creates an annotated tag, publishes a GitHub Release (notes from git-cliff, `roadie-linux-x86_64` binary plus `.sha256`), and fast-forwards `develop` to `main`.
- Conventional commits drive the bump: `feat` bumps minor, `fix` (and other releasable types) bump patch, `type!:` or a `BREAKING CHANGE` footer bumps major (while below 1.0 git-cliff follows its configured 0.x rules). `chore`, `test`, `ci`, `style` and `build` do not appear in the changelog.
- Cut a release: merge `develop` into `main` and push (or run the "Publish release" workflow manually). If there is nothing new since the latest tag, no release is made.
- Requirements: a GitHub remote with both `develop` and `main` branches must exist, and Actions needs write access to contents (Settings > Actions > Workflow permissions).
