//! The Search screen: the box, catalogue matches, Soulseek releases, and the artist and
//! album pages opened from them.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};

use delune_core::EntityKind;
use delune_core::api::{AlbumInfo, ArtistInfo, Candidate, LibraryState};

use super::theme::{
    ACCENT, CD, ERR, FAINT, HIRES, MUTED, OK, TEXT, WARN, bold, duration, fg, human_bytes, human_speed, mark_span,
    spinner,
};
use super::{Keys, empty, heading, selected_style};
use crate::app::{App, CatalogItem, Focus, Loadable, Page, SearchState};
use crate::matching;

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App) -> Keys {
    let [search, status, body] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    draw_box(frame, search.inner(Margin::new(1, 0)), app);
    draw_status(frame, status.inner(Margin::new(2, 0)), app);
    let body = body.inner(Margin::new(1, 0));

    if let Some(page) = app.pages.last() {
        return draw_page(frame, body, app, page);
    }
    if app.results.is_empty() && app.catalog.is_empty() {
        draw_idle(frame, body, app);
        return vec![("enter", "search"), ("esc", "clear"), ("2 3", "downloads, review")];
    }

    let wide = body.width >= 100;
    let catalog_rows = u16::try_from(app.catalog.len()).unwrap_or(0);
    let (catalog_area, results_area) = if app.catalog.is_empty() {
        (None, body)
    } else if app.results.is_empty() {
        (Some(body), Rect::default())
    } else if wide {
        let [left, right] = Layout::horizontal([Constraint::Length(36), Constraint::Fill(1)]).spacing(2).areas(body);
        (Some(left), right)
    } else {
        let [top, bottom] =
            Layout::vertical([Constraint::Length(catalog_rows.min(5) + 1), Constraint::Fill(1)]).spacing(1).areas(body);
        (Some(top), bottom)
    };
    if let Some(area) = catalog_area {
        draw_catalog(frame, area, app);
    }
    if !app.results.is_empty() {
        let detail_height = if results_area.height > 22 { 11 } else { 0 };
        let [table, detail] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(detail_height)]).areas(results_area);
        draw_results(frame, table, app);
        if detail_height > 0
            && let Some(candidate) = app.selected_candidate()
        {
            draw_files(frame, detail, app, candidate);
        }
    }

    match app.focus {
        Focus::Input => vec![("enter", "search"), ("tab ↓", "results"), ("esc", "clear")],
        Focus::Catalog => vec![("enter", "open"), ("tab", "releases"), ("esc", "search box")],
        _ => {
            let download = if app.can_download() { "download" } else { "ask for it" };
            vec![("d", download), ("enter", "album"), ("a", "artist"), ("tab", "switch"), ("/", "search")]
        }
    }
}

fn draw_box(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let focused = app.focus == Focus::Input;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(if focused { ACCENT } else { FAINT }))
        .padding(Padding::horizontal(1));
    let text = if app.query.is_empty() {
        let cursor = if focused { "▏" } else { "" };
        Line::from(vec![
            Span::styled("⌕ ", fg(MUTED)),
            Span::styled(cursor, fg(ACCENT)),
            Span::styled("Artist and album, or paste a link", fg(FAINT)),
        ])
    } else {
        let mut spans = vec![Span::styled("⌕ ", fg(ACCENT)), Span::styled(app.query.as_str(), fg(TEXT))];
        if focused {
            spans.push(Span::styled("▏", fg(ACCENT)));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text).block(block), area);
}

const fn kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Album => "album",
        EntityKind::Track => "track",
        EntityKind::Artist => "artist",
        EntityKind::Playlist => "playlist",
    }
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut spans = Vec::new();
    if let Some(link) = &app.resolved {
        let by = link.artist.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
        spans.push(Span::styled(format!("{} {} ", link.provider, kind_name(link.kind)), fg(MUTED)));
        spans.push(Span::styled(format!("{}{by}", link.title), fg(ACCENT)));
        spans.push(Span::styled("  ·  ", fg(FAINT)));
    }
    match &app.search {
        SearchState::Idle => {}
        SearchState::Running { started, timeout, peers, .. } => {
            spans.push(Span::styled(format!("{} ", spinner()), fg(ACCENT)));
            spans.push(Span::styled(
                format!("Searching · {} releases from {peers} peers", app.results.len()),
                fg(MUTED),
            ));
            let label_width: u16 = spans.iter().map(|s| u16::try_from(s.width()).unwrap_or(0)).sum();
            let [label, bar] =
                Layout::horizontal([Constraint::Length(label_width + 2), Constraint::Fill(1)]).areas(area);
            frame.render_widget(Line::from(spans), label);
            let ratio = (started.elapsed().as_secs_f64() / timeout.as_secs_f64().max(1.0)).min(1.0);
            if bar.width > 8 {
                frame.render_widget(
                    Gauge::default().gauge_style(fg(ACCENT).bg(FAINT)).ratio(ratio).label("").use_unicode(true),
                    bar,
                );
            }
            return;
        }
        SearchState::Done { peers, .. } => {
            let yours = app.results.iter().filter(|c| app.mark_for(c).is_some_and(matching::Mark::is_yours)).count();
            spans.push(Span::styled(format!("✓ {} releases from {peers} peers", app.results.len()), fg(MUTED)));
            if yours > 0 {
                spans.push(Span::styled(format!("  ·  {yours} you already have"), fg(OK)));
            }
            spans.push(Span::styled("  ·  best first", fg(FAINT)));
        }
        SearchState::Failed(message) => spans.push(Span::styled(format!("✗ {message}"), fg(ERR))),
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_idle(frame: &mut Frame<'_>, area: Rect, app: &App) {
    match &app.search {
        SearchState::Running { .. } => {
            empty(frame, area, "Asking Soulseek…", &["Peers answer over the next few seconds."]);
        }
        SearchState::Done { query, .. } => empty(
            frame,
            area,
            &format!("Nobody is sharing anything for “{query}”"),
            &["Try fewer words, or just the album title."],
        ),
        SearchState::Failed(_) => {
            empty(frame, area, "The search didn't work", &["Fix the problem above, then press Enter."]);
        }
        SearchState::Idle => empty(
            frame,
            area,
            "☾  Find something to listen to",
            &[
                "Type an artist and album, then press Enter.",
                "Releases stream in from Soulseek, best quality first,",
                "marked when they're already in your library or on their way.",
                "",
                "Press ? for every key.",
            ],
        ),
    }
}

fn draw_catalog(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let focused = app.focus == Focus::Catalog;
    let [title, list] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    frame.render_widget(heading("Artists & albums", "", area.width), title);
    let rows = app.catalog.iter().map(|item| match item {
        CatalogItem::Artist(a) => {
            let listeners = a.listeners.map(compact_count).map(|n| format!("  {n} fans")).unwrap_or_default();
            Row::new([Line::from(vec![
                Span::styled("◉ ", fg(ACCENT)),
                Span::styled(a.name.clone(), fg(TEXT)),
                Span::styled(listeners, fg(FAINT)),
            ])])
        }
        CatalogItem::Album(a) => {
            let mark = matching::album_mark(&a.title, false, &app.jobs);
            let year = a.year.map(|y| format!(" {y}")).unwrap_or_default();
            let mut spans = vec![
                Span::styled("◼ ", fg(MUTED)),
                Span::styled(a.title.clone(), fg(TEXT)),
                Span::styled(format!("  {}{year}", a.artist), fg(MUTED)),
            ];
            if mark.is_some() {
                spans.push(Span::raw("  "));
                spans.push(mark_span(mark));
            }
            Row::new([Line::from(spans)])
        }
    });
    let table = Table::new(rows, [Constraint::Fill(1)])
        .row_highlight_style(selected_style(focused))
        .highlight_symbol(if focused { "▌" } else { " " });
    let mut state = TableState::default().with_selected(Some(app.catalog_selected));
    frame.render_stateful_widget(table, list, &mut state);
}

fn compact_count(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let f = n as f64;
    if n >= 1_000_000 {
        format!("{:.1}M", f / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}K", f / 1e3)
    } else {
        n.to_string()
    }
}

fn quality_color(candidate: &Candidate) -> ratatui::style::Color {
    match candidate.quality {
        Some(q) if q.codec.is_lossless() && q.bit_depth.unwrap_or(16) > 16 => HIRES,
        Some(q) if q.codec.is_lossless() => CD,
        Some(_) => MUTED,
        None => FAINT,
    }
}

fn draw_results(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let focused = app.focus == Focus::Results;
    let wide = area.width >= 96;
    let rows = app.results.iter().map(|c| {
        let mark = app.mark_for(c);
        let mut quality = c.quality_label.clone().unwrap_or_else(|| "unknown".into());
        if c.mixed_quality {
            quality.push_str(" ±");
        }
        let owned = mark.is_some_and(matching::Mark::is_yours);
        let title_style = if owned { fg(MUTED) } else { fg(TEXT) };
        let release = Line::from(vec![
            Span::styled(c.title.clone(), title_style),
            Span::styled(c.parent.as_deref().map(|p| format!("  {p}")).unwrap_or_default(), fg(FAINT)),
        ]);
        let wait = if c.free_slot {
            Span::styled("now", fg(OK))
        } else {
            Span::styled(format!("#{}", c.queue_length), fg(WARN))
        };
        let mut cells = vec![
            Cell::from(mark_span(mark)),
            Cell::from(Span::styled(quality, bold(quality_color(c)))),
            Cell::from(release),
            Cell::from(Span::styled(c.audio_files.to_string(), fg(MUTED))),
            Cell::from(Span::styled(human_bytes(c.total_bytes), fg(MUTED))),
        ];
        if wide {
            cells.push(Cell::from(wait));
            cells.push(Cell::from(Span::styled(human_speed(f64::from(c.avg_speed)), fg(MUTED))));
            cells.push(Cell::from(Span::styled(c.username.clone(), fg(FAINT))));
        }
        Row::new(cells)
    });
    let mut widths = vec![
        Constraint::Length(11),
        Constraint::Length(14),
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(7),
    ];
    let mut header = vec!["", "Quality", "Release", "#", "Size"];
    if wide {
        widths.extend([Constraint::Length(4), Constraint::Length(9), Constraint::Length(14)]);
        header.extend(["Wait", "Speed", "From"]);
    }
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(fg(FAINT)))
        .column_spacing(1)
        .row_highlight_style(selected_style(focused))
        .highlight_symbol(if focused { "▌" } else { " " });
    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);

    if app.results.len() < usize::from(area.height) {
        return;
    }
    let mut scroll = ScrollbarState::new(app.results.len()).position(app.selected);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight).thumb_style(fg(FAINT)).track_symbol(None),
        area,
        &mut scroll,
    );
}

fn draw_files(frame: &mut Frame<'_>, area: Rect, app: &App, c: &Candidate) {
    let library = app.library_for(c).filter(|l| l.state == LibraryState::InLibrary);
    let titles: Vec<String> =
        library.map(|l| l.tracks.iter().map(|t| matching::title_key(&t.title)).collect()).unwrap_or_default();
    let summary = match library.and_then(|l| matching::ownership(c, l)) {
        Some(o) if o.complete() => "you have every track".to_owned(),
        Some(o) => format!("you have {} of {}", o.owned, o.total),
        None => String::new(),
    };
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(fg(FAINT))
        .title(Line::from(vec![
            Span::styled(format!(" {} ", c.folder), fg(MUTED)),
            Span::styled(if summary.is_empty() { String::new() } else { format!("· {summary} ") }, fg(OK)),
        ]))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = c.files.iter().map(|f| {
        let have = if !f.audio || library.is_none() {
            Span::raw("")
        } else if matching::owns(&titles, &f.name) {
            Span::styled("✓", fg(OK))
        } else {
            Span::styled("missing", fg(WARN))
        };
        Row::new(vec![
            Cell::from(Span::styled(f.name.clone(), if f.audio { fg(TEXT) } else { fg(FAINT) })),
            Cell::from(have),
            Cell::from(Span::styled(f.quality_label.clone().unwrap_or_default(), fg(MUTED))),
            Cell::from(Span::styled(f.duration_secs.map(duration).unwrap_or_default(), fg(MUTED))),
            Cell::from(Span::styled(human_bytes(f.size), fg(MUTED))),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(7),
                Constraint::Length(14),
                Constraint::Length(6),
                Constraint::Length(7),
            ],
        )
        .column_spacing(1),
        inner,
    );
}

fn draw_page(frame: &mut Frame<'_>, area: Rect, app: &App, page: &Page) -> Keys {
    let crumbs: Vec<String> = app
        .pages
        .iter()
        .map(|p| match p {
            Page::Artist { name, .. } => name.clone(),
            Page::Album { title, .. } => title.clone(),
        })
        .collect();
    let [crumb, body] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Line::from(vec![
            Span::styled("‹ esc  ", fg(ACCENT)),
            Span::styled(if app.results.is_empty() { "Search" } else { "Results" }, fg(MUTED)),
            Span::styled(format!(" / {}", crumbs.join(" / ")), fg(MUTED)),
        ]),
        crumb,
    );
    match page {
        Page::Artist { name, info, selected } => {
            match info {
                Loadable::Ready(info) => draw_artist(frame, body, app, info, *selected),
                Loadable::Loading => empty(frame, body, &format!("{} Looking up {name}…", spinner()), &[]),
                Loadable::Failed(e) => empty(frame, body, &format!("Couldn't look up {name}"), &[e]),
            }
            vec![("enter", "open album"), ("s", "search for it"), ("esc", "back")]
        }
        Page::Album { title, info, .. } => {
            match info {
                Loadable::Ready(info) => draw_album(frame, body, app, info, page),
                Loadable::Loading => empty(frame, body, &format!("{} Looking up {title}…", spinner()), &[]),
                Loadable::Failed(e) => empty(
                    frame,
                    body,
                    &format!("Couldn't look up {title}"),
                    &[e, "", "Press s to search Soulseek for it anyway."],
                ),
            }
            vec![("s", "search Soulseek for it"), ("a", "artist"), ("esc", "back")]
        }
    }
}

fn draw_artist(frame: &mut Frame<'_>, area: Rect, app: &App, info: &ArtistInfo, selected: usize) {
    let have = info.albums.iter().filter(|a| a.in_library).count();
    let mut facts = Vec::new();
    if let Some(n) = info.listeners {
        facts.push(format!("{} fans", compact_count(n)));
    }
    facts.push(format!("{} releases", info.albums.len()));
    let [head, list] = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(info.name.clone(), bold(TEXT))),
            Line::from(vec![
                Span::styled(facts.join(" · "), fg(MUTED)),
                Span::styled(
                    if info.in_library.is_empty() {
                        "  ·  nothing in your library yet".to_owned()
                    } else {
                        format!("  ·  {} in your library", info.in_library.len().max(have))
                    },
                    fg(if info.in_library.is_empty() { FAINT } else { OK }),
                ),
            ]),
        ]),
        head,
    );
    let rows = info.albums.iter().map(|a| {
        let mark = matching::album_mark(&a.title, a.in_library, &app.jobs);
        Row::new(vec![
            Cell::from(mark_span(mark)),
            Cell::from(Span::styled(a.year.map(|y| y.to_string()).unwrap_or_default(), fg(MUTED))),
            Cell::from(Span::styled(a.title.clone(), if a.in_library { fg(MUTED) } else { fg(TEXT) })),
            Cell::from(Span::styled(a.kind.clone(), fg(FAINT))),
        ])
    });
    let table =
        Table::new(rows, [Constraint::Length(11), Constraint::Length(5), Constraint::Fill(1), Constraint::Length(7)])
            .header(Row::new(["", "Year", "Release", ""]).style(fg(FAINT)))
            .column_spacing(1)
            .row_highlight_style(selected_style(app.focus == Focus::Page))
            .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(selected));
    frame.render_stateful_widget(table, list, &mut state);
}

fn draw_album(frame: &mut Frame<'_>, area: Rect, app: &App, info: &AlbumInfo, page: &Page) {
    let Page::Album { selected, .. } = page else { return };
    let library = (info.in_library.state == LibraryState::InLibrary).then_some(&info.in_library);
    let titles: Vec<String> =
        library.map(|l| l.tracks.iter().map(|t| matching::title_key(&t.title)).collect()).unwrap_or_default();
    let owned = info
        .tracks
        .iter()
        .filter(|t| titles.iter().any(|k| matching::same_title(&matching::title_key(&t.title), k)))
        .count();
    let length: u32 = info.tracks.iter().filter_map(|t| t.duration_secs).sum();

    let mut facts = vec![];
    if let Some(artist) = &info.artist {
        facts.push(artist.clone());
    }
    if let Some(year) = info.year {
        facts.push(year.to_string());
    }
    facts.push(format!("{} tracks", info.tracks.len()));
    if length > 0 {
        facts.push(duration(length));
    }
    let status = match (library, matching::album_mark(&info.title, false, &app.jobs)) {
        (_, Some(mark @ (matching::Mark::Downloading(_) | matching::Mark::Queued | matching::Mark::Review))) => {
            Line::from(vec![mark_span(Some(mark)), Span::styled("  a copy is on its way", fg(MUTED))])
        }
        (Some(l), _) => {
            let quality = l.quality_label.as_deref().map(|q| format!(" as {q}")).unwrap_or_default();
            if owned >= info.tracks.len() {
                Line::from(Span::styled(format!("✓ In your library{quality}"), fg(OK)))
            } else {
                Line::from(Span::styled(
                    format!(
                        "◐ Your library has {owned} of {}{quality}. Search to fill in the {} missing.",
                        info.tracks.len(),
                        info.tracks.len() - owned
                    ),
                    fg(CD),
                ))
            }
        }
        (None, _) if info.in_library.state == LibraryState::Unknown => {
            Line::from(Span::styled("Couldn't check your library.", fg(FAINT)))
        }
        (None, _) => Line::from(Span::styled("Not in your library yet. Press s to find a copy.", fg(MUTED))),
    };
    let [head, list] = Layout::vertical([Constraint::Length(4), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(info.title.clone(), bold(TEXT))),
            Line::from(Span::styled(facts.join(" · "), fg(MUTED))),
            status,
        ]),
        head,
    );
    let rows = info.tracks.iter().map(|t| {
        let have = library.is_some() && titles.iter().any(|k| matching::same_title(&matching::title_key(&t.title), k));
        let mark = if library.is_none() {
            Span::raw("")
        } else if have {
            Span::styled("✓", fg(OK))
        } else {
            Span::styled("missing", fg(WARN))
        };
        Row::new(vec![
            Cell::from(Span::styled(format!("{:>3}", t.position), fg(FAINT))),
            Cell::from(Line::from(vec![
                Span::styled(t.title.clone(), fg(TEXT)),
                Span::styled(t.artist.as_deref().map(|a| format!("  {a}")).unwrap_or_default(), fg(FAINT)),
            ])),
            Cell::from(mark),
            Cell::from(Span::styled(t.duration_secs.map(duration).unwrap_or_default(), fg(MUTED))),
        ])
    });
    let table =
        Table::new(rows, [Constraint::Length(3), Constraint::Fill(1), Constraint::Length(7), Constraint::Length(6)])
            .column_spacing(1)
            .row_highlight_style(selected_style(app.focus == Focus::Page))
            .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(*selected));
    frame.render_stateful_widget(table, list, &mut state);
    let _ = Style::new();
}
