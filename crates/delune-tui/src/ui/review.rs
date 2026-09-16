//! The Review screen: finished downloads waiting for a yes or no, and what came in lately.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
};

use delune_core::api::{DownloadJob, JobStatus, ReviewReport, ReviewState};

use super::theme::{ACCENT, ERR, FAINT, MUTED, OK, TEXT, WARN, ago, bold, duration, fg, spinner};
use super::{Keys, empty, heading, selected_style};
use crate::app::{App, Loadable};

pub fn draw(frame: &mut Frame<'_>, area: Rect, app: &App) -> Keys {
    let area = area.inner(Margin::new(1, 0));
    let jobs = app.review_list();
    if jobs.is_empty() {
        empty(
            frame,
            area,
            "Nothing to review",
            &["Finished downloads wait here until you import or discard them.", "Albums you import show up here too."],
        );
        return vec![("1", "search"), ("2", "downloads")];
    }

    let narrow = area.width < 90;
    let (list, detail) = if narrow {
        let [list, detail] = Layout::vertical([
            Constraint::Length((u16::try_from(jobs.len()).unwrap_or(4) + 2).min(8)),
            Constraint::Fill(1),
        ])
        .spacing(1)
        .areas(area);
        (list, detail)
    } else {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(34), Constraint::Fill(1)]).spacing(2).areas(area);
        (list, detail)
    };

    draw_list(frame, list, app, &jobs, narrow);
    let job = jobs.get(app.review_selected).copied();
    if let Some(job) = job {
        draw_detail(frame, detail, app, job);
    }

    match job.map(|j| j.status) {
        Some(JobStatus::Ready) => {
            let import = if job.is_some_and(|j| app.can_import(j)) { "import" } else { "needs an admin" };
            vec![("i", import), ("x", "discard"), ("f", "find another"), ("J K", "scroll")]
        }
        _ => vec![("x", "forget"), ("J K", "scroll")],
    }
}

fn verdict(app: &App, job: &DownloadJob) -> (String, ratatui::style::Color) {
    if job.status == JobStatus::Imported {
        return (ago(job.imported_at.unwrap_or(job.created_at)), FAINT);
    }
    match job.review {
        ReviewState::Waiting | ReviewState::Checking => (format!("{} checking", spinner()), MUTED),
        ReviewState::Failed => ("✗ check failed".into(), ERR),
        ReviewState::Ready => match app.reports.get(&job.id) {
            Some(Loadable::Ready(report)) => {
                if report.blocked_reason.is_some() {
                    ("✗ blocked".into(), ERR)
                } else {
                    let problems = report.tracks.iter().filter(|t| t.problem.is_some()).count();
                    if problems > 0 {
                        (format!("! {problems} to check"), WARN)
                    } else if !report.conflicts.is_empty() {
                        ("! replaces files".into(), WARN)
                    } else {
                        ("✓ clean".into(), OK)
                    }
                }
            }
            Some(Loadable::Failed(_)) => ("✗ no report".into(), ERR),
            _ => ("ready".into(), WARN),
        },
    }
}

fn draw_list(frame: &mut Frame<'_>, area: Rect, app: &App, jobs: &[&DownloadJob], narrow: bool) {
    let mut rows = Vec::new();
    let mut selected_row = 0;
    let waiting = jobs.iter().filter(|j| j.status == JobStatus::Ready).count();
    let mut section = None;
    for (i, job) in jobs.iter().enumerate() {
        let imported = job.status == JobStatus::Imported;
        if section != Some(imported) {
            if section.is_some() {
                rows.push(Row::new([""]));
            }
            let (title, count) =
                if imported { ("RECENTLY IMPORTED", jobs.len() - waiting) } else { ("TO REVIEW", waiting) };
            rows.push(Row::new([Cell::from(Line::from(vec![
                Span::styled(title, bold(MUTED)),
                Span::styled(format!("  {count}"), fg(FAINT)),
            ]))]));
            section = Some(imported);
        }
        if i == app.review_selected {
            selected_row = rows.len();
        }
        let (text, color) = verdict(app, job);
        let who = match (&app.me, &job.requested_by) {
            (Some(me), Some(by)) if *by != me.username => format!("  {by}"),
            _ => String::new(),
        };
        rows.push(
            Row::new([Cell::from(vec![
                Line::from(vec![
                    Span::styled(job.title.clone(), fg(if imported { MUTED } else { TEXT })),
                    Span::styled(who, fg(FAINT)),
                ]),
                Line::from(vec![
                    Span::styled(job.parent.clone().unwrap_or_default(), fg(FAINT)),
                    Span::raw("  "),
                    Span::styled(text, fg(color)),
                ]),
            ])])
            .height(2),
        );
    }
    let block = if narrow { Block::new() } else { Block::new().borders(Borders::RIGHT).border_style(fg(FAINT)) };
    let mut state = TableState::default().with_selected(Some(selected_row));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Fill(1)])
            .row_highlight_style(selected_style(true))
            .highlight_symbol("▌")
            .block(block),
        area,
        &mut state,
    );
}

fn draw_detail(frame: &mut Frame<'_>, area: Rect, app: &App, job: &DownloadJob) {
    if job.status == JobStatus::Imported {
        let mut lines = vec![
            Line::from(Span::styled(job.title.clone(), bold(TEXT))),
            Line::from(Span::styled(job.parent.clone().unwrap_or_default(), fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled("✓ In your library", fg(OK))),
        ];
        if let Some(to) = &job.imported_to {
            lines.push(Line::from(vec![Span::styled("  in ", fg(MUTED)), Span::styled(to.clone(), fg(TEXT))]));
        }
        if let Some(at) = job.imported_at {
            lines.push(Line::from(Span::styled(format!("  {}", ago(at)), fg(MUTED))));
        }
        if let Some(by) = &job.requested_by {
            lines.push(Line::from(Span::styled(format!("  asked for by {by}"), fg(FAINT))));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("{} files from {}", job.files.len(), if job.folder.is_empty() { "a fetch" } else { &job.username }),
            fg(FAINT),
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        return;
    }
    match (job.review, app.reports.get(&job.id)) {
        (_, Some(Loadable::Ready(report))) => {
            let lines = report_lines(app, job, report, area.width);
            frame.render_widget(Paragraph::new(lines).scroll((app.report_scroll, 0)), area);
        }
        (_, Some(Loadable::Failed(message))) => {
            empty(frame, area, "Couldn't load the report", &[message, "Press R to try again."]);
        }
        (ReviewState::Failed, _) => {
            empty(frame, area, "The files couldn't be checked", &["Discard it (x) or find another copy (f)."]);
        }
        _ => empty(
            frame,
            area,
            &format!("{} Checking the files", spinner()),
            &["Every file is played through to check its sound."],
        ),
    }
}

fn report_lines(app: &App, job: &DownloadJob, report: &ReviewReport, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(report.album.clone(), bold(TEXT))),
        Line::from(Span::styled(
            format!("{}{}", report.album_artist, report.year.map(|y| format!(" · {y}")).unwrap_or_default()),
            fg(MUTED),
        )),
        Line::from(""),
    ];
    let length: u32 = report.tracks.iter().filter_map(|t| t.duration_secs).sum();
    let problems = report.tracks.iter().filter(|t| t.problem.is_some()).count();
    if let Some(reason) = &report.blocked_reason {
        lines.push(Line::from(Span::styled(format!("✗ {reason}"), fg(ERR))));
    } else if problems == 0 {
        lines.push(Line::from(Span::styled(
            format!("✓ {} tracks, {}, all play cleanly", report.tracks.len(), duration(length)),
            fg(OK),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("! {problems} of {} tracks need a look", report.tracks.len()),
            fg(WARN),
        )));
    }
    if !app.can_import(job) {
        lines.push(Line::from(Span::styled("An admin approves this before it goes into the library.", fg(MUTED))));
    }
    for warning in &report.warnings {
        lines.push(Line::from(Span::styled(format!("! {warning}"), fg(WARN))));
    }
    if !report.conflicts.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("! Replaces {} file(s) already in your library", report.conflicts.len()),
            fg(WARN),
        )));
    }
    if let Some(dir) = report.tracks.first().and_then(|t| t.destination.rsplit_once('/')).map(|(d, _)| d.to_owned()) {
        lines.push(Line::from(vec![Span::styled("→ ", fg(ACCENT)), Span::styled(dir, fg(MUTED))]));
    }
    lines.push(Line::from(""));
    lines.push(heading("Tracks", "", width));
    let title_width = usize::from(width).saturating_sub(30).max(12);
    for t in &report.tracks {
        let title: String = if t.title.chars().count() > title_width {
            let mut cut: String = t.title.chars().take(title_width - 1).collect();
            cut.push('…');
            cut
        } else {
            format!("{:<title_width$}", t.title)
        };
        let position = if t.disc > 1 { format!("{}-{:02}", t.disc, t.track) } else { format!("{:>4}", t.track) };
        let mut spans = vec![
            Span::styled(format!("{position} "), fg(FAINT)),
            Span::styled(title, fg(TEXT)),
            Span::styled(format!(" {:>6}", t.duration_secs.map(duration).unwrap_or_default()), fg(MUTED)),
            Span::styled(format!("  {}", t.quality_label.clone().unwrap_or_default()), fg(MUTED)),
        ];
        if report.conflicts.contains(&t.destination) {
            spans.push(Span::styled("  replaces", fg(WARN)));
        }
        lines.push(Line::from(spans));
        if let Some(problem) = &t.problem {
            let color = if t.suspect_transcode { WARN } else { ERR };
            lines.push(Line::from(Span::styled(format!("       ! {problem}"), fg(color))));
        }
    }
    if let Some(cover) = &report.cover {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled("  cover ", fg(FAINT)), Span::styled(cover.clone(), fg(MUTED))]));
    }
    lines
}
