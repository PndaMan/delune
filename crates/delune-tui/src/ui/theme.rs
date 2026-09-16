//! Colours and small formatting helpers shared by every screen.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::matching::Mark;

pub const ACCENT: Color = Color::Rgb(0xae, 0xb8, 0xff);
pub const TEXT: Color = Color::Rgb(0xe4, 0xe8, 0xf4);
pub const MUTED: Color = Color::Rgb(0x8b, 0x91, 0xa8);
pub const FAINT: Color = Color::Rgb(0x4a, 0x51, 0x68);
pub const OK: Color = Color::Rgb(0x6f, 0xd3, 0x9b);
pub const WARN: Color = Color::Rgb(0xf2, 0xc1, 0x6b);
pub const ERR: Color = Color::Rgb(0xff, 0x7a, 0x85);
pub const HIRES: Color = Color::Rgb(0xf5, 0xc4, 0x6b);
pub const CD: Color = Color::Rgb(0x7d, 0xd8, 0xc0);
pub const SELECTED_BG: Color = Color::Rgb(0x2a, 0x31, 0x47);

pub fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

pub fn bold(color: Color) -> Style {
    Style::new().fg(color).add_modifier(Modifier::BOLD)
}

pub fn human_bytes(bytes: u64) -> String {
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

pub fn human_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1.0 {
        "—".into()
    } else if bytes_per_sec >= 1e6 {
        format!("{:.1} MB/s", bytes_per_sec / 1e6)
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1e3)
    }
}

pub fn duration(secs: u32) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Time left at `per_sec`, roughly.
pub fn eta(remaining: u64, per_sec: f64) -> Option<String> {
    if per_sec < 1.0 || remaining == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)] // display only
    let secs = (remaining as f64 / per_sec) as u64;
    Some(match secs {
        0..60 => format!("{secs}s left"),
        60..3600 => format!("{} min left", secs.div_ceil(60)),
        _ => format!("{}h {}m left", secs / 3600, secs / 60 % 60),
    })
}

/// "3 hours ago", for a Unix time.
pub fn ago(unix: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let secs = now.saturating_sub(unix);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

pub fn bar(ratio: f64, width: usize) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)] // display only
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("{}{}", "━".repeat(filled), "─".repeat(width - filled))
}

/// A release's mark as a short coloured label.
pub fn mark_span(mark: Option<Mark>) -> Span<'static> {
    let Some(mark) = mark else { return Span::raw("") };
    match mark {
        Mark::InLibrary => Span::styled("✓ have", fg(OK)),
        Mark::Imported => Span::styled("✓ imported", fg(OK)),
        Mark::Partial(o) => Span::styled(format!("◐ {}/{}", o.owned, o.total), fg(CD)),
        Mark::Downloading(p) => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // display only
            let percent = (p * 100.0).round() as u32;
            Span::styled(format!("↓ {percent}%"), fg(ACCENT))
        }
        Mark::Queued => Span::styled("… queued", fg(MUTED)),
        Mark::OtherCopy => Span::styled("↓ elsewhere", fg(MUTED)),
        Mark::Review => Span::styled("● review", fg(WARN)),
        Mark::Failed => Span::styled("✗ failed", fg(ERR)),
    }
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner() -> &'static str {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    SPINNER[usize::try_from(millis / 80 % 10).unwrap_or(0)]
}
