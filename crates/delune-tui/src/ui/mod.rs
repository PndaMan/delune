//! Rendering. Pure functions of [`App`] — no I/O, no state changes.

mod downloads;
mod review;
mod search;
mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap},
};

use delune_core::api::SoulseekState;

use crate::app::{App, Connection, Screen};
use theme::{ACCENT, ERR, FAINT, MUTED, OK, SELECTED_BG, TEXT, WARN, bold, fg};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    draw_header(frame, header, app);
    let keys = match app.screen {
        Screen::Search => search::draw(frame, body, app),
        Screen::Downloads => downloads::draw(frame, body, app),
        Screen::Review => review::draw(frame, body, app),
    };
    draw_footer(frame, footer, app, &keys);
    if app.help {
        draw_help(frame);
    }
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, &confirm.prompt);
    }
}

/// Key hints for the footer: (keys, what they do).
pub type Keys = Vec<(&'static str, &'static str)>;

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let [line, rule] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let narrow = area.width < 90;

    let mut spans = vec![Span::styled(" ☾ delune ", bold(ACCENT)), Span::raw("  ")];
    let active = app.active_downloads();
    let waiting = app.waiting_for_review();
    let tabs: [(&str, &str, Screen, usize); 3] = [
        ("1", "Search", Screen::Search, 0),
        ("2", "Downloads", Screen::Downloads, active),
        ("3", "Review", Screen::Review, waiting),
    ];
    for (key, label, screen, count) in tabs {
        let current = app.screen == screen;
        let style = if current { bold(TEXT).bg(SELECTED_BG) } else { fg(MUTED) };
        spans.push(Span::styled(format!(" {key} "), if current { fg(ACCENT).bg(SELECTED_BG) } else { fg(FAINT) }));
        spans.push(Span::styled(format!("{label} "), style));
        if count > 0 {
            let color = if screen == Screen::Review { WARN } else { ACCENT };
            spans.push(Span::styled(format!("{count} "), if current { fg(color).bg(SELECTED_BG) } else { fg(color) }));
        }
        spans.push(Span::raw(" "));
    }
    let used: usize = spans.iter().map(Span::width).sum();

    let (dot, status) = match &app.connection {
        Connection::Connecting => (Span::styled("◌", fg(MUTED)), "connecting…".to_owned()),
        Connection::Connected { soulseek, .. } => match soulseek {
            Some(s) if s.state == SoulseekState::Online => (Span::styled("●", fg(OK)), "Soulseek online".to_owned()),
            Some(s) => (
                Span::styled("●", fg(WARN)),
                s.message.clone().unwrap_or_else(|| format!("Soulseek {:?}", s.state).to_lowercase()),
            ),
            None => (Span::styled("●", fg(WARN)), "connected".into()),
        },
        Connection::Unreachable(e) => (Span::styled("●", fg(ERR)), e.clone()),
    };
    let host = app.server_url.split("://").nth(1).unwrap_or(&app.server_url).trim_end_matches('/');
    let who = app.me.as_ref().map(|me| format!("{}@", me.username)).unwrap_or_default();
    let mut right = vec![dot, Span::styled(format!(" {status}"), fg(MUTED))];
    if !narrow {
        right.push(Span::styled(format!("  ·  {who}{host}"), fg(FAINT)));
    }
    right.push(Span::raw(" "));
    let right_width: usize = right.iter().map(Span::width).sum();
    let gap = usize::from(area.width).saturating_sub(used + right_width);
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right);
    }
    frame.render_widget(Line::from(spans), line);
    frame.render_widget(Line::from(Span::styled("─".repeat(usize::from(area.width)), fg(FAINT))), rule);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App, keys: &Keys) {
    if let Some(notice) = app.current_notice() {
        let (mark, color) = if notice.bad { ("✗", ERR) } else { ("✓", OK) };
        frame.render_widget(
            Line::from(vec![
                Span::styled(format!(" {mark} "), fg(color)),
                Span::styled(notice.text.as_str(), fg(TEXT)),
            ]),
            area,
        );
        return;
    }
    let mut spans = vec![Span::raw(" ")];
    let mut width = 1;
    let limit = usize::from(area.width).saturating_sub(9);
    for (k, t) in keys.iter().chain(&[("?", "help")]) {
        let piece = k.chars().count() + t.chars().count() + 4;
        if width + piece > limit && *k != "?" {
            continue;
        }
        width += piece;
        spans.push(Span::styled(*k, fg(ACCENT)));
        spans.push(Span::styled(format!(" {t}   "), fg(MUTED)));
    }
    frame.render_widget(Line::from(spans), area);
}

/// A box centred in the frame, cleared.
fn overlay(frame: &mut Frame<'_>, width: u16, height: u16) -> Rect {
    let area = frame.area();
    let [area] = Layout::horizontal([Constraint::Length(width.min(area.width.saturating_sub(2)))])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height.min(area.height.saturating_sub(2)))])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(Clear, area);
    area
}

fn draw_confirm(frame: &mut Frame<'_>, prompt: &str) {
    let width = u16::try_from(prompt.chars().count() + 8).unwrap_or(64).clamp(34, 64);
    let lines_needed = u16::try_from(prompt.chars().count() / usize::from(width - 6) + 1).unwrap_or(2);
    let area = overlay(frame, width, lines_needed + 4);
    let block =
        Block::bordered().border_type(BorderType::Rounded).border_style(fg(ACCENT)).padding(Padding::horizontal(2));
    let lines = vec![
        Line::from(Span::styled(prompt, fg(TEXT))),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", fg(ACCENT)),
            Span::styled(" yes   ", fg(MUTED)),
            Span::styled("any other key", fg(ACCENT)),
            Span::styled(" no", fg(MUTED)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Everywhere",
        &[
            ("F1 F2 F3", "Search, Downloads, Review (1 2 3 outside the box)"),
            ("/", "search"),
            ("j k  ↑ ↓", "move"),
            ("g G", "first, last"),
            ("q  ctrl-c", "quit (esc never quits)"),
        ],
    ),
    (
        "Search",
        &[
            ("enter", "search Soulseek (links work too)"),
            ("tab", "move between the box and the lists"),
            ("enter  o", "open a release: its files and cover"),
            ("d", "download a release (or ask an admin)"),
            ("i", "the album's tracklist"),
            ("a", "open the artist"),
            ("space  t", "tick a track, or all of them"),
            ("s", "search Soulseek for the open album"),
            ("esc", "back"),
        ],
    ),
    (
        "Downloads",
        &[
            ("s  space", "stop or resume"),
            ("p", "download next"),
            ("f", "find another copy"),
            ("x", "remove"),
            ("enter", "review"),
            ("i", "show imported"),
        ],
    ),
    ("Review", &[("i", "import"), ("x", "discard"), ("f", "find another copy"), ("J K", "scroll the report")]),
];

fn draw_help(frame: &mut Frame<'_>) {
    let mut lines = Vec::new();
    for (section, keys) in HELP {
        lines.push(Line::from(Span::styled(*section, bold(TEXT))));
        for (k, what) in *keys {
            lines
                .push(Line::from(vec![Span::styled(format!("  {k:<11}"), fg(ACCENT)), Span::styled(*what, fg(MUTED))]));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled("Any key closes this.", fg(FAINT))));
    let height = u16::try_from(lines.len()).unwrap_or(30) + 2;
    let area = overlay(frame, 64, height);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(ACCENT))
        .title(Span::styled(" Keys ", bold(ACCENT)))
        .padding(Padding::horizontal(2));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// A cover or picture filling `area`, or a quiet placeholder while there isn't one.
fn picture(frame: &mut Frame<'_>, area: Rect, app: &App, key: &str) {
    if area.width < 4 || area.height < 2 {
        return;
    }
    let ready = match app.pictures.get(key) {
        Some(crate::app::Picture::Ready(image)) => Some(image.clone()),
        _ => None,
    };
    let mut canvas = app.canvas.borrow_mut();
    let canvas = &mut *canvas;
    if let (Some(image), Some(picker)) = (ready, canvas.picker.as_ref()) {
        let fitted =
            canvas.fitted.entry(key.to_owned()).or_insert_with(|| picker.new_resize_protocol((*image).clone()));
        frame.render_stateful_widget(ratatui_image::StatefulImage::default(), area, fitted);
        return;
    }
    let loading = matches!(app.pictures.get(key), Some(crate::app::Picture::Loading));
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(fg(MUTED));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mark = if loading { theme::spinner().to_string() } else { "♪".to_owned() };
    let [_, middle, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    frame.render_widget(Paragraph::new(Span::styled(mark, fg(MUTED))).centered(), middle);
}

/// A picture's area: `height` rows, and about twice as many columns, since cells are tall.
fn picture_area(area: Rect, height: u16) -> (Rect, Rect) {
    let height = height.min(area.height);
    let width = (height * 2 + 1).min(area.width / 3);
    let [left, right] = Layout::horizontal([Constraint::Length(width), Constraint::Fill(1)]).spacing(2).areas(area);
    let [left, _] = Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(left);
    (left, right)
}

/// A title and explanation in the middle of an empty area.
fn empty(frame: &mut Frame<'_>, area: Rect, title: &str, lines: &[&str]) {
    let mut text = vec![Line::from(Span::styled(title.to_owned(), bold(TEXT))), Line::from("")];
    text.extend(lines.iter().map(|l| Line::from(Span::styled((*l).to_owned(), fg(MUTED)))));
    let height = u16::try_from(text.len()).unwrap_or(3);
    let [_, middle, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(height), Constraint::Fill(2)]).areas(area);
    frame.render_widget(Paragraph::new(text).centered().wrap(Wrap { trim: true }), middle);
}

/// A section heading with a rule after it.
fn heading(title: &str, detail: &str, width: u16) -> Line<'static> {
    let used = title.chars().count() + detail.chars().count() + 3;
    Line::from(vec![
        Span::styled(title.to_owned(), bold(TEXT)),
        Span::styled(format!(" {detail} "), fg(MUTED)),
        Span::styled("─".repeat(usize::from(width).saturating_sub(used)), fg(FAINT)),
    ])
}

fn selected_style(focused: bool) -> Style {
    if focused { Style::new().bg(SELECTED_BG) } else { Style::new() }
}
