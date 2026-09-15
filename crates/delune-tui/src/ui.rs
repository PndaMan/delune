//! Rendering. Pure functions of [`App`] — no I/O, no state changes.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Gauge, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
};

use delune_core::EntityKind;
use delune_core::api::{Candidate, DownloadJob, FileStatus, JobStatus, ReviewState, SoulseekState};

use crate::{App, Connection, Focus, Report, Screen, SearchState};

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
    match app.screen {
        Screen::Search => draw_search_screen(frame, app),
        Screen::Downloads => draw_downloads_screen(frame, app),
        Screen::Review => draw_review_screen(frame, app),
    }
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, &confirm.prompt);
    }
}

fn draw_search_screen(frame: &mut Frame<'_>, app: &App) {
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
    let [left, tabs, right] =
        Layout::horizontal([Constraint::Length(11), Constraint::Length(40), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Line::from(Span::styled(" ☾ delune", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))),
        left,
    );
    let waiting = app.reviewable().len();
    let tab = |key: &'static str, label: String, screen: Screen| {
        let active = app.screen == screen;
        vec![
            Span::styled(key, Style::new().fg(if active { ACCENT } else { FAINT })),
            Span::styled(
                format!(" {label}   "),
                if active { Style::new().fg(TEXT).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) },
            ),
        ]
    };
    let review_label = if waiting > 0 { format!("Review ({waiting})") } else { "Review".into() };
    let mut spans = tab("1", "Search".into(), Screen::Search);
    spans.extend(tab("2", "Downloads".into(), Screen::Downloads));
    spans.extend(tab("3", review_label, Screen::Review));
    frame.render_widget(Line::from(spans), tabs);
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
        SearchState::Done { peers, .. } => {
            let mut spans = Vec::new();
            if let Some(link) = &app.resolved {
                let by = link.artist.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
                spans.push(Span::styled(
                    format!("{} {}: ", link.provider, kind_name(link.kind)),
                    Style::new().fg(MUTED),
                ));
                spans.push(Span::styled(format!("{}{by}", link.title), Style::new().fg(ACCENT)));
                spans.push(Span::styled(" · ", Style::new().fg(FAINT)));
            }
            spans.push(Span::styled(
                format!("✓ {} releases from {peers} peers, best quality first", app.results.len()),
                Style::new().fg(MUTED),
            ));
            frame.render_widget(Line::from(spans), area);
        }
        SearchState::Failed(message) => {
            frame.render_widget(Line::from(Span::styled(format!("✗ {message}"), Style::new().fg(ERR))), area);
        }
    }
}

const fn kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Album => "album",
        EntityKind::Track => "track",
        EntityKind::Artist => "artist",
        EntityKind::Playlist => "playlist",
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
    if let Some((notice, bad)) = &app.notice {
        let style = Style::new().fg(if *bad { ERR } else { OK });
        frame.render_widget(Line::from(Span::styled(format!(" {notice}"), style)), area);
        return;
    }
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
            key("d"),
            text(" download for review   "),
            key("g G"),
            text(" top / bottom   "),
            key("/"),
            text(" search   "),
            key("1 2 3"),
            text(" screens   "),
            key("q"),
            text(" quit"),
        ],
    };
    frame.render_widget(Line::from(spans), area);
}

fn footer_keys(frame: &mut Frame<'_>, area: Rect, app: &App, keys: &[(&'static str, &'static str)]) {
    if let Some((notice, bad)) = &app.notice {
        let style = Style::new().fg(if *bad { ERR } else { OK });
        frame.render_widget(Line::from(Span::styled(format!(" {notice}"), style)), area);
        return;
    }
    let mut spans = vec![Span::raw(" ")];
    for (k, t) in keys {
        spans.push(Span::styled(*k, Style::new().fg(ACCENT)));
        spans.push(Span::styled(format!(" {t}   "), Style::new().fg(MUTED)));
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_confirm(frame: &mut Frame<'_>, prompt: &str) {
    let width = u16::try_from(prompt.chars().count() + 8).unwrap_or(60).clamp(30, frame.area().width.saturating_sub(4));
    let [area] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(frame.area());
    let [area] = Layout::vertical([Constraint::Length(5)]).flex(Flex::Center).areas(area);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .padding(Padding::horizontal(2));
    let lines = vec![
        Line::from(Span::styled(prompt, Style::new().fg(TEXT))),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::new().fg(ACCENT)),
            Span::styled(" yes   ", Style::new().fg(MUTED)),
            Span::styled("any other key", Style::new().fg(ACCENT)),
            Span::styled(" no", Style::new().fg(MUTED)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

/// Where a download is, in a few words.
fn describe_job(job: &DownloadJob) -> (String, Color) {
    let audio = job.files.iter().filter(|f| is_audio(&f.name)).count();
    let done = job.files.iter().filter(|f| is_audio(&f.name) && f.status == FileStatus::Done).count();
    match job.status {
        JobStatus::Queued => {
            let place = job.files.iter().find_map(|f| f.place_in_queue);
            (
                place.map_or_else(
                    || format!("Waiting for {}", job.username),
                    |p| format!("Number {p} in {}'s queue", job.username),
                ),
                MUTED,
            )
        }
        JobStatus::Downloading => (format!("Downloading {done} of {audio} from {}", job.username), ACCENT),
        JobStatus::Ready if job.review == ReviewState::Ready => ("Ready for review".into(), OK),
        JobStatus::Ready => ("Checking the files".into(), OK),
        JobStatus::Failed => {
            let failed = job.files.iter().filter(|f| f.status == FileStatus::Failed).count();
            (format!("{failed} file(s) failed. s to retry"), ERR)
        }
        JobStatus::Cancelled => ("Stopped. s to resume".into(), WARN),
        JobStatus::Imported => ("In your library".into(), MUTED),
    }
}

fn is_audio(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| delune_core::Codec::from_extension(ext).is_some())
}

fn bar(ratio: f64, width: usize) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)] // display only
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("{}{}", "━".repeat(filled), "─".repeat(width - filled))
}

fn draw_downloads_screen(frame: &mut Frame<'_>, app: &App) {
    let [header, gap, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area());
    draw_header(frame, header, app);
    let _ = gap;

    if app.jobs.is_empty() {
        let lines = vec![
            Line::from("Nothing downloading").fg(TEXT),
            Line::from("Find an album in Search (1) and press d to download it for review.").fg(MUTED),
        ];
        let [_, middle, _] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(2), Constraint::Fill(2)]).areas(body);
        frame.render_widget(Paragraph::new(lines).centered(), middle);
    } else {
        let files_height = if body.height > 20 { 10 } else { 0 };
        let [list, files] = Layout::vertical([Constraint::Fill(1), Constraint::Length(files_height)]).areas(body);
        let rows = app.jobs.iter().map(|job| {
            let (status, color) = describe_job(job);
            #[allow(clippy::cast_precision_loss)] // display only
            let ratio = if job.total_bytes > 0 { job.bytes as f64 / job.total_bytes as f64 } else { 0.0 };
            let release = Line::from(vec![
                Span::styled(job.title.clone(), Style::new().fg(TEXT)),
                Span::styled(
                    job.parent.as_deref().map(|p| format!("  {p}")).unwrap_or_default(),
                    Style::new().fg(MUTED),
                ),
            ]);
            Row::new(vec![
                Cell::from(release),
                Cell::from(Span::styled(status, Style::new().fg(color))),
                Cell::from(Span::styled(bar(ratio, 16), Style::new().fg(color))),
                Cell::from(Span::styled(human_bytes(job.total_bytes), Style::new().fg(MUTED))),
            ])
        });
        let table = Table::new(
            rows,
            [Constraint::Fill(1), Constraint::Length(34), Constraint::Length(16), Constraint::Length(8)],
        )
        .header(Row::new(["Release", "Status", "", "Size"]).style(Style::new().fg(FAINT)))
        .column_spacing(2)
        .row_highlight_style(Style::new().bg(SELECTED_BG))
        .highlight_symbol("▌");
        let mut state = TableState::default().with_selected(Some(app.job_selected));
        frame.render_stateful_widget(table, list.inner(Margin::new(1, 0)), &mut state);

        if files_height > 0
            && let Some(job) = app.jobs.get(app.job_selected)
        {
            draw_job_files(frame, files, job);
        }
    }
    footer_keys(
        frame,
        footer,
        app,
        &[
            ("↑↓", "move"),
            ("s", "stop / resume"),
            ("x", "remove"),
            ("enter", "review"),
            ("1 3", "screens"),
            ("q", "quit"),
        ],
    );
}

fn draw_job_files(frame: &mut Frame<'_>, area: Rect, job: &DownloadJob) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(FAINT))
        .title(Span::styled(format!(" {} from {} ", job.folder, job.username), Style::new().fg(MUTED)))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = job.files.iter().map(|f| {
        let (label, color) = match f.status {
            FileStatus::Done => ("done".to_owned(), OK),
            FileStatus::Failed => (f.error.clone().unwrap_or_else(|| "failed".into()), ERR),
            FileStatus::Transferring => (format!("{} of {}", human_bytes(f.bytes), human_bytes(f.size)), ACCENT),
            FileStatus::Queued => (f.place_in_queue.map_or_else(|| "queued".into(), |p| format!("queued #{p}")), MUTED),
            other => (format!("{other:?}").to_lowercase(), MUTED),
        };
        Row::new(vec![
            Cell::from(Span::styled(f.name.clone(), Style::new().fg(TEXT))),
            Cell::from(Span::styled(label, Style::new().fg(color))),
        ])
    });
    frame.render_widget(Table::new(rows, [Constraint::Fill(1), Constraint::Length(28)]).column_spacing(2), inner);
}

fn draw_review_screen(frame: &mut Frame<'_>, app: &App) {
    let [header, gap, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area());
    draw_header(frame, header, app);
    let _ = gap;
    let jobs = app.reviewable();
    if jobs.is_empty() {
        let lines = vec![
            Line::from("Nothing to review").fg(TEXT),
            Line::from("Finished downloads wait here until you import or discard them.").fg(MUTED),
        ];
        let [_, middle, _] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(2), Constraint::Fill(2)]).areas(body);
        frame.render_widget(Paragraph::new(lines).centered(), middle);
        footer_keys(frame, footer, app, &[("1 2", "screens"), ("q", "quit")]);
        return;
    }

    let [list, detail] = Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).areas(body);
    let rows = jobs.iter().map(|job| {
        Row::new(vec![Cell::from(Line::from(vec![
            Span::styled(job.title.clone(), Style::new().fg(TEXT)),
            Span::styled(job.parent.as_deref().map(|p| format!("  {p}")).unwrap_or_default(), Style::new().fg(MUTED)),
        ]))])
    });
    let mut state = TableState::default().with_selected(Some(app.review_selected));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Fill(1)])
            .row_highlight_style(Style::new().bg(SELECTED_BG))
            .highlight_symbol("▌")
            .block(Block::new().borders(Borders::RIGHT).border_style(Style::new().fg(FAINT))),
        list.inner(Margin::new(1, 0)),
        &mut state,
    );

    let detail = detail.inner(Margin::new(2, 0));
    if let Some(job) = jobs.get(app.review_selected) {
        match app.reports.get(&job.id) {
            Some(Report::Ready(report)) => {
                let lines = report_lines(report);
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), detail);
            }
            Some(Report::Failed(message)) => {
                frame.render_widget(Line::from(Span::styled(format!("✗ {message}"), Style::new().fg(ERR))), detail);
            }
            _ => {
                frame.render_widget(
                    Line::from(Span::styled(
                        "Playing every file through and checking its sound…",
                        Style::new().fg(MUTED),
                    )),
                    detail,
                );
            }
        }
    }
    footer_keys(
        frame,
        footer,
        app,
        &[("↑↓", "move"), ("i", "import"), ("x", "discard"), ("1 2", "screens"), ("q", "quit")],
    );
}

fn report_lines(report: &delune_core::api::ReviewReport) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(report.album.clone(), Style::new().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  {}{}", report.album_artist, report.year.map(|y| format!(", {y}")).unwrap_or_default()),
                Style::new().fg(MUTED),
            ),
        ]),
        Line::from(""),
    ];
    if let Some(reason) = &report.blocked_reason {
        lines.push(Line::from(Span::styled(format!("✗ {reason}"), Style::new().fg(ERR))));
    } else {
        let problems = report.tracks.iter().filter(|t| t.problem.is_some()).count();
        let verdict = if problems == 0 {
            Span::styled(format!("✓ {} tracks play cleanly", report.tracks.len()), Style::new().fg(OK))
        } else {
            Span::styled(format!("! {problems} track(s) need a look"), Style::new().fg(WARN))
        };
        lines.push(Line::from(verdict));
    }
    for warning in report.warnings.iter().chain(&report.conflicts) {
        lines.push(Line::from(Span::styled(format!("! {warning}"), Style::new().fg(WARN))));
    }
    lines.push(Line::from(""));
    for t in &report.tracks {
        let mut spans = vec![
            Span::styled(format!("{:>3} ", t.track), Style::new().fg(FAINT)),
            Span::styled(t.title.clone(), Style::new().fg(TEXT)),
            Span::styled(format!("  {}", t.quality_label.clone().unwrap_or_default()), Style::new().fg(MUTED)),
        ];
        if let Some(problem) = &t.problem {
            spans.push(Span::styled(
                format!("  {problem}"),
                Style::new().fg(if t.suspect_transcode { WARN } else { ERR }),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::from(Span::styled(format!("      → {}", t.destination), Style::new().fg(FAINT))));
    }
    lines
}
