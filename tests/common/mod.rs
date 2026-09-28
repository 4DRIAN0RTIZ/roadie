//! Shared helpers for the integration tests: a sandbox with stub `gh` and
//! `roadmap` executables, real temp git repos, and argv logs. Nothing here
//! touches the network, the real HOME or the process-wide PATH.
#![allow(dead_code)]
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use roadie::backend::GhBackend;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("fixture {}: {e}", p.display()))
}

pub struct Sandbox {
    pub root: PathBuf,
    pub projects: PathBuf,
    pub stubs: PathBuf,
}

impl Sandbox {
    pub fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("roadie-it-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (projects, stubs) = (root.join("projects"), root.join("stubs"));
        fs::create_dir_all(&projects).unwrap();
        fs::create_dir_all(&stubs).unwrap();
        let sb = Self {
            root,
            projects,
            stubs,
        };
        sb.write_stub("gh", GH_STUB);
        sb.write_stub("roadmap", ROADMAP_STUB);
        // Defaults: a realistic issue list, description and milestones.
        sb.use_fixture("issue_list.json", "issue_list.json");
        sb.use_fixture("repo_view.txt", "repo_view_desc.txt");
        sb.use_fixture("milestones.txt", "milestones.txt");
        sb.put("release.txt", "v1.4.2\n");
        sb.use_fixture("roadmap_create.txt", "roadmap_create.txt");
        sb
    }

    fn write_stub(&self, name: &str, body: &str) {
        let path = self.stubs.join(name);
        let script = body.replace("@STUBS@", &self.stubs.display().to_string());
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Copies `tests/fixtures/<fixture>` to `<stubs>/<as_name>`.
    pub fn use_fixture(&self, as_name: &str, fixture_name: &str) {
        fs::write(self.stubs.join(as_name), fixture(fixture_name)).unwrap();
    }

    pub fn put(&self, as_name: &str, contents: &str) {
        fs::write(self.stubs.join(as_name), contents).unwrap();
    }

    /// Makes every `gh` call fail with `stderr` and exit code 1.
    pub fn fail_gh(&self, stderr: &str) {
        self.put("gh_fail", stderr);
    }

    pub fn backend(&self) -> GhBackend {
        GhBackend::with_paths(
            self.projects.clone(),
            self.stubs.join("roadmap"),
            self.stubs.join("gh"),
        )
    }

    /// Real `git init` with the given origin URL (`None`: no remote).
    pub fn git_repo(&self, name: &str, origin: Option<&str>) -> PathBuf {
        let dir = self.projects.join(name);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        if let Some(o) = origin {
            git(&dir, &["remote", "add", "origin", o]);
        }
        dir
    }

    /// One recorded call per line, args separated by a tab.
    pub fn gh_log(&self) -> Vec<String> {
        self.log("gh.log")
    }

    pub fn roadmap_log(&self) -> Vec<String> {
        self.log("roadmap.log")
    }

    fn log(&self, name: &str) -> Vec<String> {
        fs::read_to_string(self.stubs.join(name))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim_end_matches('\t').to_string())
            .collect()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git is required to run the integration tests");
    assert!(st.success(), "git {args:?} failed");
}

/// Stub `gh`: logs argv, then replays canned output from the stubs dir.
/// `issue list` serves `issue_list.<n>.json` for the n-th call when present
/// (to simulate GitHub lag), else `issue_list.json`.
const GH_STUB: &str = r#"#!/bin/sh
D="@STUBS@"
printf '%s\t' "$@" >> "$D/gh.log"; printf '\n' >> "$D/gh.log"
if [ -f "$D/gh_fail" ]; then cat "$D/gh_fail" >&2; exit 1; fi
case "$1 $2" in
  "issue list")
    n=$(cat "$D/list_count" 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > "$D/list_count"
    if [ -f "$D/issue_list.$n.json" ]; then cat "$D/issue_list.$n.json"; else cat "$D/issue_list.json"; fi ;;
  "repo view") cat "$D/repo_view.txt" ;;
  "api "*"/releases/latest")
    if [ -f "$D/release_err" ]; then cat "$D/release_err" >&2; exit 1; fi
    cat "$D/release.txt" ;;
  api*) cat "$D/milestones.txt" ;;
  *) echo "gh stub: unexpected args: $*" >&2; exit 2 ;;
esac
"#;

/// Stub `roadmap`: logs argv; `create` prints the canned URL.
const ROADMAP_STUB: &str = r#"#!/bin/sh
D="@STUBS@"
printf '%s\t' "$@" >> "$D/roadmap.log"; printf '\n' >> "$D/roadmap.log"
if [ -f "$D/roadmap_fail" ]; then cat "$D/roadmap_fail" >&2; exit 1; fi
case "$1" in
  create) cat "$D/roadmap_create.txt" ;;
  *) : ;;
esac
"#;
