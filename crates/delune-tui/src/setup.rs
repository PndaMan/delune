//! `delune setup`: a guided first run in the terminal.
//!
//! One screen per question — where things live, Navidrome, Soulseek, the network,
//! and whether to run as a service — with each connection tried before moving on.
//! The wizard only gathers answers; the `delune` binary checks them (through the
//! [`CheckFn`] it hands in) and writes the files, so this crate stays a client.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Padding, Paragraph, Wrap},
};
use tokio::task::JoinHandle;

const ACCENT: Color = Color::Rgb(0xae, 0xb8, 0xff);
const TEXT: Color = Color::Rgb(0xe4, 0xe8, 0xf4);
const MUTED: Color = Color::Rgb(0x8b, 0x91, 0xa8);
const FAINT: Color = Color::Rgb(0x4a, 0x51, 0x68);
const OK: Color = Color::Rgb(0x6f, 0xd3, 0x9b);
const ERR: Color = Color::Rgb(0xff, 0x7a, 0x85);
const FIELD_BG: Color = Color::Rgb(0x1c, 0x21, 0x33);
const FOCUS_BG: Color = Color::Rgb(0x2a, 0x31, 0x47);

/// Everything the wizard asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answers {
    pub data_dir: String,
    pub library_dir: String,
    pub navidrome_url: String,
    pub navidrome_username: String,
    pub navidrome_password: String,
    pub soulseek_username: String,
    pub soulseek_password: String,
    pub soulseek_port: String,
    pub bind: String,
    pub upnp: bool,
    pub service: Service,
    pub start_now: bool,
}

/// How delune should keep running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Service {
    /// A systemd service for this user, started at login.
    #[default]
    User,
    /// A system-wide systemd service (needs root).
    System,
    /// Nothing; run `delune serve` yourself.
    None,
}

/// A part of the setup that can be tried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Folders,
    Navidrome,
    Soulseek,
}

/// What trying a part found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub ok: bool,
    pub message: String,
}

pub type CheckFuture = Pin<Box<dyn Future<Output = Check> + Send>>;
/// Tries a part of the answers; supplied by the caller.
pub type CheckFn = Arc<dyn Fn(Part, Answers) -> CheckFuture + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Folders,
    Navidrome,
    Soulseek,
    Network,
    Service,
    Review,
}

const STEPS: [Step; 7] =
    [Step::Welcome, Step::Folders, Step::Navidrome, Step::Soulseek, Step::Network, Step::Service, Step::Review];

impl Step {
    const fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Folders => "Folders",
            Self::Navidrome => "Navidrome",
            Self::Soulseek => "Soulseek",
            Self::Network => "Network",
            Self::Service => "Keep it running",
            Self::Review => "Finish",
        }
    }

    const fn part(self) -> Option<Part> {
        match self {
            Self::Folders => Some(Part::Folders),
            Self::Navidrome => Some(Part::Navidrome),
            Self::Soulseek => Some(Part::Soulseek),
            _ => None,
        }
    }
}

/// One question on a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    DataDir,
    LibraryDir,
    NavidromeUrl,
    NavidromeUsername,
    NavidromePassword,
    SoulseekUsername,
    SoulseekPassword,
    SoulseekPort,
    Bind,
    Upnp,
    Service,
    StartNow,
}

impl Field {
    const fn label(self) -> &'static str {
        match self {
            Self::DataDir => "delune's own data",
            Self::LibraryDir => "Your music folder",
            Self::NavidromeUrl => "Address",
            Self::NavidromeUsername | Self::SoulseekUsername => "Username",
            Self::NavidromePassword | Self::SoulseekPassword => "Password",
            Self::SoulseekPort => "Listening port",
            Self::Bind => "Web UI address",
            Self::Upnp => "Open the port on my router (UPnP)",
            Self::Service => "Run delune as",
            Self::StartNow => "Start it now",
        }
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::DataDir => "Downloads awaiting review, settings, the database.",
            Self::LibraryDir => "The folder Navidrome scans; imports land here.",
            Self::NavidromeUrl => "Such as http://localhost:4533. Empty skips Navidrome.",
            Self::NavidromeUsername => "An admin account: library rescans need one.",
            Self::SoulseekUsername => "New names are registered on first sign-in. Empty skips it.",
            Self::SoulseekPort => "Peers connect here; forwarding it brings more results.",
            Self::Bind => "0.0.0.0:7474 reaches every device on your network.",
            Self::Upnp => "Asks the router to forward the Soulseek port.",
            Self::Service => "Space to change.",
            Self::NavidromePassword | Self::SoulseekPassword | Self::StartNow => "",
        }
    }

    const fn toggle(self) -> bool {
        matches!(self, Self::Upnp | Self::Service | Self::StartNow)
    }

    fn fields(step: Step) -> &'static [Self] {
        match step {
            Step::Welcome | Step::Review => &[],
            Step::Folders => &[Self::DataDir, Self::LibraryDir],
            Step::Navidrome => &[Self::NavidromeUrl, Self::NavidromeUsername, Self::NavidromePassword],
            Step::Soulseek => &[Self::SoulseekUsername, Self::SoulseekPassword, Self::SoulseekPort],
            Step::Network => &[Self::Bind, Self::Upnp],
            Step::Service => &[Self::Service, Self::StartNow],
        }
    }
}

struct Wizard {
    answers: Answers,
    step: usize,
    focus: usize,
    check: CheckFn,
    running: Option<(Step, Instant, JoinHandle<Check>)>,
    results: Vec<(Step, Check)>,
    done: Option<bool>,
}

impl Wizard {
    fn step(&self) -> Step {
        STEPS[self.step]
    }

    fn text(&mut self, field: Field) -> Option<&mut String> {
        let a = &mut self.answers;
        Some(match field {
            Field::DataDir => &mut a.data_dir,
            Field::LibraryDir => &mut a.library_dir,
            Field::NavidromeUrl => &mut a.navidrome_url,
            Field::NavidromeUsername => &mut a.navidrome_username,
            Field::NavidromePassword => &mut a.navidrome_password,
            Field::SoulseekUsername => &mut a.soulseek_username,
            Field::SoulseekPassword => &mut a.soulseek_password,
            Field::SoulseekPort => &mut a.soulseek_port,
            Field::Bind => &mut a.bind,
            Field::Upnp | Field::Service | Field::StartNow => return None,
        })
    }

    fn value(&self, field: Field) -> String {
        let a = &self.answers;
        let on = |b: bool| if b { "● Yes" } else { "○ No" }.to_owned();
        match field {
            Field::DataDir => a.data_dir.clone(),
            Field::LibraryDir => a.library_dir.clone(),
            Field::NavidromeUrl => a.navidrome_url.clone(),
            Field::NavidromeUsername => a.navidrome_username.clone(),
            Field::NavidromePassword => "•".repeat(a.navidrome_password.chars().count()),
            Field::SoulseekUsername => a.soulseek_username.clone(),
            Field::SoulseekPassword => "•".repeat(a.soulseek_password.chars().count()),
            Field::SoulseekPort => a.soulseek_port.clone(),
            Field::Bind => a.bind.clone(),
            Field::Upnp => on(a.upnp),
            Field::StartNow => on(a.start_now && a.service != Service::None),
            Field::Service => match a.service {
                Service::User => "◉ A service for you (systemd --user)".to_owned(),
                Service::System => "◉ A system service (needs root)".to_owned(),
                Service::None => "◉ Nothing, I'll run `delune serve` myself".to_owned(),
            },
        }
    }

    fn flip(&mut self, field: Field) {
        let a = &mut self.answers;
        match field {
            Field::Upnp => a.upnp = !a.upnp,
            Field::StartNow => a.start_now = !a.start_now,
            Field::Service => {
                a.service = match a.service {
                    Service::User => Service::System,
                    Service::System => Service::None,
                    Service::None => Service::User,
                };
            }
            _ => {}
        }
    }

    fn result(&self, step: Step) -> Option<&Check> {
        self.results.iter().find(|(s, _)| *s == step).map(|(_, c)| c)
    }

    /// Enter: try this screen's connection if it has one, then move on once it's good.
    fn advance(&mut self) {
        let step = self.step();
        if step == Step::Review {
            self.done = Some(true);
            return;
        }
        if let Some(part) = step.part() {
            let passed = self.result(step).is_some_and(|c| c.ok);
            if !passed {
                if self.running.is_none() {
                    self.results.retain(|(s, _)| *s != step);
                    let future = (self.check)(part, self.answers.clone());
                    self.running = Some((step, Instant::now(), tokio::spawn(future)));
                }
                return;
            }
        }
        self.step += 1;
        self.focus = 0;
    }

    fn back(&mut self) {
        if self.step > 0 {
            self.step -= 1;
            self.focus = 0;
        }
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c' | 'q')) {
            self.done = Some(false);
            return;
        }
        let fields = Field::fields(self.step());
        let field = fields.get(self.focus).copied();
        match code {
            KeyCode::Esc => self.back(),
            KeyCode::Enter => self.advance(),
            KeyCode::Down | KeyCode::Tab if !fields.is_empty() => self.focus = (self.focus + 1) % fields.len(),
            KeyCode::Up | KeyCode::BackTab if !fields.is_empty() => {
                self.focus = (self.focus + fields.len() - 1) % fields.len();
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if field.is_some_and(Field::toggle) => {
                if let Some(field) = field {
                    self.flip(field);
                }
            }
            KeyCode::Backspace => {
                if let Some(text) = field.and_then(|f| self.text(f)) {
                    text.pop();
                    self.forget_result();
                }
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(text) = field.and_then(|f| self.text(f)) {
                    text.push(c);
                    self.forget_result();
                }
            }
            _ => {}
        }
    }

    /// An edited answer needs trying again.
    fn forget_result(&mut self) {
        let step = self.step();
        self.results.retain(|(s, _)| *s != step);
    }

    fn poll(&mut self) {
        if self.running.as_ref().is_some_and(|(_, _, task)| task.is_finished()) {
            let Some((step, _, task)) = self.running.take() else { return };
            let check = futures_util::FutureExt::now_or_never(task)
                .and_then(Result::ok)
                .unwrap_or_else(|| Check { ok: false, message: "The check stopped unexpectedly.".into() });
            let ok = check.ok;
            self.results.push((step, check));
            if ok && self.step() == step {
                self.step += 1;
                self.focus = 0;
            }
        }
    }
}

/// Run the wizard. `None` when it was cancelled.
///
/// It blocks while waiting for keys, so call it from a blocking context inside a
/// multi-threaded Tokio runtime (checks run as tasks alongside).
///
/// # Errors
///
/// When the terminal can't be set up or read.
pub fn run(defaults: Answers, check: CheckFn) -> Result<Option<Answers>> {
    let mut wizard =
        Wizard { answers: defaults, step: 0, focus: 0, check, running: None, results: Vec::new(), done: None };
    let mut terminal = ratatui::init();
    let result = loop {
        wizard.poll();
        if let Err(e) = terminal.draw(|frame| draw(frame, &wizard)) {
            break Err(e.into());
        }
        let key = next_key();
        match key {
            Ok(Some((code, modifiers))) => wizard.on_key(code, modifiers),
            Ok(None) => {}
            Err(e) => break Err(e.into()),
        }
        match wizard.done {
            Some(true) => break Ok(Some(wizard.answers.clone())),
            Some(false) => break Ok(None),
            None => {}
        }
    };
    ratatui::restore();
    result
}

/// A key press, if one comes within a moment.
fn next_key() -> std::io::Result<Option<(KeyCode, KeyModifiers)>> {
    if event::poll(Duration::from_millis(80))?
        && let Event::Key(key) = event::read()?
        && key.kind == KeyEventKind::Press
    {
        return Ok(Some((key.code, key.modifiers)));
    }
    Ok(None)
}

/// The moon, waxing as the steps go by.
const PHASES: [&str; 7] = ["○", "◔", "◔", "◑", "◑", "◕", "●"];

fn draw(frame: &mut Frame<'_>, wizard: &Wizard) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(Color::Rgb(0x0f, 0x12, 0x26))), area);
    let [card] = Layout::horizontal([Constraint::Max(96)]).flex(Flex::Center).areas(area);
    let [card] = Layout::vertical([Constraint::Max(30)]).flex(Flex::Center).areas(card);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FAINT))
        .padding(Padding::new(2, 2, 1, 1))
        .title(Line::from(vec![" ☾ ".fg(ACCENT), "delune setup ".fg(TEXT).bold()]));
    let inner = block.inner(card);
    frame.render_widget(block, card);

    let [rail, body] = Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)]).spacing(3).areas(inner);
    draw_rail(frame, rail, wizard);
    let [content, footer] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(body);
    match wizard.step() {
        Step::Welcome => draw_welcome(frame, content),
        Step::Review => draw_review(frame, content, &wizard.answers),
        step => draw_form(frame, content, wizard, step),
    }
    draw_footer(frame, footer, wizard);
}

fn draw_rail(frame: &mut Frame<'_>, area: Rect, wizard: &Wizard) {
    let lines: Vec<Line> = STEPS
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let (mark, style) = match i.cmp(&wizard.step) {
                std::cmp::Ordering::Less => ("✓", Style::new().fg(OK)),
                std::cmp::Ordering::Equal => (PHASES[i], Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
                std::cmp::Ordering::Greater => (PHASES[i], Style::new().fg(FAINT)),
            };
            let label = if i == wizard.step { step.title().fg(TEXT).bold() } else { step.title().fg(MUTED) };
            Line::from(vec![Span::styled(format!("{mark}  "), style), label])
        })
        .flat_map(|line| [line, Line::default()])
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn heading<'a>(title: &'a str, intro: &'a str) -> Vec<Line<'a>> {
    vec![Line::from(title.fg(TEXT).bold()), Line::from(intro.fg(MUTED)), Line::default()]
}

fn draw_welcome(frame: &mut Frame<'_>, area: Rect) {
    // A crescent, lit on the right, drawn with half blocks.
    let moon = [
        "    ▄▄████▄▄   ",
        "  ▄█▀▀   ▀███▄ ",
        " ██        ███ ",
        " ██        ███ ",
        "  ▀█▄▄   ▄███▀ ",
        "    ▀▀████▀▀   ",
    ];
    let [art, words] = Layout::vertical([Constraint::Length(7), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Paragraph::new(moon.iter().map(|l| Line::from(l.fg(ACCENT))).collect::<Vec<_>>()), art);
    let mut lines = vec![Line::from("Let's get delune ready.".fg(TEXT).bold())];
    lines.push(Line::default());
    lines.push(Line::from(
        "A few questions: where your music lives, your Navidrome and Soulseek accounts, and how delune should \
         keep running. Each connection is tried before moving on, and anything can be changed later in Settings."
            .fg(MUTED),
    ));
    lines.push(Line::default());
    lines.push(Line::from(vec!["Press ".fg(MUTED), "Enter".fg(ACCENT).bold(), " to begin.".fg(MUTED)]));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), words);
}

/// The end of `value` when it's wider than `width`, since that's the part that differs.
fn tail(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    let keep: String = value.chars().skip(count + 1 - width).collect();
    format!("…{keep}")
}

fn draw_form(frame: &mut Frame<'_>, area: Rect, wizard: &Wizard, step: Step) {
    let intro = match step {
        Step::Folders => "Where delune keeps its things, and where your library is.",
        Step::Navidrome => "Sign-in uses Navidrome accounts, and imports trigger a library scan.",
        Step::Soulseek => "The network delune searches first.",
        Step::Network => "How the web UI and other Soulseek users reach this machine.",
        _ => "Run delune in the background, and after restarts.",
    };
    let fields = Field::fields(step);
    let mut constraints = vec![Constraint::Length(4)];
    constraints.extend(fields.iter().map(|_| Constraint::Length(4)));
    constraints.push(Constraint::Fill(1));
    let rows = Layout::vertical(constraints).split(area);
    frame.render_widget(Paragraph::new(heading(step.title(), intro)).wrap(Wrap { trim: true }), rows[0]);

    for (i, field) in fields.iter().enumerate() {
        let focused = i == wizard.focus;
        let [label, input, hint] = Layout::vertical([Constraint::Length(1); 3]).areas(rows[i + 1]);
        let label_style = if focused { Style::new().fg(ACCENT).bold() } else { Style::new().fg(TEXT) };
        frame.render_widget(Paragraph::new(field.label()).style(label_style), label);
        let mut value = wizard.value(*field);
        if focused && !field.toggle() {
            value.push('▏');
        }
        let bg = if focused { FOCUS_BG } else { FIELD_BG };
        frame.render_widget(
            Paragraph::new(format!(" {}", tail(&value, usize::from(input.width.saturating_sub(2)))))
                .style(Style::new().bg(bg).fg(if field.toggle() { ACCENT } else { TEXT })),
            input,
        );
        frame.render_widget(Paragraph::new(field.hint()).style(Style::new().fg(FAINT)), hint);
    }

    let status = rows[rows.len() - 1];
    let line = match (&wizard.running, wizard.result(step)) {
        (Some((s, started, _)), _) if *s == step => {
            let frames = ["◐", "◓", "◑", "◒"];
            let spin = frames[(started.elapsed().as_millis() / 150) as usize % frames.len()];
            Line::from(vec![format!("{spin} ").fg(ACCENT), "Trying it…".fg(MUTED)])
        }
        (_, Some(check)) if check.ok => Line::from(vec!["✓ ".fg(OK), check.message.as_str().fg(TEXT)]),
        (_, Some(check)) => Line::from(vec!["✗ ".fg(ERR), check.message.as_str().fg(ERR)]),
        _ => Line::default(),
    };
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), status);
}

fn draw_review(frame: &mut Frame<'_>, area: Rect, a: &Answers) {
    let or_skip = |value: &str| if value.trim().is_empty() { "not set".to_owned() } else { value.to_owned() };
    let row = |label: &'static str, value: String| Line::from(vec![format!("{label:<18}").fg(MUTED), value.fg(TEXT)]);
    let mut lines = heading("Ready", "This is what will be saved. Esc to go back and change anything.");
    lines.push(row("Data", a.data_dir.clone()));
    lines.push(row("Music", or_skip(&a.library_dir)));
    lines.push(row("Navidrome", or_skip(&a.navidrome_url)));
    let soulseek = if a.soulseek_username.trim().is_empty() {
        "not set".to_owned()
    } else {
        format!("{} on port {}", a.soulseek_username, a.soulseek_port)
    };
    lines.push(row("Soulseek", soulseek));
    lines.push(row("Listens on", a.bind.clone()));
    lines.push(row("Router port", if a.upnp { "opened with UPnP" } else { "left alone" }.to_owned()));
    lines.push(row(
        "Service",
        match a.service {
            Service::User => "systemd, for this user",
            Service::System => "systemd, system-wide",
            Service::None => "none",
        }
        .to_owned(),
    ));
    lines.push(Line::default());
    lines.push(Line::from(vec!["Press ".fg(MUTED), "Enter".fg(ACCENT).bold(), " to save.".fg(MUTED)]));
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, wizard: &Wizard) {
    let key = |k: &'static str| k.fg(ACCENT).bold();
    let mut spans = vec![key("Enter"), " next  ".fg(MUTED)];
    if wizard.step > 0 {
        spans.extend([key("Esc"), " back  ".fg(MUTED)]);
    }
    if !Field::fields(wizard.step()).is_empty() {
        spans.extend([key("↑↓"), " move  ".fg(MUTED), key("Space"), " choose  ".fg(MUTED)]);
    }
    spans.extend([key("Ctrl+C"), " quit".fg(MUTED)]);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wizard(check: bool) -> Wizard {
        let check: CheckFn = Arc::new(move |_, _| {
            Box::pin(async move { Check { ok: check, message: if check { "fine" } else { "nope" }.into() } })
        });
        Wizard {
            answers: Answers { data_dir: "/data".into(), soulseek_port: "2234".into(), ..Answers::default() },
            step: 0,
            focus: 0,
            check,
            running: None,
            results: Vec::new(),
            done: None,
        }
    }

    async fn settle(w: &mut Wizard) {
        while w.running.is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            w.poll();
        }
    }

    #[tokio::test]
    async fn moves_on_only_once_a_check_passes() {
        let mut w = wizard(false);
        w.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(w.step(), Step::Folders);
        w.on_key(KeyCode::Enter, KeyModifiers::NONE);
        settle(&mut w).await;
        assert_eq!(w.step(), Step::Folders, "a failed check keeps you here");
        assert_eq!(w.result(Step::Folders).unwrap().message, "nope");

        let mut w = wizard(true);
        w.on_key(KeyCode::Enter, KeyModifiers::NONE);
        w.on_key(KeyCode::Enter, KeyModifiers::NONE);
        settle(&mut w).await;
        assert_eq!(w.step(), Step::Navidrome);
        w.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(w.step(), Step::Folders);
        assert!(w.result(Step::Folders).is_some_and(|c| c.ok), "going back remembers the check");
    }

    #[tokio::test]
    async fn typing_edits_the_focused_answer_and_space_flips_choices() {
        let mut w = wizard(true);
        w.step = 1;
        w.on_key(KeyCode::Down, KeyModifiers::NONE);
        for c in "/srv/music".chars() {
            w.on_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        w.on_key(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(w.answers.library_dir, "/srv/musi");

        w.step = 5;
        w.focus = 0;
        w.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(w.answers.service, Service::System);
        w.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(w.answers.service, Service::None);

        w.on_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(w.done, Some(false));
    }

    #[test]
    fn long_values_show_their_end() {
        assert_eq!(tail("/srv/music", 20), "/srv/music");
        assert_eq!(tail("/home/someone/very/long/music", 10), "…ong/music");
    }

    #[test]
    fn passwords_never_show() {
        let mut w = Wizard { answers: Answers::default(), ..wizard_blank() };
        w.answers.soulseek_password = "secret".into();
        assert_eq!(w.value(Field::SoulseekPassword), "••••••");
        w.answers.navidrome_password = "pw".into();
        assert!(!w.value(Field::NavidromePassword).contains("pw"));
    }

    fn wizard_blank() -> Wizard {
        let check: CheckFn = Arc::new(|_, _| Box::pin(async { Check { ok: true, message: String::new() } }));
        Wizard { answers: Answers::default(), step: 0, focus: 0, check, running: None, results: Vec::new(), done: None }
    }
}
