//! End-to-end flow tests: scripted keys drive the real event handler, the
//! real background loader and the real renderer (ratatui `TestBackend`) over
//! a `FakeBackend`; assertions read the rendered buffer, not just state.
//! Time is injected (`Flow::advance`) and animations are off unless a test
//! turns them on.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

use crate::backend::fake::FakeBackend;
use crate::cache::{self, Cache};
use crate::domain::{Column, Issue, Label, Milestone, RepoSummary};

use super::app::{App, Popup, Screen, SummaryState};
use super::events::{handle_key, load_repos};
use super::loader::SummaryLoader;
use super::ui;

const FOOTER_HINT: &str = "r refresh all  u refresh repo  ? help  q quit";

fn issue(n: u64, title: &str, state: &str, status: &str, ms: Option<&str>) -> Issue {
    let mut labels = vec![Label {
        name: "roadmap".into(),
    }];
    if !status.is_empty() {
        labels.push(Label {
            name: format!("roadmap:{status}"),
        });
    }
    Issue {
        number: n,
        title: format!("[roadmap] {title}"),
        body: format!("body of {title}"),
        state: state.into(),
        labels,
        milestone: ms.map(|m| Milestone { title: m.into() }),
        url: format!("https://github.com/me/alpha/issues/{n}"),
        ..Default::default()
    }
}

/// alpha: #1 planned, #2 working (M1), #3 closed; beta: same list (shared fake).
fn backend() -> FakeBackend {
    let be = FakeBackend {
        repos: vec!["alpha".into(), "beta".into()],
        issues: vec![
            issue(1, "Alpha thing", "OPEN", "planned", None),
            issue(2, "Beta thing", "OPEN", "working", Some("M1")),
            issue(3, "Old thing", "CLOSED", "", None),
        ]
        .into(),
        milestones: vec!["M1".into()],
        ..Default::default()
    };
    let mut be = be;
    for r in ["alpha", "beta"] {
        be.summaries.insert(
            r.into(),
            RepoSummary {
                description: format!("{r} description"),
                counts: [0, 1, 0, 1],
                active: Some("Beta thing".into()),
                latest_release: None,
            },
        );
    }
    be
}

struct Flow {
    app: App,
    be: Arc<FakeBackend>,
    loader: SummaryLoader,
    term: Terminal<TestBackend>,
    now: Instant,
}

impl Flow {
    fn new(be: FakeBackend) -> Self {
        Self::sized(be, 120, 30)
    }

    fn sized(be: FakeBackend, w: u16, h: u16) -> Self {
        let be = Arc::new(be);
        let mut app = App::new();
        app.truecolor = false;
        app.fx.enabled = false;
        Self {
            loader: SummaryLoader::spawn(be.clone(), 2),
            term: Terminal::new(TestBackend::new(w, h)).unwrap(),
            app,
            be,
            now: Instant::now(),
        }
    }

    fn launch(&mut self) {
        load_repos(&mut self.app, self.be.as_ref());
        self.submit();
    }

    fn submit(&mut self) {
        for r in self.app.fetch_queue.drain(..) {
            self.loader.submit(r);
        }
    }

    /// Waits for every queued summary to come back and applies them.
    fn settle(&mut self) {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            for (r, res) in self.loader.poll() {
                self.app.apply_summary(r, res, self.now);
            }
            let pending = self
                .app
                .summaries
                .values()
                .any(|s| matches!(s, SummaryState::Loading));
            if !pending || Instant::now() > end {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn advance(&mut self, ms: u64) {
        self.now += Duration::from_millis(ms);
        self.app.tick(self.now);
    }

    fn press(&mut self, code: KeyCode) {
        let k = KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press);
        // Same as the real event loop: a key press clears the last message.
        self.app.message = None;
        handle_key(&mut self.app, self.be.as_ref(), k);
        self.app.after_input(self.now);
        self.submit();
    }

    fn keys(&mut self, s: &str) {
        for c in s.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    fn ctrl(&mut self, c: char) {
        let k =
            KeyEvent::new_with_kind(KeyCode::Char(c), KeyModifiers::CONTROL, KeyEventKind::Press);
        handle_key(&mut self.app, self.be.as_ref(), k);
        self.app.after_input(self.now);
    }

    fn draw(&mut self) -> Buffer {
        let (app, now) = (&self.app, self.now);
        self.term.draw(|f| ui::draw_at(f, app, now)).unwrap();
        self.term.backend().buffer().clone()
    }

    fn screen(&mut self) -> String {
        let b = self.draw();
        rows(&b).join("\n")
    }

    fn footer(&mut self) -> String {
        let b = self.draw();
        rows(&b)
            .last()
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// Cell (x, y) where `needle` starts on the screen.
    fn find(&mut self, needle: &str) -> Option<(u16, u16)> {
        let b = self.draw();
        rows(&b).iter().enumerate().find_map(|(y, r)| {
            r.find(needle)
                .map(|byte| (r[..byte].chars().count() as u16, y as u16))
        })
    }

    fn open_alpha(&mut self) {
        self.press(KeyCode::Char('j'));
        self.press(KeyCode::Enter);
        assert_eq!(self.app.screen, Screen::Board);
    }
}

fn rows(b: &Buffer) -> Vec<String> {
    (0..b.area.height)
        .map(|y| {
            (0..b.area.width)
                .map(|x| b[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

fn tmp_cache_path(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("roadie-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d.join(cache::FILE_NAME)
}

// ---------------------------------------------------------------- launch

#[test]
fn empty_cache_launch_shows_fetching_then_cards_then_done_then_hints() {
    let mut f = Flow::new(backend());
    f.launch();
    // Instant list, but cards are not drawn until their summary arrives.
    let s = f.screen();
    assert!(s.contains("0/2 loaded") && !s.contains("alpha description"));
    assert!(f.footer().contains("fetching 0/2 repos..."));
    f.settle();
    let s = f.screen();
    assert!(s.contains("alpha") && s.contains("beta description"));
    assert!(s.contains("\u{f073}1") && s.contains("\u{f085}1"));
    assert!(s.contains("> Beta thing"));
    assert_eq!(f.footer(), "done, 2 repos loaded");
    f.advance(2900);
    assert_eq!(f.footer(), "done, 2 repos loaded");
    f.advance(200);
    assert_eq!(f.footer(), FOOTER_HINT);
}

#[test]
fn relaunch_from_disk_cache_shows_cards_without_backend_reads() {
    let path = tmp_cache_path("relaunch");
    {
        let mut f = Flow::new(backend());
        f.app.persist = Some(cache::spawn_writer(path.clone()));
        f.launch();
        f.settle();
        f.open_alpha(); // also caches alpha's board
        let end = Instant::now() + Duration::from_secs(3);
        while Cache::load(&path).board("alpha").is_none() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let mut f = Flow::new(backend());
    f.app.cache = Cache::load(&path);
    f.launch();
    f.settle();
    assert!(f.be.reads().is_empty(), "reads: {:?}", f.be.reads());
    let s = f.screen();
    assert!(s.contains("alpha description") && s.contains("beta description"));
    assert!(f.footer().contains("loaded from cache"));
    // Opening the cached repo is instant too.
    f.open_alpha();
    assert!(f.be.reads().is_empty());
    let s = f.screen();
    assert!(s.contains("#1 Alpha thing") && s.contains("#2 Beta thing"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn enter_opens_a_repo_and_esc_returns_with_the_selection_kept() {
    let mut f = Flow::new(backend());
    f.launch();
    f.settle();
    f.keys("jl"); // beta
    f.press(KeyCode::Enter);
    assert_eq!(f.app.screen, Screen::Board);
    assert_eq!(f.app.repo.as_deref(), Some("beta"));
    let s = f.screen();
    assert!(s.contains("planned (1)") && s.contains("#1 Alpha thing"));
    f.press(KeyCode::Esc);
    assert_eq!(f.app.screen, Screen::Repos);
    assert_eq!(f.app.repo_sel, 2);
    assert!(f.screen().contains("beta description"));
    f.press(KeyCode::Esc); // nothing on the grid
    assert!(!f.app.should_quit);
}

// ---------------------------------------------------------------- create

fn create(f: &mut Flow, title: &str) {
    f.press(KeyCode::Char('n'));
    f.keys(title);
    f.ctrl('s');
}

fn assert_cursor_on(f: &mut Flow, title: &str) {
    let (x, y) = f
        .find(title)
        .unwrap_or_else(|| panic!("{title} not rendered"));
    let b = f.draw();
    assert_eq!(
        b[(x, y)].bg,
        f.app.theme.selection_bg,
        "cursor not on {title}"
    );
}

#[test]
fn creating_an_issue_shows_it_with_the_cursor_on_it() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    create(&mut f, "Brand new");
    assert!(f.screen().contains("#4 Brand new"));
    assert_cursor_on(&mut f, "#4 Brand new");
    assert!(f.footer().contains("created #4"));
}

#[test]
fn creating_with_an_active_milestone_filter_clears_it_and_shows_the_card() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    f.press(KeyCode::Char('f'));
    assert_eq!(f.app.filter.milestone.as_deref(), Some("M1"));
    assert!(!f.screen().contains("#1 Alpha thing"));
    create(&mut f, "Brand new");
    let s = f.screen();
    assert!(s.contains("#4 Brand new") && s.contains("#1 Alpha thing"));
    assert!(f.footer().contains("filter cleared"));
    assert_cursor_on(&mut f, "#4 Brand new");
}

#[test]
fn creating_while_the_github_list_lags_still_shows_the_card() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    *f.be.list_lag.lock().unwrap() = 1;
    create(&mut f, "Lagging one");
    assert!(f.screen().contains("#4 Lagging one"));
    assert_cursor_on(&mut f, "#4 Lagging one");
    f.press(KeyCode::Char('r')); // the refetch confirms it
    assert!(f.screen().contains("#4 Lagging one"));
}

// ---------------------------------------------------------- move / edit

#[test]
fn shift_l_moves_a_card_and_wip_limit_needs_confirmation() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    assert_eq!(f.app.column(), Column::Planned);
    f.press(KeyCode::Char('L')); // planned -> now
    assert_eq!(f.be.calls(), vec!["status alpha 1 now"]);
    let s = f.screen();
    assert!(s.contains("now (1)") && s.contains("planned (0)"));
    f.press(KeyCode::Char('L')); // now -> working: #2 is already there
    assert!(matches!(f.app.popup, Popup::WipWarn { .. }));
    assert!(f.screen().contains("WIP limit"));
    f.press(KeyCode::Char('n')); // any other key cancels
    assert_eq!(f.be.calls().len(), 1);
    f.press(KeyCode::Char('L'));
    f.press(KeyCode::Char('y'));
    assert_eq!(f.be.calls().last().unwrap(), "status alpha 1 working");
    assert!(f.screen().contains("working (2)"));
    f.press(KeyCode::Char('H')); // and back
    assert_eq!(f.be.calls().last().unwrap(), "status alpha 1 now");
}

#[test]
fn editing_title_and_body_updates_the_rendered_card() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    f.press(KeyCode::Char('e'));
    f.keys("!");
    f.press(KeyCode::Tab);
    f.keys("+");
    f.ctrl('s');
    assert!(f.screen().contains("#1 Alpha thing!"));
    assert_eq!(
        f.be.calls(),
        vec![r#"edit alpha 1 "[roadmap] Alpha thing!" "body of Alpha thing+""#]
    );
    f.press(KeyCode::Enter); // detail popup shows the new body
    assert!(f.screen().contains("body of Alpha thing+"));
}

#[test]
fn commenting_records_the_comment_and_reports_it() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    f.press(KeyCode::Char('c'));
    f.keys("looks good");
    assert!(f.screen().contains("looks good_"));
    f.press(KeyCode::Enter);
    assert_eq!(f.be.calls(), vec![r#"comment alpha 1 "looks good""#]);
    assert!(f.footer().contains("commented on #1"));
}

#[test]
fn close_and_reopen_move_the_card_between_columns() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    f.press(KeyCode::Char('x'));
    f.keys("shipped");
    f.press(KeyCode::Enter);
    let s = f.screen();
    assert!(s.contains("done (2)") && s.contains("planned (0)"));
    assert_eq!(f.app.column(), Column::Done);
    assert_eq!(f.app.selected_issue().unwrap().number, 1);
    f.press(KeyCode::Char('u')); // reopen
    let s = f.screen();
    assert!(s.contains("done (1)"), "{s}");
    assert_eq!(
        f.be.calls(),
        vec![r#"close alpha 1 Some("shipped")"#, "reopen alpha 1"]
    );
}

// ------------------------------------------------------------- filters

#[test]
fn search_and_milestone_filters_narrow_the_board_and_esc_clears_them() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    f.press(KeyCode::Char('/'));
    f.keys("Alpha");
    f.press(KeyCode::Enter);
    let s = f.screen();
    assert!(s.contains("#1 Alpha thing") && !s.contains("#2 Beta thing"));
    assert!(s.contains("filtered"));
    f.press(KeyCode::Esc);
    assert!(f.screen().contains("#2 Beta thing"));
    f.press(KeyCode::Char('f'));
    let s = f.screen();
    assert!(s.contains("#2 Beta thing") && !s.contains("#1 Alpha thing"));
    assert!(s.contains("milestone:M1"));
    f.press(KeyCode::Esc);
    assert!(f.screen().contains("#1 Alpha thing"));
    assert_eq!(f.app.screen, Screen::Board); // Esc cleared, did not go back
    f.press(KeyCode::Esc);
    assert_eq!(f.app.screen, Screen::Repos);
}

// ------------------------------------------------- refetch / errors

#[test]
fn r_refetches_and_replays_the_entrance() {
    let mut f = Flow::new(backend());
    f.app.fx.enabled = true;
    f.launch();
    f.open_alpha();
    f.advance(3000); // entrance finished
    assert_eq!(f.app.fx.board_level(0, 0, f.now), 1.0);
    let before = f.be.reads().len();
    f.press(KeyCode::Char('r'));
    assert_eq!(f.be.reads().len(), before + 1);
    // Events stamp effects with the real clock, so probe with real time.
    let real = Instant::now();
    assert!(f.app.fx.board_level(0, 0, real) < 1.0, "entrance restarts");
    assert!(f.footer().contains("done"));
    assert_eq!(
        f.app.fx.board_level(0, 0, real + Duration::from_secs(3)),
        1.0
    );
}

#[test]
fn backend_error_shows_in_the_footer_and_keeps_old_data() {
    let mut f = Flow::new(backend());
    f.launch();
    f.open_alpha();
    *f.be.fail_with.lock().unwrap() = Some("gh exploded".into());
    f.press(KeyCode::Char('r'));
    assert!(f.footer().contains("gh exploded"));
    let s = f.screen();
    assert!(s.contains("#1 Alpha thing") && s.contains("#2 Beta thing"));
    // Errors persist until the next key.
    f.advance(10_000);
    assert!(f.footer().contains("gh exploded"));
    *f.be.fail_with.lock().unwrap() = None;
    f.press(KeyCode::Char('j'));
    assert!(!f.footer().contains("gh exploded"));
}

#[test]
fn a_failing_summary_shows_an_err_card_without_blocking_the_others() {
    let mut be = backend();
    be.summaries.remove("beta");
    let mut f = Flow::new(be);
    f.launch();
    f.settle();
    let s = f.screen();
    assert!(s.contains("alpha description") && s.contains("err"));
    assert!(s.contains("no summary for beta"));
    assert_eq!(f.footer(), "done, 1 error");
}

// ---------------------------------------------------------- tiny sizes

#[test]
fn every_screen_and_popup_renders_at_any_terminal_size_without_panicking() {
    for (w, h) in [
        (1, 1),
        (10, 3),
        (20, 5),
        (34, 8),
        (80, 24),
        (200, 60),
        (300, 4),
    ] {
        let mut f = Flow::sized(backend(), w, h);
        f.launch();
        f.draw(); // cards still loading
        f.settle();
        f.draw();
        f.press(KeyCode::Char('?')); // help
        f.draw();
        f.press(KeyCode::Esc);
        f.keys("jjjj");
        f.draw();
        f.press(KeyCode::Char('g')); // wide all-repos card
        f.press(KeyCode::Enter); // overview
        f.draw();
        f.press(KeyCode::Esc);
        f.open_alpha();
        f.draw();
        for popup in ["\n", "n", "e", "c", "x", "m", "/", "?"] {
            for c in popup.chars() {
                let code = if c == '\n' {
                    KeyCode::Enter
                } else {
                    KeyCode::Char(c)
                };
                f.press(code);
            }
            f.draw();
            f.press(KeyCode::Esc);
            f.press(KeyCode::Esc);
            if f.app.screen != Screen::Board {
                f.press(KeyCode::Enter);
            }
        }
        f.keys("LL");
        f.draw(); // WIP warning
        f.press(KeyCode::Char('y'));
        f.draw();
        f.advance(5000);
        f.draw();
    }
}

#[test]
fn animations_on_render_at_any_size_without_panicking() {
    for (w, h) in [(1, 1), (20, 5), (80, 24)] {
        let mut f = Flow::sized(backend(), w, h);
        f.app.truecolor = true;
        f.app.fx.enabled = true;
        f.launch();
        f.settle();
        for ms in [0, 100, 400, 900, 2000] {
            f.advance(ms);
            f.draw();
        }
        f.open_alpha();
        f.keys("LL");
        f.press(KeyCode::Char('y'));
        for ms in [0, 60, 200, 1500] {
            f.advance(ms);
            f.draw();
        }
    }
}
