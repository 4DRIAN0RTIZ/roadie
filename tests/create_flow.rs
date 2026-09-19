//! Full stack (events + `GhBackend` + stub `gh`/`roadmap`): creating an issue
//! while GitHub's list lags must still show and focus the new card.
mod common;

use common::Sandbox;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use roadie::tui::app::{App, Popup, Screen};
use roadie::tui::events::{handle_key, load_repos};

fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind(code, mods, KeyEventKind::Press)
}

fn press(app: &mut App, be: &dyn roadie::backend::Backend, code: KeyCode) {
    handle_key(app, be, key(code, KeyModifiers::NONE));
}

#[test]
fn created_issue_is_shown_even_if_the_first_list_after_create_lags() {
    let sb = Sandbox::new("lagflow");
    sb.git_repo("widgets", Some("git@github.com:acme/widgets.git"));
    // 1st list: open the board; 2nd (right after create): lags; 3rd: `r`.
    sb.use_fixture("issue_list.1.json", "issue_list.json");
    sb.use_fixture("issue_list.2.json", "issue_list.json");
    sb.use_fixture("issue_list.3.json", "issue_list_with_new.json");
    let be = sb.backend();
    let mut app = App::new();
    app.fx.enabled = false;
    load_repos(&mut app, &be);
    press(&mut app, &be, KeyCode::Char('j'));
    press(&mut app, &be, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Board);
    assert_eq!(app.issues.len(), 4);

    press(&mut app, &be, KeyCode::Char('n'));
    for c in "Brand new".chars() {
        press(&mut app, &be, KeyCode::Char(c));
    }
    handle_key(
        &mut app,
        &be,
        key(KeyCode::Char('s'), KeyModifiers::CONTROL),
    );
    assert!(matches!(app.popup, Popup::None));
    assert_eq!(
        sb.roadmap_log().last().unwrap(),
        "create\twidgets\tBrand new\t--body\t\t--status\tidea"
    );
    // The lagging list omitted #42, yet it is on the board and focused.
    let sel = app.selected_issue().expect("new card focused");
    assert_eq!(sel.number, 42);
    assert_eq!(sel.display_title(), "Brand new");

    // A real refetch now returns it from the server and keeps it.
    press(&mut app, &be, KeyCode::Char('r'));
    assert_eq!(app.issues.iter().filter(|i| i.number == 42).count(), 1);
    let lists = sb
        .gh_log()
        .iter()
        .filter(|l| l.starts_with("issue\tlist"))
        .count();
    assert_eq!(lists, 3);
}
