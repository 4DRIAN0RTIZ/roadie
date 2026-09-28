//! Per-repo roadmap stats shown on the repo cards.
use serde::{Deserialize, Serialize};

use super::{Column, Issue};

/// Columns shown as badges, in display order.
pub const BADGE_COLUMNS: [Column; 4] =
    [Column::Idea, Column::Planned, Column::Now, Column::Working];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoSummary {
    pub description: String,
    /// Open issue counts, indexed like `BADGE_COLUMNS`.
    pub counts: [usize; 4],
    /// Title of the working issue, else the lowest-numbered `now` issue.
    pub active: Option<String>,

    pub latest_release: Option<String>,
}

impl RepoSummary {
    /// Builds a summary from a repo's open roadmap issues (closed ones are skipped).
    pub fn from_issues(description: &str, issues: &[Issue]) -> Self {
        let mut counts = [0; 4];
        let mut open: Vec<&Issue> = issues
            .iter()
            .filter(|i| i.column() != Column::Done)
            .collect();
        open.sort_by_key(|i| i.number);
        for i in &open {
            if let Some(p) = BADGE_COLUMNS.iter().position(|c| *c == i.column()) {
                counts[p] += 1;
            }
        }
        let active = [Column::Working, Column::Now].into_iter().find_map(|c| {
            open.iter()
                .find(|i| i.column() == c)
                .map(|i| i.display_title().to_string())
        });
        Self {
            description: description.trim().to_string(),
            counts,
            active,
            latest_release: None,
        }
    }

    pub fn total(&self) -> usize {
        self.counts.iter().sum()
    }
}

/// Sums the counts of several summaries (the "all repos" card).
pub fn aggregate<'a>(summaries: impl IntoIterator<Item = &'a RepoSummary>) -> [usize; 4] {
    let mut counts = [0; 4];
    for s in summaries {
        for (t, c) in counts.iter_mut().zip(s.counts) {
            *t += c;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Label;

    fn issue(n: u64, state: &str, labels: &[&str]) -> Issue {
        Issue {
            number: n,
            title: format!("[roadmap] T{n}"),
            state: state.into(),
            labels: labels.iter().map(|l| Label { name: (*l).into() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn counts_per_column_and_skips_closed() {
        let s = RepoSummary::from_issues(
            " desc ",
            &[
                issue(1, "OPEN", &["roadmap"]),
                issue(2, "OPEN", &["roadmap", "roadmap:idea"]),
                issue(3, "OPEN", &["roadmap:planned"]),
                issue(4, "OPEN", &["roadmap:now"]),
                issue(5, "OPEN", &["roadmap:working"]),
                issue(6, "CLOSED", &["roadmap:working"]),
            ],
        );
        assert_eq!(s.counts, [2, 1, 1, 1]);
        assert_eq!(s.total(), 5);
        assert_eq!(s.description, "desc");
    }

    #[test]
    fn active_prefers_working_then_lowest_now() {
        let s = RepoSummary::from_issues(
            "",
            &[
                issue(9, "OPEN", &["roadmap:now"]),
                issue(4, "OPEN", &["roadmap:now"]),
            ],
        );
        assert_eq!(s.active.as_deref(), Some("T4"));
        let s = RepoSummary::from_issues(
            "",
            &[
                issue(4, "OPEN", &["roadmap:now"]),
                issue(7, "OPEN", &["roadmap:working"]),
            ],
        );
        assert_eq!(s.active.as_deref(), Some("T7"));
        assert_eq!(RepoSummary::from_issues("", &[]).active, None);
    }

    #[test]
    fn aggregate_sums_counts() {
        let a = RepoSummary {
            counts: [1, 2, 3, 4],
            ..Default::default()
        };
        let b = RepoSummary {
            counts: [1, 0, 0, 1],
            ..Default::default()
        };
        assert_eq!(aggregate([&a, &b]), [2, 2, 3, 5]);
        assert_eq!(aggregate([]), [0; 4]);
    }
}
