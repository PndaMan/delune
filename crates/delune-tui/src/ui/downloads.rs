//! The Downloads screen: everything on its way, grouped by where it's got to.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Padding, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState},
};

use delune_core::api::{DownloadJob, FileStatus, JobStatus, ReviewState};

use super::theme::{
    ACCENT, ERR, FAINT, MUTED, OK, SELECTED_BG, TEXT, WARN, ago, bar, bold, eta, fg, human_bytes, human_speed,
};
use super::{Keys, empty};
use crate::app::{App, Group};
use crate::matching::{self, is_audio};

/// Where a download is, in a few words.
fn describe(job: &DownloadJob) -> (String, Color) {
    let audio = job.files.iter().filter(|f| is_audio(&f.name)).count();
    let done = job.files.iter().filter(|f| is_audio(&f.name) && f.status == FileStatus::Done).count();
    let fetched = job.folder.is_empty();
    match job.status {
        JobStatus::Queued => {
            let text = if let Some(turn) = job.waiting_for_slot {
                format!("Waiting for a turn · number {turn}")
            } else if let Some(place) = job.files.iter().find_map(|f| f.place_in_queue) {
                format!("Number {place} in {}'s queue", job.username)
            } else if fetched {
                "Starting".into()
            } else {
                format!("Waiting for {}", job.username)
            };
            (text, MUTED)
        }
        JobStatus::Downloading if fetched => ("Fetching".into(), ACCENT),
        JobStatus::Downloading => (format!("{done} of {audio} tracks from {}", job.username), ACCENT),
        JobStatus::Ready => match job.review {
            ReviewState::Ready => ("Ready for review · enter".into(), WARN),
            ReviewState::Failed => ("Couldn't check the files".into(), ERR),
            _ => ("Checking the files…".into(), OK),
        },
        JobStatus::Failed => {
            if let Some(error) = &job.error {
                return (error.clone(), ERR);
            }
            let failed: Vec<_> = job.files.iter().filter(|f| f.status == FileStatus::Failed).collect();
            let why = failed.first().and_then(|f| f.error.as_deref()).map(|e| format!(": {e}")).unwrap_or_default();
            (format!("{} file(s) failed{why}", failed.len()), ERR)
        }
        JobStatus::Cancelled => ("Stopped · s resumes".into(), WARN),
        JobStatus::Imported => (
            format!("In your library{}", job.imported_to.as_deref().map(|to| format!(" · {to}")).unwrap_or_default()),
            OK,
        ),
    }
}

const fn icon(job: &DownloadJob) -> (&'static str, Color) {
    match job.status {
        JobStatus::Downloading => ("↓", ACCENT),
        JobStatus::Queued => ("…", MUTED),
        JobStatus::Ready => ("●", WARN),
        JobStatus::Failed => ("✗", ERR),
        JobStatus::Cancelled => ("■", WARN),
        JobStatus::Imported => ("✓", OK),
    }
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App) -> Keys {
    let area = area.inner(ratatui::layout::Margin::new(1, 0));
    let jobs = app.download_list();
    if jobs.is_empty() {
        let hidden = app.jobs.iter().filter(|j| j.status == JobStatus::Imported).count();
        let hint = if hidden > 0 {
            format!("{hidden} imported album(s) hidden. Press i to show them.")
        } else {
            String::new()
        };
        if app.jobs_loaded {
            empty(frame, area, "Nothing downloading", &["Find an album in Search (1) and press d.", &hint]);
        } else {
            empty(frame, area, "Loading downloads…", &[]);
        }
        return vec![("1", "search"), ("i", "show imported")];
    }

    let [summary, rest] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
    draw_summary(frame, summary, app);
    let files_height = if rest.height > 24 { rest.height / 3 } else { 0 };
    let [list, files] = Layout::vertical([Constraint::Fill(1), Constraint::Length(files_height)]).areas(rest);

    let wide = list.width >= 96;
    let others = app.manages()
        && jobs
            .iter()
            .any(|j| app.me.as_ref().is_some_and(|me| j.requested_by.as_deref().is_some_and(|by| by != me.username)));
    let mut rows = Vec::new();
    let mut selected_row = 0;
    let mut last_group = None;
    for (i, job) in jobs.iter().enumerate() {
        let group = Group::of(job);
        if last_group != Some(group) {
            let count = jobs.iter().filter(|j| Group::of(j) == group).count();
            if last_group.is_some() {
                rows.push(Row::new([""]).height(1));
            }
            rows.push(Row::new([
                Cell::from(""),
                Cell::from(Line::from(vec![
                    Span::styled(group.title().to_uppercase(), bold(MUTED)),
                    Span::styled(format!("  {count}"), fg(FAINT)),
                ])),
            ]));
            last_group = Some(group);
        }
        if i == app.job_selected {
            selected_row = rows.len();
        }
        rows.push(job_row(app, job, wide, others));
    }

    let mut widths = vec![Constraint::Length(2), Constraint::Fill(1), Constraint::Length(20)];
    if wide {
        widths.push(Constraint::Length(18));
    }
    let table = Table::new(rows, widths)
        .column_spacing(1)
        .row_highlight_style(ratatui::style::Style::new().bg(SELECTED_BG))
        .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(selected_row));
    frame.render_stateful_widget(table, list, &mut state);
    if usize::from(list.height) < jobs.len() * 2 + 4 {
        let mut scroll = ScrollbarState::new(jobs.len()).position(app.job_selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).thumb_style(fg(FAINT)).track_symbol(None),
            list,
            &mut scroll,
        );
    }

    let job = jobs.get(app.job_selected).copied();
    if files_height > 0
        && let Some(job) = job
    {
        draw_files(frame, files, job);
    }

    let mut keys: Keys = Vec::new();
    match job.map(|j| j.status) {
        Some(JobStatus::Queued | JobStatus::Downloading) => {
            keys.push(("s", "stop"));
            if job.is_some_and(|j| j.waiting_for_slot.is_some()) {
                keys.push(("p", "next"));
            }
        }
        Some(JobStatus::Failed | JobStatus::Cancelled) => {
            keys.push(("s", "resume"));
            keys.push(("f", "find another"));
        }
        Some(JobStatus::Ready | JobStatus::Imported) => keys.push(("enter", "review")),
        None => {}
    }
    keys.push(("x", "remove"));
    keys.push(("i", if app.show_imported { "hide imported" } else { "show imported" }));
    keys
}

fn draw_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let active: Vec<&DownloadJob> = app.jobs.iter().filter(|j| matching::in_flight(j)).collect();
    let speed: f64 = active.iter().filter_map(|j| app.rates.get(&j.id)).map(|r| r.per_sec).sum();
    let remaining: u64 = active.iter().map(|j| j.total_bytes.saturating_sub(j.bytes)).sum();
    let mut spans = vec![Span::styled(format!("{} on the way", active.len()), bold(TEXT))];
    if !active.is_empty() {
        spans.push(Span::styled(format!("  ·  {} to go", human_bytes(remaining)), fg(MUTED)));
        if speed >= 1.0 {
            spans.push(Span::styled(format!("  ·  {}", human_speed(speed)), fg(ACCENT)));
        }
        if let Some(left) = eta(remaining, speed) {
            spans.push(Span::styled(format!("  ·  {left}"), fg(MUTED)));
        }
    }
    let review = app.waiting_for_review();
    if review > 0 {
        spans.push(Span::styled(format!("  ·  {review} waiting for review (3)"), fg(WARN)));
    }
    frame.render_widget(Line::from(spans), area);
}

fn job_row<'a>(app: &App, job: &'a DownloadJob, wide: bool, others: bool) -> Row<'a> {
    let (symbol, color) = icon(job);
    let (status, status_color) = describe(job);
    let ratio = matching::progress(job);
    let mut who = String::new();
    if others && let Some(by) = &job.requested_by {
        who = format!("  for {by}");
    }
    let title = Line::from(vec![
        Span::styled(job.title.clone(), fg(TEXT)),
        Span::styled(job.parent.as_deref().map(|p| format!("  {p}")).unwrap_or_default(), fg(MUTED)),
        Span::styled(who, fg(FAINT)),
    ]);
    let detail = Line::from(Span::styled(status, fg(status_color)));

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // display only
    let percent = (ratio * 100.0).round() as u32;
    let progress = if matching::in_flight(job) || job.status == JobStatus::Failed || job.status == JobStatus::Cancelled
    {
        Line::from(vec![Span::styled(bar(ratio, 14), fg(color)), Span::styled(format!(" {percent:>3}%"), fg(MUTED))])
    } else {
        Line::from(Span::styled(human_bytes(job.total_bytes), fg(MUTED)))
    };
    let size =
        Line::from(Span::styled(format!("{} of {}", human_bytes(job.bytes), human_bytes(job.total_bytes)), fg(FAINT)));
    let mut cells = vec![
        Cell::from(Span::styled(symbol, fg(color))),
        Cell::from(vec![title, detail]),
        Cell::from(vec![progress, if matching::in_flight(job) { size } else { Line::from("") }]),
    ];
    if wide {
        let rate = app.rates.get(&job.id).map_or(0.0, |r| r.per_sec);
        let lines = if job.status == JobStatus::Downloading {
            vec![
                Line::from(Span::styled(human_speed(rate), fg(ACCENT))),
                Line::from(Span::styled(
                    eta(job.total_bytes.saturating_sub(job.bytes), rate).unwrap_or_default(),
                    fg(FAINT),
                )),
            ]
        } else {
            let when = job.imported_at.unwrap_or(job.created_at);
            vec![Line::from(""), Line::from(Span::styled(ago(when), fg(FAINT)))]
        };
        cells.push(Cell::from(lines));
    }
    Row::new(cells).height(2)
}

fn draw_files(frame: &mut Frame<'_>, area: Rect, job: &DownloadJob) {
    let source =
        if job.folder.is_empty() { "fetched".to_owned() } else { format!("{} from {}", job.folder, job.username) };
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(fg(FAINT))
        .title(Span::styled(format!(" {source} "), fg(MUTED)))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = job.files.iter().map(|f| {
        let (label, color) = match f.status {
            FileStatus::Done => ("✓ done".to_owned(), OK),
            FileStatus::Failed => (format!("✗ {}", f.error.clone().unwrap_or_else(|| "failed".into())), ERR),
            FileStatus::Transferring => {
                #[allow(clippy::cast_precision_loss)] // display only
                let ratio = if f.size > 0 { f.bytes as f64 / f.size as f64 } else { 0.0 };
                (format!("{} {}", bar(ratio, 10), human_bytes(f.bytes)), ACCENT)
            }
            FileStatus::Queued => {
                (f.place_in_queue.map_or_else(|| "queued".into(), |p| format!("queued · #{p}")), MUTED)
            }
            FileStatus::Connecting | FileStatus::Starting => ("connecting".into(), MUTED),
            FileStatus::Waiting => ("waiting".into(), FAINT),
            FileStatus::Cancelled => ("stopped".into(), WARN),
        };
        Row::new(vec![
            Cell::from(Span::styled(f.name.clone(), if is_audio(&f.name) { fg(TEXT) } else { fg(FAINT) })),
            Cell::from(Span::styled(label, fg(color))),
            Cell::from(Span::styled(human_bytes(f.size), fg(MUTED))),
        ])
    });
    frame.render_widget(
        Table::new(rows, [Constraint::Fill(1), Constraint::Length(26), Constraint::Length(8)]).column_spacing(1),
        inner,
    );
}
