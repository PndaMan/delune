//! One place to see whether delune is healthy, and what to do when it isn't.
//!
//! Each check is cheap (the slowest asks Navidrome and the metadata service, with short
//! timeouts) and answers in the reader's terms: what's wrong, and where to fix it.

use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{Json, extract::State, response::IntoResponse, response::Response};
use delune_core::api::{CheckState, DiagnosticCheck, Diagnostics, JobStatus, PortMappingState, SoulseekState};

use crate::AppState;
use crate::accounts::CurrentUser;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Note when the server started, so young connections get time to settle.
pub fn mark_start() {
    let _ = STARTED.get_or_init(Instant::now);
}

fn uptime() -> Duration {
    STARTED.get().map_or(Duration::ZERO, Instant::elapsed)
}

const GB: u64 = 1_000_000_000;

struct Check(DiagnosticCheck);

impl Check {
    fn new(id: &str, area: &str, title: &str, state: CheckState, summary: impl Into<String>) -> Self {
        Self(DiagnosticCheck {
            id: id.into(),
            area: area.into(),
            title: title.into(),
            state,
            summary: summary.into(),
            fix: None,
            link: None,
        })
    }

    fn fix(mut self, fix: impl Into<String>) -> Self {
        self.0.fix = Some(fix.into());
        self
    }

    fn link(mut self, link: &str) -> Self {
        self.0.link = Some(link.into());
        self
    }
}

/// "navidrome" → "Navidrome".
fn display_name(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn human_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let b = bytes as f64;
    if bytes >= GB { format!("{:.1} GB", b / 1e9) } else { format!("{:.0} MB", b / 1e6) }
}

/// Free space where `path` lives.
#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize))
}

#[cfg(not(unix))]
fn free_space(_: &Path) -> Option<u64> {
    None
}

fn disk_check(id: &str, area: &str, title: &str, path: &Path, what: &str) -> Check {
    match free_space(path) {
        Some(free) if free < 2 * GB => Check::new(
            id,
            area,
            title,
            CheckState::Problem,
            format!("Only {} free where {what} lives.", human_bytes(free)),
        )
        .fix("Free some space. Downloads and imports fail when the disk fills up."),
        Some(free) if free < 10 * GB => {
            Check::new(id, area, title, CheckState::Warning, format!("{} free where {what} lives.", human_bytes(free)))
                .fix("Space is getting low; a few albums in hi-res fill it.")
        }
        Some(free) => Check::new(id, area, title, CheckState::Ok, format!("{} free.", human_bytes(free))),
        None => Check::new(id, area, title, CheckState::Info, "Couldn't tell how much space is free."),
    }
}

/// Whether delune can create files in `dir`.
fn writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(".delune-write-check");
    std::fs::write(&probe, b"").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

fn soulseek_checks(app: &AppState, checks: &mut Vec<Check>) {
    let status = crate::soulseek_status_of(app);
    let area = "soulseek";
    let login = match status.state {
        SoulseekState::NotConfigured => Check::new(
            "soulseek-login",
            area,
            "Soulseek account",
            CheckState::Problem,
            "No Soulseek account is set up, so nothing can be searched or downloaded.",
        )
        .fix("Add a Soulseek username and password.")
        .link("/settings/connections"),
        SoulseekState::Online => Check::new(
            "soulseek-login",
            area,
            "Soulseek account",
            CheckState::Ok,
            format!("Signed in as {}.", status.username.as_deref().unwrap_or("?")),
        ),
        SoulseekState::Connecting | SoulseekState::Reconnecting => Check::new(
            "soulseek-login",
            area,
            "Soulseek account",
            CheckState::Warning,
            status.message.clone().unwrap_or_else(|| "Connecting to Soulseek.".into()),
        )
        .fix("If this lasts, check that this machine (or its VPN) can reach the internet."),
        SoulseekState::Stopped => Check::new(
            "soulseek-login",
            area,
            "Soulseek account",
            CheckState::Problem,
            status.message.clone().unwrap_or_else(|| "The Soulseek client stopped.".into()),
        )
        .fix("Check the username and password, then restart delune.")
        .link("/settings/connections"),
    };
    checks.push(login);
    if status.state != SoulseekState::Online {
        return;
    }

    let port = status.listen_port.map_or_else(|| "its port".to_owned(), |p| format!("port {p}"));
    let reachable = if status.reachable {
        Check::new(
            "soulseek-port",
            area,
            "Reachable from the internet",
            CheckState::Ok,
            format!("Other people connect to {port}, so you get more results and can share."),
        )
    } else if uptime() < Duration::from_secs(15 * 60) {
        Check::new(
            "soulseek-port",
            area,
            "Reachable from the internet",
            CheckState::Info,
            format!("Nobody has connected to {port} yet; delune started recently, so give it a few minutes."),
        )
    } else {
        let mapping = match status.port_mapping.state {
            PortMappingState::Failed => format!(
                " Automatic port forwarding failed: {}.",
                status.port_mapping.message.as_deref().unwrap_or("no reason given")
            ),
            _ => String::new(),
        };
        Check::new(
            "soulseek-port",
            area,
            "Reachable from the internet",
            CheckState::Warning,
            format!("Nobody has connected to {port} since delune started.{mapping}"),
        )
        .fix(format!(
            "Forward {port} (TCP) on your router, or on your VPN if delune uses one, or turn on automatic port forwarding. Searches work without it, but you'll find fewer results and nobody can download from you."
        ))
        .link("/settings/connections")
    };
    checks.push(reachable);

    if let Some(ip) = &status.public_ip {
        checks.push(Check::new(
            "soulseek-address",
            area,
            "Public address",
            CheckState::Info,
            format!("Soulseek sees you at {ip}. Behind a VPN, this should be the VPN's address, not your home's."),
        ));
    }
}

fn sharing_checks(app: &AppState, checks: &mut Vec<Check>) {
    let area = "sharing";
    let status = app.sharing.status(app.library.library_dir.as_deref());
    let check = if !status.settings.enabled {
        Check::new(
            "sharing",
            area,
            "Sharing",
            CheckState::Info,
            "Sharing is off. Many people only upload to those who share back.",
        )
        .fix("Share your library to give something back.")
        .link("/settings/sharing")
    } else if let Some(error) = &status.error {
        Check::new("sharing", area, "Sharing", CheckState::Problem, format!("The library couldn't be scanned: {error}"))
            .link("/settings/sharing")
    } else if status.scanning {
        Check::new("sharing", area, "Sharing", CheckState::Info, "Scanning the library for files to share.")
    } else if status.files == 0 {
        Check::new("sharing", area, "Sharing", CheckState::Warning, "Sharing is on, but no files were found to share.")
            .fix("Check the library folder has music in it and delune can read it.")
            .link("/settings/sharing")
    } else {
        Check::new(
            "sharing",
            area,
            "Sharing",
            CheckState::Ok,
            format!("Sharing {} files in {} folders.", status.files, status.folders),
        )
    };
    checks.push(check);

    if status.settings.enabled
        && let Some(client) = &app.soulseek
        && crate::soulseek_status_of(app).state == SoulseekState::Online
    {
        let check = if client.has_search_parent() {
            Check::new(
                "search-network",
                area,
                "Other people's searches",
                CheckState::Ok,
                "Connected to the search network, so people searching can find your files.",
            )
        } else if uptime() < Duration::from_secs(10 * 60) {
            Check::new(
                "search-network",
                area,
                "Other people's searches",
                CheckState::Info,
                "Joining the search network; this takes a few minutes after starting.",
            )
        } else {
            Check::new(
                "search-network",
                area,
                "Other people's searches",
                CheckState::Warning,
                "Not connected to the search network, so few people will find your files.",
            )
            .fix("This usually follows from the listening port not being reachable.")
        };
        checks.push(check);
    }
}

async fn navidrome_check(app: &AppState) -> Check {
    let area = "library";
    match &app.navidrome {
        None => Check::new(
            "navidrome",
            area,
            "Navidrome",
            CheckState::Warning,
            "No Navidrome is connected, so delune can't tell what you already have.",
        )
        .link("/settings/connections"),
        Some(client) => match tokio::time::timeout(Duration::from_secs(6), client.ping()).await {
            Ok(Ok(info)) => {
                let version = info.server_version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
                let scanning = match tokio::time::timeout(Duration::from_secs(4), client.scan_status()).await {
                    Ok(Ok(scan)) if scan.scanning => " It's scanning the library now.".to_owned(),
                    Ok(Ok(scan)) => format!(" {} songs in the library.", scan.count),
                    _ => String::new(),
                };
                Check::new(
                    "navidrome",
                    area,
                    "Navidrome",
                    CheckState::Ok,
                    format!(
                        "Connected to {}{version}.{scanning}",
                        display_name(info.server_type.as_deref().unwrap_or("Navidrome"))
                    ),
                )
            }
            Ok(Err(error)) => Check::new(
                "navidrome",
                area,
                "Navidrome",
                CheckState::Problem,
                format!("Navidrome didn't answer: {error}."),
            )
            .fix("Check Navidrome is running and the address, username and password are right.")
            .link("/settings/connections"),
            Err(_) => {
                Check::new("navidrome", area, "Navidrome", CheckState::Problem, "Navidrome didn't answer in time.")
                    .fix("Check Navidrome is running and reachable from delune.")
                    .link("/settings/connections")
            }
        },
    }
}

async fn library_checks(app: &AppState, checks: &mut Vec<Check>) {
    let area = "library";
    checks.push(navidrome_check(app).await);

    match &app.library.library_dir {
        None => checks.push(
            Check::new(
                "library-folder",
                area,
                "Library folder",
                CheckState::Problem,
                "No library folder is set, so downloads can't be imported.",
            )
            .fix("Set the folder Navidrome reads your music from.")
            .link("/settings/connections"),
        ),
        Some(dir) => {
            let dir = dir.clone();
            let (write, disk) = tokio::task::spawn_blocking(move || {
                let write = if dir.is_dir() { writable(&dir) } else { Err("it doesn't exist".to_owned()) };
                (write, disk_check("library-disk", "library", "Space for music", &dir, "the library"))
            })
            .await
            .unwrap_or_else(|_| {
                (
                    Err("couldn't check".into()),
                    Check::new("library-disk", area, "Space for music", CheckState::Info, "Couldn't check."),
                )
            });
            let shown = app.library.library_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
            checks.push(match write {
                Ok(()) => Check::new(
                    "library-folder",
                    area,
                    "Library folder",
                    CheckState::Ok,
                    format!("Imports go to {shown}."),
                ),
                Err(e) => Check::new(
                    "library-folder",
                    area,
                    "Library folder",
                    CheckState::Problem,
                    format!("delune can't write to {shown}: {e}."),
                )
                .fix("Check the folder exists, is mounted, and belongs to the user delune runs as."),
            });
            checks.push(disk);
        }
    }

    let data_dir = app.data_dir.clone();
    if let Ok(check) = tokio::task::spawn_blocking(move || {
        disk_check("data-disk", "library", "Space for delune's data", &data_dir, "delune's data and downloads")
    })
    .await
    {
        checks.push(check);
    }
}

fn download_checks(app: &AppState, checks: &mut Vec<Check>) {
    let area = "downloads";
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let jobs = app.downloads.list();
    let failed =
        jobs.iter().filter(|j| j.status == JobStatus::Failed && now.saturating_sub(j.created_at) < 7 * 86_400).count();
    checks.push(if failed == 0 {
        Check::new("failed-downloads", area, "Failed downloads", CheckState::Ok, "Nothing failed this week.")
    } else {
        Check::new(
            "failed-downloads",
            area,
            "Failed downloads",
            CheckState::Warning,
            format!("{failed} download(s) failed this week."),
        )
        .fix("Resume them, or look for another copy.")
        .link("/downloads")
    });
    let stale =
        jobs.iter().filter(|j| j.status == JobStatus::Ready && now.saturating_sub(j.created_at) > 3 * 86_400).count();
    if stale > 0 {
        checks.push(
            Check::new(
                "review-waiting",
                area,
                "Waiting for review",
                CheckState::Info,
                format!("{stale} download(s) have waited more than three days to be reviewed."),
            )
            .link("/review"),
        );
    }
}

async fn service_checks(app: &AppState, checks: &mut Vec<Check>) {
    let reachable =
        tokio::time::timeout(Duration::from_secs(6), app.music_http.get("https://api.deezer.com/infos").send())
            .await
            .is_ok_and(|r| r.is_ok_and(|r| r.status().is_success()));
    checks.push(if reachable {
        Check::new(
            "metadata",
            "services",
            "Album information",
            CheckState::Ok,
            "Covers, tracklists and release dates are loading.",
        )
    } else {
        Check::new(
            "metadata",
            "services",
            "Album information",
            CheckState::Warning,
            "The music catalogue (Deezer) isn't answering, so covers, album pages and follows won't update.",
        )
        .fix("Check this machine can reach api.deezer.com; it usually recovers on its own.")
    });
}

/// `GET /api/v1/diagnostics`
#[utoipa::path(
    get,
    operation_id = "diagnostics",
    path = "/api/v1/diagnostics",
    tag = "server",
    responses(
        (status = 200, description = "OK", body = delune_core::api::Diagnostics),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn diagnostics(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "see diagnostics") {
        return denied;
    }
    let mut checks = Vec::new();
    soulseek_checks(&app, &mut checks);
    sharing_checks(&app, &mut checks);
    let mut library = Vec::new();
    let mut services = Vec::new();
    tokio::join!(library_checks(&app, &mut library), service_checks(&app, &mut services));
    checks.extend(library);
    download_checks(&app, &mut checks);
    checks.extend(services);
    Json(Diagnostics {
        checked_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        checks: checks.into_iter().map(|c| c.0).collect(),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nearly_full_disk_is_a_problem() {
        let dir = std::env::temp_dir();
        let check = disk_check("d", "library", "Space", &dir, "it").0;
        assert_ne!(check.summary, "", "free space is read on this system");
        assert!(writable(&dir).is_ok());
        assert!(writable(Path::new("/proc/definitely-not-writable")).is_err());
        assert_eq!(human_bytes(5 * GB / 2), "2.5 GB");
    }
}
