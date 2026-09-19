//! Pure domain logic: issues, columns, label rotation, filters.
use anyhow::Result;
use serde::{Deserialize, Serialize};

mod summary;
pub use summary::{aggregate, RepoSummary, BADGE_COLUMNS};

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Column {
    Idea,
    Planned,
    Now,
    Working,
    Done,
}

impl Column {
    pub const ALL: [Column; 5] = [
        Column::Idea,
        Column::Planned,
        Column::Now,
        Column::Working,
        Column::Done,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Column::Idea => "idea",
            Column::Planned => "planned",
            Column::Now => "now",
            Column::Working => "working",
            Column::Done => "done",
        }
    }
    pub fn status_arg(self) -> Option<&'static str> {
        match self {
            Column::Done => None,
            c => Some(c.title()),
        }
    }
    pub fn index(self) -> usize {
        Column::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
    pub fn next(self) -> Option<Column> {
        Column::ALL.get(self.index() + 1).copied()
    }
    pub fn prev(self) -> Option<Column> {
        self.index().checked_sub(1).map(|i| Column::ALL[i])
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Label {
    pub name: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Milestone {
    pub title: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Assignee {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<Label>,
    pub state: String,
    pub milestone: Option<Milestone>,
    pub assignees: Vec<Assignee>,
    pub url: String,
    pub updated_at: String,
}

impl Issue {
    pub fn column(&self) -> Column {
        if self.state.eq_ignore_ascii_case("closed") {
            return Column::Done;
        }
        [Column::Working, Column::Now, Column::Planned]
            .into_iter()
            .find(|c| {
                self.labels
                    .iter()
                    .any(|l| l.name == format!("{LABEL}:{}", c.title()))
            })
            .unwrap_or(Column::Idea)
    }
    pub fn milestone_title(&self) -> Option<&str> {
        self.milestone.as_ref().map(|m| m.title.as_str())
    }
    pub fn display_title(&self) -> &str {
        bare_title(&self.title)
    }
}

pub fn parse_issues(json: &str) -> Result<Vec<Issue>> {
    Ok(serde_json::from_str(json)?)
}

#[derive(Debug, PartialEq)]
pub struct LabelOps {
    pub add: String,
    pub remove: Vec<String>,
}
pub fn rotation_ops(target: Column) -> Option<LabelOps> {
    let add = format!("{LABEL}:{}", target.status_arg()?);
    let remove = STATUS_LABELS
        .iter()
        .filter(|s| **s != target.title())
        .map(|s| format!("{LABEL}:{s}"))
        .collect();
    Some(LabelOps { add, remove })
}
pub const LABEL: &str = "roadmap";
pub const STATUS_LABELS: [&str; 4] = ["working", "planned", "idea", "now"];
pub const TITLE_PREFIX: &str = "[roadmap] ";
pub fn normalize_title(t: &str) -> String {
    let t = t.trim();
    if t.starts_with("[roadmap]") {
        t.to_string()
    } else {
        format!("{TITLE_PREFIX}{t}")
    }
}
pub fn bare_title(t: &str) -> &str {
    t.strip_prefix(TITLE_PREFIX).unwrap_or(t)
}
/// Issue number from the URL `gh issue create` prints (last non-empty line
/// ending in `/issues/<n>`).
pub fn issue_number_from_url(out: &str) -> Option<u64> {
    let line = out.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let (_, n) = line.trim_end_matches('/').rsplit_once("/issues/")?;
    n.parse().ok()
}

pub fn parse_remote(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|p| u.strip_prefix(p))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (o, r) = (parts.next()?, parts.next()?);
    if o.is_empty() || r.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{o}/{r}"))
}
pub fn wip_conflict(issues: &[Issue], target: Column, moving: u64) -> Option<&Issue> {
    if target != Column::Working {
        return None;
    }
    issues
        .iter()
        .find(|i| i.number != moving && i.column() == Column::Working)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub milestone: Option<String>,
    pub text: String,
}
impl Filter {
    pub fn matches(&self, i: &Issue) -> bool {
        if let Some(m) = &self.milestone {
            if i.milestone_title() != Some(m.as_str()) {
                return false;
            }
        }
        let t = self.text.trim().to_lowercase();
        if t.is_empty() {
            return true;
        }
        format!("#{} {} {}", i.number, i.title, i.body)
            .to_lowercase()
            .contains(&t)
    }
    pub fn is_active(&self) -> bool {
        self.milestone.is_some() || !self.text.trim().is_empty()
    }
}
pub fn milestones_of(issues: &[Issue]) -> Vec<String> {
    let mut v: Vec<String> = issues
        .iter()
        .filter_map(|i| i.milestone_title().map(String::from))
        .collect();
    v.sort();
    v.dedup();
    v
}
pub fn column_issues<'a>(issues: &'a [Issue], col: Column, f: &Filter) -> Vec<&'a Issue> {
    let mut v: Vec<&Issue> = issues
        .iter()
        .filter(|i| i.column() == col && f.matches(i))
        .collect();
    if col == Column::Done {
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    } else {
        v.sort_by_key(|i| i.number);
    }
    v
}

#[cfg(test)]
mod tests;
