//! Data layer. Reads go through `gh` (JSON); mutations shell out to the
//! `roadmap` CLI so behavior (label rotation, close semantics) stays identical.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};

use crate::domain::{self, Column, Issue, RepoSummary};

/// Payload for creating a roadmap issue. `title` is already normalized
/// (`[roadmap] ` prefix present).
#[derive(Debug, Clone, PartialEq)]
pub struct NewIssue {
    pub title: String,
    pub body: String,
    pub status: Column,
    pub milestone: Option<String>,
}

/// `Send + Sync` so summary workers can share one backend across threads.
pub trait Backend: Send + Sync {
    /// Folder names under the projects dir that have a GitHub remote.
    fn repos(&self) -> Result<Vec<String>>;
    /// `owner/repo` of the repo's `origin` remote.
    fn slug(&self, repo: &str) -> Result<String>;
    /// All roadmap issues (open and closed).
    fn list_issues(&self, repo: &str) -> Result<Vec<Issue>>;
    fn milestones(&self, repo: &str) -> Result<Vec<String>>;
    /// Description plus open-issue stats: one `gh` call for issues, one for the repo.
    fn repo_summary(&self, repo: &str) -> Result<RepoSummary>;
    fn create(&self, repo: &str, issue: &NewIssue) -> Result<String>;
    fn edit(&self, repo: &str, number: u64, title: &str, body: &str) -> Result<()>;
    /// Rotate the state label (`roadmap:<status>`); `status` must not be Done.
    fn set_status(&self, repo: &str, number: u64, status: Column) -> Result<()>;
    fn comment(&self, repo: &str, number: u64, text: &str) -> Result<()>;
    fn close(&self, repo: &str, number: u64, comment: Option<&str>) -> Result<()>;
    fn reopen(&self, repo: &str, number: u64) -> Result<()>;
    fn open_url(&self, url: &str) -> Result<()>;
}

/// Argument list for `roadmap create` (after the subcommand). The script adds
/// the `[roadmap] ` prefix itself, so it is stripped here to avoid doubling.
pub fn create_args(repo: &str, issue: &NewIssue) -> Vec<String> {
    let mut a = vec![
        "create".to_string(),
        repo.to_string(),
        domain::bare_title(&issue.title).to_string(),
        "--body".into(),
        issue.body.clone(),
    ];
    if let Some(s) = issue.status.status_arg() {
        a.push("--status".into());
        a.push(s.into());
    }
    if let Some(m) = &issue.milestone {
        a.push("--milestone".into());
        a.push(m.clone());
    }
    a
}

pub struct GhBackend {
    projects_dir: PathBuf,
    roadmap_bin: PathBuf,
    gh_bin: PathBuf,
}

impl GhBackend {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let projects_dir = std::env::var_os("ROADMAP_PROJECTS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Projects"));
        let roadmap_bin = std::env::var_os("ROADMAP_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let local = home.join("bin/roadmap");
                if local.exists() {
                    local
                } else {
                    PathBuf::from("roadmap")
                }
            });
        let gh_bin = std::env::var_os("ROADMAP_GH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gh"));
        Self::with_paths(projects_dir, roadmap_bin, gh_bin)
    }

    /// Explicit program paths (tests point these at stub scripts, so no
    /// process-wide PATH mutation is needed).
    pub fn with_paths(projects_dir: PathBuf, roadmap_bin: PathBuf, gh_bin: PathBuf) -> Self {
        Self {
            projects_dir,
            roadmap_bin,
            gh_bin,
        }
    }

    fn dir(&self, repo: &str) -> Result<PathBuf> {
        if repo.is_empty() || repo.contains('/') || repo.starts_with('.') {
            bail!("invalid repo name: {repo}");
        }
        Ok(self.projects_dir.join(repo))
    }

    fn exec(&self, prog: &Path, args: &[String], dir: Option<&Path>) -> Result<String> {
        let mut cmd = Command::new(prog);
        cmd.args(args).stdin(Stdio::null());
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        let out = cmd
            .output()
            .with_context(|| format!("failed to run {}", prog.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = err.trim();
            let msg = if msg.is_empty() {
                "command failed"
            } else {
                msg
            };
            return Err(anyhow!(
                "{} {}: {}",
                prog.display(),
                args.first().map(String::as_str).unwrap_or(""),
                msg
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn roadmap(&self, args: Vec<String>) -> Result<String> {
        self.exec(&self.roadmap_bin, &args, None)
    }

    fn gh(&self, repo: &str, args: &[&str]) -> Result<String> {
        let dir = self.dir(repo)?;
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.exec(&self.gh_bin, &args, Some(&dir))
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

impl Backend for GhBackend {
    fn repos(&self) -> Result<Vec<String>> {
        // Local scan (no network): `roadmap repos` runs `gh repo view` per folder
        // and takes ~15s. A GitHub-looking origin remote is enough here.
        let mut names: Vec<String> = std::fs::read_dir(&self.projects_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join(".git").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| self.slug(name).is_ok())
            .collect();
        names.sort();
        Ok(names)
    }

    fn slug(&self, repo: &str) -> Result<String> {
        let dir = self.dir(repo)?;
        let out = self.exec(
            Path::new("git"),
            &[s("remote"), s("get-url"), s("origin")],
            Some(&dir),
        )?;
        domain::parse_remote(&out).ok_or_else(|| anyhow!("origin of {repo} is not a GitHub remote"))
    }

    fn list_issues(&self, repo: &str) -> Result<Vec<Issue>> {
        let out = self.gh(
            repo,
            &[
                "issue",
                "list",
                "--label",
                "roadmap",
                "--state",
                "all",
                "--limit",
                "300",
                "--json",
                "number,title,body,labels,state,milestone,assignees,url,updatedAt",
            ],
        )?;
        domain::parse_issues(&out)
    }

    fn repo_summary(&self, repo: &str) -> Result<RepoSummary> {
        let out = self.gh(
            repo,
            &[
                "issue",
                "list",
                "--label",
                "roadmap",
                "--state",
                "open",
                "--limit",
                "300",
                "--json",
                "number,title,labels",
            ],
        )?;
        let issues = domain::parse_issues(&out)?;
        let desc = self.gh(
            repo,
            &[
                "repo",
                "view",
                "--json",
                "description",
                "--jq",
                ".description // \"\"",
            ],
        )?;
        Ok(RepoSummary::from_issues(&desc, &issues))
    }

    fn milestones(&self, repo: &str) -> Result<Vec<String>> {
        let slug = self.slug(repo)?;
        let out = self.gh(
            repo,
            &[
                "api",
                &format!("repos/{slug}/milestones"),
                "--jq",
                ".[].title",
            ],
        )?;
        Ok(out
            .lines()
            .map(String::from)
            .filter(|l| !l.is_empty())
            .collect())
    }

    fn create(&self, repo: &str, issue: &NewIssue) -> Result<String> {
        Ok(self.roadmap(create_args(repo, issue))?.trim().to_string())
    }

    fn edit(&self, repo: &str, number: u64, title: &str, body: &str) -> Result<()> {
        self.roadmap(vec![
            s("edit"),
            s(repo),
            number.to_string(),
            s("--title"),
            s(title),
            s("--body"),
            s(body),
        ])
        .map(drop)
    }

    fn set_status(&self, repo: &str, number: u64, status: Column) -> Result<()> {
        let st = status
            .status_arg()
            .ok_or_else(|| anyhow!("done is not a status label"))?;
        self.roadmap(vec![s("status"), s(repo), number.to_string(), s(st)])
            .map(drop)
    }

    fn comment(&self, repo: &str, number: u64, text: &str) -> Result<()> {
        self.roadmap(vec![s("comment"), s(repo), number.to_string(), s(text)])
            .map(drop)
    }

    fn close(&self, repo: &str, number: u64, comment: Option<&str>) -> Result<()> {
        let mut a = vec![s("close"), s(repo), number.to_string()];
        if let Some(c) = comment {
            a.push(s("--comment"));
            a.push(s(c));
        }
        self.roadmap(a).map(drop)
    }

    fn reopen(&self, repo: &str, number: u64) -> Result<()> {
        self.roadmap(vec![s("reopen"), s(repo), number.to_string()])
            .map(drop)
    }

    fn open_url(&self, url: &str) -> Result<()> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        Command::new(opener)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to run {opener}"))?;
        Ok(())
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory backend recording every mutation as a string.
    #[derive(Default)]
    pub struct FakeBackend {
        pub repos: Vec<String>,
        pub issues: Mutex<Vec<Issue>>,
        pub summaries: HashMap<String, RepoSummary>,
        pub milestones: Vec<String>,
        pub calls: Mutex<Vec<String>>,
        pub fail_with: Mutex<Option<String>>,
        /// Every read (`list_issues`, `repo_summary`) as `"kind repo"`.
        pub reads: Mutex<Vec<String>>,
        /// Number of upcoming `list_issues` calls that do not yet include
        /// issues created through `create` (simulates GitHub list lag).
        pub list_lag: Mutex<u32>,
        pub pending: Mutex<Vec<Issue>>,
    }

    impl FakeBackend {
        pub fn reads(&self) -> Vec<String> {
            self.reads.lock().unwrap().clone()
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        /// Applies a mutation to the stored issue so reloads see it.
        fn patch(&self, n: u64, f: impl FnOnce(&mut Issue)) {
            if let Some(i) = self
                .issues
                .lock()
                .unwrap()
                .iter_mut()
                .find(|i| i.number == n)
            {
                f(i);
            }
        }
        fn rec(&self, c: String) -> Result<()> {
            if let Some(e) = self.fail_with.lock().unwrap().clone() {
                bail!(e);
            }
            self.calls.lock().unwrap().push(c);
            Ok(())
        }
    }

    impl Backend for FakeBackend {
        fn repos(&self) -> Result<Vec<String>> {
            Ok(self.repos.clone())
        }
        fn slug(&self, repo: &str) -> Result<String> {
            Ok(format!("me/{repo}"))
        }
        fn list_issues(&self, repo: &str) -> Result<Vec<Issue>> {
            self.reads.lock().unwrap().push(format!("issues {repo}"));
            if let Some(e) = self.fail_with.lock().unwrap().clone() {
                bail!(e);
            }
            let mut lag = self.list_lag.lock().unwrap();
            let mut issues = self.issues.lock().unwrap();
            if *lag > 0 {
                *lag -= 1;
            } else {
                issues.append(&mut self.pending.lock().unwrap());
            }
            Ok(issues.clone())
        }
        fn milestones(&self, _repo: &str) -> Result<Vec<String>> {
            Ok(self.milestones.clone())
        }
        fn repo_summary(&self, repo: &str) -> Result<RepoSummary> {
            self.reads.lock().unwrap().push(format!("summary {repo}"));
            if let Some(e) = self.fail_with.lock().unwrap().clone() {
                bail!(e);
            }
            self.summaries
                .get(repo)
                .cloned()
                .ok_or_else(|| anyhow!("no summary for {repo}"))
        }
        fn create(&self, repo: &str, i: &NewIssue) -> Result<String> {
            self.rec(format!(
                "create {repo} {:?} {:?} {:?} {:?}",
                i.title, i.body, i.status, i.milestone
            ))?;
            let n = {
                let all = self.issues.lock().unwrap();
                let pend = self.pending.lock().unwrap();
                all.iter()
                    .chain(pend.iter())
                    .map(|i| i.number)
                    .max()
                    .unwrap_or(0)
                    + 1
            };
            let mut labels = vec![domain::Label {
                name: "roadmap".into(),
            }];
            if let Some(st) = i.status.status_arg() {
                labels.push(domain::Label {
                    name: format!("roadmap:{st}"),
                });
            }
            let made = Issue {
                number: n,
                title: i.title.clone(),
                body: i.body.clone(),
                state: "OPEN".into(),
                labels,
                milestone: i.milestone.clone().map(|title| domain::Milestone { title }),
                url: format!("https://github.com/me/{repo}/issues/{n}"),
                ..Default::default()
            };
            self.pending.lock().unwrap().push(made);
            if *self.list_lag.lock().unwrap() == 0 {
                self.issues
                    .lock()
                    .unwrap()
                    .append(&mut self.pending.lock().unwrap());
            }
            Ok(format!("https://github.com/me/{repo}/issues/{n}"))
        }
        fn edit(&self, repo: &str, n: u64, title: &str, body: &str) -> Result<()> {
            self.rec(format!("edit {repo} {n} {title:?} {body:?}"))?;
            self.patch(n, |i| {
                i.title = title.to_string();
                i.body = body.to_string();
            });
            Ok(())
        }
        fn set_status(&self, repo: &str, n: u64, st: Column) -> Result<()> {
            self.rec(format!("status {repo} {n} {}", st.title()))?;
            self.patch(n, |i| {
                i.labels.retain(|l| !l.name.starts_with("roadmap:"));
                i.labels.push(domain::Label {
                    name: format!("roadmap:{}", st.title()),
                });
                i.state = "OPEN".into();
            });
            Ok(())
        }
        fn comment(&self, repo: &str, n: u64, t: &str) -> Result<()> {
            self.rec(format!("comment {repo} {n} {t:?}"))
        }
        fn close(&self, repo: &str, n: u64, c: Option<&str>) -> Result<()> {
            self.rec(format!("close {repo} {n} {c:?}"))?;
            self.patch(n, |i| i.state = "CLOSED".into());
            Ok(())
        }
        fn reopen(&self, repo: &str, n: u64) -> Result<()> {
            self.rec(format!("reopen {repo} {n}"))?;
            self.patch(n, |i| i.state = "OPEN".into());
            Ok(())
        }
        fn open_url(&self, url: &str) -> Result<()> {
            self.rec(format!("open {url}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_strip_prefix_and_map_options() {
        let n = NewIssue {
            title: "[roadmap] Foo".into(),
            body: "b".into(),
            status: Column::Planned,
            milestone: Some("M1".into()),
        };
        assert_eq!(
            create_args("r", &n),
            vec![
                "create",
                "r",
                "Foo",
                "--body",
                "b",
                "--status",
                "planned",
                "--milestone",
                "M1"
            ]
        );
    }

    #[test]
    fn dir_rejects_traversal() {
        let b = GhBackend {
            projects_dir: "/x".into(),
            roadmap_bin: "roadmap".into(),
            gh_bin: "gh".into(),
        };
        assert!(b.dir("../etc").is_err());
        assert!(b.dir("a/b").is_err());
        assert_eq!(b.dir("hteam-cli").unwrap(), PathBuf::from("/x/hteam-cli"));
    }
}
