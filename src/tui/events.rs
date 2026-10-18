//! Key handling and the glue between UI state and the `Backend`.
//! Backend errors never propagate: they land in the status bar.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use std::time::Instant;

use crate::backend::Backend;
use crate::cache;
use crate::domain::{self, Column};

#[cfg(test)]
use super::app::SummaryState;
use super::app::{App, Field, Form, FormKind, Popup, Screen};
use super::widgets::TextInput;

// ---------- loading ----------

/// Rescans the local repo folders; true when the list was loaded.
fn scan_repos(app: &mut App, be: &dyn Backend) -> bool {
    match be.repos() {
        Ok(r) => {
            app.repos = r;
            app.repo_sel = app.repo_sel.min(app.repo_entries() - 1);
            app.message = None;
            true
        }
        Err(e) => {
            app.set_error(format!("{e:#}"));
            false
        }
    }
}

/// Startup: shows cached cards immediately and fetches only uncached repos.
pub fn load_repos(app: &mut App, be: &dyn Backend) {
    if scan_repos(app, be) {
        app.request_summaries(false, Instant::now());
    }
}

/// `r` on the grid: rescans and refetches every card.
pub fn refresh_repos(app: &mut App, be: &dyn Backend) {
    if scan_repos(app, be) {
        app.request_summaries(true, Instant::now());
    }
}

/// `u` on a repo card: refetches just that card's summary (bypasses cache).
fn refresh_repo(app: &mut App) {
    if app.repo_sel == 0 {
        return;
    }
    if let Some(repo) = app.repos.get(app.repo_sel - 1).cloned() {
        app.request_summary(&repo);
    }
}

/// Refetches the open board's issues; true on success (cache updated).
/// On failure the old issues and cache stay and the error is shown.
pub fn reload(app: &mut App, be: &dyn Backend) -> bool {
    let Some(repo) = app.repo.clone() else {
        return false;
    };
    match be.list_issues(&repo) {
        Ok(issues) => {
            let keep = app.selected_issue().map(|i| i.number);
            app.set_issues(issues);
            if let Some(n) = keep {
                app.focus_issue(n);
            }
            app.sync_repo_cache(&repo);
            true
        }
        Err(e) => {
            app.set_error(format!("{e:#}"));
            false
        }
    }
}

/// Reload after the user's own mutation: if it cannot be refreshed the
/// cached copy of the repo is dropped so it never shows stale data.
fn reload_after_mutation(app: &mut App, be: &dyn Backend) {
    if !reload(app, be) {
        if let Some(repo) = app.repo.clone() {
            app.invalidate_repo_cache(&repo);
        }
    }
}

/// Opens a repo's board: instantly from the cache when present, else fetched.
pub fn open_repo(app: &mut App, be: &dyn Backend, repo: &str) {
    app.repo = Some(repo.to_string());
    app.slug = be.slug(repo).ok();
    app.filter = domain::Filter::default();
    app.rows = [0; 5];
    app.col = 0;
    app.issues.clear();
    let cached = app.cache.board(repo).cloned();
    let from_cache = cached.is_some();
    match cached.map(|b| Ok(b.value)).unwrap_or_else(|| {
        be.list_issues(repo).map(|issues| cache::Board {
            issues,
            milestones: vec![],
        })
    }) {
        Ok(board) => {
            app.set_issues(board.issues);
            for m in board.milestones {
                if !app.milestones.contains(&m) {
                    app.milestones.push(m);
                }
            }
            app.milestones.sort();
            app.screen = Screen::Board;
            app.reset_board_effects(Instant::now());
            // Start on the first non-empty open column.
            if let Some(c) = Column::ALL.iter().find(|c| !app.visible(**c).is_empty()) {
                app.col = c.index();
            }
            if from_cache {
                let at = app.cache.board(repo).map_or(0, |b| b.at);
                let age = cache::age_text((app.clock)(), at);
                app.set_status(format!("cached ({age}), r to refresh"));
                return;
            }
            app.set_status("done");
        }
        Err(e) => {
            app.set_error(format!("{e:#}"));
            return;
        }
    }
    if let Ok(ms) = be.milestones(repo) {
        for m in ms {
            if !app.milestones.contains(&m) {
                app.milestones.push(m);
            }
        }
        app.milestones.sort();
    }
    app.sync_repo_cache(repo);
}

/// Cross-repo overview. Cached boards are reused unless `force`.
pub fn load_overview(app: &mut App, be: &dyn Backend, force: bool) {
    let mut rows = vec![];
    let mut errors = vec![];
    let mut dirty = false;
    for repo in app.repos.clone() {
        let cached = (!force)
            .then(|| app.cache.board(&repo).map(|b| b.value.issues.clone()))
            .flatten();
        let fetched = match cached {
            Some(i) => Ok(i),
            None => be.list_issues(&repo).inspect(|issues| {
                let ms = domain::milestones_of(issues);
                app.cache
                    .put_board(&repo, issues.clone(), ms, (app.clock)());
                dirty = true;
            }),
        };
        // A failed refetch falls back to the old cached copy.
        let result = fetched.or_else(|e| match app.cache.board(&repo) {
            Some(b) => {
                errors.push(format!("{repo}: {e:#}"));
                Ok(b.value.issues.clone())
            }
            None => Err(e),
        });
        match result {
            Ok(mut issues) => {
                issues.retain(|i| i.column() != Column::Done);
                issues.sort_by_key(|i| (std::cmp::Reverse(i.column().index()), i.number));
                rows.extend(issues.into_iter().map(|i| (repo.clone(), i)));
            }
            Err(e) => errors.push(format!("{repo}: {e:#}")),
        }
    }
    if dirty {
        app.persist_cache();
    }
    app.overview = rows;
    app.overview_sel = app.overview_sel.min(app.overview.len().saturating_sub(1));
    app.screen = Screen::Overview;
    match errors.first() {
        Some(e) => app.set_error(e.clone()),
        None => app.set_status("done"),
    }
}

// ---------- key dispatch ----------

pub fn handle_paste(app: &mut App, text: &str) {
    match &mut app.popup {
        Popup::Form(f) => {
            if let Some(t) = f.focused_text_mut() {
                t.paste(text);
            }
        }
        Popup::Comment(t) | Popup::Close(t) | Popup::Search(t) => t.push_str(text),
        _ => {}
    }
}

pub fn handle_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    match app.popup {
        Popup::None => match app.screen {
            Screen::Repos => repos_key(app, be, key),
            Screen::Overview => overview_key(app, be, key),
            Screen::Board => board_key(app, be, key),
        },
        Popup::Help => help_key(app, key),
        Popup::Detail => detail_key(app, be, key),
        Popup::Form(_) => form_key(app, be, key),
        Popup::Comment(_) => text_popup_key(app, be, key),
        Popup::Close(_) => text_popup_key(app, be, key),
        Popup::Search(_) => text_popup_key(app, be, key),
        Popup::Move(_) => move_key(app, be, key),
        Popup::WipWarn { .. } => wip_key(app, be, key),
    }
}

fn vertical(code: KeyCode) -> Option<isize> {
    match code {
        KeyCode::Char('j') | KeyCode::Down => Some(1),
        KeyCode::Char('k') | KeyCode::Up => Some(-1),
        _ => None,
    }
}

fn open_help(app: &mut App) {
    app.help_scroll.reset();
    app.popup = Popup::Help;
}

fn repos_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => app.move_repo_sel(-1),
        KeyCode::Char('l') | KeyCode::Right => app.move_repo_sel(1),
        KeyCode::Char('j') | KeyCode::Down => app.move_repo_row(true),
        KeyCode::Char('k') | KeyCode::Up => app.move_repo_row(false),
        KeyCode::Char('g') => app.jump_repo(false),
        KeyCode::Char('G') => app.jump_repo(true),
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => open_help(app),
        KeyCode::Char('r') => refresh_repos(app, be),
        KeyCode::Char('u') => refresh_repo(app),
        KeyCode::Enter => {
            if app.repo_sel == 0 {
                load_overview(app, be, false);
            } else if let Some(repo) = app.repos.get(app.repo_sel - 1).cloned() {
                open_repo(app, be, &repo);
            }
        }
        _ => {}
    }
}

fn overview_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    if let Some(d) = vertical(key.code) {
        app.move_overview_sel(d);
        return;
    }
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc | KeyCode::Char('b') | KeyCode::Backspace => app.screen = Screen::Repos,
        KeyCode::Char('?') => open_help(app),
        KeyCode::Char('r') => load_overview(app, be, true),
        KeyCode::Char('o') => {
            if let Some((_, i)) = app.overview.get(app.overview_sel) {
                let url = i.url.clone();
                open_url(app, be, &url);
            }
        }
        KeyCode::Enter => {
            if let Some((repo, i)) = app.overview.get(app.overview_sel).cloned() {
                open_repo(app, be, &repo);
                app.focus_issue(i.number);
            }
        }
        _ => {}
    }
}

fn open_url(app: &mut App, be: &dyn Backend, url: &str) {
    if url.is_empty() {
        app.set_error("issue has no URL");
    } else if let Err(e) = be.open_url(url) {
        app.set_error(format!("{e:#}"));
    }
}

fn board_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    if let Some(d) = vertical(key.code) {
        app.move_row(d);
        return;
    }
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => app.move_col(-1),
        KeyCode::Char('l') | KeyCode::Right => app.move_col(1),
        KeyCode::Char('g') => app.jump_row(false),
        KeyCode::Char('G') => app.jump_row(true),
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => open_help(app),
        KeyCode::Char('r') => {
            if reload(app, be) {
                app.fx.start_board(Instant::now());
                app.set_status("done");
            }
        }
        KeyCode::Char('b') | KeyCode::Backspace => app.screen = Screen::Repos,
        // Esc: clear an active filter first, otherwise go back.
        KeyCode::Esc if app.filter.is_active() => {
            app.filter = domain::Filter::default();
            app.clamp();
        }
        KeyCode::Esc => app.screen = Screen::Repos,
        KeyCode::Char('/') => app.popup = Popup::Search(TextInput::new(app.filter.text.clone())),
        KeyCode::Char('f') => {
            app.cycle_milestone_filter();
            let label = app
                .filter
                .milestone
                .clone()
                .unwrap_or_else(|| "all milestones".into());
            app.set_status(format!("milestone: {label}"));
        }
        KeyCode::Char('n') => {
            let mut ms = app.milestones.clone();
            ms.sort();
            app.popup = Popup::Form(Form::new_create(ms));
        }
        KeyCode::Char('H') => step_card(app, be, -1),
        KeyCode::Char('L') => step_card(app, be, 1),
        _ => card_key(app, be, key),
    }
}

/// Keys that need a selected card.
fn card_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    let Some(issue) = app.selected_issue().cloned() else {
        return;
    };
    match key.code {
        KeyCode::Enter => {
            app.detail_scroll.reset();
            app.popup = Popup::Detail;
        }
        KeyCode::Char('o') => open_url(app, be, &issue.url),
        KeyCode::Char('m') => app.popup = Popup::Move(issue.column().index()),
        KeyCode::Char('e') => app.popup = Popup::Form(Form::new_edit(&issue)),
        KeyCode::Char('c') => app.popup = Popup::Comment(TextInput::default()),
        KeyCode::Char('x') => {
            if issue.column() == Column::Done {
                app.set_error("already closed");
            } else {
                app.popup = Popup::Close(TextInput::default());
            }
        }
        KeyCode::Char('u') => reopen(app, be, issue.number, issue.column()),
        _ => {}
    }
}

fn step_card(app: &mut App, be: &dyn Backend, delta: isize) {
    let Some(issue) = app.selected_issue().cloned() else {
        return;
    };
    let cur = issue.column();
    let target = if delta > 0 { cur.next() } else { cur.prev() };
    if let Some(t) = target {
        move_issue(app, be, issue.number, t, false);
    }
}

fn reopen(app: &mut App, be: &dyn Backend, number: u64, cur: Column) {
    if cur != Column::Done {
        app.set_error("issue is already open");
        return;
    }
    let Some(repo) = app.repo.clone() else { return };
    match be.reopen(&repo, number) {
        Ok(()) => {
            app.set_status(format!("reopened #{number}"));
            reload_after_mutation(app, be);
            app.focus_issue(number);
        }
        Err(e) => app.set_error(format!("{e:#}")),
    }
}

/// Moves a card to `target`, mirroring `roadmap status` / `roadmap close`.
pub fn move_issue(app: &mut App, be: &dyn Backend, number: u64, target: Column, force: bool) {
    let Some(repo) = app.repo.clone() else { return };
    let Some(issue) = app.issues.iter().find(|i| i.number == number).cloned() else {
        return;
    };
    let cur = issue.column();
    if cur == target {
        return;
    }
    if !force {
        if let Some(other) = domain::wip_conflict(&app.issues, target, number) {
            app.set_error(format!("WIP limit: #{} is already working", other.number));
            app.popup = Popup::WipWarn { number, target };
            return;
        }
    }
    let result = if target == Column::Done {
        be.close(&repo, number, None)
    } else {
        if cur == Column::Done {
            if let Err(e) = be.reopen(&repo, number) {
                app.set_error(format!("{e:#}"));
                return;
            }
        }
        be.set_status(&repo, number, target)
    };
    match result {
        Ok(()) => {
            let labels = domain::rotation_ops(target)
                .map(|o| format!(" (+{})", o.add))
                .unwrap_or_default();
            app.set_status(format!("#{number} -> {}{labels}", target.title()));
            reload_after_mutation(app, be);
            app.focus_issue(number);
        }
        Err(e) => app.set_error(format!("{e:#}")),
    }
}

fn help_key(app: &mut App, key: KeyEvent) {
    if let Some(d) = vertical(key.code) {
        app.help_scroll.by(d as i32);
        return;
    }
    if matches!(
        key.code,
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
    ) {
        app.popup = Popup::None;
    }
}

fn detail_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    if let Some(d) = vertical(key.code) {
        app.detail_scroll.by(d as i32);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => app.popup = Popup::None,
        KeyCode::Char('o') => {
            if let Some(url) = app.selected_issue().map(|i| i.url.clone()) {
                open_url(app, be, &url);
            }
        }
        KeyCode::Char('e') => {
            if let Some(i) = app.selected_issue().cloned() {
                app.popup = Popup::Form(Form::new_edit(&i));
            }
        }
        _ => {}
    }
}

fn move_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    let Popup::Move(sel) = app.popup else { return };
    let pick = |app: &mut App, idx: usize| {
        app.popup = Popup::None;
        if let Some(n) = app.selected_issue().map(|i| i.number) {
            move_issue(app, be, n, Column::ALL[idx], false);
        }
    };
    if let Some(d) = vertical(key.code) {
        app.popup = Popup::Move((sel as isize + d).clamp(0, 4) as usize);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.popup = Popup::None,
        KeyCode::Enter => pick(app, sel),
        KeyCode::Char(c @ '1'..='5') => pick(app, c as usize - '1' as usize),
        _ => {}
    }
}

fn wip_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    let Popup::WipWarn { number, target } = app.popup else {
        return;
    };
    app.popup = Popup::None;
    if matches!(
        key.code,
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
    ) {
        move_issue(app, be, number, target, true);
    } else {
        app.message = None;
    }
}

fn text_popup_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.popup = Popup::None;
            return;
        }
        KeyCode::Enter => return submit_text_popup(app, be),
        _ => {}
    }
    let input = match &mut app.popup {
        Popup::Comment(t) | Popup::Close(t) | Popup::Search(t) => t,
        _ => return,
    };
    match key.code {
        KeyCode::Backspace => input.backspace(),
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => input.push(c),
        _ => {}
    }
}

fn submit_text_popup(app: &mut App, be: &dyn Backend) {
    let popup = std::mem::replace(&mut app.popup, Popup::None);
    if let Popup::Search(t) = popup {
        app.filter.text = t.trim().to_string();
        app.clamp();
        return;
    }
    let (Some(repo), Some(number)) = (app.repo.clone(), app.selected_issue().map(|i| i.number))
    else {
        return;
    };
    match popup {
        Popup::Comment(t) => {
            if t.trim().is_empty() {
                app.set_error("comment is empty");
                app.popup = Popup::Comment(t);
                return;
            }
            match be.comment(&repo, number, &t.trim()) {
                Ok(()) => app.set_status(format!("commented on #{number}")),
                Err(e) => {
                    app.set_error(format!("{e:#}"));
                    app.popup = Popup::Comment(t);
                }
            }
        }
        Popup::Close(t) => {
            let c = (!t.trim().is_empty()).then(|| t.trim().to_string());
            match be.close(&repo, number, c.as_deref()) {
                Ok(()) => {
                    app.set_status(format!("closed #{number}"));
                    reload_after_mutation(app, be);
                    app.focus_issue(number);
                }
                Err(e) => {
                    app.set_error(format!("{e:#}"));
                    app.popup = Popup::Close(t);
                }
            }
        }
        _ => {}
    }
}

fn form_key(app: &mut App, be: &dyn Backend, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Popup::Form(form) = &mut app.popup else {
        return;
    };
    match key.code {
        KeyCode::Esc => app.popup = Popup::None,
        KeyCode::Char('s') if ctrl => submit_form(app, be),
        KeyCode::Char('e') if ctrl => {
            if form.focus == Field::Body {
                app.editor_request = true;
            }
        }
        KeyCode::Tab => form.next_focus(),
        KeyCode::BackTab => form.prev_focus(),
        KeyCode::Enter => match form.focus {
            Field::Body => form.body.push('\n'),
            _ => form.next_focus(),
        },
        KeyCode::Left | KeyCode::Right => {
            let d = if key.code == KeyCode::Left { -1 } else { 1 };
            match form.focus {
                Field::Status => form.cycle_status(d),
                Field::Milestone => form.cycle_milestone(d),
                _ => {}
            }
        }
        _ => {
            if let Some(t) = form.focused_text_mut() {
                t.input(key);
            }
        }
    }
}

/// Issue built from the form, used when GitHub's list does not include a
/// just-created issue yet (the next real refetch replaces it).
fn issue_from_new(n: u64, new: &crate::backend::NewIssue, url: &str) -> domain::Issue {
    let mut labels = vec![domain::Label {
        name: domain::LABEL.into(),
    }];
    if let Some(st) = new.status.status_arg() {
        labels.push(domain::Label {
            name: format!("{}:{st}", domain::LABEL),
        });
    }
    domain::Issue {
        number: n,
        title: new.title.clone(),
        body: new.body.clone(),
        state: "OPEN".into(),
        labels,
        milestone: new
            .milestone
            .clone()
            .map(|title| domain::Milestone { title }),
        url: url.trim().to_string(),
        ..Default::default()
    }
}

/// Makes a just-created issue visible: merges it in if the reload missed it,
/// clears filters that would hide it, focuses it and flashes it.
fn show_created(app: &mut App, n: u64, made: domain::Issue) {
    if !app.issues.iter().any(|i| i.number == n) {
        let mut all = app.issues.clone();
        all.push(made);
        app.set_issues(all);
        if let Some(repo) = app.repo.clone() {
            app.sync_repo_cache(&repo);
        }
    }
    let shown = |app: &App| {
        Column::ALL
            .iter()
            .any(|c| app.visible(*c).iter().any(|i| i.number == n))
    };
    let mut msg = format!("created #{n}");
    if !shown(app) {
        app.filter = domain::Filter::default();
        app.clamp();
        msg.push_str(" (filter cleared)");
    }
    app.focus_issue(n);
    app.fx.flash(n, Instant::now());
    app.set_status(msg);
}

fn submit_form(app: &mut App, be: &dyn Backend) {
    let Popup::Form(form) = &app.popup else {
        return;
    };
    let Some(repo) = app.repo.clone() else { return };
    let form = form.clone();
    let before: Vec<u64> = app.issues.iter().map(|i| i.number).collect();
    let mut created: Option<(Option<u64>, String, crate::backend::NewIssue)> = None;
    let result = match form.kind {
        FormKind::Create => form.build_new().and_then(|n| {
            be.create(&repo, &n)
                .map(|out| {
                    created = Some((domain::issue_number_from_url(&out), out, n));
                    "created".to_string()
                })
                .map_err(|e| format!("{e:#}"))
        }),
        FormKind::Edit(n) => form.build_edit().and_then(|(t, b)| {
            be.edit(&repo, n, &t, &b)
                .map(|_| format!("edited #{n}"))
                .map_err(|e| format!("{e:#}"))
        }),
    };
    match result {
        Ok(msg) => {
            app.popup = Popup::None;
            app.set_status(msg);
            reload_after_mutation(app, be);
            if let Some((num, out, new)) = created {
                // No parsable URL: fall back to the newest issue we did not know.
                let num = num.or_else(|| {
                    app.issues
                        .iter()
                        .map(|i| i.number)
                        .filter(|n| !before.contains(n))
                        .max()
                });
                if let Some(n) = num {
                    show_created(app, n, issue_from_new(n, &new, &out));
                }
            }
        }
        // Keep the form open so typed text is never lost.
        Err(e) => app.set_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::domain::{Issue, Label};
    use crossterm::event::KeyEventKind;

    fn issue(n: u64, state: &str, labels: &[&str]) -> Issue {
        Issue {
            number: n,
            title: format!("[roadmap] T{n}"),
            body: "body".into(),
            state: state.into(),
            url: format!("https://x/{n}"),
            labels: labels.iter().map(|l| Label { name: (*l).into() }).collect(),
            ..Default::default()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }
    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn press(app: &mut App, be: &FakeBackend, keys: &[KeyEvent]) {
        for k in keys {
            handle_key(app, be, *k);
        }
    }
    fn type_str(app: &mut App, be: &FakeBackend, s: &str) {
        for c in s.chars() {
            handle_key(app, be, ch(c));
        }
    }

    fn setup(issues: Vec<Issue>) -> (App, FakeBackend) {
        let be = FakeBackend {
            repos: vec!["alpha".into(), "beta".into()],
            issues: issues.into(),
            milestones: vec!["M1".into()],
            ..Default::default()
        };
        let mut app = App::new();
        load_repos(&mut app, &be);
        press(&mut app, &be, &[ch('j'), key(KeyCode::Enter)]);
        (app, be)
    }

    fn standard() -> (App, FakeBackend) {
        setup(vec![
            issue(1, "OPEN", &["roadmap", "roadmap:planned"]),
            issue(2, "OPEN", &["roadmap", "roadmap:working"]),
            issue(3, "CLOSED", &["roadmap"]),
        ])
    }

    #[test]
    fn u_on_repo_card_refetches_only_that_repo() {
        let be = FakeBackend {
            repos: (0..3).map(|i| format!("r{i}")).collect(),
            ..Default::default()
        };
        let mut app = App::new();
        load_repos(&mut app, &be);
        app.fetch_queue.clear();
        app.activity = crate::tui::activity::Activity::Idle;
        for r in ["r0", "r1", "r2"] {
            app.summaries
                .insert(r.into(), SummaryState::Ready(Default::default()));
        }
        // On the overview entry `u` does nothing.
        press(&mut app, &be, &[ch('u')]);
        assert!(app.fetch_queue.is_empty());
        // On a card it queues just that repo; a second press does not duplicate.
        app.repo_sel = 2;
        press(&mut app, &be, &[ch('u'), ch('u')]);
        assert_eq!(app.fetch_queue, vec!["r1"]);
        assert!(matches!(app.summaries["r1"], SummaryState::Loading));
        assert!(matches!(app.summaries["r0"], SummaryState::Ready(_)));
        assert!(matches!(app.summaries["r2"], SummaryState::Ready(_)));
        assert_eq!(app.activity.line().unwrap().0, " fetching 0/1 repos...");
    }

    #[test]
    fn repo_grid_keys_and_refresh_requeue_summaries() {
        let be = FakeBackend {
            repos: (0..5).map(|i| format!("r{i}")).collect(),
            ..Default::default()
        };
        let mut app = App::new();
        load_repos(&mut app, &be);
        assert_eq!(app.fetch_queue.len(), 5);
        app.grid_cols.set(2); // [0] / [1 2] [3 4] [5]
        press(&mut app, &be, &[ch('l'), ch('j')]);
        assert_eq!(app.repo_sel, 1); // l on the wide card does nothing
        press(&mut app, &be, &[ch('l'), ch('j')]);
        assert_eq!(app.repo_sel, 4);
        press(&mut app, &be, &[key(KeyCode::Down), key(KeyCode::Left)]);
        assert_eq!(app.repo_sel, 4);
        press(&mut app, &be, &[ch('k'), key(KeyCode::Right), ch('h')]);
        assert_eq!(app.repo_sel, 2);
        press(&mut app, &be, &[ch('k'), ch('k')]);
        assert_eq!(app.repo_sel, 0);
        press(&mut app, &be, &[ch('G')]);
        assert_eq!(app.repo_sel, 5);
        press(&mut app, &be, &[ch('g')]);
        assert_eq!(app.repo_sel, 0);
        app.fetch_queue.clear();
        press(&mut app, &be, &[ch('r')]);
        assert_eq!(app.fetch_queue.len(), 5);
        press(&mut app, &be, &[key(KeyCode::Enter)]);
        assert_eq!(app.screen, Screen::Overview);
    }

    #[test]
    fn esc_and_b_go_back_one_level_without_stealing_other_escs() {
        let (mut app, be) = standard();
        assert_eq!(app.screen, Screen::Board);
        // Active filter: Esc clears it first and stays on the board.
        type_str(&mut app, &be, "/T1");
        press(&mut app, &be, &[key(KeyCode::Enter)]);
        assert!(app.filter.is_active());
        press(&mut app, &be, &[key(KeyCode::Esc)]);
        assert!(!app.filter.is_active());
        assert_eq!(app.screen, Screen::Board);
        // Popup: Esc closes it first.
        press(&mut app, &be, &[ch('?'), key(KeyCode::Esc)]);
        assert_eq!(app.screen, Screen::Board);
        assert!(matches!(app.popup, Popup::None));
        // Nothing else applies: Esc goes back, selection is kept.
        let sel = app.repo_sel;
        press(&mut app, &be, &[key(KeyCode::Esc)]);
        assert_eq!(app.screen, Screen::Repos);
        assert_eq!(app.repo_sel, sel);
        // Esc on the grid does nothing (q quits).
        press(&mut app, &be, &[key(KeyCode::Esc)]);
        assert!(!app.should_quit);
        // `b` still works from the board and Esc/b from the overview.
        press(&mut app, &be, &[key(KeyCode::Enter), ch('b')]);
        assert_eq!(app.screen, Screen::Repos);
        press(&mut app, &be, &[ch('g'), key(KeyCode::Enter)]);
        assert_eq!(app.screen, Screen::Overview);
        press(&mut app, &be, &[key(KeyCode::Esc)]);
        assert_eq!(app.screen, Screen::Repos);
        press(&mut app, &be, &[ch('g'), key(KeyCode::Enter), ch('b')]);
        assert_eq!(app.screen, Screen::Repos);
        press(&mut app, &be, &[ch('q')]);
        assert!(app.should_quit);
    }

    #[test]
    fn reentering_a_repo_uses_the_cache_and_r_refetches() {
        let (mut app, be) = standard();
        assert_eq!(be.reads(), vec!["issues alpha"]);
        assert!(app.cache.board("alpha").is_some());
        press(&mut app, &be, &[key(KeyCode::Esc), key(KeyCode::Enter)]);
        assert_eq!(app.screen, Screen::Board);
        assert_eq!(be.reads(), vec!["issues alpha"]); // no new gh read
        assert!(app.message.as_ref().unwrap().text.contains("cached"));
        assert_eq!(app.visible(Column::Planned).len(), 1);
        // r forces a real refetch and refreshes the cache.
        be.issues
            .lock()
            .unwrap()
            .push(issue(9, "OPEN", &["roadmap:idea"]));
        press(&mut app, &be, &[ch('r')]);
        assert_eq!(be.reads().len(), 2);
        assert_eq!(app.cache.board("alpha").unwrap().value.issues.len(), 4);
    }

    #[test]
    fn failed_refetch_keeps_cached_board_and_shows_error() {
        let (mut app, be) = standard();
        *be.fail_with.lock().unwrap() = Some("gh down".into());
        press(&mut app, &be, &[ch('r')]);
        assert!(app.message.as_ref().unwrap().error);
        assert_eq!(app.issues.len(), 3);
        assert_eq!(app.cache.board("alpha").unwrap().value.issues.len(), 3);
    }

    #[test]
    fn mutations_update_cache_and_failed_reload_invalidates_it() {
        let (mut app, be) = standard();
        app.cache.put_summary(
            "alpha",
            crate::domain::RepoSummary {
                description: "keep me".into(),
                ..Default::default()
            },
            1,
        );
        // Selected card is #1 (planned); L moves it to now on the fake list.
        be.issues.lock().unwrap()[0] = issue(1, "OPEN", &["roadmap", "roadmap:now"]);
        press(&mut app, &be, &[ch('L')]);
        let board = &app.cache.board("alpha").unwrap().value;
        assert_eq!(board.issues[0].column(), Column::Now);
        let s = &app.cache.summary("alpha").unwrap().value;
        assert_eq!(s.description, "keep me");
        assert_eq!(s.counts, [0, 0, 1, 1]);
        match &app.summaries["alpha"] {
            SummaryState::Ready(s) => assert_eq!(s.counts, [0, 0, 1, 1]),
            other => panic!("{other:?}"),
        }
        // Mutation succeeds but the follow-up reload fails: cache is dropped.
        be.issues.lock().unwrap()[0] = issue(1, "OPEN", &["roadmap", "roadmap:planned"]);
        reload_after_mutation_for_test(&mut app, &be);
        assert!(app.cache.board("alpha").is_none());
        assert!(app.cache.summary("alpha").is_none());
    }

    fn reload_after_mutation_for_test(app: &mut App, be: &FakeBackend) {
        *be.fail_with.lock().unwrap() = Some("gh down".into());
        super::reload_after_mutation(app, be);
    }

    #[test]
    fn startup_shows_cached_cards_and_fetches_only_uncached() {
        let be = FakeBackend {
            repos: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        let mut app = App::new();
        app.clock = || 100_000;
        app.cache.put_summary(
            "a",
            crate::domain::RepoSummary {
                counts: [1, 0, 0, 0],
                ..Default::default()
            },
            100_000 - 3 * 3600,
        );
        load_repos(&mut app, &be);
        assert!(matches!(app.summaries["a"], SummaryState::Ready(_)));
        assert_eq!(app.summaries["b"], SummaryState::Loading);
        assert_eq!(app.fetch_queue, vec!["b"]); // only the uncached repo
        let m = app.message.as_ref().unwrap();
        assert!(m.text.contains("loaded from cache (3h ago), r to refresh"));
        assert_eq!(app.activity.line().unwrap().0, " fetching 0/1 repos...");
        // `r` on the grid refetches everything (cards hide again).
        press(&mut app, &be, &[ch('r')]);
        assert_eq!(app.fetch_queue, vec!["a", "b"]);
        assert_eq!(app.summaries["a"], SummaryState::Loading);
    }

    #[test]
    fn failed_summary_refetch_keeps_old_card_and_reports() {
        let mut app = App::new();
        app.repos = vec!["a".into()];
        app.cache.put_summary(
            "a",
            crate::domain::RepoSummary {
                counts: [2, 0, 0, 0],
                ..Default::default()
            },
            1,
        );
        app.request_summaries(true, Instant::now());
        app.apply_summary("a".into(), Err("boom".into()), Instant::now());
        match &app.summaries["a"] {
            SummaryState::Ready(s) => assert_eq!(s.counts, [2, 0, 0, 0]),
            other => panic!("{other:?}"),
        }
        assert!(app.message.as_ref().unwrap().error);
    }

    fn create_via_form(app: &mut App, be: &FakeBackend, title: &str) {
        press(app, be, &[ch('n')]);
        type_str(app, be, title);
        press(app, be, &[ctrl('s')]);
    }

    #[test]
    fn created_issue_appears_focused_and_flashes() {
        let (mut app, be) = standard();
        app.fx.enabled = true;
        create_via_form(&mut app, &be, "Fresh");
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(app.selected_issue().map(|i| i.number), Some(4));
        assert_eq!(app.column(), Column::Idea);
        assert!(app.fx.flash_level(4, Instant::now()) < 1.0);
        assert!(app
            .cache
            .board("alpha")
            .unwrap()
            .value
            .issues
            .iter()
            .any(|i| i.number == 4));
    }

    #[test]
    fn created_issue_shows_even_when_github_list_lags() {
        let (mut app, be) = standard();
        *be.list_lag.lock().unwrap() = 1; // reload right after create misses it
        create_via_form(&mut app, &be, "Lagging");
        assert_eq!(app.selected_issue().map(|i| i.number), Some(4));
        assert!(app.visible(Column::Idea).iter().any(|i| i.number == 4));
        // The next real refetch confirms it.
        press(&mut app, &be, &[ch('r')]);
        assert!(app.visible(Column::Idea).iter().any(|i| i.number == 4));
    }

    #[test]
    fn active_filters_never_hide_a_created_issue() {
        let (mut app, be) = standard();
        be.issues.lock().unwrap()[0].milestone =
            Some(crate::domain::Milestone { title: "M1".into() });
        press(&mut app, &be, &[ch('r')]);
        app.filter.milestone = Some("M1".into()); // form default milestone: none
        create_via_form(&mut app, &be, "Hidden?");
        assert!(!app.filter.is_active());
        assert_eq!(app.selected_issue().map(|i| i.number), Some(4));
        assert!(app.message.as_ref().unwrap().text.contains("filter"));
    }

    #[test]
    fn picking_repo_opens_board_on_first_nonempty_column() {
        let (app, _) = standard();
        assert_eq!(app.screen, Screen::Board);
        assert_eq!(app.repo.as_deref(), Some("alpha"));
        assert_eq!(app.slug.as_deref(), Some("me/alpha"));
        assert_eq!(app.column(), Column::Planned);
        assert!(app.milestones.contains(&"M1".to_string()));
    }

    #[test]
    fn all_repos_overview_lists_open_issues() {
        let be = FakeBackend {
            repos: vec!["a".into()],
            issues: vec![issue(1, "OPEN", &["roadmap:now"]), issue(2, "CLOSED", &[])].into(),
            ..Default::default()
        };
        let mut app = App::new();
        load_repos(&mut app, &be);
        handle_key(&mut app, &be, key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Overview);
        assert_eq!(app.overview.len(), 1);
        handle_key(&mut app, &be, key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Board);
        assert_eq!(app.selected_issue().unwrap().number, 1);
    }

    #[test]
    fn shift_l_rotates_status_label() {
        let (mut app, be) = standard();
        handle_key(&mut app, &be, ch('L'));
        assert_eq!(be.calls(), vec!["status alpha 1 now"]);
    }

    #[test]
    fn move_to_done_closes_and_from_done_reopens() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('m'), ch('5')]);
        assert_eq!(be.calls(), vec!["close alpha 1 None"]);
        // select the closed card (done column) and move it back to idea
        app.col = Column::Done.index();
        app.rows[Column::Done.index()] = 1; // #1 (just closed) is row 0
        press(&mut app, &be, &[ch('m'), ch('1')]);
        assert_eq!(be.calls()[1..], ["reopen alpha 3", "status alpha 3 idea"]);
    }

    #[test]
    fn wip_limit_warns_and_requires_confirmation() {
        let (mut app, be) = standard();
        // card 1 is planned; moving to working while #2 is working
        press(&mut app, &be, &[ch('m'), ch('4')]);
        assert!(be.calls().is_empty());
        assert!(matches!(app.popup, Popup::WipWarn { number: 1, .. }));
        assert!(app.message.as_ref().unwrap().error);
        handle_key(&mut app, &be, ch('n'));
        assert!(be.calls().is_empty());
        press(&mut app, &be, &[ch('m'), ch('4'), ch('y')]);
        assert_eq!(be.calls(), vec!["status alpha 1 working"]);
    }

    #[test]
    fn create_issue_prefixes_title_and_uses_status_and_milestone() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('n')]);
        type_str(&mut app, &be, "New thing");
        press(&mut app, &be, &[key(KeyCode::Tab)]);
        type_str(&mut app, &be, "line1");
        press(&mut app, &be, &[key(KeyCode::Enter)]);
        type_str(&mut app, &be, "line2");
        press(&mut app, &be, &[key(KeyCode::Tab), key(KeyCode::Right)]); // status -> planned
        press(&mut app, &be, &[key(KeyCode::Tab), key(KeyCode::Right)]); // milestone -> M1
        press(&mut app, &be, &[ctrl('s')]);
        assert_eq!(
            be.calls(),
            vec![
                r###"create alpha "[roadmap] New thing" "## Resumen\n\n## Tareas\n\n===\n## Detalle interno\nline1\nline2" Planned Some("M1")"###
            ]
        );
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn create_without_title_keeps_form_open() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('n'), ctrl('s')]);
        assert!(matches!(app.popup, Popup::Form(_)));
        assert!(app.message.as_ref().unwrap().error);
        assert!(be.calls().is_empty());
    }

    #[test]
    fn backend_error_is_reported_and_form_kept() {
        let (mut app, be) = standard();
        *be.fail_with.lock().unwrap() = Some("gh exploded".into());
        press(&mut app, &be, &[ch('n')]);
        type_str(&mut app, &be, "x");
        handle_key(&mut app, &be, ctrl('s'));
        assert!(matches!(app.popup, Popup::Form(_)));
        assert!(app.message.as_ref().unwrap().text.contains("gh exploded"));
        handle_key(&mut app, &be, key(KeyCode::Esc));
        handle_key(&mut app, &be, ch('r')); // list also fails: no panic
        assert!(app.message.as_ref().unwrap().error);
    }

    #[test]
    fn edit_sends_title_and_body() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('e')]);
        type_str(&mut app, &be, "!");
        press(
            &mut app,
            &be,
            &[key(KeyCode::Tab), key(KeyCode::Backspace), ctrl('s')],
        );
        assert_eq!(be.calls(), vec![r#"edit alpha 1 "[roadmap] T1!" "bod""#]);
    }

    #[test]
    fn ctrl_e_on_body_requests_editor() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('e'), ctrl('e')]);
        assert!(!app.editor_request);
        press(&mut app, &be, &[key(KeyCode::Tab), ctrl('e')]);
        assert!(app.editor_request);
    }

    #[test]
    fn comment_and_close_with_comment() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('c')]);
        type_str(&mut app, &be, "hi");
        handle_key(&mut app, &be, key(KeyCode::Enter));
        press(&mut app, &be, &[ch('x')]);
        type_str(&mut app, &be, "done");
        handle_key(&mut app, &be, key(KeyCode::Enter));
        app.col = Column::Working.index(); // #1 is closed now; pick #2
        press(&mut app, &be, &[ch('x'), key(KeyCode::Enter)]);
        assert_eq!(
            be.calls(),
            vec![
                r#"comment alpha 1 "hi""#,
                r#"close alpha 1 Some("done")"#,
                "close alpha 2 None"
            ]
        );
    }

    #[test]
    fn empty_comment_rejected() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('c'), key(KeyCode::Enter)]);
        assert!(be.calls().is_empty());
        assert!(matches!(app.popup, Popup::Comment(_)));
    }

    #[test]
    fn reopen_only_on_done_cards() {
        let (mut app, be) = standard();
        handle_key(&mut app, &be, ch('u'));
        assert!(be.calls().is_empty());
        app.col = Column::Done.index();
        handle_key(&mut app, &be, ch('u'));
        assert_eq!(be.calls(), vec!["reopen alpha 3"]);
    }

    #[test]
    fn search_filters_and_esc_clears() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('/')]);
        type_str(&mut app, &be, "zzz");
        handle_key(&mut app, &be, key(KeyCode::Enter));
        assert!(app.selected_issue().is_none());
        handle_key(&mut app, &be, key(KeyCode::Esc));
        assert!(app.selected_issue().is_some());
    }

    #[test]
    fn open_url_and_detail_popup() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('o'), key(KeyCode::Enter)]);
        assert_eq!(be.calls(), vec!["open https://x/1"]);
        assert!(matches!(app.popup, Popup::Detail));
        press(&mut app, &be, &[ch('j'), ch('j'), ch('k')]);
        assert_eq!(app.detail_scroll.offset(), 1);
        handle_key(&mut app, &be, key(KeyCode::Esc));
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn help_quit_and_ctrl_c() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('?')]);
        assert!(matches!(app.popup, Popup::Help));
        handle_key(&mut app, &be, ch('q'));
        assert!(!app.should_quit);
        handle_key(&mut app, &be, ch('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn paste_goes_to_focused_field() {
        let (mut app, be) = standard();
        press(&mut app, &be, &[ch('n')]);
        handle_paste(&mut app, "pasted");
        let Popup::Form(f) = &app.popup else { panic!() };
        assert_eq!(f.title.as_str(), "pasted");
    }
}
