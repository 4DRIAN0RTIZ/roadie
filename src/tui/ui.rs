//! Rendering. All colors come from `App::theme`.
use std::time::Instant;

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::domain::{self, Column, Issue, RepoSummary, BADGE_COLUMNS};

use super::activity::Tone;
use super::anim;
use super::app::{App, Field, Form, FormKind, Popup, Screen, SummaryState};
use super::grid;
use super::widgets::{centered_rect, draw_frame};

pub const HELP: &str = "\
Repo grid
  h/l, arrows   prev / next card     j/k   row down / up
  g / G         first / last card    Enter open board (top full-width card: all repos)
  All-repos card: h/l no-op, j enters repos,
  k from the first repo row returns to it
  r             refetch all cards (otherwise cached)
  u             refetch only the selected card
                footer: fetching N/M repos... -> done
  Esc           does nothing here (q quits)
  ?             this help            q     quit

Board
  h/l, arrows   change column        j/k   move in column
  g / G         top / bottom         Enter issue detail
  H / L         move card left / right (rotates label)
  m             move menu (1-5 or j/k + Enter)
  n             new issue            e     edit title/body
  c             comment              x     close (optional comment)
  u             reopen (done column) o     open in browser
  f             cycle milestone filter
  /             text search
  Esc           close popup, else clear filters, else back
  b             back to repos (Esc alias)
  r             refetch this board (boards/cards are cached)
  ?             this help            q     quit

Forms
  Tab / Shift+Tab  next / prev field  Left/Right  cycle status/milestone
  Enter            newline in body    Ctrl+S      submit
  Ctrl+E           edit body in $EDITOR   Esc      cancel

WIP limit: only one issue in `working` per repo. Moving a second
one asks for confirmation (y).

Body convention: short prose, 2-4 lines, no bold+colon bullet lists.";

const STATUS_GLYPHS: [&str; 4] = ["\u{f0eb}", "\u{f073}", "\u{f144}", "\u{f085}"];

pub fn draw(frame: &mut Frame, app: &App) {
    draw_at(frame, app, Instant::now());
}

pub(super) fn draw_at(frame: &mut Frame, app: &App, now: Instant) {
    let t = &app.theme;
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(t.background).fg(t.foreground)),
        area,
    );
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, app, header);
    match app.screen {
        Screen::Repos => draw_repos(frame, app, body, now),
        Screen::Overview => draw_overview(frame, app, body),
        Screen::Board => draw_board(frame, app, body, now),
    }
    draw_footer(frame, app, footer);
    draw_popup(frame, app, area);
    fade_popup(frame, app, area, now);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let mut spans = vec![Span::styled(
        " roadmap ",
        Style::default()
            .fg(t.selection_fg)
            .bg(t.highlight)
            .add_modifier(Modifier::BOLD),
    )];
    if app.screen == Screen::Board {
        let name = app
            .slug
            .clone()
            .or_else(|| app.repo.clone())
            .unwrap_or_default();
        spans.push(Span::raw(format!(" {name} ")));
        let wip = app.wip_count();
        let color = if wip > 1 { t.danger } else { t.muted };
        spans.push(Span::styled(
            format!(" WIP {wip}/1 "),
            Style::default().fg(color),
        ));
        if app.filter.is_active() {
            spans.push(Span::styled(" filtered ", Style::default().fg(t.warning)));
        }
        if let Some(m) = &app.filter.milestone {
            spans.push(Span::styled(
                format!(" milestone:{m} "),
                Style::default().fg(t.warning),
            ));
        }
        if !app.filter.text.is_empty() {
            spans.push(Span::styled(
                format!(" /{} ", app.filter.text),
                Style::default().fg(t.warning),
            ));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(footer_line(app)), area);
}

/// Footer priority (same idiom as hteam-cli's status bar): an action
/// result/error, else the live loading activity, else the key hints.
fn footer_line(app: &App) -> Line<'static> {
    let t = &app.theme;
    if let Some(m) = &app.message {
        return Line::styled(
            format!(" {}", m.text.lines().next().unwrap_or("")),
            Style::default().fg(if m.error { t.danger } else { t.success }),
        );
    }
    if let Some((text, tone)) = app.activity.line() {
        let color = match tone {
            Tone::Busy => t.warning,
            Tone::Ok => t.success,
            Tone::Error => t.danger,
        };
        return Line::styled(text, Style::default().fg(color));
    }
    let hint = match app.screen {
        Screen::Repos => " r refresh all  u refresh repo  ? help  q quit",
        Screen::Overview | Screen::Board => " esc back  ? help  q quit",
    };
    Line::styled(hint, Style::default().fg(t.muted))
}

fn selected_style(app: &App) -> Style {
    Style::default()
        .bg(app.theme.selection_bg)
        .fg(app.theme.selection_fg)
}

fn draw_repos(frame: &mut Frame, app: &App, area: Rect, now: Instant) {
    let n = app.repo_entries();
    let cols = grid::columns(area.width);
    app.grid_cols.set(cols);
    let visible = ((area.height / grid::CARD_HEIGHT) as usize).max(1);
    let offset = grid::scroll_offset(
        app.grid_offset.get(),
        grid::row_of(app.repo_sel, cols),
        visible,
    )
    .min(grid::total_rows(n, cols).saturating_sub(visible));
    app.grid_offset.set(offset);
    let card_w = area.width / cols as u16;
    for idx in 0..n {
        let row = grid::row_of(idx, cols);
        if row < offset || row >= offset + visible {
            continue;
        }
        // Entry 0 spans every grid column; repo cards fill the rows below it.
        let (col, width) = if idx == 0 {
            (0, card_w * cols as u16)
        } else {
            (((idx - 1) % cols) as u16, card_w)
        };
        let rect = Rect::new(
            area.x + col * card_w,
            area.y + (row - offset) as u16 * grid::CARD_HEIGHT,
            width,
            grid::CARD_HEIGHT,
        )
        .intersection(area);
        if rect.width > 0 && rect.height > 0 {
            draw_card(frame, app, idx, rect, now);
        }
    }
}

fn badge_line(app: &App, counts: [usize; 4], fade: f32) -> Line<'static> {
    let t = &app.theme;
    let mut spans = vec![];
    for ((c, n), glyph) in BADGE_COLUMNS.iter().zip(counts).zip(STATUS_GLYPHS) {
        let style = if n == 0 {
            Style::default().fg(faded(app, t.muted, fade))
        } else {
            Style::default()
                .fg(faded(app, t.column_color(*c), fade))
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(format!("{glyph}{n}"), style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

/// `color` faded in from the theme background (`fade` is already eased).
fn faded(app: &App, color: Color, fade: f32) -> Color {
    anim::fade_toward(app.theme.background, color, fade)
}

fn all_repos_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let dim = Style::default().fg(app.theme.muted);
    let (ready, total) = (loaded_repos(app), app.repos.len());
    let counts = domain::aggregate(ready.iter().copied());
    let open_issues = counts.iter().sum::<usize>();
    let active_repos = ready
        .iter()
        .filter(|summary| summary.active.is_some())
        .count();
    let failed_repos = app
        .repos
        .iter()
        .filter(|repo| matches!(app.summaries.get(*repo), Some(SummaryState::Failed(_))))
        .count();
    let metrics = if width < 40 {
        format!(
            "{open_issues} open·{active_repos} active·{}/{} loaded",
            ready.len(),
            total
        )
    } else {
        format!(
            "{open_issues} open issues · {active_repos} active repos · {}/{} loaded",
            ready.len(),
            total
        )
    };
    let active: Vec<String> = app
        .repos
        .iter()
        .filter_map(|repo| {
            app.summaries.get(repo).and_then(|state| match state {
                SummaryState::Ready(summary) => summary
                    .active
                    .as_ref()
                    .map(|title| format!("{repo} — {title}")),
                _ => None,
            })
        })
        .take(3)
        .collect();
    let mut lines = vec![badge_line(app, counts, 1.0)];
    let mut metric_spans = vec![Span::styled(
        grid::truncate(&metrics, usize::from(width)),
        dim,
    )];
    let remaining = usize::from(width).saturating_sub(metrics.chars().count());
    if failed_repos > 0 && remaining > 0 {
        metric_spans.push(Span::styled(
            grid::truncate(&format!(" · ! {failed_repos} failed"), remaining),
            Style::default().fg(app.theme.danger),
        ));
    }
    lines.push(Line::from(metric_spans));
    if active.is_empty() {
        lines.push(Line::styled("No active work", dim));
    } else {
        lines.extend(
            active
                .iter()
                .map(|work| Line::styled(grid::truncate(work, usize::from(width)), dim)),
        );
    }
    lines
}

fn draw_all_repos_card(frame: &mut Frame, app: &App, inner: Rect) {
    frame.render_widget(
        Paragraph::new(all_repos_lines(app, inner.width)).alignment(Alignment::Center),
        inner,
    );
}

fn loaded_repos(app: &App) -> Vec<&RepoSummary> {
    app.repos
        .iter()
        .filter_map(|r| match app.summaries.get(r) {
            Some(SummaryState::Ready(s)) => Some(s),
            _ => None,
        })
        .collect()
}

fn draw_card(frame: &mut Frame, app: &App, idx: usize, rect: Rect, now: Instant) {
    let t = &app.theme;
    // Repo cards are not drawn at all until their load settled; the slot stays
    // reserved so the layout never reflows.
    let repo = match idx.checked_sub(1).and_then(|i| app.repos.get(i)) {
        Some(r) if !matches!(app.summaries.get(r), None | Some(SummaryState::Loading)) => Some(r),
        Some(_) => return,
        None => None,
    };
    let fade = repo.map_or(1.0, |r| app.fade_of(r, now));
    let f = |c: Color| faded(app, c, fade);
    let dim = Style::default().fg(f(t.muted));
    let selected = idx == app.repo_sel;
    let border = if selected {
        Style::default()
            .fg(f(t.border_active))
            .add_modifier(Modifier::BOLD)
    } else {
        dim
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if selected {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(border);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let w = inner.width as usize;
    let bold = Style::default().add_modifier(Modifier::BOLD);

    if idx == 0 && app.grid_cols.get() > 1 {
        draw_all_repos_card(frame, app, inner);
        return;
    }
    let mut lines: Vec<Line> = vec![];
    if idx == 0 {
        lines = all_repos_lines(app, inner.width as u16);
    } else if let Some(repo) = repo {
        let state = app.summaries.get(repo);
        let empty = matches!(state, Some(SummaryState::Ready(s)) if s.total() == 0);
        let text = if empty { t.muted } else { t.foreground };
        let release = match state {
            Some(SummaryState::Ready(s)) => s.latest_release.as_deref(),
            _ => None,
        };
        // Drop the release when the card is too narrow to keep a readable name.
        let release = release.filter(|r| r.chars().count() + 6 <= w);
        let name_w = match release {
            Some(r) => w.saturating_sub(r.chars().count() + 1),
            None => w.saturating_sub(4),
        };
        let name = grid::truncate(repo, name_w);
        let mut title = vec![Span::styled(name.clone(), bold.fg(f(text)))];
        if let Some(r) = release {
            let pad = w.saturating_sub(name.chars().count() + r.chars().count());
            title.push(Span::raw(" ".repeat(pad)));
            title.push(Span::styled(r.to_string(), dim));
        }
        if matches!(state, Some(SummaryState::Failed(_))) {
            title.push(Span::styled(" err", Style::default().fg(f(t.danger))));
        }
        lines.push(Line::from(title));
        match state {
            Some(SummaryState::Failed(e)) => {
                lines.extend(
                    grid::wrap_lines(e, w, 2)
                        .into_iter()
                        .map(|l| Line::styled(l, Style::default().fg(f(t.danger)))),
                );
            }
            Some(SummaryState::Ready(s)) if s.description.is_empty() => {
                lines.push(Line::styled("no description", dim));
            }
            Some(SummaryState::Ready(s)) => {
                lines.extend(
                    grid::wrap_lines(&s.description, w, 2)
                        .into_iter()
                        .map(|l| Line::styled(l, Style::default().fg(f(text)))),
                );
            }
            _ => {}
        }
        lines.truncate(3);
        while lines.len() < 3 {
            lines.push(Line::raw(""));
        }
        if let Some(SummaryState::Ready(s)) = state {
            lines.push(badge_line(app, s.counts, fade));
            lines.push(match &s.active {
                Some(a) => Line::styled(
                    grid::truncate(&format!("> {a}"), w),
                    Style::default().fg(f(text)),
                ),
                None => Line::styled("no active issue", dim),
            });
        }
    }
    let paragraph = Paragraph::new(lines);
    if idx == 0 {
        frame.render_widget(paragraph.alignment(Alignment::Center), inner);
    } else {
        frame.render_widget(paragraph, inner);
    }
}

fn draw_overview(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let items: Vec<ListItem> = app
        .overview
        .iter()
        .map(|(repo, i)| {
            let col = i.column();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{repo:<18}"), Style::default().fg(t.highlight)),
                Span::styled(
                    format!("{:<8}", col.title()),
                    Style::default().fg(t.column_color(col)),
                ),
                Span::raw(format!("#{} {}", i.number, i.display_title())),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_active))
                .title(" All repos: open roadmap issues (Enter opens board) "),
        )
        .highlight_style(selected_style(app));
    let mut st =
        ListState::default().with_selected((!app.overview.is_empty()).then_some(app.overview_sel));
    frame.render_stateful_widget(list, area, &mut st);
}

fn draw_board(frame: &mut Frame, app: &App, area: Rect, now: Instant) {
    let t = &app.theme;
    let fx = &app.fx;
    let fade = |c: Color, level: f32| anim::fade_toward(t.background, c, level);
    let cols = Layout::horizontal(vec![Constraint::Ratio(1, 5); 5]).split(area);
    for c in Column::ALL {
        let items = app.visible(c);
        let active = app.col == c.index();
        let head = fx.board_level(c.index(), 0, now);
        let border = fade(if active { t.border_active } else { t.muted }, head);
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(row, i)| {
                let ms = i
                    .milestone_title()
                    .map(|m| format!(" [{m}]"))
                    .unwrap_or_default();
                let enter = fx.board_level(c.index(), row, now);
                let flash = fx.flash_level(i.number, now);
                let selected = active && row == app.rows[c.index()];
                let mut style = Style::default();
                if selected {
                    let blend = fx.select_level(i.number, now).min(enter);
                    style = style.fg(fade(t.selection_fg, enter));
                    style = style.bg(fade(t.selection_bg, blend));
                } else {
                    style = style.fg(fade(t.foreground, enter));
                }
                if flash < 1.0 {
                    // Accent easing back to normal, on top of the entrance fade.
                    let base = if selected {
                        t.selection_fg
                    } else {
                        t.foreground
                    };
                    let accent = anim::fade_toward(t.highlight, base, flash);
                    style = style.fg(fade(accent, enter)).add_modifier(Modifier::BOLD);
                }
                ListItem::new(Line::styled(
                    format!("#{} {}{}", i.number, i.display_title(), ms),
                    style,
                ))
                .style(Style::default().bg(style.bg.unwrap_or(Color::Reset)))
            })
            .collect();
        let name_color = fade(t.column_color(c), head);
        let count_color =
            anim::fade_toward(Color::White, name_color, fx.count_level(c.index(), now));
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let title = Line::from(vec![
            Span::styled(format!(" {} (", c.title()), bold.fg(name_color)),
            Span::styled(items.len().to_string(), bold.fg(count_color)),
            Span::styled(") ", bold.fg(name_color)),
        ]);
        let list = List::new(list_items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border))
                .title(title),
        );
        let sel = (active && !items.is_empty()).then(|| app.rows[c.index()]);
        let mut st = ListState::default().with_selected(sel);
        frame.render_stateful_widget(list, cols[c.index()], &mut st);
    }
}

/// Rectangle a popup occupies (mirrors the sizes used by `draw_popup`).
fn popup_rect(popup: &Popup, area: Rect) -> Option<Rect> {
    Some(match popup {
        Popup::None => return None,
        Popup::Help | Popup::Form(_) => centered_rect(70, 80, area),
        Popup::Detail => centered_rect(80, 85, area),
        Popup::Comment(_) | Popup::Close(_) | Popup::Search(_) => centered_rect(60, 30, area),
        Popup::Move(_) => centered_rect(30, 40, area),
        Popup::WipWarn { .. } => centered_rect(50, 25, area),
    })
}

/// Short fade-in of a freshly opened popup: blends its text and border
/// colors from the background (visual only, applied after drawing).
fn fade_popup(frame: &mut Frame, app: &App, area: Rect, now: Instant) {
    let level = app.fx.popup_level(now);
    let Some(rect) = popup_rect(&app.popup, area) else {
        return;
    };
    if level >= 1.0 {
        return;
    }
    let bg = app.theme.background;
    let buf = frame.buffer_mut();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.symbol() != " " {
                    cell.fg = anim::fade_toward(bg, cell.fg, level);
                }
            }
        }
    }
}

fn draw_popup(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    match &app.popup {
        Popup::None => {}
        Popup::Help => {
            let r = centered_rect(70, 80, area);
            let inner = draw_frame(
                frame,
                r,
                " Help (Esc closes) ",
                t.border_popup,
                t.background,
                t.foreground,
            );
            frame.render_widget(
                Paragraph::new(HELP).scroll((app.help_scroll.offset(), 0)),
                inner,
            );
        }
        Popup::Detail => {
            if let Some(i) = app.selected_issue() {
                draw_detail(frame, app, i, centered_rect(80, 85, area));
            }
        }
        Popup::Form(f) => draw_form(frame, app, f, centered_rect(70, 80, area)),
        Popup::Comment(inp) => draw_input(
            frame,
            app,
            " Comment (Enter sends, Esc cancels) ",
            inp.as_str(),
            area,
        ),
        Popup::Close(inp) => draw_input(
            frame,
            app,
            " Close: optional comment (Enter, Esc cancels) ",
            inp.as_str(),
            area,
        ),
        Popup::Search(inp) => draw_input(
            frame,
            app,
            " Search (Enter applies, Esc cancels) ",
            inp.as_str(),
            area,
        ),
        Popup::Move(sel) => {
            let r = centered_rect(30, 40, area);
            let inner = draw_frame(
                frame,
                r,
                " Move to ",
                t.border_popup,
                t.background,
                t.foreground,
            );
            let items: Vec<ListItem> = Column::ALL
                .iter()
                .enumerate()
                .map(|(n, c)| {
                    ListItem::new(Span::styled(
                        format!("{} {}", n + 1, c.title()),
                        Style::default().fg(t.column_color(*c)),
                    ))
                })
                .collect();
            let mut st = ListState::default().with_selected(Some(*sel));
            frame.render_stateful_widget(
                List::new(items).highlight_style(selected_style(app)),
                inner,
                &mut st,
            );
        }
        Popup::WipWarn { number, .. } => {
            let r = centered_rect(50, 25, area);
            let inner = draw_frame(
                frame,
                r,
                " WIP limit ",
                t.danger,
                t.background,
                t.foreground,
            );
            let text = format!(
                "Another issue is already `working`.\nMove #{number} to working anyway?\n\ny confirm    any other key cancel"
            );
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        }
    }
}

fn draw_input(frame: &mut Frame, app: &App, title: &str, text: &str, area: Rect) {
    let t = &app.theme;
    let r = centered_rect(60, 30, area);
    let inner = draw_frame(frame, r, title, t.border_popup, t.background, t.foreground);
    frame.render_widget(
        Paragraph::new(format!("{text}_")).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_detail(frame: &mut Frame, app: &App, i: &Issue, r: Rect) {
    let t = &app.theme;
    let inner = draw_frame(
        frame,
        r,
        format!(" #{} (j/k scroll, o browser, e edit, Esc) ", i.number),
        t.border_popup,
        t.background,
        t.foreground,
    );
    let labels: Vec<&str> = i.labels.iter().map(|l| l.name.as_str()).collect();
    let assignees: Vec<&str> = i.assignees.iter().map(|a| a.login.as_str()).collect();
    let mut lines = vec![
        Line::styled(
            i.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(vec![
            Span::styled(
                format!("[{}] ", i.column().title()),
                Style::default().fg(t.column_color(i.column())),
            ),
            Span::raw(labels.join(", ")),
        ]),
        Line::raw(format!("milestone: {}", i.milestone_title().unwrap_or("-"))),
        Line::raw(format!(
            "assignees: {}",
            if assignees.is_empty() {
                "-".into()
            } else {
                assignees.join(", ")
            }
        )),
        Line::styled(i.url.clone(), Style::default().fg(t.muted)),
        Line::raw(""),
    ];
    lines.extend(i.body.lines().map(|l| Line::raw(l.to_string())));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll.offset(), 0)),
        inner,
    );
}

fn draw_form(frame: &mut Frame, app: &App, f: &Form, r: Rect) {
    let t = &app.theme;
    let title = match f.kind {
        FormKind::Create => " New issue (Tab next, Ctrl+S save, Esc cancel) ".to_string(),
        FormKind::Edit(n) => format!(" Edit #{n} (Tab next, Ctrl+S save, Esc cancel) "),
    };
    let inner = draw_frame(frame, r, title, t.border_popup, t.background, t.foreground);
    let mut constraints = vec![Constraint::Length(3), Constraint::Min(4)];
    if f.kind == FormKind::Create {
        constraints.extend([Constraint::Length(3), Constraint::Length(3)]);
    }
    constraints.push(Constraint::Length(2));
    let areas = Layout::vertical(constraints).split(inner);

    let block = |name: &str, field: Field| {
        let color = if f.focus == field {
            t.border_active
        } else {
            t.muted
        };
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(name.to_string())
    };
    let cursor = |field: Field| if f.focus == field { "_" } else { "" };
    frame.render_widget(
        Paragraph::new(format!("{}{}", f.title.as_str(), cursor(Field::Title)))
            .block(block(" Title ([roadmap] added if missing) ", Field::Title)),
        areas[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}{}", f.body.as_str(), cursor(Field::Body)))
            .wrap(Wrap { trim: false })
            .block(block(" Body (Enter newline, Ctrl+E editor) ", Field::Body)),
        areas[1],
    );
    let mut hint_idx = 2;
    if f.kind == FormKind::Create {
        let sts: Vec<Span> = super::app::FORM_STATUSES
            .iter()
            .map(|c| {
                let st = if *c == f.status {
                    Style::default().fg(t.selection_fg).bg(t.column_color(*c))
                } else {
                    Style::default().fg(t.column_color(*c))
                };
                Span::styled(format!(" {} ", c.title()), st)
            })
            .collect();
        frame.render_widget(
            Paragraph::new(Line::from(sts)).block(block(" Status (Left/Right) ", Field::Status)),
            areas[2],
        );
        frame.render_widget(
            Paragraph::new(f.milestone().unwrap_or("(none)"))
                .block(block(" Milestone (Left/Right) ", Field::Milestone)),
            areas[3],
        );
        hint_idx = 4;
    }
    frame.render_widget(
        Paragraph::new("Body: short prose, 2-4 lines. No bold+colon bullet lists.")
            .style(Style::default().fg(t.muted)),
        areas[hint_idx],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
    use std::time::Duration;

    fn render_at(app: &App, w: u16, h: u16, now: Instant) -> Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_at(f, app, now)).unwrap();
        term.backend().buffer().clone()
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    /// Column (in cells) where `needle` starts in `row`.
    fn col_of(row: &str, needle: &str) -> Option<usize> {
        row.find(needle).map(|b| row[..b].chars().count())
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| row_text(buf, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ready(desc: &str, counts: [usize; 4]) -> SummaryState {
        SummaryState::Ready(RepoSummary {
            description: desc.into(),
            counts,
            active: None,
            latest_release: None,
        })
    }

    fn app_with(repos: &[&str]) -> App {
        let mut app = App::new();
        app.truecolor = false;
        app.repos = repos.iter().map(|r| r.to_string()).collect();
        app
    }

    #[test]
    fn unloaded_slots_are_blank_and_loaded_cards_render() {
        let mut app = app_with(&["repo0", "repo1", "repo2", "repo3"]);
        app.summaries
            .insert("repo0".into(), SummaryState::Failed("boom".into()));
        app.summaries.insert(
            "repo1".into(),
            SummaryState::Ready(RepoSummary {
                description: "A nice repo".into(),
                counts: [1, 0, 0, 2],
                active: Some("Ship it".into()),
                latest_release: None,
            }),
        );
        app.summaries.insert("repo2".into(), ready("", [0; 4]));
        app.summaries.insert("repo3".into(), SummaryState::Loading);
        let out = text(&render_at(&app, 70, 24, Instant::now()));
        assert_eq!(app_grid_cols(&app), 2);
        for needle in ["err", "boom", "A nice repo", "Ship it", "no description"] {
            assert!(out.contains(needle), "missing {needle}");
        }
        assert!(out.contains("\u{f0eb}1") && out.contains("\u{f085}2"));
        assert!(!out.contains("█▀█ █▀█ █▀█ ██▄ ███ ███"));
        assert!(out.contains("3 open issues"));
        assert!(out.contains("2/4 loaded"));
        assert!(out.contains("1 active repos"));
        assert!(out.contains("! 1 failed"));
        // Loading slot: nothing at all, and no placeholder text anywhere.
        assert!(!out.contains("repo3") && !out.contains("loading..."));
    }

    fn app_grid_cols(app: &App) -> usize {
        app.grid_cols.get()
    }

    #[test]
    fn grid_scrolls_selected_card_into_view() {
        let mut app = app_with(&[]);
        app.repos = (0..8).map(|i| format!("repo{i}")).collect();
        for r in app.repos.clone() {
            app.summaries.insert(r, ready("d", [0; 4]));
        }
        let initial = text(&render_at(&app, 70, 24, Instant::now()));
        assert!(initial.contains("No active work"));
        app.repo_sel = 8;
        let out = text(&render_at(&app, 70, 24, Instant::now()));
        assert!(out.contains("repo7") && !out.contains("repo1 "));
        // tiny terminals must not panic
        render_at(&app, 10, 3, Instant::now());
    }

    #[test]
    fn wide_card_alone_first_and_layout_stable_while_loading() {
        let mut app = app_with(&["one", "two"]);
        app.summaries.insert(
            "one".into(),
            SummaryState::Ready(RepoSummary {
                counts: [2, 0, 0, 0],
                active: Some("Ship it".into()),
                ..RepoSummary::default()
            }),
        );
        app.summaries.insert("two".into(), SummaryState::Loading);
        let before = render_at(&app, 70, 22, Instant::now());
        let wide: String = (1..8).map(|y| row_text(&before, y)).collect();
        let badges = "\u{f0eb}2 \u{f073}0 \u{f144}0 \u{f085}0 ";
        assert_eq!(
            col_of(&row_text(&before, 2), badges),
            Some((70 - badges.chars().count() + 1) / 2)
        );
        for glyph in ["\u{f0eb}2", "\u{f073}0", "\u{f144}0", "\u{f085}0"] {
            assert!(wide.contains(glyph), "missing {glyph}");
        }
        assert!(!wide.contains("█▀█ █▀█ █▀█ ██▄ ███ ███"));
        assert!(wide.contains("2 open issues"));
        assert!(wide.contains("1/2 loaded"));
        assert_eq!(wide.matches("1/2 loaded").count(), 1);
        assert!(wide.contains("1 active repos"));
        assert!(wide.contains("one — Ship it"));
        assert!(!wide.contains("repos failed"));
        let line = row_text(&before, 9);
        assert!(line.contains("one") && !line.contains("two"));
        // The reserved slot is blank (no border, nothing) on the right half.
        for y in 8..15 {
            assert!(
                row_text(&before, y)
                    .chars()
                    .skip(35)
                    .collect::<String>()
                    .trim()
                    .is_empty(),
                "row {y}"
            );
        }
        // Loading finishes: "one" does not move, "two" appears in its slot.
        app.summaries.insert("two".into(), ready("", [0; 4]));
        let after = render_at(&app, 70, 22, Instant::now());
        assert_eq!(
            col_of(&row_text(&before, 9), "one"),
            col_of(&row_text(&after, 9), "one")
        );
        assert_eq!(col_of(&row_text(&after, 9), "two"), Some(35 + 1));
        let after_text = text(&after);
        assert!(after_text.contains("2/2 loaded"));
        assert!(after_text.contains("2 open issues"));
        // One column: the all-repos card is a normal card.
        let narrow = render_at(&app, 30, 30, Instant::now());
        let narrow_text = text(&narrow);
        assert_eq!(
            col_of(&row_text(&narrow, 2), badges),
            Some((30 - badges.chars().count() + 1) / 2)
        );
        for glyph in ["\u{f0eb}2", "\u{f073}0", "\u{f144}0", "\u{f085}0"] {
            assert!(narrow_text.contains(glyph), "missing {glyph}");
        }
        assert!(!narrow_text.contains("█▀█ █▀█ █▀█ ██▄ ███ ███"));
        assert!(narrow_text.contains("2 open·1 active·2/2 loaded"));
        assert!(narrow_text.contains("2/2 loaded"));
        assert_eq!(narrow_text.matches("2/2 loaded").count(), 1);
        assert!(narrow_text.contains("1 active"));
        assert!(narrow_text.contains("one — Ship it"));
        assert!(!narrow_text.contains("repos failed"));
    }

    #[test]
    fn whole_card_fades_in_border_and_text_together() {
        let mut app = app_with(&["one"]);
        app.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        // Not drawn while loading, even on truecolor.
        app.truecolor = true;
        let blank = render_at(&app, 70, 24, t0);
        assert!(row_text(&blank, 8).trim().is_empty());
        app.apply_summary(
            "one".into(),
            Ok(RepoSummary {
                description: "hello".into(),
                counts: [1, 0, 0, 0],
                ..Default::default()
            }),
            t0,
        );
        // Border corner is at (0, 8); the description is on the third card row.
        let find = |buf: &Buffer| {
            let mut out = None;
            for y in 0..buf.area.height {
                if let Some(x) = col_of(&row_text(buf, y), "hello") {
                    out = Some(buf[(x as u16, y)].fg);
                }
            }
            (buf[(0, 8)].fg, out.expect("description drawn"))
        };
        let bg = Color::Rgb(20, 20, 24);
        assert_eq!(find(&render_at(&app, 70, 24, t0)), (bg, bg));
        let mid = find(&render_at(&app, 70, 24, t0 + Duration::from_millis(200)));
        assert!(matches!(mid.0, Color::Rgb(..)) && matches!(mid.1, Color::Rgb(..)));
        assert_ne!(mid.0, bg);
        let done = find(&render_at(&app, 70, 24, t0 + anim::FADE));
        assert_eq!(done, (app.theme.muted, app.theme.foreground));
        // No truecolor: instant, exact colors.
        app.truecolor = false;
        assert_eq!(
            find(&render_at(&app, 70, 24, t0)),
            (app.theme.muted, app.theme.foreground)
        );
    }

    #[test]
    fn error_card_appears_and_fades_in() {
        let mut app = app_with(&["bad"]);
        app.truecolor = true;
        app.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        app.apply_summary("bad".into(), Err("gh failed".into()), t0);
        let out = text(&render_at(&app, 70, 24, t0 + anim::FADE));
        assert!(out.contains("bad") && out.contains("err") && out.contains("gh failed"));
        let buf = render_at(&app, 70, 24, t0);
        assert_eq!(buf[(0, 8)].fg, Color::Rgb(20, 20, 24));
    }

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn footer_shows_activity_then_hints_and_messages_win() {
        let mut app = app_with(&["a", "b"]);
        assert_eq!(
            line_text(&footer_line(&app)),
            " r refresh all  u refresh repo  ? help  q quit"
        );
        app.screen = Screen::Board;
        assert_eq!(line_text(&footer_line(&app)), " esc back  ? help  q quit");
        app.screen = Screen::Repos;
        app.request_summaries(true, Instant::now());
        let t0 = Instant::now();
        let l = footer_line(&app);
        assert_eq!(line_text(&l), " fetching 0/2 repos...");
        assert_eq!(l.style.fg, Some(app.theme.warning));
        app.apply_summary("a".into(), Ok(RepoSummary::default()), t0);
        app.apply_summary("b".into(), Err("x".into()), t0);
        let l = footer_line(&app);
        assert_eq!(line_text(&l), " done, 1 error");
        assert_eq!(l.style.fg, Some(app.theme.danger));
        // An action result takes the footer over.
        app.set_status("done");
        assert_eq!(line_text(&footer_line(&app)), " done");
        app.message = None;
        app.tick(t0 + crate::tui::activity::DONE_TTL);
        assert_eq!(
            line_text(&footer_line(&app)),
            " r refresh all  u refresh repo  ? help  q quit"
        );
    }

    fn board_app() -> App {
        use crate::domain::Label;
        let mut app = App::new();
        app.truecolor = true;
        app.fx.enabled = true;
        let issue = |n: u64, state: &str, label: &str| crate::domain::Issue {
            number: n,
            title: format!("[roadmap] T{n}"),
            state: state.into(),
            labels: vec![Label { name: label.into() }],
            ..Default::default()
        };
        app.set_issues(vec![
            issue(1, "OPEN", "roadmap:idea"),
            issue(2, "OPEN", "roadmap:idea"),
            issue(3, "CLOSED", "roadmap"),
        ]);
        app.screen = Screen::Board;
        app
    }

    const BG: Color = Color::Rgb(20, 20, 24);

    #[test]
    fn board_entrance_fades_columns_left_to_right() {
        let mut app = board_app();
        let t0 = Instant::now();
        app.fx.start_board(t0);
        // Unselected card in column 0 (row 1) and the done card (column 4).
        let at = |t: Instant| {
            let b = render_at(&app, 100, 20, t);
            (b[(1, 3)].fg, b[(81, 2)].fg, b[(0, 1)].fg)
        };
        let (c0, _, border) = at(t0);
        assert_eq!(c0, BG);
        assert_eq!(border, BG); // column border fades with its content
                                // Column 4 starts 320ms in: still invisible while column 0 shows.
        let (c0, c4, _) = at(t0 + std::time::Duration::from_millis(300));
        assert_ne!(c0, BG);
        assert_eq!(c4, BG);
        let end = t0 + anim::FADE + super::super::effects::BOARD_DELAY_MAX;
        let (c0, c4, border) = at(end);
        assert_eq!((c0, c4), (app.theme.foreground, app.theme.foreground));
        assert_eq!(border, app.theme.border_active);
        // Effects off: instant, exact colors.
        app.fx.enabled = false;
        let b = render_at(&app, 100, 20, t0);
        assert_eq!(b[(1, 3)].fg, app.theme.foreground);
    }

    #[test]
    fn flash_selection_and_count_blend_toward_normal() {
        let mut app = board_app();
        let t0 = Instant::now();
        app.fx.flash(2, t0);
        app.fx.select(1, t0);
        app.fx.count_flash(0, t0);
        let b = render_at(&app, 100, 20, t0);
        assert_eq!(b[(1, 3)].fg, Color::Rgb(17, 168, 205)); // flash accent
        assert_eq!(b[(1, 2)].bg, BG); // selection blend starts at background
        assert_eq!(b[(8, 1)].fg, Color::Rgb(255, 255, 255)); // count flash
        let end = t0 + std::time::Duration::from_millis(600);
        let b = render_at(&app, 100, 20, end);
        assert_eq!(b[(1, 3)].fg, app.theme.foreground);
        assert_eq!(b[(1, 2)].bg, app.theme.selection_bg);
        assert_eq!(b[(8, 1)].fg, app.theme.column_color(Column::Idea));
    }

    #[test]
    fn popup_fades_in_over_its_frame() {
        let mut app = board_app();
        let t0 = Instant::now();
        app.popup = Popup::Help;
        app.fx.start_popup(t0);
        let area = Rect::new(0, 0, 100, 20);
        let r = popup_rect(&app.popup, area).unwrap();
        let corner = |t: Instant| render_at(&app, 100, 20, t)[(r.x, r.y)].fg;
        assert_eq!(corner(t0), BG);
        assert_eq!(
            corner(t0 + super::super::effects::POPUP_FADE),
            app.theme.border_popup
        );
    }
}
