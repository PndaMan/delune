//! Rendering. Pure functions of [`App`] — no I/O, no state changes.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};

use delune_core::api::{Candidate, SoulseekState};

use crate::{App, Connection, Focus, SearchState};

// Temporary palette until design tokens generate this module.
const ACCENT: Color = Color::Rgb(0xae, 0xb8, 0xff);
const TEXT: Color = Color::Rgb(0xe4, 0xe8, 0xf4);
const MUTED: Color = Color::Rgb(0x8b, 0x91, 0xa8);
const FAINT: Color = Color::Rgb(0x4a, 0x51, 0x68);
const OK: Color = Color::Rgb(0x6f, 0xd3, 0x9b);
const WARN: Color = Color::Rgb(0xf2, 0xc1, 0x6b);
const ERR: Color = Color::Rgb(0xff, 0x7a, 0x85);
const HIRES: Color = Color::Rgb(0xf5, 0xc4, 0x6b);
const CD: Color = Color::Rgb(0x7d, 0xd8, 0xc0);
const SELECTED_BG: Color = Color::Rgb(0x2a, 0x31, 0x47);

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, search, status, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    draw_search(frame, search, app);
    draw_status(frame, status, app);
    if app.results.is_empty() {
        draw_empty_state(frame, body, app);
    } else {
        let detail_height = if body.height > 24 { 12 } else { 0 };
        let [table, detail] = Layout::vertical([Constraint::Fill(1), Constraint::Length(detail_height)]).areas(body);
        draw_results(frame, table, app);
        if detail_height > 0
            && let Some(candidate) = app.selected_candidate()
        {
            draw_detail(frame, detail, candidate);
        }
    }
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let (dot, label) = match &app.connection {
        Connection::Connecting => {
            (Span::styled("◌", Style::new().fg(MUTED)), format!("connecting to {}", app.server_url))
        }
        Connection::Connected { soulseek, .. } => match soulseek {
            Some(s) if s.state == SoulseekState::Online => (
                Span::styled("●", Style::new().fg(OK)),
                format!("Soulseek as {}", s.username.as_deref().unwrap_or("?")),
            ),
            Some(s) => (
                Span::styled("●", Style::new().fg(WARN)),
                s.message.clone().unwrap_or_else(|| format!("Soulseek {:?}", s.state).to_lowercase()),
            ),
            None => (Span::styled("●", Style::new().fg(WARN)), "server connected".into()),
        },
        Connection::Unreachable(e) => (Span::styled("●", Style::new().fg(ERR)), format!("{} · {e}", app.server_url)),
    };
    let [left, right] = Layout::horizontal([Constraint::Length(12), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Line::from(Span::styled(" ☾ delune", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))),
        left,
    );
    frame.render_widget(
        Line::from(vec![dot, Span::raw(" "), Span::styled(label, Style::new().fg(MUTED)), Span::raw(" ")])
            .right_aligned(),
        right,
    );
}

fn draw_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let focused = app.focus == Focus::Input;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { FAINT }))
        .padding(Padding::horizontal(1));
    let text = if app.query.is_empty() {
        Line::from(vec![
            Span::styled("⌕ ", Style::new().fg(MUTED)),
            Span::styled("Search Soulseek: artist and album", Style::new().fg(MUTED)),
        ])
    } else {
        let mut spans =
            vec![Span::styled("⌕ ", Style::new().fg(ACCENT)), Span::styled(app.query.as_str(), Style::new().fg(TEXT))];
        if focused {
            spans.push(Span::styled("▏", Style::new().fg(ACCENT)));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let area = area.inner(Margin::new(1, 0));
    match &app.search {
        SearchState::Idle => {}
        SearchState::Running { started, timeout, peers, .. } => {
            let [label, bar] = Layout::horizontal([Constraint::Length(44), Constraint::Fill(1)]).areas(area);
            let spinner =
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(started.elapsed().as_millis() / 80) as usize % 10];
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{spinner} "), Style::new().fg(ACCENT)),
                    Span::styled(
                        format!("Searching · {peers} peers · {} releases", app.results.len()),
                        Style::new().fg(MUTED),
                    ),
                ]),
                label,
            );
            let ratio = (started.elapsed().as_secs_f64() / timeout.as_secs_f64().max(1.0)).min(1.0);
            frame.render_widget(
                Gauge::default()
                    .gauge_style(Style::new().fg(ACCENT).bg(FAINT))
                    .ratio(ratio)
                    .label("")
                    .use_unicode(true),
                bar.inner(Margin::new(0, 0)),
            );
        }
        SearchState::Done { peers, .. } => frame.render_widget(
            Line::from(Span::styled(
                format!("✓ {} releases from {peers} peers, best quality first", app.results.len()),
                Style::new().fg(MUTED),
            )),
            area,
        ),
        SearchState::Failed(message) => {
            frame.render_widget(Line::from(Span::styled(format!("✗ {message}"), Style::new().fg(ERR))), area);
        }
    }
}

fn draw_empty_state(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let lines: Vec<Line<'_>> = match &app.search {
        SearchState::Running { .. } => vec![Line::from("Waiting for peers to answer…").fg(MUTED)],
        SearchState::Done { query, .. } => vec![
            Line::from(format!("Nobody is sharing anything for “{query}”.")).fg(TEXT),
            Line::from("Try fewer words, or just the album title.").fg(MUTED),
        ],
        SearchState::Failed(_) => vec![Line::from("Fix the problem above, then press Enter to try again.").fg(MUTED)],
        SearchState::Idle => vec![
            Line::from("☾").fg(ACCENT),
            Line::from(""),
            Line::from("Type an artist and album, then press Enter.").fg(TEXT),
            Line::from("Results stream in from Soulseek as peers answer.").fg(MUTED),
        ],
    };
    let height = u16::try_from(lines.len()).unwrap_or(1);
    let [_, middle, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(height), Constraint::Fill(2)]).areas(area);
    frame.render_widget(Paragraph::new(lines).centered(), middle);
}

fn quality_color(candidate: &Candidate) -> Color {
    match candidate.quality {
        Some(q) if q.codec.is_lossless() && q.bit_depth.unwrap_or(16) > 16 => HIRES,
        Some(q) if q.codec.is_lossless() => CD,
        Some(_) => MUTED,
        None => FAINT,
    }
}

fn human_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

fn human_speed(bytes_per_sec: u32) -> String {
    match bytes_per_sec {
        0 => "—".into(),
        b if b >= 1_000_000 => format!("{:.1} MB/s", f64::from(b) / 1e6),
        b => format!("{} KB/s", b / 1000),
    }
}

fn draw_results(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let focused = app.focus == Focus::Results;
    let header = Row::new(["Quality", "Release", "Tracks", "Size", "Wait", "Speed", "From"])
        .style(Style::new().fg(FAINT))
        .bottom_margin(0);

    let rows = app.results.iter().map(|c| {
        let mut quality = c.quality_label.clone().unwrap_or_else(|| "unknown".into());
        if c.mixed_quality {
            quality.push_str(" ±");
        }
        let release = match &c.parent {
            Some(parent) => Line::from(vec![
                Span::styled(c.title.clone(), Style::new().fg(TEXT)),
                Span::styled(format!("  {parent}"), Style::new().fg(MUTED)),
            ]),
            None => Line::from(Span::styled(c.title.clone(), Style::new().fg(TEXT))),
        };
        let wait = if c.free_slot {
            Span::styled("now", Style::new().fg(OK))
        } else {
            Span::styled(format!("#{}", c.queue_length), Style::new().fg(WARN))
        };
        Row::new(vec![
            Cell::from(Span::styled(quality, Style::new().fg(quality_color(c)).add_modifier(Modifier::BOLD))),
            Cell::from(release),
            Cell::from(Span::styled(c.audio_files.to_string(), Style::new().fg(MUTED))),
            Cell::from(Span::styled(human_bytes(c.total_bytes), Style::new().fg(MUTED))),
            Cell::from(wait),
            Cell::from(Span::styled(human_speed(c.avg_speed), Style::new().fg(MUTED))),
            Cell::from(Span::styled(c.username.clone(), Style::new().fg(MUTED))),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(15),
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(16),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(if focused { Style::new().bg(SELECTED_BG) } else { Style::new() })
    .highlight_symbol(if focused { "▌" } else { " " })
    .block(Block::new().padding(Padding::horizontal(0)));

    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);

    let mut scroll = ScrollbarState::new(app.results.len()).position(app.selected);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight).thumb_style(Style::new().fg(FAINT)).track_symbol(None),
        area,
        &mut scroll,
    );
}

fn draw_detail(frame: &mut Frame<'_>, area: Rect, c: &Candidate) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(FAINT))
        .title(Span::styled(format!(" {} ", c.folder), Style::new().fg(MUTED)))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = c.files.iter().map(|f| {
        let style = if f.audio { Style::new().fg(TEXT) } else { Style::new().fg(FAINT) };
        Row::new(vec![
            Cell::from(Span::styled(f.name.clone(), style)),
            Cell::from(Span::styled(f.quality_label.clone().unwrap_or_default(), Style::new().fg(MUTED))),
            Cell::from(Span::styled(
                f.duration_secs.map(|d| format!("{}:{:02}", d / 60, d % 60)).unwrap_or_default(),
                Style::new().fg(MUTED),
            )),
            Cell::from(Span::styled(human_bytes(f.size), Style::new().fg(MUTED))),
        ])
    });
    frame.render_widget(
        Table::new(rows, [Constraint::Fill(1), Constraint::Length(14), Constraint::Length(6), Constraint::Length(8)])
            .column_spacing(2),
        inner,
    );
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let key = |k: &'static str| Span::styled(k, Style::new().fg(ACCENT));
    let text = |t: &'static str| Span::styled(t, Style::new().fg(MUTED));
    let spans = match app.focus {
        Focus::Input => vec![
            Span::raw(" "),
            key("enter"),
            text(" search   "),
            key("↓"),
            text(" results   "),
            key("esc"),
            text(" clear / quit"),
        ],
        Focus::Results => vec![
            Span::raw(" "),
            key("↑↓ jk"),
            text(" move   "),
            key("g G"),
            text(" top / bottom   "),
            key("/"),
            text(" search   "),
            key("q"),
            text(" quit"),
        ],
    };
    frame.render_widget(Line::from(spans), area);
}
