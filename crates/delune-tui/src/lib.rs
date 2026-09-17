//! # delune-tui
//!
//! The terminal client. It is a *client*: everything it shows comes from a delune
//! server over the same HTTP API the web UI uses, so you can run it on your laptop
//! against the server next to Navidrome.
//!
//! Architecture follows the usual ratatui shape — a single [`App`] state, a pure
//! `ui::draw` function, and an event loop that merges key presses with messages
//! from background tasks over a channel. Background tasks never touch the terminal.

mod api;
pub mod app;
pub mod auth;
pub mod connect;
pub mod matching;
pub mod setup;
mod sse;
mod tasks;
mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use tokio::sync::mpsc;

use app::{Action, is_link};
pub use app::{App, Connection, Screen};

/// Why the TUI stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The person quit.
    Quit,
    /// The server stopped accepting the session; sign in again.
    SignedOut,
}

/// Run the TUI against `server_url` until the user quits or the session ends.
///
/// Blocks the calling thread on the terminal event loop, and must be called from
/// inside a multi-threaded Tokio runtime so background tasks keep running.
///
/// # Errors
///
/// When the terminal can't be drawn to or read from.
pub fn run(server_url: String, http: &reqwest::Client) -> Result<Exit> {
    let base = server_url.trim_end_matches('/').to_owned();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cx = tasks::Context::new(http.clone(), base, tx);
    tasks::start(&cx);

    let mut terminal = ratatui::init();
    let mut app = App::new(server_url);
    // Real images where the terminal can show them (kitty, iTerm2, sixel), blocks elsewhere.
    app.canvas.get_mut().picker = Some(
        ratatui_image::picker::Picker::from_query_stdio()
            .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
    );
    let result = loop {
        if let Err(e) = terminal.draw(|frame| ui::draw(frame, &app)) {
            break Err(e.into());
        }
        while let Ok(msg) = rx.try_recv() {
            app.on_message(msg);
        }
        if app.signed_out {
            break Ok(Exit::SignedOut);
        }
        app.want_selected_cover();
        let mut actions = std::mem::take(&mut app.effects);
        actions.extend(app.lookups_wanted());
        if app.screen == app::Screen::Review {
            actions.push(app.report_to_load());
        }
        // Poll briefly so background messages and progress repaint promptly
        // without busy-looping.
        match event::poll(Duration::from_millis(80)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    let action = app.on_key(key.code, key.modifiers);
                    if !matches!(action, Action::None) {
                        app.notice = None;
                    }
                    actions.push(action);
                }
                Ok(_) => {}
                Err(e) => break Err(e.into()),
            },
            Ok(false) => {}
            Err(e) => break Err(e.into()),
        }
        for action in actions {
            match action {
                Action::None => {}
                Action::StartSearch(query) => {
                    app.begin_search(&query);
                    if !is_link(&query) && query.chars().count() >= 2 {
                        tasks::perform(Action::Catalog(query.clone()), &cx);
                    }
                    app.search_task = Some(tokio::spawn(tasks::stream_search(cx.clone(), query)));
                }
                action => tasks::perform(action, &cx),
            }
        }
        if app.should_quit {
            break Ok(Exit::Quit);
        }
    };
    if let Some(task) = app.search_task.take() {
        task.abort();
    }
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::app::{Focus, Loadable, Message, Page, SearchState};
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use delune_core::api::{
        AlbumInfo, AlbumTrack, Candidate, CandidateFile, DownloadJob, JobStatus, LibraryMatch, LibraryState,
        LibraryTrack, ReviewReport, ReviewState, SearchEvent,
    };
    use delune_core::{Codec, Quality};

    fn press(app: &mut App, code: KeyCode) -> Action {
        app.on_key(code, KeyModifiers::NONE)
    }

    fn candidate(id: &str, quality: Quality) -> Candidate {
        Candidate {
            id: id.into(),
            username: "peer".into(),
            folder: format!("music\\Boards of Canada\\{id}"),
            title: id.into(),
            parent: Some("Boards of Canada".into()),
            files: vec![CandidateFile {
                path: "01 Sixtyniner.flac".into(),
                name: "01 Sixtyniner.flac".into(),
                size: 1,
                audio: true,
                quality: Some(quality),
                quality_label: None,
                duration_secs: Some(300),
            }],
            audio_files: 1,
            total_bytes: 1,
            duration_secs: None,
            quality: Some(quality),
            quality_label: Some(quality.to_string()),
            quality_rank: quality.rank(),
            mixed_quality: false,
            has_cover: false,
            free_slot: true,
            avg_speed: 1,
            queue_length: 0,
            peer: None,
        }
    }

    fn job(id: &str, status: JobStatus) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            username: "peer".into(),
            folder: format!("music\\{id}"),
            title: format!("Album {id}"),
            parent: Some("Boards of Canada".into()),
            created_at: 0,
            status,
            files: vec![delune_core::api::JobFile {
                path: r"music\Twoism\01 Sixtyniner.flac".into(),
                name: "01 Sixtyniner.flac".into(),
                size: 100,
                status: delune_core::api::FileStatus::Transferring,
                bytes: 40,
                place_in_queue: None,
                error: None,
            }],
            bytes: 40,
            total_bytes: 100,
            review: ReviewState::Waiting,
            requested_by: None,
            imported_to: None,
            imported_at: None,
            priority: 0,
            waiting_for_slot: None,
            error: None,
        }
    }

    fn report() -> ReviewReport {
        ReviewReport {
            album_artist: "Boards of Canada".into(),
            album: "Twoism".into(),
            year: Some(1995),
            tracks: vec![],
            cover: None,
            warnings: vec![],
            conflicts: vec![],
            library_dir: None,
            blocked_reason: None,
        }
    }

    #[test]
    fn typing_enter_and_escape() {
        let mut app = App::new("http://localhost:7474");
        for c in "ok computer".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(press(&mut app, KeyCode::Enter), Action::StartSearch("ok computer".into()));
        app.on_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(app.query, "ok ", "ctrl-w deletes a word");
        press(&mut app, KeyCode::Esc);
        assert!(app.query.is_empty() && !app.should_quit, "first Esc clears the query");
        assert_eq!(press(&mut app, KeyCode::Enter), Action::None, "empty query doesn't search");
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit, "Esc never quits");
        // Screens switch even while typing.
        press(&mut app, KeyCode::F(2));
        assert_eq!(app.screen, app::Screen::Downloads);
        app.on_key(KeyCode::Char('3'), KeyModifiers::ALT);
        assert_eq!(app.screen, app::Screen::Review);
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.screen, app::Screen::Search);
        // Searching hands the keys to the results, so 2 switches screens straight away.
        app.begin_search("ok computer");
        assert_eq!(app.focus, Focus::Results);
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.screen, app::Screen::Downloads);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit, "q quits");
    }

    #[test]
    fn downloading_warns_about_what_you_already_have() {
        let mut app = App::new("x");
        app.on_search_event(SearchEvent::Candidates {
            items: vec![candidate("Twoism", Quality::lossless(Codec::Flac, 16, 44_100))],
        });
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.query, "1", "digits type into the search box");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.focus, Focus::Results);
        assert!(matches!(press(&mut app, KeyCode::Char('d')), Action::Download(c) if c.id == "Twoism"));

        // The library lookup runs for visible rows, once.
        let wanted = app.lookups_wanted();
        assert!(
            matches!(&wanted[..], [Action::Library { album, artist: Some(a), .. }] if album == "Twoism" && a == "Boards of Canada")
        );
        assert!(app.lookups_wanted().is_empty());
        let Action::Library { key, .. } = &wanted[0] else { unreachable!() };
        app.on_message(Message::Library(
            key.clone(),
            LibraryMatch {
                state: LibraryState::InLibrary,
                album: Some("Twoism".into()),
                artist: None,
                year: None,
                tracks: vec![LibraryTrack { title: "Sixtyniner".into(), track: Some(1), disc: None }],
                quality_label: Some("FLAC 16/44.1".into()),
            },
        ));
        assert_eq!(app.mark_for(&app.results[0].clone()), Some(matching::Mark::InLibrary));
        assert_eq!(press(&mut app, KeyCode::Char('d')), Action::None, "asks first");
        assert!(app.confirm.as_ref().is_some_and(|c| c.prompt.contains("already in your library as FLAC")));
        assert!(matches!(press(&mut app, KeyCode::Char('y')), Action::Download(_)));

        // Already downloading this copy: just says so.
        let mut active = job("x", JobStatus::Downloading);
        active.folder.clone_from(&app.results[0].folder);
        app.on_message(Message::Jobs(vec![active]));
        assert!(matches!(app.mark_for(&app.results[0].clone()), Some(matching::Mark::Downloading(_))));
        assert_eq!(press(&mut app, KeyCode::Char('d')), Action::None);
        assert!(app.confirm.is_none() && app.notice.is_some());
    }

    #[test]
    fn opening_albums_and_artists() {
        let mut app = App::new("x");
        app.on_search_event(SearchEvent::Candidates {
            items: vec![candidate("Twoism", Quality::lossless(Codec::Flac, 16, 44_100))],
        });
        app.focus = Focus::Results;
        // Enter opens that person's folder: its tracks, ticked, ready to download.
        assert_eq!(press(&mut app, KeyCode::Enter), Action::None);
        assert!(matches!(app.pages.last(), Some(Page::Release { .. })));
        assert!(matches!(app.effects.as_slice(), [Action::Cover { album, .. }] if album == "Twoism"));
        assert!(matches!(press(&mut app, KeyCode::Char('d')), Action::Download(c) if c.id == "Twoism"));
        // i shows the album's catalogue page, which can still download that copy.
        let Action::Album { key, artist, title } = press(&mut app, KeyCode::Char('i')) else {
            panic!("opens the album")
        };
        assert_eq!((artist.as_deref(), title.as_str()), (Some("Boards of Canada"), "Twoism"));
        assert_eq!(app.focus, Focus::Page);
        assert!(matches!(press(&mut app, KeyCode::Char('d')), Action::Download(c) if c.id == "Twoism"));
        app.on_message(Message::Album(
            key,
            Ok(AlbumInfo {
                title: "Twoism".into(),
                artist: Some("Boards of Canada".into()),
                year: Some(1995),
                cover: None,
                tracks: vec![AlbumTrack { position: 1, title: "Sixtyniner".into(), artist: None, duration_secs: None }],
                in_library: LibraryMatch {
                    state: LibraryState::NotInLibrary,
                    album: None,
                    artist: None,
                    year: None,
                    tracks: vec![],
                    quality_label: None,
                },
            }),
        ));
        assert!(matches!(app.pages.last(), Some(Page::Album { info: Loadable::Ready(_), .. })));
        assert_eq!(press(&mut app, KeyCode::Char('a')), Action::Artist("Boards of Canada".into()));
        assert_eq!(app.pages.len(), 3, "folder, album, artist");
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            press(&mut app, KeyCode::Char('s')),
            Action::StartSearch("Boards of Canada Twoism".into()),
            "s searches Soulseek for the album"
        );
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.pages.last(), Some(Page::Release { .. })), "back to the folder");
        press(&mut app, KeyCode::Esc);
        assert!(app.pages.is_empty());
        assert_eq!(app.focus, Focus::Results, "closing the last page returns to the results");
    }

    #[test]
    fn downloads_group_and_act() {
        let mut app = App::new("x");
        let mut waiting = job("w", JobStatus::Queued);
        waiting.waiting_for_slot = Some(2);
        app.on_message(Message::Jobs(vec![
            job("done", JobStatus::Imported),
            job("b", JobStatus::Cancelled),
            waiting,
            job("a", JobStatus::Downloading),
        ]));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.query, "2", "the search box has the keys at first");
        app.query.clear();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus, Focus::Input, "nothing to move to yet");
        app.focus = Focus::Results;
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.screen, Screen::Downloads);
        let order: Vec<&str> = app.download_list().iter().map(|j| j.id.as_str()).collect();
        assert_eq!(order, ["a", "w", "b"], "active first, imported hidden");
        assert_eq!(press(&mut app, KeyCode::Char('s')), Action::Stop("a".into()));
        press(&mut app, KeyCode::Down);
        assert_eq!(press(&mut app, KeyCode::Char('p')), Action::Prioritise("w".into()));
        press(&mut app, KeyCode::Down);
        assert_eq!(press(&mut app, KeyCode::Char('s')), Action::Resume("b".into()));

        assert_eq!(press(&mut app, KeyCode::Char('x')), Action::None);
        assert!(app.confirm.is_some(), "removing asks first");
        assert_eq!(press(&mut app, KeyCode::Char('n')), Action::None);
        assert!(app.confirm.is_none());
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(press(&mut app, KeyCode::Char('y')), Action::Remove("b".into()));

        assert_eq!(
            press(&mut app, KeyCode::Char('f')),
            Action::StartSearch("Boards of Canada Album b".into()),
            "f looks for another copy"
        );
        assert_eq!(app.screen, Screen::Search);

        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Char('i'));
        assert_eq!(app.download_list().len(), 4, "i shows imported albums too");

        // A newer list keeps the same job selected.
        app.job_selected = 0;
        app.on_message(Message::Jobs(vec![job("new", JobStatus::Downloading), job("a", JobStatus::Downloading)]));
        assert_eq!(app.download_list()[app.job_selected].id, "a");
    }

    #[test]
    fn review_waits_for_checks_before_importing() {
        let mut app = App::new("x");
        let mut ready = job("r", JobStatus::Ready);
        ready.review = ReviewState::Ready;
        app.on_message(Message::Jobs(vec![ready, job("old", JobStatus::Imported)]));
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        app.focus = Focus::Results;
        assert_eq!(press(&mut app, KeyCode::Char('3')), Action::LoadReport("r".into()));
        assert_eq!(app.review_list().len(), 2, "recent imports are listed after");
        press(&mut app, KeyCode::Char('i'));
        assert!(app.notice.as_ref().is_some_and(|n| n.bad), "can't import before the report arrives");

        app.on_message(Message::Report("r".into(), Ok(report())));
        press(&mut app, KeyCode::Char('i'));
        assert_eq!(press(&mut app, KeyCode::Char('y')), Action::Import("r".into()));

        // Someone who can't import their own downloads is told why.
        app.me = Some(delune_core::api::Me {
            username: "sam".into(),
            admin: false,
            permissions: delune_core::api::Permissions::MEMBER,
            can_import: false,
            mode: delune_core::api::AuthMode::Navidrome,
            appearance: delune_core::api::Appearance::default(),
            avatar: None,
            token: None,
        });
        assert_eq!(press(&mut app, KeyCode::Char('i')), Action::None);
        assert!(app.confirm.is_none());
        assert!(app.notice.as_ref().is_some_and(|n| n.text.contains("admin")));
    }

    #[test]
    fn a_refused_session_ends_the_run() {
        let mut app = App::new("x");
        app.on_message(Message::SignedOut);
        assert!(app.signed_out);
    }

    #[test]
    fn renders_every_screen_at_several_sizes() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new("http://localhost:7474");
        let mut ready = job("r", JobStatus::Ready);
        ready.review = ReviewState::Ready;
        app.on_message(Message::Jobs(vec![job("a", JobStatus::Downloading), ready, job("i", JobStatus::Imported)]));
        app.on_message(Message::Report("r".into(), Ok(report())));
        app.on_search_event(SearchEvent::Started { query: "twoism".into(), timeout_secs: 20 });
        app.on_search_event(SearchEvent::Candidates {
            items: vec![candidate("Twoism", Quality::lossless(Codec::Flac, 24, 96_000))],
        });
        app.catalog = vec![super::app::CatalogItem::Artist(delune_core::api::ArtistHit {
            name: "Boards of Canada".into(),
            picture: None,
            listeners: Some(1_200_000),
        })];
        for (width, height) in [(120, 36), (80, 24), (60, 16)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for screen in [Screen::Search, Screen::Downloads, Screen::Review] {
                app.screen = screen;
                app.help = screen == Screen::Review && width == 120;
                terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
                let text: String =
                    terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect();
                if std::env::var_os("DELUNE_PRINT_SCREENS").is_some() {
                    for row in text.chars().collect::<Vec<_>>().chunks(usize::from(width)) {
                        println!("{}", row.iter().collect::<String>());
                    }
                }
                assert!(text.contains("Search"), "the header lists the screens at {width}x{height}");
                if screen == Screen::Downloads && width >= 80 {
                    assert!(text.contains("Album a"), "{text}");
                }
            }
        }
        assert!(matches!(app.search, SearchState::Running { .. }));

        // A release page, with its cover drawn in blocks.
        app.canvas.get_mut().picker = Some(ratatui_image::picker::Picker::halfblocks());
        app.screen = Screen::Search;
        app.help = false;
        app.focus = Focus::Results;
        press(&mut app, KeyCode::Enter);
        let key = App::cover_key(&app.results[0]);
        let cover = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90])));
        app.on_message(Message::Picture(key, Some(cover)));
        for (width, height) in [(120, 36), (60, 16)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            let text: String =
                terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect();
            if std::env::var_os("DELUNE_PRINT_SCREENS").is_some() {
                for row in text.chars().collect::<Vec<_>>().chunks(usize::from(width)) {
                    println!("{}", row.iter().collect::<String>());
                }
            }
            assert!(text.contains("[x]"), "tracks are ticked at {width}x{height}: {text}");
            assert!(text.contains('▀') || text.contains('▄'), "the cover is drawn at {width}x{height}");
        }
    }
}
