//! `GhBackend` against stub `gh`/`roadmap` executables and real temp git
//! repos: exact argv, fixture parsing, error surfacing. Hermetic and offline.
mod common;

use common::{fixture, Sandbox};
use roadie::backend::{Backend, NewIssue};
use roadie::domain::{self, Column};

const ACME: &str = "git@github.com:acme/widgets.git";

fn sandbox_with_repo(name: &str) -> Sandbox {
    let sb = Sandbox::new(name);
    sb.git_repo("widgets", Some(ACME));
    sb
}

#[test]
fn repos_scan_finds_only_git_dirs_with_a_github_origin_sorted() {
    let sb = Sandbox::new("scan");
    sb.git_repo("zeta", Some("https://github.com/acme/zeta"));
    sb.git_repo("alpha", Some("ssh://git@github.com/acme/alpha.git"));
    sb.git_repo("gitlab", Some("git@gitlab.com:acme/gitlab.git"));
    sb.git_repo("noremote", None);
    sb.git_repo(".hidden", Some(ACME));
    std::fs::create_dir_all(sb.projects.join("plain-dir")).unwrap();
    std::fs::write(sb.projects.join("file.txt"), "x").unwrap();
    let repos = sb.backend().repos().unwrap();
    assert_eq!(repos, vec!["alpha", "zeta"]);
    assert!(sb.gh_log().is_empty(), "the scan must not call gh");
}

#[test]
fn repos_errors_when_projects_dir_is_missing() {
    let sb = Sandbox::new("nodir");
    std::fs::remove_dir_all(&sb.projects).unwrap();
    assert!(sb.backend().repos().is_err());
}

#[test]
fn slug_comes_from_the_origin_remote() {
    let sb = sandbox_with_repo("slug");
    assert_eq!(sb.backend().slug("widgets").unwrap(), "acme/widgets");
    sb.git_repo("other", Some("git@gitlab.com:a/b.git"));
    assert!(sb.backend().slug("other").is_err());
}

#[test]
fn list_issues_parses_fixture_and_asks_for_all_states_limit_300() {
    let sb = sandbox_with_repo("list");
    let issues = sb.backend().list_issues("widgets").unwrap();
    assert_eq!(issues.len(), 4);
    let by = |n: u64| issues.iter().find(|i| i.number == n).unwrap();
    assert_eq!(by(12).column(), Column::Working);
    assert_eq!(by(12).milestone_title(), Some("M1"));
    assert_eq!(by(12).assignees[0].login, "neandertech");
    assert_eq!(by(12).display_title(), "Card grid");
    assert_eq!(by(13).column(), Column::Planned);
    assert_eq!(by(13).milestone, None);
    assert_eq!(by(14).column(), Column::Idea);
    assert_eq!(by(15).column(), Column::Done);
    let log = sb.gh_log();
    assert_eq!(log.len(), 1);
    let call = &log[0];
    assert!(call.starts_with("issue\tlist\t"), "{call}");
    for part in [
        "--label\troadmap",
        "--state\tall",
        "--limit\t300",
        "--json\t",
    ] {
        assert!(call.contains(part), "missing {part} in {call}");
    }
}

#[test]
fn repo_summary_counts_open_issues_and_reads_the_description() {
    let sb = sandbox_with_repo("summary");
    let s = sb.backend().repo_summary("widgets").unwrap();
    assert_eq!(s.counts, [1, 1, 0, 1]); // idea, planned, now, working (open only)
    assert_eq!(s.active.as_deref(), Some("Card grid"));
    assert_eq!(s.description, "Kanban TUI over GitHub Issues");
    let log = sb.gh_log();
    assert_eq!(log.len(), 2, "one issues call and one description call");
    for part in ["--state\topen", "--limit\t300", "number,title,labels"] {
        assert!(log[0].contains(part), "missing {part} in {}", log[0]);
    }
    assert!(log[1].starts_with("repo\tview\t"));
    assert!(log[1].contains("--json\tdescription"));
}

#[test]
fn null_description_becomes_empty() {
    let sb = sandbox_with_repo("nulldesc");
    sb.use_fixture("repo_view.txt", "repo_view_null.txt");
    assert_eq!(
        sb.backend().repo_summary("widgets").unwrap().description,
        ""
    );
}

#[test]
fn milestones_come_from_the_api_using_the_slug() {
    let sb = sandbox_with_repo("ms");
    assert_eq!(
        sb.backend().milestones("widgets").unwrap(),
        vec!["M1", "M2"]
    );
    let call = &sb.gh_log()[0];
    assert!(
        call.starts_with("api\trepos/acme/widgets/milestones\t"),
        "{call}"
    );
}

#[test]
fn gh_failure_surfaces_stderr_without_panicking() {
    let sb = sandbox_with_repo("fail");
    sb.fail_gh("HTTP 401: Bad credentials\n");
    let b = sb.backend();
    for err in [
        b.list_issues("widgets").unwrap_err(),
        b.repo_summary("widgets").map(|_| ()).unwrap_err(),
        b.milestones("widgets").map(|_| ()).unwrap_err(),
    ] {
        let msg = format!("{err:#}");
        assert!(msg.contains("Bad credentials"), "{msg}");
        assert!(msg.contains("gh"), "{msg}");
    }
}

#[test]
fn missing_program_and_silent_failure_are_readable() {
    let sb = sandbox_with_repo("missing");
    let b = roadie::backend::GhBackend::with_paths(
        sb.projects.clone(),
        sb.stubs.join("nope-roadmap"),
        sb.stubs.join("nope-gh"),
    );
    let msg = format!("{:#}", b.list_issues("widgets").unwrap_err());
    assert!(msg.contains("failed to run"), "{msg}");
    sb.fail_gh(""); // exit 1 with empty stderr
    let msg = format!("{:#}", sb.backend().list_issues("widgets").unwrap_err());
    assert!(msg.contains("command failed"), "{msg}");
}

#[test]
fn malformed_json_is_an_error_not_a_panic() {
    let sb = sandbox_with_repo("badjson");
    sb.use_fixture("issue_list.json", "issue_list_malformed.json");
    assert!(sb.backend().list_issues("widgets").is_err());
    assert!(sb.backend().repo_summary("widgets").is_err());
}

#[test]
fn invalid_repo_names_are_rejected_before_any_command_runs() {
    let sb = sandbox_with_repo("names");
    let b = sb.backend();
    for bad in ["../etc", "/abs/path", ".hidden", "a/b", ""] {
        assert!(b.list_issues(bad).is_err(), "{bad:?}");
        assert!(b.slug(bad).is_err(), "{bad:?}");
        assert!(b.repo_summary(bad).is_err(), "{bad:?}");
    }
    assert!(sb.gh_log().is_empty());
}

#[test]
fn create_returns_the_url_and_passes_the_expected_args() {
    let sb = sandbox_with_repo("create");
    let out = sb
        .backend()
        .create(
            "widgets",
            &NewIssue {
                title: "[roadmap] Brand new".into(),
                body: "why".into(),
                status: Column::Planned,
                milestone: Some("M1".into()),
            },
        )
        .unwrap();
    assert_eq!(out, "https://github.com/acme/widgets/issues/42");
    assert_eq!(domain::issue_number_from_url(&out), Some(42));
    assert_eq!(
        sb.roadmap_log(),
        vec!["create\twidgets\tBrand new\t--body\twhy\t--status\tplanned\t--milestone\tM1"]
    );
}

#[test]
fn mutations_shell_out_to_roadmap_with_exact_args() {
    let sb = sandbox_with_repo("mut");
    let b = sb.backend();
    b.set_status("widgets", 5, Column::Now).unwrap();
    b.edit("widgets", 5, "[roadmap] T", "body text").unwrap();
    b.comment("widgets", 5, "hello world").unwrap();
    b.close("widgets", 5, Some("shipped")).unwrap();
    b.close("widgets", 6, None).unwrap();
    b.reopen("widgets", 5).unwrap();
    assert_eq!(
        sb.roadmap_log(),
        vec![
            "status\twidgets\t5\tnow",
            "edit\twidgets\t5\t--title\t[roadmap] T\t--body\tbody text",
            "comment\twidgets\t5\thello world",
            "close\twidgets\t5\t--comment\tshipped",
            "close\twidgets\t6",
            "reopen\twidgets\t5",
        ]
    );
    assert!(b.set_status("widgets", 5, Column::Done).is_err());
    assert!(sb.gh_log().is_empty(), "mutations never call gh directly");
}

#[test]
fn roadmap_failure_is_reported() {
    let sb = sandbox_with_repo("rmfail");
    sb.put("roadmap_fail", "label not found\n");
    let err = sb
        .backend()
        .set_status("widgets", 1, Column::Now)
        .unwrap_err();
    assert!(format!("{err:#}").contains("label not found"));
}

#[test]
fn fixtures_parse_on_their_own() {
    assert_eq!(
        domain::parse_issues(&fixture("issue_list.json"))
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        domain::parse_issues(&fixture("issue_list_with_new.json"))
            .unwrap()
            .len(),
        5
    );
}
