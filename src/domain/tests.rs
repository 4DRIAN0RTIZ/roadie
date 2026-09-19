use super::*;

fn issue(n: u64, state: &str, labels: &[&str]) -> Issue {
    Issue {
        number: n,
        title: format!("[roadmap] Issue {n}"),
        state: state.into(),
        labels: labels.iter().map(|l| Label { name: (*l).into() }).collect(),
        ..Default::default()
    }
}

#[test]
fn parses_gh_json_with_null_milestone() {
    let json = r#"[{"number":3,"title":"[roadmap] A","body":"hi","labels":[{"name":"roadmap"},{"name":"roadmap:now","color":"fff"}],"state":"OPEN","milestone":null,"assignees":[{"login":"me"}],"url":"https://github.com/o/r/issues/3","updatedAt":"2026-01-01T00:00:00Z"}]"#;
    let v = parse_issues(json).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].number, 3);
    assert_eq!(v[0].milestone, None);
    assert_eq!(v[0].column(), Column::Now);
    assert_eq!(v[0].updated_at, "2026-01-01T00:00:00Z");
}

#[test]
fn parses_milestone_and_missing_fields() {
    let v = parse_issues(r#"[{"number":1,"milestone":{"title":"M1"}}]"#).unwrap();
    assert_eq!(v[0].milestone_title(), Some("M1"));
    assert!(parse_issues("nope").is_err());
}

#[test]
fn column_from_labels_and_state() {
    assert_eq!(issue(1, "OPEN", &["roadmap"]).column(), Column::Idea);
    assert_eq!(
        issue(1, "OPEN", &["roadmap", "roadmap:planned"]).column(),
        Column::Planned
    );
    assert_eq!(issue(1, "OPEN", &["roadmap:idea"]).column(), Column::Idea);
    assert_eq!(
        issue(1, "CLOSED", &["roadmap:working"]).column(),
        Column::Done
    );
    // most advanced label wins if several leaked in
    assert_eq!(
        issue(1, "OPEN", &["roadmap:idea", "roadmap:working"]).column(),
        Column::Working
    );
}

#[test]
fn column_navigation_clamps() {
    assert_eq!(Column::Idea.prev(), None);
    assert_eq!(Column::Idea.next(), Some(Column::Planned));
    assert_eq!(Column::Working.next(), Some(Column::Done));
    assert_eq!(Column::Done.next(), None);
    assert_eq!(Column::Done.index(), 4);
    assert_eq!(Column::Now.status_arg(), Some("now"));
    assert_eq!(Column::Done.status_arg(), None);
}

#[test]
fn rotation_removes_all_other_state_labels() {
    let ops = rotation_ops(Column::Now).unwrap();
    assert_eq!(ops.add, "roadmap:now");
    let mut rem = ops.remove.clone();
    rem.sort();
    assert_eq!(
        rem,
        vec!["roadmap:idea", "roadmap:planned", "roadmap:working"]
    );
    assert!(rotation_ops(Column::Done).is_none());
}

#[test]
fn title_prefix_added_once() {
    assert_eq!(normalize_title("Foo"), "[roadmap] Foo");
    assert_eq!(normalize_title("  Foo "), "[roadmap] Foo");
    assert_eq!(normalize_title("[roadmap] Foo"), "[roadmap] Foo");
    assert_eq!(bare_title("[roadmap] Foo"), "Foo");
    assert_eq!(bare_title("Foo"), "Foo");
    let i = issue(1, "OPEN", &[]);
    assert_eq!(i.display_title(), "Issue 1");
}

#[test]
fn parses_remote_urls() {
    assert_eq!(
        parse_remote("https://github.com/o/r.git\n"),
        Some("o/r".into())
    );
    assert_eq!(parse_remote("https://github.com/o/r"), Some("o/r".into()));
    assert_eq!(parse_remote("git@github.com:o/r.git"), Some("o/r".into()));
    assert_eq!(
        parse_remote("ssh://git@github.com/o/r.git"),
        Some("o/r".into())
    );
    assert_eq!(parse_remote("https://gitlab.com/o/r.git"), None);
    assert_eq!(parse_remote(""), None);
}

#[test]
fn wip_limit_detects_second_working() {
    let issues = vec![
        issue(1, "OPEN", &["roadmap:working"]),
        issue(2, "OPEN", &["roadmap:now"]),
    ];
    assert_eq!(
        wip_conflict(&issues, Column::Working, 2).map(|i| i.number),
        Some(1)
    );
    assert!(wip_conflict(&issues, Column::Working, 1).is_none());
    assert!(wip_conflict(&issues, Column::Now, 2).is_none());
    let closed = vec![issue(1, "CLOSED", &["roadmap:working"])];
    assert!(wip_conflict(&closed, Column::Working, 2).is_none());
}

#[test]
fn filter_by_milestone_and_text() {
    let mut a = issue(1, "OPEN", &["roadmap:now"]);
    a.milestone = Some(Milestone { title: "M1".into() });
    a.body = "Refactor the parser".into();
    let b = issue(2, "OPEN", &["roadmap:now"]);
    let f = Filter {
        milestone: Some("M1".into()),
        text: String::new(),
    };
    assert!(f.matches(&a) && !f.matches(&b));
    let f = Filter {
        milestone: None,
        text: "PARSER".into(),
    };
    assert!(f.matches(&a) && !f.matches(&b));
    let f = Filter {
        milestone: None,
        text: "#2".into(),
    };
    assert!(f.matches(&b) && !f.matches(&a));
    assert!(!Filter::default().is_active());
    assert!(Filter {
        milestone: None,
        text: "x".into()
    }
    .is_active());
    assert!(Filter::default().matches(&a));
}

#[test]
fn milestones_are_unique_sorted() {
    let mut a = issue(1, "OPEN", &[]);
    a.milestone = Some(Milestone { title: "B".into() });
    let mut b = issue(2, "OPEN", &[]);
    b.milestone = Some(Milestone { title: "A".into() });
    let c = a.clone();
    assert_eq!(milestones_of(&[a, b, c]), vec!["A", "B"]);
}

#[test]
fn column_issues_sorted_and_filtered() {
    let mut d1 = issue(1, "CLOSED", &[]);
    d1.updated_at = "2026-01-01".into();
    let mut d2 = issue(2, "CLOSED", &[]);
    d2.updated_at = "2026-02-01".into();
    let p3 = issue(3, "OPEN", &["roadmap:planned"]);
    let p2 = issue(4, "OPEN", &["roadmap:planned"]);
    let all = vec![d1, d2, p3, p2];
    let done: Vec<u64> = column_issues(&all, Column::Done, &Filter::default())
        .iter()
        .map(|i| i.number)
        .collect();
    assert_eq!(done, vec![2, 1]); // most recently updated first
    let planned: Vec<u64> = column_issues(&all, Column::Planned, &Filter::default())
        .iter()
        .map(|i| i.number)
        .collect();
    assert_eq!(planned, vec![3, 4]); // by number
}

#[test]
fn parses_issue_number_from_create_output() {
    assert_eq!(
        issue_number_from_url("https://github.com/o/r/issues/42\n"),
        Some(42)
    );
    assert_eq!(
        issue_number_from_url("Creating issue in o/r\n\nhttps://github.com/o/r/issues/7/"),
        Some(7)
    );
    assert_eq!(issue_number_from_url("no url here"), None);
    assert_eq!(issue_number_from_url(""), None);
}
