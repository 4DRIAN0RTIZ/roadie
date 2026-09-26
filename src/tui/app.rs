//! UI state. Pure (no I/O): every transition here is unit-tested.
use crate::backend::NewIssue;
use std::cell::Cell;
use std::collections::HashMap;
use std::time::Instant;

use crate::cache::{self, Cache};
use crate::domain::{self, Column, Filter, Issue, RepoSummary};

use super::activity::{Activity, DONE_TTL};
use super::effects::Effects;
use super::{anim, grid};

use super::theme::Theme;
use super::widgets::{Scroll, TextInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Repos,
    Overview,
    Board,
}

/// Lazily loaded card data for one repo.
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryState {
    Loading,
    Ready(RepoSummary),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Create,
    Edit(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Body,
    Status,
    Milestone,
}

/// Statuses selectable in the create form (Done is not a label).
pub const FORM_STATUSES: [Column; 4] =
    [Column::Idea, Column::Planned, Column::Now, Column::Working];

#[derive(Debug, Clone)]
pub struct Form {
    pub kind: FormKind,
    pub title: TextInput,
    pub body: TextInput,
    pub status: Column,
    /// Choices for the milestone field; index 0 of the cycle means "none".
    pub milestones: Vec<String>,
    pub milestone_idx: usize,
    pub focus: Field,
}

impl Form {
    pub fn new_create(milestones: Vec<String>) -> Self {
        Self {
            kind: FormKind::Create,
            title: TextInput::default(),
            body: TextInput::default(),
            status: Column::Idea,
            milestones,
            milestone_idx: 0,
            focus: Field::Title,
        }
    }

    pub fn new_edit(issue: &Issue) -> Self {
        Self {
            kind: FormKind::Edit(issue.number),
            title: TextInput::new(issue.title.clone()),
            body: TextInput::new(issue.body.clone()),
            status: issue.column(),
            milestones: vec![],
            milestone_idx: 0,
            focus: Field::Title,
        }
    }

    pub fn fields(&self) -> &'static [Field] {
        match self.kind {
            FormKind::Create => &[Field::Title, Field::Body, Field::Status, Field::Milestone],
            FormKind::Edit(_) => &[Field::Title, Field::Body],
        }
    }

    fn shift_focus(&mut self, delta: isize) {
        let f = self.fields();
        let i = f.iter().position(|x| *x == self.focus).unwrap_or(0) as isize;
        self.focus = f[(i + delta).rem_euclid(f.len() as isize) as usize];
    }

    pub fn next_focus(&mut self) {
        self.shift_focus(1);
    }

    pub fn prev_focus(&mut self) {
        self.shift_focus(-1);
    }

    pub fn cycle_status(&mut self, delta: isize) {
        let i = FORM_STATUSES
            .iter()
            .position(|c| *c == self.status)
            .unwrap_or(0) as isize;
        self.status = FORM_STATUSES[(i + delta).rem_euclid(FORM_STATUSES.len() as isize) as usize];
    }

    pub fn cycle_milestone(&mut self, delta: isize) {
        let n = self.milestones.len() as isize + 1;
        self.milestone_idx = (self.milestone_idx as isize + delta).rem_euclid(n) as usize;
    }

    pub fn milestone(&self) -> Option<&str> {
        self.milestone_idx
            .checked_sub(1)
            .and_then(|i| self.milestones.get(i))
            .map(String::as_str)
    }

    /// Text buffer under focus, if the focused field is a text field.
    pub fn focused_text_mut(&mut self) -> Option<&mut TextInput> {
        match self.focus {
            Field::Title => Some(&mut self.title),
            Field::Body => Some(&mut self.body),
            _ => None,
        }
    }

    fn clean_title(&self) -> Result<String, String> {
        let t = self.title.trim();
        if t.strip_prefix("[roadmap]").unwrap_or(t).trim().is_empty() {
            return Err("title is required".into());
        }
        Ok(domain::normalize_title(t))
    }

    pub fn build_new(&self) -> Result<NewIssue, String> {
        Ok(NewIssue {
            title: self.clean_title()?,
            body: self.body.trim().to_string(),
            status: self.status,
            milestone: self.milestone().map(String::from),
        })
    }

    pub fn build_edit(&self) -> Result<(String, String), String> {
        Ok((self.clean_title()?, self.body.trim().to_string()))
    }
}

#[derive(Debug, Clone)]
pub enum Popup {
    None,
    Help,
    Detail,
    Form(Form),
    Comment(TextInput),
    Close(TextInput),
    /// Highlighted index into `Column::ALL`.
    Move(usize),
    Search(TextInput),
    /// Second card being moved to `working` while another is already there.
    WipWarn {
        number: u64,
        target: Column,
    },
}

#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Action results (not errors) clear themselves after this long.
pub const STATUS_TTL: std::time::Duration = DONE_TTL;

pub struct App {
    pub screen: Screen,
    pub popup: Popup,
    pub theme: Theme,
    pub should_quit: bool,
    pub message: Option<Message>,
    /// Session + on-disk cache of summaries and boards.
    pub cache: Cache,
    /// Snapshot sink of the background cache writer (none in tests).
    pub persist: Option<std::sync::mpsc::Sender<String>>,
    /// Unix-seconds clock (injected in tests).
    pub clock: fn() -> u64,
    /// Visual-only effects; never affects state.
    pub fx: Effects,
    last_sel: Option<u64>,
    last_counts: Option<[usize; 5]>,
    last_popup: Option<std::mem::Discriminant<Popup>>,
    /// Footer state of the background repo loading.
    pub activity: Activity,

    pub repos: Vec<String>,
    /// 0 is the "all repos" overview entry, then `repos`.
    pub repo_sel: usize,
    pub summaries: HashMap<String, SummaryState>,
    /// When each card's data arrived (drives its fade-in).
    pub ready_at: HashMap<String, Instant>,
    /// Fades only run on truecolor terminals.
    pub truecolor: bool,
    /// Repos waiting to be handed to the background loader.
    pub fetch_queue: Vec<String>,
    /// Card columns and first visible card row, written by the renderer.
    pub grid_cols: Cell<usize>,
    pub grid_offset: Cell<usize>,
    pub repo: Option<String>,
    pub slug: Option<String>,
    pub issues: Vec<Issue>,
    pub milestones: Vec<String>,
    pub filter: Filter,
    pub col: usize,
    pub rows: [usize; 5],
    /// Open issues across repos for the overview screen.
    pub overview: Vec<(String, Issue)>,
    pub overview_sel: usize,

    pub help_scroll: Scroll,
    pub detail_scroll: Scroll,
    /// Set when the user asked to edit the form body in `$EDITOR`.
    pub editor_request: bool,
}

impl App {
    pub fn new() -> Self {
        let truecolor = anim::truecolor_from(std::env::var("COLORTERM").ok().as_deref());
        Self {
            screen: Screen::Repos,
            popup: Popup::None,
            theme: Theme::default(),
            should_quit: false,
            message: None,
            cache: Cache::default(),
            persist: None,
            clock: cache::unix_now,
            fx: Effects::new(truecolor),
            last_sel: None,
            last_counts: None,
            last_popup: None,
            activity: Activity::default(),
            repos: vec![],
            repo_sel: 0,
            summaries: HashMap::new(),
            ready_at: HashMap::new(),
            truecolor,
            fetch_queue: vec![],
            grid_cols: Cell::new(1),
            grid_offset: Cell::new(0),
            repo: None,
            slug: None,
            issues: vec![],
            milestones: vec![],
            filter: Filter::default(),
            col: 0,
            rows: [0; 5],
            overview: vec![],
            overview_sel: 0,
            help_scroll: Scroll::default(),
            detail_scroll: Scroll::default(),
            editor_request: false,
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    pub fn set_error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
            at: Instant::now(),
        });
    }

    /// Time-based housekeeping: drops an expired "done" activity line and
    /// expired action results. Errors stay until the next key press.
    pub fn tick(&mut self, now: Instant) {
        self.activity = self.activity.expire(now);
        self.fx.prune(now);
        if self
            .message
            .as_ref()
            .is_some_and(|m| !m.error && now.saturating_duration_since(m.at) >= STATUS_TTL)
        {
            self.message = None;
        }
    }

    /// True when the redraw tick must run fast: fades, pending loads, or a
    /// footer activity line that still has to be cleared.
    pub fn needs_fast_tick(&self, now: Instant) -> bool {
        self.activity.is_active() || self.animating(now) || self.fx.is_active(now)
    }

    pub fn column(&self) -> Column {
        Column::ALL[self.col]
    }

    pub fn visible(&self, col: Column) -> Vec<&Issue> {
        domain::column_issues(&self.issues, col, &self.filter)
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        let col = self.column();
        self.visible(col).get(self.rows[col.index()]).copied()
    }

    /// Keeps every column's cursor inside its (possibly shrunk) list.
    pub fn clamp(&mut self) {
        for c in Column::ALL {
            let len = self.visible(c).len();
            let r = &mut self.rows[c.index()];
            *r = (*r).min(len.saturating_sub(1));
        }
    }

    pub fn move_col(&mut self, delta: isize) {
        self.col = (self.col as isize + delta).clamp(0, 4) as usize;
    }

    pub fn move_row(&mut self, delta: isize) {
        let col = self.column();
        let len = self.visible(col).len() as isize;
        if len == 0 {
            return;
        }
        let r = &mut self.rows[col.index()];
        *r = (*r as isize + delta).clamp(0, len - 1) as usize;
    }

    pub fn jump_row(&mut self, end: bool) {
        let len = self.visible(self.column()).len();
        self.rows[self.col] = if end { len.saturating_sub(1) } else { 0 };
    }

    /// Moves the cursor onto issue `number`, wherever it now lives.
    pub fn focus_issue(&mut self, number: u64) {
        for c in Column::ALL {
            if let Some(pos) = self.visible(c).iter().position(|i| i.number == number) {
                self.col = c.index();
                self.rows[c.index()] = pos;
                return;
            }
        }
    }

    /// Replaces the issue list, refreshing milestone choices and cursors.
    pub fn set_issues(&mut self, issues: Vec<Issue>) {
        self.set_issues_at(issues, Instant::now());
    }

    /// Like `set_issues`, flashing every card that is new or changed (column,
    /// title, body or update time) unless the board was empty before.
    pub fn set_issues_at(&mut self, issues: Vec<Issue>, now: Instant) {
        if !self.issues.is_empty() {
            let old: HashMap<u64, &Issue> = self.issues.iter().map(|i| (i.number, i)).collect();
            for i in &issues {
                let same = old.get(&i.number).is_some_and(|o| {
                    o.column() == i.column()
                        && o.title == i.title
                        && o.body == i.body
                        && o.updated_at == i.updated_at
                });
                if !same {
                    self.fx.flash(i.number, now);
                }
            }
        }
        self.milestones = domain::milestones_of(&issues);
        self.issues = issues;
        if let Some(m) = &self.filter.milestone {
            if !self.milestones.contains(m) {
                self.filter.milestone = None;
            }
        }
        self.clamp();
    }

    /// Call after input was handled: notices selection, column-count and popup
    /// changes and starts the matching (visual-only) effects.
    pub fn after_input(&mut self, now: Instant) {
        if self.screen == Screen::Board {
            let sel = self.selected_issue().map(|i| i.number);
            if sel != self.last_sel {
                if let Some(n) = sel {
                    self.fx.select(n, now);
                }
                self.last_sel = sel;
            }
            let mut cur = [0; 5];
            for c in Column::ALL {
                cur[c.index()] = self.visible(c).len();
            }
            if let Some(prev) = self.last_counts {
                for (c, (a, b)) in prev.iter().zip(cur).enumerate() {
                    if *a != b {
                        self.fx.count_flash(c, now);
                    }
                }
            }
            self.last_counts = Some(cur);
        } else {
            self.last_sel = None;
            self.last_counts = None;
        }
        let disc = std::mem::discriminant(&self.popup);
        if self.last_popup != Some(disc) {
            if !matches!(self.popup, Popup::None) {
                self.fx.start_popup(now);
            }
            self.last_popup = Some(disc);
        }
    }

    /// Forgets board baselines so opening a board does not flash counts.
    pub fn reset_board_effects(&mut self, now: Instant) {
        self.last_sel = None;
        self.last_counts = None;
        self.fx.start_board(now);
    }

    /// Cycles the milestone filter: none -> each milestone -> none.
    pub fn cycle_milestone_filter(&mut self) {
        let next = match &self.filter.milestone {
            None => self.milestones.first().cloned(),
            Some(cur) => {
                let i = self.milestones.iter().position(|m| m == cur).unwrap_or(0);
                self.milestones.get(i + 1).cloned()
            }
        };
        self.filter.milestone = next;
        self.clamp();
    }

    pub fn repo_entries(&self) -> usize {
        self.repos.len() + 1
    }

    /// h/l: previous/next card.
    pub fn move_repo_sel(&mut self, delta: isize) {
        self.repo_sel = grid::move_horizontal(self.repo_sel, delta, self.repo_entries());
    }

    /// j/k: one card row down/up.
    pub fn move_repo_row(&mut self, down: bool) {
        self.repo_sel = grid::move_vertical(
            self.repo_sel,
            down,
            self.grid_cols.get(),
            self.repo_entries(),
        );
    }

    pub fn jump_repo(&mut self, end: bool) {
        self.repo_sel = if end { self.repo_entries() - 1 } else { 0 };
    }

    /// Queues summary loads. Without `force`, repos with a cached summary are
    /// shown straight from the cache (no fetch); with it, every card hides and
    /// is refetched. Only fetched repos count towards the footer progress.
    pub fn request_summaries(&mut self, force: bool, now: Instant) {
        self.fetch_queue.clear();
        self.ready_at.clear();
        let mut cached = 0;
        let mut oldest: Option<u64> = None;
        for r in self.repos.clone() {
            let hit = (!force).then(|| self.cache.summary(&r).cloned()).flatten();
            match hit {
                Some(c) => {
                    oldest = Some(oldest.map_or(c.at, |o| o.min(c.at)));
                    self.summaries
                        .insert(r.clone(), SummaryState::Ready(c.value));
                    self.ready_at.insert(r, now + anim::stagger(cached));
                    cached += 1;
                }
                None => {
                    self.summaries.insert(r.clone(), SummaryState::Loading);
                    self.fetch_queue.push(r);
                }
            }
        }
        self.activity = Activity::start(self.fetch_queue.len());
        if let Some(at) = oldest {
            self.set_status(format!(
                "loaded from cache ({}), r to refresh",
                cache::age_text((self.clock)(), at)
            ));
        }
    }

    /// Forces a refetch of one repo's summary, leaving every other card and
    /// queued fetch untouched. No-op if that repo is already loading.
    pub fn request_summary(&mut self, repo: &str) {
        if matches!(self.summaries.get(repo), Some(SummaryState::Loading)) {
            return;
        }
        self.ready_at.remove(repo);
        self.summaries
            .insert(repo.to_string(), SummaryState::Loading);
        if !self.fetch_queue.iter().any(|r| r == repo) {
            self.fetch_queue.push(repo.to_string());
        }
        self.activity = match self.activity {
            Activity::Fetching {
                done,
                total,
                errors,
            } => Activity::Fetching {
                done,
                total: total + 1,
                errors,
            },
            _ => Activity::start(1),
        };
    }

    /// Sends the current cache to the background writer.
    pub fn persist_cache(&self) {
        if let Some(tx) = &self.persist {
            let _ = tx.send(self.cache.to_json());
        }
    }

    pub fn apply_summary(
        &mut self,
        repo: String,
        result: Result<RepoSummary, String>,
        now: Instant,
    ) {
        // Errors are shown (and fade in) like loaded cards.
        self.ready_at.insert(repo.clone(), now);
        // Only results of the current round count towards the footer progress.
        if matches!(self.summaries.get(&repo), Some(SummaryState::Loading)) {
            self.activity = self.activity.complete(result.is_ok(), now);
        }
        let st = match result {
            Ok(s) => {
                self.cache.put_summary(&repo, s.clone(), (self.clock)());
                // A cached board that disagrees with fresh counts is stale.
                let stale = self.cache.board(&repo).is_some_and(|b| {
                    RepoSummary::from_issues("", &b.value.issues).counts != s.counts
                });
                if stale {
                    self.cache.drop_board(&repo);
                }
                self.persist_cache();
                SummaryState::Ready(s)
            }
            // Refetch failed: keep the old cached data and report the error.
            Err(e) => match self.cache.summary(&repo).cloned() {
                Some(old) => {
                    self.set_error(format!("{repo}: {e}"));
                    SummaryState::Ready(old.value)
                }
                None => SummaryState::Failed(e),
            },
        };
        self.summaries.insert(repo, st);
    }

    /// Refreshes cache and card counts after a board reload (own actions
    /// included), so neither shows stale data.
    pub fn sync_repo_cache(&mut self, repo: &str) {
        let now = (self.clock)();
        let old = self
            .cache
            .summary(repo)
            .map(|s| s.value.description.clone());
        let desc = old.or_else(|| match self.summaries.get(repo) {
            Some(SummaryState::Ready(s)) => Some(s.description.clone()),
            _ => None,
        });
        self.cache
            .put_board(repo, self.issues.clone(), self.milestones.clone(), now);
        if let Some(desc) = desc {
            let s = RepoSummary::from_issues(&desc, &self.issues);
            self.cache.put_summary(repo, s.clone(), now);
            self.summaries
                .insert(repo.to_string(), SummaryState::Ready(s));
        }
        self.persist_cache();
    }

    /// Drops everything cached for `repo` (reload after a mutation failed).
    pub fn invalidate_repo_cache(&mut self, repo: &str) {
        self.cache.invalidate(repo);
        self.persist_cache();
    }

    /// Eased fade-in progress of a card's content (1.0 = fully shown).
    pub fn fade_of(&self, repo: &str, now: Instant) -> f32 {
        match (self.truecolor, self.ready_at.get(repo)) {
            (true, Some(t)) => anim::ease_in_out(anim::progress(now.saturating_duration_since(*t))),
            _ => 1.0,
        }
    }

    /// True while a load is pending or a fade is running (fast redraw tick).
    pub fn animating(&self, now: Instant) -> bool {
        self.summaries
            .iter()
            .any(|(r, s)| matches!(s, SummaryState::Loading) || self.fade_of(r, now) < 1.0)
    }

    pub fn move_overview_sel(&mut self, delta: isize) {
        let len = self.overview.len() as isize;
        if len > 0 {
            self.overview_sel = (self.overview_sel as isize + delta).clamp(0, len - 1) as usize;
        }
    }

    pub fn wip_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.column() == Column::Working)
            .count()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Label, Milestone};

    fn issue(n: u64, labels: &[&str], ms: Option<&str>) -> Issue {
        Issue {
            number: n,
            title: format!("[roadmap] T{n}"),
            state: "OPEN".into(),
            labels: labels.iter().map(|l| Label { name: (*l).into() }).collect(),
            milestone: ms.map(|m| Milestone { title: m.into() }),
            ..Default::default()
        }
    }

    fn app() -> App {
        let mut a = App::new();
        a.set_issues(vec![
            issue(1, &["roadmap:idea"], Some("M1")),
            issue(2, &["roadmap:idea"], None),
            issue(3, &["roadmap:planned"], Some("M2")),
        ]);
        a
    }

    #[test]
    fn navigation_clamps() {
        let mut a = app();
        a.move_col(-1);
        assert_eq!(a.col, 0);
        a.move_row(5);
        assert_eq!(a.selected_issue().unwrap().number, 2);
        a.move_row(-9);
        assert_eq!(a.selected_issue().unwrap().number, 1);
        a.move_col(1);
        assert_eq!(a.selected_issue().unwrap().number, 3);
        a.move_col(9);
        assert_eq!(a.column(), Column::Done);
        assert!(a.selected_issue().is_none());
        a.move_row(1); // empty column: no panic
        a.jump_row(true);
    }

    #[test]
    fn focus_follows_issue_and_clamp_after_shrink() {
        let mut a = app();
        a.rows[0] = 1;
        a.set_issues(vec![issue(1, &["roadmap:idea"], None)]);
        assert_eq!(a.rows[0], 0);
        a.set_issues(vec![issue(1, &["roadmap:now"], None)]);
        a.focus_issue(1);
        assert_eq!(a.column(), Column::Now);
    }

    #[test]
    fn milestone_filter_cycles_and_filters() {
        let mut a = app();
        assert_eq!(a.milestones, vec!["M1", "M2"]);
        a.cycle_milestone_filter();
        assert_eq!(a.filter.milestone.as_deref(), Some("M1"));
        assert_eq!(a.visible(Column::Idea).len(), 1);
        a.cycle_milestone_filter();
        assert_eq!(a.filter.milestone.as_deref(), Some("M2"));
        a.cycle_milestone_filter();
        assert_eq!(a.filter.milestone, None);
    }

    #[test]
    fn stale_milestone_filter_dropped_on_refresh() {
        let mut a = app();
        a.filter.milestone = Some("M1".into());
        a.set_issues(vec![issue(9, &[], None)]);
        assert_eq!(a.filter.milestone, None);
    }

    #[test]
    fn repo_selection_includes_all_entry() {
        let mut a = App::new();
        a.repos = vec!["a".into(), "b".into()];
        a.move_repo_sel(10); // wide card: no row neighbors
        assert_eq!(a.repo_sel, 0);
        a.repo_sel = 1;
        a.move_repo_sel(10);
        assert_eq!(a.repo_sel, 2);
        a.move_repo_sel(-10);
        assert_eq!(a.repo_sel, 1);
    }

    #[test]
    fn repo_grid_rows_and_jumps() {
        let mut a = App::new();
        a.repos = (0..6).map(|i| format!("r{i}")).collect(); // wide + 6 repos
        a.grid_cols.set(3); // [0] / [1 2 3] [4 5 6]
        a.move_repo_sel(1); // wide card: no row neighbors
        assert_eq!(a.repo_sel, 0);
        a.move_repo_row(true);
        assert_eq!(a.repo_sel, 1);
        a.move_repo_sel(1);
        a.move_repo_row(true);
        assert_eq!(a.repo_sel, 5);
        a.move_repo_row(false);
        a.move_repo_row(false);
        assert_eq!(a.repo_sel, 0);
        a.jump_repo(true);
        assert_eq!(a.repo_sel, 6);
        a.jump_repo(false);
        assert_eq!(a.repo_sel, 0);
    }

    #[test]
    fn summaries_start_loading_and_record_results() {
        let mut a = App::new();
        a.repos = vec!["a".into(), "b".into()];
        a.request_summaries(true, Instant::now());
        assert_eq!(a.fetch_queue, vec!["a", "b"]);
        assert_eq!(a.summaries["a"], SummaryState::Loading);
        a.apply_summary("a".into(), Ok(RepoSummary::default()), Instant::now());
        a.apply_summary("b".into(), Err("boom".into()), Instant::now());
        assert!(matches!(a.summaries["a"], SummaryState::Ready(_)));
        assert_eq!(a.summaries["b"], SummaryState::Failed("boom".into()));
    }

    #[test]
    fn form_create_builds_prefixed_issue() {
        let mut f = Form::new_create(vec!["M1".into(), "M2".into()]);
        assert!(f.build_new().is_err());
        f.title.set("Ship it");
        f.body.set(" short prose \n");
        f.cycle_status(1);
        f.cycle_milestone(1);
        let n = f.build_new().unwrap();
        assert_eq!(n.title, "[roadmap] Ship it");
        assert_eq!(n.body, "short prose");
        assert_eq!(n.status, Column::Planned);
        assert_eq!(n.milestone.as_deref(), Some("M1"));
        f.cycle_milestone(-1);
        assert_eq!(f.build_new().unwrap().milestone, None);
        f.cycle_milestone(-1); // wraps to last
        assert_eq!(f.milestone(), Some("M2"));
    }

    #[test]
    fn form_rejects_prefix_only_title() {
        let mut f = Form::new_create(vec![]);
        f.title.set("[roadmap] ");
        assert!(f.build_new().is_err());
    }

    #[test]
    fn form_focus_cycles_per_kind() {
        let mut f = Form::new_create(vec![]);
        for _ in 0..4 {
            f.next_focus();
        }
        assert_eq!(f.focus, Field::Title);
        f.prev_focus();
        assert_eq!(f.focus, Field::Milestone);
        let mut e = Form::new_edit(&issue(1, &[], None));
        e.next_focus();
        e.next_focus();
        assert_eq!(e.focus, Field::Title);
        assert_eq!(e.build_edit().unwrap().0, "[roadmap] T1");
    }

    #[test]
    fn form_status_cycle_wraps() {
        let mut f = Form::new_create(vec![]);
        f.cycle_status(-1);
        assert_eq!(f.status, Column::Working);
        f.cycle_status(1);
        assert_eq!(f.status, Column::Idea);
    }

    #[test]
    fn fade_and_animating_follow_load_state() {
        let mut a = App::new();
        a.truecolor = true;
        a.repos = vec!["a".into(), "b".into()];
        assert!(!a.animating(Instant::now())); // nothing requested yet
        a.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        assert!(a.animating(t0)); // pending loads
        a.apply_summary("a".into(), Ok(RepoSummary::default()), t0);
        assert_eq!(a.fade_of("a", t0), 0.0);
        let mid = t0 + std::time::Duration::from_millis(350);
        assert!(a.fade_of("a", mid) > 0.4 && a.fade_of("a", mid) < 1.0);
        let end = t0 + anim::FADE;
        assert_eq!(a.fade_of("a", end), 1.0);
        a.apply_summary("b".into(), Err("x".into()), t0);
        assert!(a.fade_of("b", mid) < 1.0); // error cards fade in too
        assert!(!a.animating(end)); // all settled -> idle tick
        assert!(a.animating(mid)); // "a" still fading
        a.truecolor = false;
        assert_eq!(a.fade_of("a", t0), 1.0); // no truecolor: instant
        assert!(!a.animating(mid));
    }

    #[test]
    fn staggered_start_delays_fade_and_keeps_animating() {
        let mut a = App::new();
        a.truecolor = true;
        a.repos = vec!["a".into(), "b".into()];
        a.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        a.apply_summary("a".into(), Ok(RepoSummary::default()), t0);
        a.apply_summary(
            "b".into(),
            Ok(RepoSummary::default()),
            t0 + anim::stagger(1),
        );
        let mid = t0 + anim::FADE / 2;
        assert!(a.fade_of("a", mid) > a.fade_of("b", mid)); // b starts later
        assert_eq!(a.fade_of("b", t0), 0.0); // still waiting for its delay
        let a_done = t0 + anim::FADE;
        assert_eq!(a.fade_of("a", a_done), 1.0);
        assert!(a.fade_of("b", a_done) < 1.0);
        assert!(a.animating(a_done)); // tick stays fast until b finishes
        assert!(!a.animating(t0 + anim::stagger(1) + anim::FADE));
    }

    #[test]
    fn footer_activity_tracks_loading_rounds() {
        let mut a = App::new();
        a.repos = vec!["a".into(), "b".into()];
        a.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        assert_eq!(a.activity.line().unwrap().0, " fetching 0/2 repos...");
        a.apply_summary("a".into(), Ok(RepoSummary::default()), t0);
        assert_eq!(a.activity.line().unwrap().0, " fetching 1/2 repos...");
        // A stale duplicate result for an already finished repo is not counted.
        a.apply_summary("a".into(), Ok(RepoSummary::default()), t0);
        assert_eq!(a.activity.line().unwrap().0, " fetching 1/2 repos...");
        a.apply_summary("b".into(), Err("x".into()), t0);
        assert_eq!(a.activity.line().unwrap().0, " done, 1 error");
        // Fast tick keeps running until the done line clears (3s), then idles.
        a.truecolor = false;
        assert!(a.needs_fast_tick(t0 + std::time::Duration::from_millis(2900)));
        a.tick(t0 + std::time::Duration::from_secs(3));
        assert!(!a.needs_fast_tick(t0 + std::time::Duration::from_secs(3)));
        assert_eq!(a.activity.line(), None);
        // `r` refresh restarts fetching.
        a.request_summaries(true, Instant::now());
        assert_eq!(a.activity.line().unwrap().0, " fetching 0/2 repos...");
    }

    #[test]
    fn action_results_expire_but_errors_stay() {
        let mut a = App::new();
        a.set_status("done");
        let at = a.message.as_ref().unwrap().at;
        a.tick(at + std::time::Duration::from_millis(2999));
        assert!(a.message.is_some());
        a.tick(at + STATUS_TTL);
        assert!(a.message.is_none());
        a.set_error("boom");
        let at = a.message.as_ref().unwrap().at;
        a.tick(at + std::time::Duration::from_secs(60));
        assert!(a.message.is_some());
    }

    #[test]
    fn changed_or_new_cards_flash_but_first_load_does_not() {
        let mut a = app();
        a.fx.enabled = true;
        let t0 = Instant::now();
        assert_eq!(a.fx.flash_level(1, t0), 1.0); // initial load: no flashes
        let mut moved = issue(1, &["roadmap:now"], Some("M1"));
        moved.updated_at = "later".into();
        a.set_issues_at(
            vec![
                moved,
                issue(2, &["roadmap:idea"], None),
                issue(9, &[], None),
            ],
            t0,
        );
        assert_eq!(a.fx.flash_level(1, t0), 0.0); // moved column
        assert_eq!(a.fx.flash_level(9, t0), 0.0); // new
        assert_eq!(a.fx.flash_level(2, t0), 1.0); // untouched
        assert!(a.fx.is_active(t0));
    }

    #[test]
    fn after_input_tracks_selection_counts_and_popups() {
        let mut a = app();
        a.fx.enabled = true;
        a.screen = Screen::Board;
        let t0 = Instant::now();
        a.reset_board_effects(t0);
        a.after_input(t0); // baseline: selects card 1, no count flash
        assert_eq!(a.fx.select_level(1, t0), 0.0);
        assert_eq!(a.fx.count_level(0, t0), 1.0);
        a.move_row(1);
        let t1 = t0 + std::time::Duration::from_millis(500);
        a.after_input(t1);
        assert_eq!(a.fx.select_level(2, t1), 0.0);
        // Only columns whose count changed flash (idea 2->1, planned 1->0).
        a.set_issues_at(vec![issue(1, &["roadmap:idea"], None)], t1);
        a.after_input(t1);
        assert_eq!(a.fx.count_level(0, t1), 0.0);
        assert_eq!(a.fx.count_level(1, t1), 0.0);
        assert_eq!(a.fx.count_level(2, t1), 1.0);
        // Opening a popup starts its fade; closing does not.
        assert_eq!(a.fx.popup_level(t1), 1.0);
        a.popup = Popup::Help;
        a.after_input(t1);
        assert_eq!(a.fx.popup_level(t1), 0.0);
        a.popup = Popup::None;
        a.after_input(t1 + std::time::Duration::from_secs(1));
        assert!(a.fx.popup_level(t1 + std::time::Duration::from_secs(1)) >= 1.0);
    }
}
