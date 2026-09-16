//! Everything the TUI knows, and what each key press does to it.
//!
//! Pure state: key presses and background messages change it, and anything that needs
//! the network comes back as an [`Action`] for the background tasks to carry out.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use delune_core::api::{
    AlbumHit, AlbumInfo, ArtistHit, ArtistInfo, Candidate, DownloadJob, Health, JobStatus, LibraryMatch, Me,
    MusicSearch, ResolvedLink, ReviewReport, ReviewState, SearchEvent, SoulseekStatus,
};
use tokio::task::JoinHandle;

use crate::matching::{self, Mark};

/// How long a notice stays in the footer.
pub const NOTICE_FOR: Duration = Duration::from_secs(6);
/// How many rows around the selection get their library status looked up.
const LOOKUP_WINDOW: usize = 30;
/// Imported albums listed under Review.
pub const RECENT_IMPORTS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected { health: Health, soulseek: Option<SoulseekStatus> },
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchState {
    Idle,
    Running { query: String, started: Instant, timeout: Duration, peers: u32 },
    Done { query: String, peers: u32 },
    Failed(String),
}

/// The three places the TUI can be, like the web app's first tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Search,
    Downloads,
    Review,
}

/// Something fetched, as far as it has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loadable<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// An artist or album opened from Search, stacked over the results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Artist { name: String, info: Loadable<Box<ArtistInfo>>, selected: usize },
    Album { artist: Option<String>, title: String, info: Loadable<Box<AlbumInfo>>, selected: usize },
}

impl Page {
    fn selected_mut(&mut self) -> &mut usize {
        match self {
            Self::Artist { selected, .. } | Self::Album { selected, .. } => selected,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Artist { info: Loadable::Ready(info), .. } => info.albums.len(),
            Self::Album { info: Loadable::Ready(info), .. } => info.tracks.len(),
            _ => 0,
        }
    }
}

/// An artist or album that matched the search in the music catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogItem {
    Artist(ArtistHit),
    Album(AlbumHit),
}

/// A question waiting for y or n.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub prompt: String,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Catalog,
    Results,
    Page,
}

/// A short message about the last action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub bad: bool,
    pub at: Instant,
}

/// Download speed, smoothed between updates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    bytes: u64,
    at: Instant,
    pub per_sec: f64,
}

/// A group on the Downloads screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Active,
    Waiting,
    Attention,
    Review,
    Imported,
}

impl Group {
    #[must_use]
    pub const fn of(job: &DownloadJob) -> Self {
        match job.status {
            JobStatus::Downloading => Self::Active,
            JobStatus::Queued => Self::Waiting,
            JobStatus::Failed | JobStatus::Cancelled => Self::Attention,
            JobStatus::Ready => Self::Review,
            JobStatus::Imported => Self::Imported,
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Active => "Downloading",
            Self::Waiting => "Waiting",
            Self::Attention => "Needs attention",
            Self::Review => "Ready for review",
            Self::Imported => "Imported",
        }
    }
}

/// What came back from the background.
#[derive(Debug)]
pub enum Message {
    Status(Result<(Health, Option<SoulseekStatus>), String>),
    Me(Option<Me>),
    Search(SearchEvent),
    SearchError(String),
    Catalog(String, Result<MusicSearch, String>),
    Library(String, LibraryMatch),
    Artist(String, Result<ArtistInfo, String>),
    Album(String, Result<AlbumInfo, String>),
    Jobs(Vec<DownloadJob>),
    Report(String, Result<ReviewReport, String>),
    Notice(Result<String, String>),
    /// The server stopped accepting this session.
    SignedOut,
}

/// Something that needs the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    StartSearch(String),
    Catalog(String),
    Download(Box<Candidate>),
    Request(Box<Candidate>),
    Stop(String),
    Resume(String),
    Remove(String),
    Import(String),
    Prioritise(String),
    LoadReport(String),
    Library { key: String, artist: Option<String>, album: String, context: String },
    Artist(String),
    Album { key: String, artist: Option<String>, title: String },
}

/// All state the UI renders from.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools, reason = "independent flags the screens read")]
pub struct App {
    pub server_url: String,
    pub connection: Connection,
    pub me: Option<Me>,
    pub screen: Screen,
    pub focus: Focus,
    pub query: String,
    pub search: SearchState,
    /// Sorted best-first.
    pub results: Vec<Candidate>,
    /// What a pasted link turned out to be.
    pub resolved: Option<ResolvedLink>,
    pub selected: usize,
    pub catalog: Vec<CatalogItem>,
    pub catalog_selected: usize,
    /// Library matches by [`library_key`]; `None` while being looked up.
    pub library: HashMap<String, Option<LibraryMatch>>,
    pub pages: Vec<Page>,
    /// Every download job the server shows us, newest first.
    pub jobs: Vec<DownloadJob>,
    pub rates: HashMap<String, Rate>,
    pub jobs_loaded: bool,
    pub job_selected: usize,
    pub show_imported: bool,
    pub review_selected: usize,
    pub report_scroll: u16,
    pub reports: HashMap<String, Loadable<Box<ReviewReport>>>,
    pub notice: Option<Notice>,
    pub confirm: Option<Confirm>,
    pub help: bool,
    pub should_quit: bool,
    pub signed_out: bool,
    /// Work to start that didn't come from a key press.
    pub effects: Vec<Action>,
    pub(crate) search_task: Option<JoinHandle<()>>,
}

/// The key for a library lookup.
#[must_use]
pub fn library_key(artist: Option<&str>, album: &str) -> String {
    format!("{}\u{1f}{}", matching::title_key(artist.unwrap_or("")), matching::title_key(album))
}

fn page_key(artist: Option<&str>, title: &str) -> String {
    format!("{}\u{1f}{}", artist.unwrap_or(""), title)
}

impl App {
    #[must_use]
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            connection: Connection::Connecting,
            me: None,
            screen: Screen::Search,
            focus: Focus::Input,
            query: String::new(),
            search: SearchState::Idle,
            results: Vec::new(),
            resolved: None,
            selected: 0,
            catalog: Vec::new(),
            catalog_selected: 0,
            library: HashMap::new(),
            pages: Vec::new(),
            jobs: Vec::new(),
            rates: HashMap::new(),
            jobs_loaded: false,
            job_selected: 0,
            show_imported: false,
            review_selected: 0,
            report_scroll: 0,
            reports: HashMap::new(),
            notice: None,
            confirm: None,
            help: false,
            should_quit: false,
            signed_out: false,
            effects: Vec::new(),
            search_task: None,
        }
    }

    // ----- Derived views -----

    #[must_use]
    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.results.get(self.selected)
    }

    /// The library's copy of a search result's album, once looked up.
    #[must_use]
    pub fn library_for(&self, candidate: &Candidate) -> Option<&LibraryMatch> {
        let artist = matching::artist_from_folder(candidate.parent.as_deref());
        self.library.get(&library_key(artist.as_deref(), &candidate.title))?.as_ref()
    }

    #[must_use]
    pub fn mark_for(&self, candidate: &Candidate) -> Option<Mark> {
        matching::mark(candidate, &self.jobs, self.library_for(candidate))
    }

    /// Whether this person may start downloads (unknown counts as yes; the server decides).
    #[must_use]
    pub fn can_download(&self) -> bool {
        self.me.as_ref().is_none_or(|me| me.permissions.download)
    }

    #[must_use]
    pub fn manages(&self) -> bool {
        self.me.as_ref().is_some_and(|me| me.permissions.manage)
    }

    /// Whether this person can import `job` themselves.
    #[must_use]
    pub fn can_import(&self, job: &DownloadJob) -> bool {
        let Some(me) = &self.me else { return true };
        me.permissions.manage || (me.can_import && job.requested_by.as_deref().is_none_or(|by| by == me.username))
    }

    /// The Downloads screen's jobs, grouped and in order.
    #[must_use]
    pub fn download_list(&self) -> Vec<&DownloadJob> {
        let mut jobs: Vec<&DownloadJob> =
            self.jobs.iter().filter(|j| self.show_imported || j.status != JobStatus::Imported).collect();
        jobs.sort_by_key(|j| Group::of(j));
        jobs
    }

    /// The Review screen's list: waiting first, then recent imports.
    #[must_use]
    pub fn review_list(&self) -> Vec<&DownloadJob> {
        let mut waiting: Vec<&DownloadJob> = self.jobs.iter().filter(|j| j.status == JobStatus::Ready).collect();
        let mut imported: Vec<&DownloadJob> = self.jobs.iter().filter(|j| j.status == JobStatus::Imported).collect();
        imported.sort_by_key(|j| std::cmp::Reverse(j.imported_at.unwrap_or(j.created_at)));
        imported.truncate(RECENT_IMPORTS);
        waiting.extend(imported);
        waiting
    }

    /// How many downloads wait for review.
    #[must_use]
    pub fn waiting_for_review(&self) -> usize {
        self.jobs.iter().filter(|j| j.status == JobStatus::Ready).count()
    }

    #[must_use]
    pub fn active_downloads(&self) -> usize {
        self.jobs.iter().filter(|j| matching::in_flight(j)).count()
    }

    #[must_use]
    pub fn current_notice(&self) -> Option<&Notice> {
        self.notice.as_ref().filter(|n| n.at.elapsed() < NOTICE_FOR)
    }

    fn say(&mut self, text: impl Into<String>, bad: bool) {
        self.notice = Some(Notice { text: text.into(), bad, at: Instant::now() });
    }

    fn ask(&mut self, prompt: impl Into<String>, action: Action) {
        self.confirm = Some(Confirm { prompt: prompt.into(), action });
    }

    // ----- Keys -----

    pub fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Action::None;
        }
        if let Some(confirm) = self.confirm.take() {
            return match code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => confirm.action,
                _ => Action::None,
            };
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        let typing = self.screen == Screen::Search && self.focus == Focus::Input;
        if !typing {
            match code {
                KeyCode::Char('1') => {
                    self.screen = Screen::Search;
                    return Action::None;
                }
                KeyCode::Char('2') => {
                    self.screen = Screen::Downloads;
                    return Action::None;
                }
                KeyCode::Char('3') => {
                    self.screen = Screen::Review;
                    return self.report_to_load();
                }
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Action::None;
                }
                KeyCode::Char('?') => {
                    self.help = true;
                    return Action::None;
                }
                KeyCode::Char('/') => {
                    self.screen = Screen::Search;
                    self.focus = Focus::Input;
                    return Action::None;
                }
                _ => {}
            }
        }
        match self.screen {
            Screen::Search => self.on_search_key(code, modifiers),
            Screen::Downloads => self.on_downloads_key(code),
            Screen::Review => self.on_review_key(code),
        }
    }

    fn on_search_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        match self.focus {
            Focus::Input => self.on_input_key(code, modifiers),
            Focus::Results => self.on_results_key(code),
            Focus::Catalog => self.on_catalog_key(code),
            Focus::Page => self.on_page_key(code),
        }
    }

    fn on_input_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Enter if !self.query.trim().is_empty() => return Action::StartSearch(self.query.trim().into()),
            KeyCode::Esc | KeyCode::Down | KeyCode::Tab if !self.pages.is_empty() => self.focus = Focus::Page,
            KeyCode::Esc if self.query.is_empty() => self.should_quit = true,
            KeyCode::Esc => self.query.clear(),
            KeyCode::Down | KeyCode::Tab if !self.results.is_empty() => self.focus = Focus::Results,
            KeyCode::Down | KeyCode::Tab if !self.catalog.is_empty() => self.focus = Focus::Catalog,
            KeyCode::Char('u') if ctrl => self.query.clear(),
            KeyCode::Char('w') if ctrl => {
                let kept = self.query.trim_end().rsplit_once(' ').map_or("", |(head, _)| head).len();
                self.query.truncate(if kept == 0 { 0 } else { kept + 1 });
            }
            KeyCode::Backspace => {
                self.query.pop();
            }
            KeyCode::Char(c) if !ctrl => self.query.push(c),
            _ => {}
        }
        Action::None
    }

    fn on_results_key(&mut self, code: KeyCode) -> Action {
        let last = self.results.len().saturating_sub(1);
        match code {
            KeyCode::Char('d') => return self.download_selected(),
            KeyCode::Enter | KeyCode::Char('o') => {
                if let Some(c) = self.selected_candidate() {
                    let artist = matching::artist_from_folder(c.parent.as_deref());
                    let title = c.title.clone();
                    return self.open_album(artist, title);
                }
            }
            KeyCode::Char('a') => {
                if let Some(artist) =
                    self.selected_candidate().and_then(|c| matching::artist_from_folder(c.parent.as_deref()))
                {
                    return self.open_artist(artist);
                }
                self.say("This folder doesn't say who the artist is.", true);
            }
            KeyCode::Tab | KeyCode::Left | KeyCode::Char('h') if !self.catalog.is_empty() => {
                self.focus = Focus::Catalog;
            }
            KeyCode::Esc | KeyCode::Tab => self.focus = Focus::Input,
            KeyCode::Up | KeyCode::Char('k') if self.selected == 0 => self.focus = Focus::Input,
            KeyCode::Up | KeyCode::Char('k') => self.selected -= 1,
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::PageDown => self.selected = (self.selected + 10).min(last),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            _ => {}
        }
        Action::None
    }

    fn on_catalog_key(&mut self, code: KeyCode) -> Action {
        let last = self.catalog.len().saturating_sub(1);
        match code {
            KeyCode::Up | KeyCode::Char('k') if self.catalog_selected == 0 => self.focus = Focus::Input,
            KeyCode::Up | KeyCode::Char('k') => self.catalog_selected -= 1,
            KeyCode::Down | KeyCode::Char('j') => self.catalog_selected = (self.catalog_selected + 1).min(last),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if !self.results.is_empty() => {
                self.focus = Focus::Results;
            }
            KeyCode::Tab | KeyCode::Esc => self.focus = Focus::Input,
            KeyCode::Enter | KeyCode::Char('o') => match self.catalog.get(self.catalog_selected).cloned() {
                Some(CatalogItem::Artist(a)) => return self.open_artist(a.name),
                Some(CatalogItem::Album(a)) => return self.open_album(Some(a.artist), a.title),
                None => {}
            },
            _ => {}
        }
        Action::None
    }

    fn on_page_key(&mut self, code: KeyCode) -> Action {
        let Some(page) = self.pages.last_mut() else {
            self.focus = Focus::Input;
            return Action::None;
        };
        let last = page.len().saturating_sub(1);
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                let selected = page.selected_mut();
                *selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let selected = page.selected_mut();
                *selected = (*selected + 1).min(last);
            }
            KeyCode::Home | KeyCode::Char('g') => *page.selected_mut() = 0,
            KeyCode::End | KeyCode::Char('G') => *page.selected_mut() = last,
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                self.pages.pop();
                if self.pages.is_empty() {
                    self.focus = if self.results.is_empty() {
                        if self.catalog.is_empty() { Focus::Input } else { Focus::Catalog }
                    } else {
                        Focus::Results
                    };
                }
            }
            KeyCode::Tab => self.focus = Focus::Input,
            KeyCode::Enter | KeyCode::Char('o') => {
                if let Page::Artist { name, info: Loadable::Ready(info), selected } = page
                    && let Some(album) = info.albums.get(*selected)
                {
                    let (artist, title) = (name.clone(), album.title.clone());
                    return self.open_album(Some(artist), title);
                }
                if let Page::Album { .. } = page {
                    return self.search_page();
                }
            }
            KeyCode::Char('s' | 'd') => return self.search_page(),
            KeyCode::Char('a') => {
                if let Page::Album { artist: Some(artist), .. } = page {
                    let artist = artist.clone();
                    return self.open_artist(artist);
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Search Soulseek for what the open page shows.
    fn search_page(&mut self) -> Action {
        let query = match self.pages.last() {
            Some(Page::Artist { name, info: Loadable::Ready(info), selected }) => {
                info.albums.get(*selected).map(|a| format!("{name} {}", a.title))
            }
            Some(Page::Album { artist, title, .. }) => {
                Some(artist.as_deref().map_or_else(|| title.clone(), |a| format!("{a} {title}")))
            }
            _ => None,
        };
        query.map_or(Action::None, |q| {
            self.query.clone_from(&q);
            Action::StartSearch(q)
        })
    }

    fn open_artist(&mut self, name: String) -> Action {
        self.pages.push(Page::Artist { name: name.clone(), info: Loadable::Loading, selected: 0 });
        self.focus = Focus::Page;
        Action::Artist(name)
    }

    fn open_album(&mut self, artist: Option<String>, title: String) -> Action {
        self.pages.push(Page::Album {
            artist: artist.clone(),
            title: title.clone(),
            info: Loadable::Loading,
            selected: 0,
        });
        self.focus = Focus::Page;
        Action::Album { key: page_key(artist.as_deref(), &title), artist, title }
    }

    fn download_selected(&mut self) -> Action {
        let Some(candidate) = self.selected_candidate().cloned() else { return Action::None };
        let title = candidate.title.clone();
        if !self.can_download() {
            if self.me.as_ref().is_some_and(|me| me.permissions.request) {
                self.ask(format!("Ask an admin for “{title}”?"), Action::Request(Box::new(candidate)));
            } else {
                self.say("Your account can't start downloads. Ask an admin.", true);
            }
            return Action::None;
        }
        let download = Action::Download(Box::new(candidate.clone()));
        match self.mark_for(&candidate) {
            Some(Mark::Downloading(_) | Mark::Queued) => {
                self.say("Already downloading this copy. Press 2 to watch it.", false);
            }
            Some(Mark::Review) => self.say("Already downloaded. It's waiting in Review (3).", false),
            Some(Mark::InLibrary | Mark::Imported) => {
                let quality = self
                    .library_for(&candidate)
                    .and_then(|l| l.quality_label.clone())
                    .map(|q| format!(" as {q}"))
                    .unwrap_or_default();
                self.ask(format!("“{title}” is already in your library{quality}. Download it again?"), download);
            }
            Some(Mark::Partial(owned)) => {
                // Only what the library is missing, with the artwork and other extras.
                let mut missing = candidate.clone();
                if let Some(library) = self.library_for(&candidate) {
                    let titles: Vec<String> = library.tracks.iter().map(|t| matching::title_key(&t.title)).collect();
                    missing.files.retain(|f| !f.audio || !matching::owns(&titles, &f.name));
                }
                let count = owned.total - owned.owned;
                missing.audio_files = u32::try_from(count).unwrap_or(u32::MAX);
                return {
                    self.say(
                        format!(
                            "You have {} of these {} songs; getting the {count} missing.",
                            owned.owned, owned.total
                        ),
                        false,
                    );
                    Action::Download(Box::new(missing))
                };
            }
            Some(Mark::OtherCopy) => {
                self.ask(format!("Another copy of “{title}” is already downloading. Get this one too?"), download);
            }
            Some(Mark::Failed) | None => return download,
        }
        Action::None
    }

    fn on_downloads_key(&mut self, code: KeyCode) -> Action {
        let shown = self.download_list();
        let last = shown.len().saturating_sub(1);
        let job = shown.get(self.job_selected).map(|j| (*j).clone());
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.job_selected = self.job_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.job_selected = (self.job_selected + 1).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.job_selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.job_selected = last,
            KeyCode::PageDown => self.job_selected = (self.job_selected + 10).min(last),
            KeyCode::PageUp => self.job_selected = self.job_selected.saturating_sub(10),
            KeyCode::Esc => {
                self.screen = Screen::Search;
            }
            KeyCode::Char('i') => {
                self.show_imported = !self.show_imported;
                self.job_selected = self.job_selected.min(self.download_list().len().saturating_sub(1));
            }
            KeyCode::Char('s' | ' ') => {
                if let Some(job) = job {
                    let fetched = job.folder.is_empty();
                    return match job.status {
                        JobStatus::Queued | JobStatus::Downloading => Action::Stop(job.id),
                        JobStatus::Failed | JobStatus::Cancelled if fetched => {
                            self.say("Fetched downloads can't resume. Press f to find another copy.", true);
                            Action::None
                        }
                        JobStatus::Failed | JobStatus::Cancelled => Action::Resume(job.id),
                        _ => Action::None,
                    };
                }
            }
            KeyCode::Char('p') => {
                if let Some(job) = job {
                    if job.waiting_for_slot.is_some() {
                        return Action::Prioritise(job.id);
                    }
                    self.say("Only downloads waiting for a turn can go first.", true);
                }
            }
            KeyCode::Char('x' | 'D') | KeyCode::Delete => {
                if let Some(job) = job {
                    let what = if job.status == JobStatus::Imported {
                        format!("Forget “{}”? The imported album stays in your library.", job.title)
                    } else {
                        format!("Remove “{}” and its downloaded files?", job.title)
                    };
                    self.ask(what, Action::Remove(job.id));
                }
            }
            KeyCode::Char('f') => {
                if let Some(job) = job {
                    return self.find_another(&job);
                }
            }
            KeyCode::Enter | KeyCode::Char('r') => {
                if let Some(job) = job.filter(|j| matches!(j.status, JobStatus::Ready | JobStatus::Imported)) {
                    self.screen = Screen::Review;
                    self.review_selected = self.review_list().iter().position(|j| j.id == job.id).unwrap_or(0);
                    self.report_scroll = 0;
                    return self.report_to_load();
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Search again for a download's album.
    fn find_another(&mut self, job: &DownloadJob) -> Action {
        let artist = matching::artist_from_folder(job.parent.as_deref());
        let query = artist.map_or_else(|| job.title.clone(), |a| format!("{a} {}", job.title));
        self.screen = Screen::Search;
        self.query.clone_from(&query);
        Action::StartSearch(query)
    }

    fn on_review_key(&mut self, code: KeyCode) -> Action {
        let shown: Vec<DownloadJob> = self.review_list().into_iter().cloned().collect();
        let last = shown.len().saturating_sub(1);
        let job = shown.get(self.review_selected).cloned();
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.review_selected = self.review_selected.saturating_sub(1);
                self.report_scroll = 0;
                return self.report_to_load();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.review_selected = (self.review_selected + 1).min(last);
                self.report_scroll = 0;
                return self.report_to_load();
            }
            KeyCode::PageDown | KeyCode::Char('J') => self.report_scroll = self.report_scroll.saturating_add(10),
            KeyCode::PageUp | KeyCode::Char('K') => self.report_scroll = self.report_scroll.saturating_sub(10),
            KeyCode::Esc => self.screen = Screen::Search,
            KeyCode::Char('i' | 'a') => {
                let Some(job) = job.filter(|j| j.status == JobStatus::Ready) else { return Action::None };
                if !self.can_import(&job) {
                    self.say("An admin needs to approve this before it goes into the library.", true);
                    return Action::None;
                }
                let blocked = match self.reports.get(&job.id) {
                    Some(Loadable::Ready(report)) => report.blocked_reason.clone(),
                    Some(Loadable::Failed(message)) => Some(message.clone()),
                    _ => Some("The files are still being checked.".into()),
                };
                if let Some(reason) = blocked {
                    self.say(reason, true);
                } else {
                    let replaces = match self.reports.get(&job.id) {
                        Some(Loadable::Ready(report)) if !report.conflicts.is_empty() => {
                            format!(" It replaces {} file(s) already there.", report.conflicts.len())
                        }
                        _ => String::new(),
                    };
                    self.ask(format!("Import “{}” into the library?{replaces}", job.title), Action::Import(job.id));
                }
            }
            KeyCode::Char('x' | 'D') | KeyCode::Delete => {
                if let Some(job) = job {
                    let what = if job.status == JobStatus::Imported {
                        format!("Forget “{}”? The album stays in your library.", job.title)
                    } else {
                        format!("Discard “{}” and its downloaded files?", job.title)
                    };
                    self.ask(what, Action::Remove(job.id));
                }
            }
            KeyCode::Char('f') => {
                if let Some(job) = job {
                    return self.find_another(&job);
                }
            }
            KeyCode::Char('R') => {
                if let Some(job) = job {
                    self.reports.remove(&job.id);
                    return self.report_to_load();
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Ask for the selected job's review report if we don't have it yet.
    pub fn report_to_load(&mut self) -> Action {
        let Some((id, review, status)) =
            self.review_list().get(self.review_selected).map(|j| (j.id.clone(), j.review, j.status))
        else {
            return Action::None;
        };
        if status == JobStatus::Ready && review == ReviewState::Ready && !self.reports.contains_key(&id) {
            self.reports.insert(id.clone(), Loadable::Loading);
            return Action::LoadReport(id);
        }
        Action::None
    }

    /// Library lookups for results near the selection that haven't been looked up.
    pub fn lookups_wanted(&mut self) -> Vec<Action> {
        if self.results.is_empty() {
            return Vec::new();
        }
        let context = match &self.search {
            SearchState::Running { query, .. } | SearchState::Done { query, .. } => query.clone(),
            _ => self.query.clone(),
        };
        let start = self.selected.saturating_sub(LOOKUP_WINDOW);
        let end = (self.selected + LOOKUP_WINDOW).min(self.results.len());
        let mut wanted = Vec::new();
        for candidate in &self.results[start..end] {
            let artist = matching::artist_from_folder(candidate.parent.as_deref());
            let key = library_key(artist.as_deref(), &candidate.title);
            if self.library.contains_key(&key) {
                continue;
            }
            self.library.insert(key.clone(), None);
            wanted.push(Action::Library { key, artist, album: candidate.title.clone(), context: context.clone() });
        }
        wanted
    }

    // ----- Messages -----

    pub fn on_message(&mut self, msg: Message) {
        match msg {
            Message::Status(Ok((health, soulseek))) => self.connection = Connection::Connected { health, soulseek },
            Message::Status(Err(e)) => self.connection = Connection::Unreachable(e),
            Message::Me(me) => self.me = me,
            Message::SearchError(e) => self.search = SearchState::Failed(e),
            Message::Search(event) => self.on_search_event(event),
            Message::Catalog(query, result) => {
                let current = match &self.search {
                    SearchState::Running { query, .. } | SearchState::Done { query, .. } => query.as_str(),
                    _ => self.query.as_str(),
                };
                if query != current {
                    return;
                }
                if let Ok(found) = result {
                    self.catalog = found
                        .artists
                        .into_iter()
                        .take(3)
                        .map(CatalogItem::Artist)
                        .chain(found.albums.into_iter().take(8).map(CatalogItem::Album))
                        .collect();
                    self.catalog_selected = 0;
                    if self.focus == Focus::Catalog && self.catalog.is_empty() {
                        self.focus = Focus::Input;
                    }
                }
            }
            Message::Library(key, found) => {
                self.library.insert(key, Some(found));
            }
            Message::Artist(name, result) => {
                for page in &mut self.pages {
                    if let Page::Artist { name: n, info, .. } = page
                        && *n == name
                    {
                        *info = match &result {
                            Ok(found) => Loadable::Ready(Box::new(found.clone())),
                            Err(e) => Loadable::Failed(e.clone()),
                        };
                    }
                }
            }
            Message::Album(key, result) => {
                for page in &mut self.pages {
                    if let Page::Album { artist, title, info, .. } = page
                        && page_key(artist.as_deref(), title) == key
                    {
                        *info = match &result {
                            Ok(found) => Loadable::Ready(Box::new(found.clone())),
                            Err(e) => Loadable::Failed(e.clone()),
                        };
                    }
                }
            }
            Message::Jobs(jobs) => self.on_jobs(jobs),
            Message::Report(id, result) => {
                let report = result.map_or_else(Loadable::Failed, |r| Loadable::Ready(Box::new(r)));
                self.reports.insert(id, report);
            }
            Message::Notice(Ok(text)) => self.say(text, false),
            Message::Notice(Err(text)) => self.say(text, true),
            Message::SignedOut => self.signed_out = true,
        }
    }

    fn on_jobs(&mut self, jobs: Vec<DownloadJob>) {
        let now = Instant::now();
        let selected_id = self.download_list().get(self.job_selected).map(|j| j.id.clone());
        let reviewing_id = self.review_list().get(self.review_selected).map(|j| j.id.clone());
        for job in &jobs {
            // Reviews that were redone need fetching again.
            let stale = job.review != ReviewState::Ready
                || self.jobs.iter().any(|old| old.id == job.id && old.review != ReviewState::Ready);
            if stale && !matches!(self.reports.get(&job.id), Some(Loadable::Loading)) {
                self.reports.remove(&job.id);
            }
            if matching::in_flight(job) {
                let rate = self.rates.get(&job.id).copied();
                let per_sec = rate.map_or(0.0, |r| {
                    let secs = now.duration_since(r.at).as_secs_f64();
                    if secs < 0.5 {
                        return r.per_sec;
                    }
                    #[allow(clippy::cast_precision_loss)] // display only
                    let instant = job.bytes.saturating_sub(r.bytes) as f64 / secs;
                    if r.per_sec == 0.0 { instant } else { r.per_sec.mul_add(0.6, instant * 0.4) }
                });
                if rate.is_none_or(|r| now.duration_since(r.at).as_secs_f64() >= 0.5) {
                    self.rates.insert(job.id.clone(), Rate { bytes: job.bytes, at: now, per_sec });
                }
            } else {
                self.rates.remove(&job.id);
            }
        }
        self.jobs = jobs;
        self.jobs_loaded = true;
        // Keep the same job selected as the lists reorder.
        let list = self.download_list();
        let index = selected_id.and_then(|id| list.iter().position(|j| j.id == id));
        let len = list.len();
        self.job_selected = index.unwrap_or(self.job_selected).min(len.saturating_sub(1));
        let review = self.review_list();
        let index = reviewing_id.and_then(|id| review.iter().position(|j| j.id == id));
        let len = review.len();
        self.review_selected = index.unwrap_or(self.review_selected).min(len.saturating_sub(1));
    }

    pub fn on_search_event(&mut self, event: SearchEvent) {
        match event {
            SearchEvent::Resolved { link } => self.resolved = Some(*link),
            SearchEvent::Started { query, timeout_secs } => {
                let peers = match self.search {
                    SearchState::Running { peers, .. } => peers,
                    _ => 0,
                };
                self.search = SearchState::Running {
                    query,
                    started: Instant::now(),
                    timeout: Duration::from_secs(timeout_secs.into()),
                    peers,
                };
            }
            SearchEvent::Candidates { items } => {
                if let SearchState::Running { peers, .. } = &mut self.search {
                    *peers += 1;
                }
                // Keep the same candidate selected while new results slot in around it.
                let selected_id = self.selected_candidate().map(|c| c.id.clone());
                self.results.extend(items);
                Candidate::rank(&mut self.results);
                if let Some(id) = selected_id {
                    self.selected = self.results.iter().position(|c| c.id == id).unwrap_or(0);
                }
            }
            SearchEvent::Finished { peers, .. } => {
                let query = match &self.search {
                    SearchState::Running { query, .. } => query.clone(),
                    _ => String::new(),
                };
                self.search = SearchState::Done { query, peers };
            }
            SearchEvent::Failed { error } => self.search = SearchState::Failed(error.message),
        }
    }

    /// Reset for a new search; the caller starts the stream.
    pub fn begin_search(&mut self, query: &str) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        query.clone_into(&mut self.query);
        self.results.clear();
        self.catalog.clear();
        self.catalog_selected = 0;
        self.pages.clear();
        self.resolved = None;
        self.selected = 0;
        self.screen = Screen::Search;
        self.focus = Focus::Input;
        self.search = SearchState::Running {
            query: query.to_owned(),
            started: Instant::now(),
            timeout: Duration::from_secs(20),
            peers: 0,
        };
    }
}

/// Whether a search looks like a pasted link rather than words.
#[must_use]
pub fn is_link(query: &str) -> bool {
    query.contains("://") || query.starts_with("www.")
}
