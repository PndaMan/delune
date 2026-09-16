//! Connections set from the web UI: the music folder, Navidrome and Soulseek.
//!
//! They're saved to `<data dir>/config.toml` (readable only by delune, since it
//! holds passwords), which fills in whatever command-line flags and environment
//! variables leave out; those always win, and the UI shows such settings as fixed.
//! Changing a connection restarts the server in place, because the Soulseek client
//! and Navidrome accounts are set up once at startup.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::ApiError;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::accounts::CurrentUser;

/// The file's contents. Every part is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soulseek: Option<SoulseekFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navidrome: Option<NavidromeFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SoulseekFile {
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NavidromeFile {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl FileConfig {
    /// Where the file lives for a data directory.
    #[must_use]
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.toml")
    }

    /// Read the file; missing means empty.
    ///
    /// # Errors
    ///
    /// When the file exists but can't be read or isn't valid.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{} is invalid: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
        }
    }

    /// Write the file, readable only by its owner.
    ///
    /// # Errors
    ///
    /// When the file or its folder can't be written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let text = format!(
            "# delune's connections, from `delune setup` or the web UI. Command-line flags and\n# DELUNE_* environment variables override anything here.\n\n{text}"
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        write_private(&tmp, text.as_bytes())?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// Which connections came from flags or environment variables, so the UI can't change them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Locked {
    pub library: bool,
    pub soulseek: bool,
    pub navidrome: bool,
}

/// `GET /api/v1/setup`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetupStatus {
    /// Nothing is connected yet: show the first-run setup.
    pub needed: bool,
    pub library_dir: Option<String>,
    pub soulseek_username: Option<String>,
    pub soulseek_port: Option<u16>,
    pub navidrome_url: Option<String>,
    pub navidrome_username: Option<String>,
    pub locked: Locked,
    /// Where settings are saved.
    pub config_path: String,
    /// Whether this server can restart itself to apply changes.
    pub can_restart: bool,
}

/// `PUT /api/v1/setup` and `POST /api/v1/setup/check`. Empty passwords keep the saved ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetupRequest {
    #[serde(default)]
    pub library_dir: Option<String>,
    #[serde(default)]
    pub soulseek: Option<SoulseekFile>,
    #[serde(default)]
    pub navidrome: Option<NavidromeFile>,
}

/// What checking each part found; `None` for parts not being changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetupCheck {
    pub library: Option<CheckResult>,
    pub soulseek: Option<CheckResult>,
    pub navidrome: Option<CheckResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CheckResult {
    pub ok: bool,
    pub message: String,
}

impl CheckResult {
    fn ok(message: impl Into<String>) -> Self {
        Self { ok: true, message: message.into() }
    }

    fn problem(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into() }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// `GET /api/v1/setup`
#[utoipa::path(
    get,
    operation_id = "setup_status",
    path = "/api/v1/setup",
    tag = "settings",
    responses(
        (status = 200, description = "OK", body = SetupStatus),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn status(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change server connections") {
        return denied;
    }
    let navidrome = app.navidrome_account.as_ref();
    Json(SetupStatus {
        needed: app.soulseek.is_none() && navidrome.is_none() && app.library.library_dir.is_none(),
        library_dir: app.library.library_dir.as_ref().map(|p| p.display().to_string()),
        soulseek_username: app.soulseek_username.clone(),
        soulseek_port: app.soulseek_port,
        navidrome_url: navidrome.map(|(url, _)| url.clone()),
        navidrome_username: navidrome.map(|(_, username)| username.clone()),
        locked: app.locked,
        config_path: FileConfig::path(&app.data_dir).display().to_string(),
        can_restart: cfg!(unix),
    })
    .into_response()
}

/// `POST /api/v1/setup/check`: try the connections without saving them.
#[utoipa::path(
    post,
    operation_id = "setup_check",
    path = "/api/v1/setup/check",
    tag = "settings",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "OK", body = SetupCheck),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn check(State(app): State<AppState>, user: CurrentUser, Json(request): Json<SetupRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change server connections") {
        return denied;
    }
    let saved = match FileConfig::load(&FileConfig::path(&app.data_dir)) {
        Ok(saved) => saved,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, "config-unreadable", &message),
    };
    let request = with_saved_passwords(request, &saved);
    Json(run_checks(&app, &request).await).into_response()
}

/// `PUT /api/v1/setup`: check, save and restart.
#[utoipa::path(
    put,
    operation_id = "setup_update",
    path = "/api/v1/setup",
    tag = "settings",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Saved; delune restarts", body = SetupCheck),
        (status = 422, description = "A check failed", body = SetupCheck),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn update(State(app): State<AppState>, user: CurrentUser, Json(request): Json<SetupRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change server connections") {
        return denied;
    }
    let path = FileConfig::path(&app.data_dir);
    let mut saved = match FileConfig::load(&path) {
        Ok(saved) => saved,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, "config-unreadable", &message),
    };
    let request = with_saved_passwords(request, &saved);
    let locked = |part: bool, given: bool| part && given;
    if locked(app.locked.library, request.library_dir.is_some())
        || locked(app.locked.soulseek, request.soulseek.is_some())
        || locked(app.locked.navidrome, request.navidrome.is_some())
    {
        return error(
            StatusCode::CONFLICT,
            "set-elsewhere",
            "That connection is set by a command-line flag or environment variable; change it there.",
        );
    }

    let checks = run_checks(&app, &request).await;
    if [&checks.library, &checks.soulseek, &checks.navidrome].into_iter().flatten().any(|c| !c.ok) {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(checks)).into_response();
    }

    if let Some(dir) = request.library_dir {
        saved.library_dir = (!dir.trim().is_empty()).then(|| PathBuf::from(dir.trim()));
    }
    if let Some(soulseek) = request.soulseek {
        saved.soulseek = (!soulseek.username.trim().is_empty()).then_some(soulseek);
    }
    if let Some(navidrome) = request.navidrome {
        saved.navidrome = (!navidrome.url.trim().is_empty()).then_some(navidrome);
    }
    if let Err(e) = saved.save(&path) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "config-unwritable", &format!("Couldn't save settings: {e}."));
    }
    tracing::info!(by = %user.username, path = %path.display(), "connections changed; restarting");
    restart_soon(&app);
    Json(checks).into_response()
}

/// Keep passwords that were left blank.
fn with_saved_passwords(mut request: SetupRequest, saved: &FileConfig) -> SetupRequest {
    if let (Some(given), Some(old)) = (request.soulseek.as_mut(), saved.soulseek.as_ref())
        && given.password.is_empty()
        && given.username == old.username
    {
        given.password.clone_from(&old.password);
    }
    if let (Some(given), Some(old)) = (request.navidrome.as_mut(), saved.navidrome.as_ref())
        && given.password.is_empty()
        && given.username == old.username
    {
        given.password.clone_from(&old.password);
    }
    request
}

/// Try connections outside a running server, as `delune setup` does before saving.
pub async fn check_offline(request: &SetupRequest) -> SetupCheck {
    run_checks(&AppState::default(), request).await
}

async fn run_checks(app: &AppState, request: &SetupRequest) -> SetupCheck {
    let library = request.library_dir.as_deref().map(|dir| check_library(dir.trim()));
    let navidrome = match &request.navidrome {
        Some(n) if n.url.trim().is_empty() => {
            Some(CheckResult::ok("Navidrome will be disconnected, and anyone who can reach delune can use it."))
        }
        Some(n) => Some(check_navidrome(n).await),
        None => None,
    };
    let soulseek = match &request.soulseek {
        Some(s) if s.username.trim().is_empty() => Some(CheckResult::ok("Soulseek will be disconnected.")),
        Some(s) => Some(check_soulseek(app, s).await),
        None => None,
    };
    SetupCheck { library, soulseek, navidrome }
}

fn check_library(dir: &str) -> CheckResult {
    if dir.is_empty() {
        return CheckResult::ok("No music folder: downloads can be reviewed but not imported.");
    }
    let path = Path::new(dir);
    if !path.is_absolute() {
        return CheckResult::problem("Use the full path to the folder, starting with /.");
    }
    if !path.is_dir() {
        return CheckResult::problem("That folder doesn't exist, or delune can't see it.");
    }
    let probe = path.join(".delune-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            CheckResult::ok("delune can read and write this folder.")
        }
        Err(e) => CheckResult::problem(format!("delune can't write to this folder ({e}); imports need that.")),
    }
}

async fn check_navidrome(settings: &NavidromeFile) -> CheckResult {
    let credentials =
        delune_navidrome::Credentials { username: settings.username.clone(), password: settings.password.clone() };
    let client = match delune_navidrome::Client::new(settings.url.trim(), credentials) {
        Ok(client) => client,
        Err(e) => return CheckResult::problem(format!("That address doesn't look right: {e}.")),
    };
    if let Err(e) = client.ping().await {
        return CheckResult::problem(format!("Couldn't sign in to Navidrome: {e}."));
    }
    match client.user(&settings.username).await {
        Ok(user) if user.admin_role => CheckResult::ok("Signed in to Navidrome as an admin."),
        Ok(_) => CheckResult::problem("That account isn't a Navidrome admin; library rescans need one."),
        Err(e) => CheckResult::problem(format!("Signed in, but couldn't read the account: {e}.")),
    }
}

async fn check_soulseek(app: &AppState, settings: &SoulseekFile) -> CheckResult {
    if settings.password.is_empty() {
        return CheckResult::problem("Enter the Soulseek password.");
    }
    // A second login with the running account would sign the server out.
    if app.soulseek.is_some() && app.soulseek_username.as_deref() == Some(settings.username.as_str()) {
        return CheckResult::ok("This is the account delune is using.");
    }
    let mut config = delune_soulseek::Config::new(settings.username.clone(), settings.password.clone());
    config.listen_port = None;
    let client = delune_soulseek::Client::start(config);
    let mut state = client.state();
    let outcome = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match state.borrow_and_update().clone() {
                delune_soulseek::SessionState::Online { .. } => {
                    return CheckResult::ok("Signed in to Soulseek. New names are registered on first sign-in.");
                }
                delune_soulseek::SessionState::Stopped(reason) => {
                    return CheckResult::problem(format!("Soulseek refused the sign-in: {reason:?}."));
                }
                delune_soulseek::SessionState::Reconnecting { reason, .. } => {
                    return CheckResult::problem(format!("Couldn't reach Soulseek: {reason}."));
                }
                delune_soulseek::SessionState::Connecting { .. } => {}
            }
            if state.changed().await.is_err() {
                return CheckResult::problem("The Soulseek check stopped unexpectedly.");
            }
        }
    })
    .await;
    outcome.unwrap_or_else(|_| CheckResult::problem("Soulseek didn't answer in time. Try again in a minute."))
}

/// Save what's pending and start this binary again with the same arguments.
fn restart_soon(app: &AppState) {
    let downloads = app.downloads.clone();
    tokio::spawn(async move {
        // Let the response reach the browser first.
        tokio::time::sleep(Duration::from_millis(500)).await;
        downloads.save_if_changed();
        restart();
    });
}

#[cfg(unix)]
fn restart() {
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        tracing::error!("can't find delune's own binary to restart; restart it by hand");
        return;
    };
    let error = std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec();
    tracing::error!(%error, "couldn't restart; restart delune by hand");
}

#[cfg(not(unix))]
fn restart() {
    tracing::warn!("restart delune to apply the new connections");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_files_round_trip_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("delune-setup-{}", std::process::id()));
        let path = FileConfig::path(&dir);
        assert_eq!(FileConfig::load(&path).unwrap(), FileConfig::default());
        let config = FileConfig {
            library_dir: Some("/srv/music".into()),
            soulseek: Some(SoulseekFile { username: "moon".into(), password: "secret".into(), port: Some(2240) }),
            navidrome: None,
        };
        config.save(&path).unwrap();
        assert_eq!(FileConfig::load(&path).unwrap(), config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&path, "library_dir = [").unwrap();
        assert!(FileConfig::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn blank_passwords_keep_the_saved_ones_for_the_same_account() {
        let saved = FileConfig {
            soulseek: Some(SoulseekFile { username: "moon".into(), password: "secret".into(), port: None }),
            ..FileConfig::default()
        };
        let keep = SetupRequest {
            soulseek: Some(SoulseekFile { username: "moon".into(), password: String::new(), port: None }),
            ..SetupRequest::default()
        };
        assert_eq!(with_saved_passwords(keep, &saved).soulseek.unwrap().password, "secret");
        let other = SetupRequest {
            soulseek: Some(SoulseekFile { username: "sun".into(), password: String::new(), port: None }),
            ..SetupRequest::default()
        };
        assert_eq!(with_saved_passwords(other, &saved).soulseek.unwrap().password, "");
    }

    #[test]
    fn library_checks_need_a_writable_absolute_folder() {
        assert!(!check_library("music").ok);
        assert!(!check_library("/definitely/not/here").ok);
        assert!(check_library("").ok);
        assert!(check_library(&std::env::temp_dir().display().to_string()).ok);
    }
}
