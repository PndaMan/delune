//! Rendering. Pure functions of [`App`] — no I/O, no state changes.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
};

use crate::{App, Connection};

// Temporary palette until design tokens generate this module (see design/tokens).
const ACCENT: Color = Color::Rgb(0x9c, 0xa8, 0xff);
const MUTED: Color = Color::Rgb(0x6b, 0x6f, 0x80);
const OK: Color = Color::Rgb(0x6f, 0xd3, 0x9b);
const ERR: Color = Color::Rgb(0xff, 0x7a, 0x85);

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, search, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(3), Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area());

    draw_header(frame, header, app);
    draw_search(frame, search, app);
    draw_empty_state(frame, body, app);
    frame.render_widget(
        Line::from(vec![
            Span::styled("type", Style::new().fg(ACCENT)),
            Span::styled(" search or paste a link   ", Style::new().fg(MUTED)),
            Span::styled("esc", Style::new().fg(ACCENT)),
            Span::styled(" clear / quit", Style::new().fg(MUTED)),
        ]),
        footer,
    );
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let (dot, label) = match &app.connection {
        Connection::Connecting => {
            (Span::styled("●", Style::new().fg(MUTED)), format!("connecting to {}", app.server_url))
        }
        Connection::Connected(h) => {
            (Span::styled("●", Style::new().fg(OK)), format!("{} · v{}", app.server_url, h.version))
        }
        Connection::Unreachable(e) => (Span::styled("●", Style::new().fg(ERR)), format!("{} · {e}", app.server_url)),
    };
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Line::from(Span::styled(" ☾ delune", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))),
        left,
    );
    frame.render_widget(
        Line::from(vec![dot, Span::raw(" "), Span::styled(label, Style::new().fg(MUTED))]).right_aligned(),
        right,
    );
}

fn draw_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(ACCENT));
    let text = if app.query.is_empty() {
        Line::from(Span::styled(
            "Search Soulseek or paste a Spotify, Apple Music, Tidal, Qobuz… link",
            Style::new().fg(MUTED),
        ))
    } else {
        Line::from(vec![Span::raw(app.query.as_str()), Span::styled("▏", Style::new().fg(ACCENT))])
    };
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_empty_state(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let hint = if app.query.is_empty() {
        "Nothing searched yet."
    } else {
        "Search is coming in v0.1 — the server doesn't expose it yet."
    };
    let [_, middle, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Line::from(hint).fg(MUTED).centered(), middle);
}
