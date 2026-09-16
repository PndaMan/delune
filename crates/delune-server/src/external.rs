//! Fetching from somewhere other than Soulseek, with a command you choose.
//!
//! delune doesn't get around anyone's copy protection or terms, so it has no
//! built-in downloaders for streaming services. Instead, people who manage delune
//! can point it at a program they already use and trust — `yt-dlp` for a YouTube
//! or SoundCloud link, say — and delune runs it for a link, then treats whatever
//! lands in the staging folder like any other download: checked, reviewed and
//! imported.
//!
//! The program and its arguments come from settings, never from the request, and
//! they're run directly (no shell), so a pasted link can't turn into a command. Only
//! `{url}` and `{output}` are substituted. Whoever can set this can run programs as
//! the delune server, which is why it needs the manage permission and is off by
//! default.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, ExternalSource, JobStatus};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

/// How long a fetch may run before it's given up on.
const TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Lines of the program's output kept to show when it fails.
const KEEP_OUTPUT: usize = 20;

#[derive(Debug, Default)]
pub struct External {
    store: Option<Arc<Database>>,
    settings: std::sync::Mutex<ExternalSource>,
}

impl External {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let settings = db.load("external").unwrap_or_default();
        Self { store: Some(db.clone()), settings: std::sync::Mutex::new(settings) }
    }

    #[must_use]
    pub fn settings(&self) -> ExternalSource {
        self.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn set(&self, settings: &ExternalSource) {
        *self.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.clone();
        if let Some(db) = &self.store {
            db.save("external", settings);
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// What to fetch, and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FetchRequest {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
}

/// `GET /api/v1/external`
#[utoipa::path(
    get,
    operation_id = "external_settings",
    path = "/api/v1/external",
    tag = "settings",
    responses(
        (status = 200, description = "OK", body = ExternalSource),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn settings(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change where delune fetches from") {
        return denied;
    }
    Json(app.external.settings()).into_response()
}

/// `PUT /api/v1/external`
#[utoipa::path(
    put,
    operation_id = "external_update",
    path = "/api/v1/external",
    tag = "settings",
    request_body = ExternalSource,
    responses(
        (status = 200, description = "Saved", body = ExternalSource),
        (status = 400, description = "Bad settings", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(mut settings): Json<ExternalSource>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change where delune fetches from") {
        return denied;
    }
    settings.program = settings.program.trim().to_owned();
    settings.arguments = settings.arguments.iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect();
    if settings.enabled {
        if settings.program.is_empty() {
            return error(StatusCode::BAD_REQUEST, "no-program", "Name the program delune should run.");
        }
        if !settings.arguments.iter().any(|a| a.contains("{url}")) {
            return error(StatusCode::BAD_REQUEST, "no-url", "One argument has to contain {url}.");
        }
        if !settings.arguments.iter().any(|a| a.contains("{output}")) {
            return error(
                StatusCode::BAD_REQUEST,
                "no-output",
                "One argument has to contain {output}, the folder to write to.",
            );
        }
    }
    tracing::info!(by = %user.username, program = %settings.program, enabled = settings.enabled, "fetch command changed");
    app.external.set(&settings);
    Json(app.external.settings()).into_response()
}

/// `POST /api/v1/external/fetch`: run the command for a link, then review what it got.
#[utoipa::path(
    post,
    operation_id = "external_fetch",
    path = "/api/v1/external/fetch",
    tag = "downloads",
    request_body = FetchRequest,
    responses(
        (status = 201, description = "Fetching", body = delune_core::api::DownloadJob),
        (status = 400, description = "Not a web address", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 409, description = "No fetch command is set up", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn fetch(State(app): State<AppState>, user: CurrentUser, Json(request): Json<FetchRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "fetch with the command") {
        return denied;
    }
    let settings = app.external.settings();
    if !settings.enabled || settings.program.is_empty() {
        return error(
            StatusCode::CONFLICT,
            "no-command",
            "No fetch command is set up. Add one under Settings if you have a downloader you trust.",
        );
    }
    let url = request.url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(char::is_whitespace) {
        return error(StatusCode::BAD_REQUEST, "bad-url", "That doesn't look like a web address.");
    }
    let title = request.title.trim();
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no-title", "Say what this is, so it can be reviewed.");
    }

    let job =
        crate::downloads::begin_external(&app, &settings.program, title, request.artist.as_deref(), &user.username);
    let staging = crate::downloads::staging_dir(&app.data_dir, &job.id);
    tracing::info!(id = %job.id, by = %user.username, program = %settings.program, "fetching with a command");
    let (app, id, url) = (app.clone(), job.id.clone(), url.to_owned());
    tokio::spawn(async move {
        let outcome = run(&settings, &url, &staging).await;
        crate::downloads::finish_external(&app, &id, outcome).await;
    });
    (StatusCode::CREATED, Json(job)).into_response()
}

/// Run the command, returning what went wrong if anything did.
async fn run(settings: &ExternalSource, url: &str, staging: &Path) -> Result<(), String> {
    if let Err(e) = tokio::fs::create_dir_all(staging).await {
        return Err(format!("Couldn't make a folder to download into: {e}"));
    }
    let output = staging.display().to_string();
    let arguments: Vec<String> =
        settings.arguments.iter().map(|a| a.replace("{url}", url).replace("{output}", &output)).collect();
    let mut child = match tokio::process::Command::new(&settings.program)
        .args(&arguments)
        .current_dir(staging)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{} isn't installed where delune can see it.", settings.program));
        }
        Err(e) => return Err(format!("Couldn't run {}: {e}", settings.program)),
    };

    // Keep the last few lines of output; a failure is usually explained there.
    let mut tail: Vec<String> = Vec::new();
    let mut lines = BufReader::new(child.stderr.take().expect("stderr is piped")).lines();
    let wait = async {
        while let Ok(Some(line)) = lines.next_line().await {
            if tail.len() == KEEP_OUTPUT {
                tail.remove(0);
            }
            tail.push(line);
        }
        child.wait().await
    };
    match tokio::time::timeout(TIMEOUT, wait).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("{} stopped ({status}). {}", settings.program, tail.join(" "))),
        Ok(Err(e)) => Err(format!("Couldn't wait for {}: {e}", settings.program)),
        Err(_) => Err(format!("{} took longer than an hour; gave up.", settings.program)),
    }
}

/// Whether a finished fetch left anything worth reviewing.
#[must_use]
pub fn status_of(files: usize, outcome: &Result<(), String>) -> JobStatus {
    match (files, outcome) {
        (0, _) | (_, Err(_)) => JobStatus::Failed,
        _ => JobStatus::Ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(program: &str, arguments: &[&str]) -> ExternalSource {
        ExternalSource {
            enabled: true,
            program: program.into(),
            arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[tokio::test]
    async fn runs_the_command_with_the_link_and_folder_filled_in() {
        let dir = std::env::temp_dir().join(format!("delune-external-{}", std::process::id()));
        let settings =
            command("sh", &["-c", "printf %s \"$1\" > \"$2/what-it-got.txt\"", "delune", "{url}", "{output}"]);
        run(&settings, "https://example.com/song", &dir).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("what-it-got.txt")).unwrap(), "https://example.com/song");

        // A link can't become a command: arguments are passed as arguments.
        let sneaky = "https://example.com/x;touch%20/tmp/delune-should-not-exist";
        run(&settings, sneaky, &dir).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("what-it-got.txt")).unwrap(), sneaky);
        assert!(!Path::new("/tmp/delune-should-not-exist").exists());

        let failing =
            run(&command("sh", &["-c", "echo nope >&2; exit 3", "delune", "{url}", "{output}"]), "https://x/y", &dir)
                .await;
        assert!(failing.unwrap_err().contains("nope"));
        let missing = run(&command("delune-not-a-program", &["{url}", "{output}"]), "https://x/y", &dir).await;
        assert!(missing.unwrap_err().contains("isn't installed"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_fetch_that_got_nothing_counts_as_failed() {
        assert_eq!(status_of(3, &Ok(())), JobStatus::Ready);
        assert_eq!(status_of(0, &Ok(())), JobStatus::Failed);
        assert_eq!(status_of(3, &Err("stopped".into())), JobStatus::Failed);
    }
}
