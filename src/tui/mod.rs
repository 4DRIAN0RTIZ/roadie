//! Terminal setup and the event loop.
mod activity;
mod anim;
pub mod app;
mod editor;
mod effects;
pub mod events;
#[cfg(test)]
mod flows;
mod grid;
mod loader;
pub mod theme;
mod ui;
pub mod widgets;

use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::backend::Backend;
use crate::cache;
use app::{App, Popup};
use loader::SummaryLoader;

/// Redraw interval while a load is pending or a fade is running.
const FAST_TICK: Duration = Duration::from_millis(33);
const IDLE_TICK: Duration = Duration::from_millis(250);

/// Footer text while a key's (possibly blocking) work runs; `None` when the
/// key is served instantly (e.g. a board opened from the cache).
fn busy_label(app: &App, key: &crossterm::event::KeyEvent) -> Option<&'static str> {
    use app::Screen;
    use crossterm::event::KeyCode::{Char, Enter};
    if !matches!(app.popup, Popup::None) {
        return Some("working...");
    }
    match (app.screen, key.code) {
        (Screen::Repos, Enter) => {
            let cached = match app.repo_sel.checked_sub(1) {
                Some(i) => app
                    .repos
                    .get(i)
                    .is_some_and(|r| app.cache.board(r).is_some()),
                None => app.repos.iter().all(|r| app.cache.board(r).is_some()),
            };
            (!cached).then_some("fetching issues...")
        }
        (Screen::Overview, Enter) => {
            let cached = app
                .overview
                .get(app.overview_sel)
                .is_some_and(|(r, _)| app.cache.board(r).is_some());
            (!cached).then_some("fetching issues...")
        }
        (Screen::Overview, Char('r')) | (Screen::Board, Char('r')) => Some("fetching issues..."),
        _ => Some("working..."),
    }
}

fn enter() -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    Ok(())
}

fn leave() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
}

/// Restores the terminal on drop (normal exit, `?` early return, unwinding).
struct Guard;

impl Guard {
    fn new() -> Result<Self> {
        enter()?;
        Ok(Guard)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        leave();
    }
}

pub fn run(be: Arc<dyn Backend>) -> Result<()> {
    // Restore the terminal before the panic message prints.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        default_hook(info);
    }));

    let mut app = App::new();
    // Small synchronous read only; writes go through a background thread.
    if let Some(path) = cache::default_path(|k| std::env::var_os(k)) {
        app.cache = cache::Cache::load(&path);
        app.persist = Some(cache::spawn_writer(path));
    }
    events::load_repos(&mut app, be.as_ref());
    let loader = SummaryLoader::spawn(be.clone(), loader::WORKERS);

    let _guard = Guard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = event_loop(&mut terminal, &mut app, be.as_ref(), &loader);
    let _ = terminal.show_cursor();
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    be: &dyn Backend,
    loader: &SummaryLoader,
) -> Result<()> {
    while !app.should_quit {
        for repo in app.fetch_queue.drain(..) {
            loader.submit(repo);
        }
        let now = Instant::now();
        app.tick(now);
        for (i, (repo, res)) in loader.poll().into_iter().enumerate() {
            app.apply_summary(repo, res, now + anim::stagger(i));
        }
        terminal.draw(|f| ui::draw(f, app))?;
        let tick = if app.needs_fast_tick(Instant::now()) {
            FAST_TICK
        } else {
            IDLE_TICK
        };
        if !event::poll(tick)? {
            continue;
        }
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                // Blocking `gh` calls follow some keys: show that we are busy.
                if let Some(label) = busy_label(app, &k) {
                    app.set_status(label);
                    terminal.draw(|f| ui::draw(f, app))?;
                }
                app.message = None;
                events::handle_key(app, be, k);
                app.after_input(Instant::now());
            }
            Event::Paste(text) => events::handle_paste(app, &text),
            _ => {}
        }
        if app.editor_request {
            app.editor_request = false;
            run_editor(terminal, app);
        }
    }
    Ok(())
}

/// Suspends the TUI, edits the focused form body in `$EDITOR`, resumes.
fn run_editor(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) {
    let Popup::Form(form) = &app.popup else {
        return;
    };
    let initial = form.body.as_str().to_string();
    leave();
    let edited = editor::edit_text(&initial);
    let _ = enter();
    let _ = terminal.clear();
    match edited {
        Ok(text) => {
            if let Popup::Form(f) = &mut app.popup {
                f.body.set(text.trim_end());
            }
        }
        Err(e) => app.set_error(format!("{e:#}")),
    }
}
