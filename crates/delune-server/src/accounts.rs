//! Accounts: signing in with Navidrome, sessions, and what each person may do.
//!
//! delune keeps no passwords. Signing in asks Navidrome whether the username and
//! password work (`getUser`, which also says whether the person is a Navidrome
//! admin), then delune issues its own session token. The browser gets it as an
//! `HttpOnly` cookie; the TUI asks for it in the response and sends it as a bearer
//! token. Only a SHA-256 hash of each token is stored, in `<data dir>/accounts.json`.
//!
//! Admins come from Navidrome and can do everything. Everyone else gets
//! [`Permissions::MEMBER`] the first time they sign in, which an admin can change.
//!
//! Without Navidrome configured there is nobody to ask, so delune runs in open mode:
//! whoever can reach it is treated as the admin, and the server says so at startup.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{FromRequestParts, Path as UrlPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, Appearance, AuthMode, LoginRequest, Me, People, Permissions, Person, SessionInfo};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::store::Database;

pub const COOKIE: &str = "delune_session";
/// Sessions end after this long without being used.
const SESSION_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Failed sign-ins allowed per username before a pause.
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);
/// The name of the implicit admin in open mode.
pub const OPEN_MODE_USER: &str = "admin";

#[derive(Debug)]
pub struct Accounts {
    navidrome_url: Option<String>,
    store: Option<Arc<Database>>,
    avatars: Option<PathBuf>,
    state: Mutex<Stored>,
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    require_approval: bool,
    #[serde(default)]
    users: BTreeMap<String, UserRecord>,
    /// Keyed by the SHA-256 of the token.
    #[serde(default)]
    sessions: HashMap<String, Session>,
    /// Appearance and profile pictures, by username (open mode's admin included).
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Profile {
    #[serde(default)]
    appearance: Appearance,
    /// Content type and version of their picture, when they've set one.
    #[serde(default)]
    avatar: Option<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserRecord {
    username: String,
    admin: bool,
    permissions: Permissions,
    created_at: u64,
    last_login: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    username: String,
    created_at: u64,
    last_seen: u64,
    /// What signed in, from its user agent.
    #[serde(default)]
    device: Option<String>,
}

/// The person making a request.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub username: String,
    pub admin: bool,
    pub permissions: Permissions,
    pub can_import: bool,
    /// Public id of the session this request came with (none in open mode).
    pub session: Option<String>,
}

impl CurrentUser {
    /// A 403 response unless `allowed` holds for this person's permissions.
    #[must_use]
    pub fn refuse_unless(&self, allowed: fn(&Permissions) -> bool, what: &str) -> Option<Response> {
        (!allowed(&self.permissions)).then(|| {
            error(StatusCode::FORBIDDEN, "not-allowed", &format!("You don't have permission to {what}. Ask an admin."))
        })
    }

    /// Whether this person may see or act on something `owner` started. Things with
    /// no owner (from before accounts) belong to the people who manage delune.
    #[must_use]
    pub fn can_see(&self, owner: Option<&str>) -> bool {
        self.permissions.manage || owner == Some(self.username.as_str())
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The id shown for a session: a prefix of its stored hash, so it can't be used to sign in.
fn public_id(key: &str) -> String {
    key.chars().take(16).collect()
}

/// "Firefox on Linux", from a user agent.
fn describe_device(user_agent: &str) -> String {
    if user_agent.starts_with("delune") || user_agent.starts_with("reqwest") {
        return "Terminal UI".into();
    }
    let browser =
        [("Edg/", "Edge"), ("OPR/", "Opera"), ("Firefox/", "Firefox"), ("Chrome/", "Chrome"), ("Safari/", "Safari")]
            .into_iter()
            .find(|(marker, _)| user_agent.contains(marker))
            .map(|(_, name)| name);
    let system = [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("Mac OS X", "macOS"),
        ("Windows", "Windows"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find(|(marker, _)| user_agent.contains(marker))
    .map(|(_, name)| name);
    match (browser, system) {
        (Some(browser), Some(system)) => format!("{browser} on {system}"),
        (Some(name), None) | (None, Some(name)) => name.to_owned(),
        (None, None) => "Another app".to_owned(),
    }
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    // The OS random source failing is unrecoverable; refusing to sign anyone in is right.
    getrandom::fill(&mut bytes).expect("the operating system's random number generator is unavailable");
    hex::encode(bytes)
}

impl Accounts {
    /// Load accounts from `db`; profile pictures live in `data_dir`. `navidrome_url`
    /// switches sign-in on.
    #[must_use]
    pub fn open(db: &Arc<Database>, data_dir: &Path, navidrome_url: Option<String>) -> Self {
        let state = db.load("accounts").unwrap_or_default();
        if navidrome_url.is_none() {
            tracing::warn!("no Navidrome configured: anyone who can reach delune can use it as an admin");
        }
        Self {
            navidrome_url,
            store: Some(db.clone()),
            avatars: Some(data_dir.join("avatars")),
            state: Mutex::new(state),
            failures: Mutex::default(),
        }
    }

    /// In-memory accounts for tests.
    #[must_use]
    pub fn in_memory(navidrome_url: Option<String>) -> Self {
        Self { navidrome_url, store: None, avatars: None, state: Mutex::default(), failures: Mutex::default() }
    }

    #[must_use]
    pub const fn mode(&self) -> AuthMode {
        if self.navidrome_url.is_some() { AuthMode::Navidrome } else { AuthMode::Open }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Stored> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, state: &mut Stored) {
        let Some(db) = &self.store else { return };
        let cutoff = now().saturating_sub(SESSION_IDLE.as_secs());
        state.sessions.retain(|_, s| s.last_seen >= cutoff);
        db.save("accounts", &*state);
    }

    fn current(record: &UserRecord, require_approval: bool) -> CurrentUser {
        let permissions = if record.admin { Permissions::ALL } else { record.permissions };
        CurrentUser {
            username: record.username.clone(),
            admin: record.admin,
            permissions,
            can_import: permissions.manage || !require_approval || permissions.skip_approval,
            session: None,
        }
    }

    /// The signed-in person for a token, refreshing the session's idle timer.
    pub fn authenticate(&self, token: Option<&str>) -> Option<CurrentUser> {
        if self.mode() == AuthMode::Open {
            let require_approval = self.lock().require_approval;
            let admin = UserRecord {
                username: OPEN_MODE_USER.into(),
                admin: true,
                permissions: Permissions::ALL,
                created_at: 0,
                last_login: 0,
            };
            return Some(Self::current(&admin, require_approval));
        }
        let key = hash(token?);
        let mut state = self.lock();
        let now = now();
        let session = state.sessions.get_mut(&key)?;
        if now.saturating_sub(session.last_seen) > SESSION_IDLE.as_secs() {
            state.sessions.remove(&key);
            return None;
        }
        // Save the idle timer at most daily; it only needs to be roughly right.
        let save_timer = now.saturating_sub(session.last_seen) > 24 * 60 * 60;
        session.last_seen = now;
        let username = session.username.clone();
        let record = state.users.get(&username)?.clone();
        let mut user = Self::current(&record, state.require_approval);
        user.session = Some(public_id(&key));
        if save_timer {
            self.save(&mut state);
        }
        Some(user)
    }

    /// Remember a successful Navidrome sign-in and open a session. Returns the token.
    #[cfg(test)]
    pub(crate) fn signed_in(&self, username: &str, admin: bool) -> (String, CurrentUser) {
        self.signed_in_on(username, admin, None)
    }

    /// Like [`Self::signed_in`], remembering what signed in.
    fn signed_in_on(&self, username: &str, admin: bool, device: Option<String>) -> (String, CurrentUser) {
        let token = new_token();
        let now = now();
        let mut state = self.lock();
        let record = state.users.entry(username.to_owned()).or_insert_with(|| UserRecord {
            username: username.to_owned(),
            admin,
            permissions: Permissions::MEMBER,
            created_at: now,
            last_login: now,
        });
        // Navidrome decides who is an admin, every time.
        record.admin = admin;
        record.last_login = now;
        let record = record.clone();
        let key = hash(&token);
        state
            .sessions
            .insert(key.clone(), Session { username: username.to_owned(), created_at: now, last_seen: now, device });
        let mut user = Self::current(&record, state.require_approval);
        user.session = Some(public_id(&key));
        self.save(&mut state);
        (token, user)
    }

    fn sign_out(&self, token: &str) {
        let mut state = self.lock();
        if state.sessions.remove(&hash(token)).is_some() {
            self.save(&mut state);
        }
    }

    fn sessions_of(&self, username: &str, current: Option<&str>) -> Vec<SessionInfo> {
        let state = self.lock();
        let mut sessions: Vec<SessionInfo> = state
            .sessions
            .iter()
            .filter(|(_, s)| s.username == username)
            .map(|(key, s)| {
                let id = public_id(key);
                SessionInfo {
                    current: current == Some(id.as_str()),
                    id,
                    device: s.device.clone().unwrap_or_else(|| "Unknown device".into()),
                    created_at: s.created_at,
                    last_seen: s.last_seen,
                }
            })
            .collect();
        sessions.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_seen.cmp(&a.last_seen)));
        sessions
    }

    /// End `username`'s sessions that `revoke` picks out by public id. Returns how many ended.
    fn revoke(&self, username: &str, revoke: impl Fn(&str) -> bool) -> usize {
        let mut state = self.lock();
        let before = state.sessions.len();
        state.sessions.retain(|key, s| s.username != username || !revoke(&public_id(key)));
        let ended = before - state.sessions.len();
        if ended > 0 {
            self.save(&mut state);
        }
        ended
    }

    fn locked_out(&self, username: &str) -> bool {
        let failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        failures
            .get(&username.to_lowercase())
            .is_some_and(|(count, at)| *count >= MAX_FAILURES && at.elapsed() < LOCKOUT)
    }

    fn record_failure(&self, username: &str) {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = failures.entry(username.to_lowercase()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= LOCKOUT {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
        entry.1 = Instant::now();
    }

    fn clear_failures(&self, username: &str) {
        self.failures.lock().unwrap_or_else(PoisonError::into_inner).remove(&username.to_lowercase());
    }

    /// Everyone who can approve requests: admins and people allowed to manage.
    #[must_use]
    pub fn managers(&self) -> Vec<String> {
        if self.mode() == AuthMode::Open {
            return vec![OPEN_MODE_USER.to_owned()];
        }
        self.lock().users.values().filter(|u| u.admin || u.permissions.manage).map(|u| u.username.clone()).collect()
    }

    fn people(&self) -> People {
        let state = self.lock();
        People {
            require_approval: state.require_approval,
            people: state
                .users
                .values()
                .map(|u| Person {
                    username: u.username.clone(),
                    admin: u.admin,
                    permissions: if u.admin { Permissions::ALL } else { u.permissions },
                    last_login: u.last_login,
                    avatar: state.profiles.get(&u.username).and_then(|p| p.avatar.as_ref()).map(|(_, v)| *v),
                    sessions: u32::try_from(state.sessions.values().filter(|s| s.username == u.username).count())
                        .unwrap_or(u32::MAX),
                })
                .collect(),
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// The session token from the cookie, or from `Authorization: Bearer`.
fn token_from(headers: &HeaderMap) -> Option<String> {
    if let Some(bearer) =
        headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(bearer.trim().to_owned());
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('=').map(str::to_owned))
        .filter(|t| !t.is_empty())
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Response;

    #[allow(clippy::unused_async_trait_impl, reason = "matches the trait; the lookup is synchronous")]
    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        app.accounts
            .authenticate(token_from(&parts.headers).as_deref())
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "signed-out", "Sign in to continue."))
    }
}

/// Middleware: turn away requests without a valid session.
pub async fn require_session(
    State(app): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if app.accounts.authenticate(token_from(request.headers()).as_deref()).is_none() {
        return error(StatusCode::UNAUTHORIZED, "signed-out", "Sign in to continue.");
    }
    next.run(request).await
}

fn session_cookie(token: &str, headers: &HeaderMap, max_age: u64) -> HeaderValue {
    // Mark the cookie Secure when the browser reached us over HTTPS (usually via a proxy).
    let https =
        headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let secure = if https { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn me(accounts: &Accounts, user: &CurrentUser, token: Option<String>) -> Me {
    let profile = accounts.lock().profiles.get(&user.username).cloned().unwrap_or_default();
    Me {
        username: user.username.clone(),
        admin: user.admin,
        permissions: user.permissions,
        can_import: user.can_import,
        mode: accounts.mode(),
        appearance: profile.appearance,
        avatar: profile.avatar.map(|(_, version)| version),
        token,
    }
}

/// Where someone's picture lives: named by a hash, so usernames never become paths.
fn avatar_path(dir: &Path, username: &str) -> PathBuf {
    dir.join(hash(username))
}

/// `PUT /api/v1/session/appearance`
#[utoipa::path(
    put,
    operation_id = "accounts_set_appearance",
    path = "/api/v1/session/appearance",
    tag = "session",
    request_body = delune_core::api::Appearance,
    responses(
        (status = 200, description = "OK", body = delune_core::api::Appearance),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn set_appearance(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(appearance): Json<Appearance>,
) -> Json<Appearance> {
    let accounts = &app.accounts;
    let mut state = accounts.lock();
    state.profiles.entry(user.username.clone()).or_default().appearance = appearance;
    accounts.save(&mut state);
    Json(appearance)
}

const MAX_AVATAR_BYTES: usize = 1 << 20;

/// `PUT /api/v1/session/avatar`: the request body is the image.
#[utoipa::path(
    put,
    operation_id = "accounts_set_avatar",
    path = "/api/v1/session/avatar",
    tag = "session",
    request_body = Vec<u8>,
    responses(
        (status = 200, description = "Saved; returns the picture's version"),
        (status = 400, description = "Not a PNG, JPEG, WebP or GIF under 1 MiB", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn set_avatar(State(app): State<AppState>, user: CurrentUser, body: axum::body::Bytes) -> Response {
    let Some(dir) = app.accounts.avatars.clone() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "no-storage", "Pictures can't be saved here.");
    };
    if body.len() > MAX_AVATAR_BYTES {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "too-big", "Pick a picture under 1 MB.");
    }
    // Trust the bytes, not the file name or the header the browser sent.
    let kind = match body.get(..12) {
        Some([0x89, b'P', b'N', b'G', ..]) => "image/png",
        Some([0xFF, 0xD8, 0xFF, ..]) => "image/jpeg",
        Some([b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P']) => "image/webp",
        Some([b'G', b'I', b'F', b'8', ..]) => "image/gif",
        _ => return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not-an-image", "Use a PNG, JPEG, WebP or GIF picture."),
    };
    let path = avatar_path(&dir, &user.username);
    let written = tokio::fs::create_dir_all(&dir).await.and(tokio::fs::write(&path, &body).await);
    if let Err(e) = written {
        tracing::warn!(error = %e, "couldn't save a profile picture");
        return error(StatusCode::INTERNAL_SERVER_ERROR, "save-failed", "Couldn't save the picture.");
    }
    let version = now();
    let accounts = &app.accounts;
    let mut state = accounts.lock();
    state.profiles.entry(user.username.clone()).or_default().avatar = Some((kind.to_owned(), version));
    accounts.save(&mut state);
    Json(serde_json::json!({ "avatar": version })).into_response()
}

/// `DELETE /api/v1/session/avatar`
#[utoipa::path(
    delete,
    operation_id = "accounts_remove_avatar",
    path = "/api/v1/session/avatar",
    tag = "session",
    responses(
        (status = 204, description = "Done"),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn remove_avatar(State(app): State<AppState>, user: CurrentUser) -> StatusCode {
    if let Some(dir) = &app.accounts.avatars {
        let _ = tokio::fs::remove_file(avatar_path(dir, &user.username)).await;
    }
    let accounts = &app.accounts;
    let mut state = accounts.lock();
    if let Some(profile) = state.profiles.get_mut(&user.username) {
        profile.avatar = None;
    }
    accounts.save(&mut state);
    StatusCode::NO_CONTENT
}

/// `GET /api/v1/avatars/{username}`
#[utoipa::path(
    get,
    operation_id = "accounts_avatar",
    path = "/api/v1/avatars/{username}",
    tag = "people",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "The picture", content_type = "image/*"),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn avatar(State(app): State<AppState>, UrlPath(username): UrlPath<String>) -> Response {
    let kind = app.accounts.lock().profiles.get(&username).and_then(|p| p.avatar.clone()).map(|(kind, _)| kind);
    let (Some(kind), Some(dir)) = (kind, app.accounts.avatars.as_ref()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(avatar_path(dir, &username)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, kind),
                (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_owned()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /api/v1/session`: who is signed in, or `null` when nobody is.
///
/// Answering `200 null` rather than 401 keeps "not signed in yet" from showing up as
/// an error in the browser console on every visit.
#[utoipa::path(
    get,
    operation_id = "accounts_session",
    path = "/api/v1/session",
    tag = "session",
    security(()),
    responses(
        (status = 200, description = "The signed-in person, or null", body = Option<delune_core::api::Me>),
    ),
)]
pub async fn session(State(app): State<AppState>, headers: HeaderMap) -> Json<Option<Me>> {
    Json(app.accounts.authenticate(token_from(&headers).as_deref()).map(|user| me(&app.accounts, &user, None)))
}

/// `POST /api/v1/session`: sign in with a Navidrome username and password.
#[utoipa::path(
    post,
    operation_id = "accounts_sign_in",
    path = "/api/v1/session",
    tag = "session",
    request_body = delune_core::api::LoginRequest,
    security(()),
    responses(
        (status = 200, description = "OK", body = delune_core::api::Me),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
        (status = 429, description = "Too many attempts", body = delune_core::api::ApiError),
    ),
)]
pub async fn sign_in(State(app): State<AppState>, headers: HeaderMap, Json(request): Json<LoginRequest>) -> Response {
    let accounts = &app.accounts;
    let Some(url) = accounts.navidrome_url.clone() else {
        // Open mode: nobody to check the password with, and nothing to sign in to.
        return match accounts.authenticate(None) {
            Some(user) => Json(me(accounts, &user, None)).into_response(),
            None => error(StatusCode::INTERNAL_SERVER_ERROR, "no-session", "Couldn't start a session."),
        };
    };
    let username = request.username.trim();
    if username.is_empty() || request.password.is_empty() {
        return error(StatusCode::BAD_REQUEST, "missing-credentials", "Enter your Navidrome username and password.");
    }
    if accounts.locked_out(username) {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "too-many-attempts",
            "Too many wrong passwords. Wait a minute, then try again.",
        );
    }

    let credentials =
        delune_navidrome::Credentials { username: username.to_owned(), password: request.password.clone() };
    let checked = match delune_navidrome::Client::new(&url, credentials) {
        Ok(client) => client.user(username).await,
        Err(e) => return error(StatusCode::SERVICE_UNAVAILABLE, "navidrome-unavailable", &e.to_string()),
    };
    let navidrome_user = match checked {
        Ok(user) => user,
        Err(e) if e.is_bad_credentials() => {
            accounts.record_failure(username);
            return error(
                StatusCode::UNAUTHORIZED,
                "wrong-credentials",
                "That username and password didn't work on Navidrome.",
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "sign-in couldn't reach Navidrome");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "navidrome-unavailable",
                "Couldn't reach Navidrome to check your password. Try again shortly.",
            );
        }
    };
    accounts.clear_failures(username);
    // Use Navidrome's spelling of the name, so "Aidan" and "aidan" are one person.
    let device = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(describe_device);
    let (token, user) = accounts.signed_in_on(&navidrome_user.username, navidrome_user.admin_role, device);
    tracing::info!(username = %user.username, admin = user.admin, "signed in");

    let body = me(accounts, &user, request.token.then(|| token.clone()));
    let mut response = Json(body).into_response();
    response.headers_mut().insert(header::SET_COOKIE, session_cookie(&token, &headers, SESSION_IDLE.as_secs()));
    response
}

/// `DELETE /api/v1/session`
#[utoipa::path(
    delete,
    operation_id = "accounts_sign_out",
    path = "/api/v1/session",
    tag = "session",
    security(()),
    responses(
        (status = 204, description = "Done"),
    ),
)]
pub async fn sign_out(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = token_from(&headers) {
        app.accounts.sign_out(&token);
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, session_cookie("", &headers, 0));
    response
}

/// `GET /api/v1/session/devices`: where you're signed in.
#[utoipa::path(
    get,
    operation_id = "accounts_devices",
    path = "/api/v1/session/devices",
    tag = "session",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::SessionInfo>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn devices(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<SessionInfo>> {
    Json(app.accounts.sessions_of(&user.username, user.session.as_deref()))
}

/// `DELETE /api/v1/session/devices/{id}`: sign out one of your devices.
#[utoipa::path(
    delete,
    operation_id = "accounts_revoke_device",
    path = "/api/v1/session/devices/{id}",
    tag = "session",
    params(
        ("id" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::SessionInfo>),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn revoke_device(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    match app.accounts.revoke(&user.username, |session| session == id) {
        0 => error(StatusCode::NOT_FOUND, "no-such-session", "That device is already signed out."),
        _ => Json(app.accounts.sessions_of(&user.username, user.session.as_deref())).into_response(),
    }
}

/// `POST /api/v1/session/devices/sign-out-others`: sign out everywhere but here.
#[utoipa::path(
    post,
    operation_id = "accounts_revoke_other_devices",
    path = "/api/v1/session/devices/sign-out-others",
    tag = "session",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::SessionInfo>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn revoke_other_devices(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<SessionInfo>> {
    let current = user.session.clone();
    let ended = app.accounts.revoke(&user.username, |session| Some(session) != current.as_deref());
    tracing::info!(username = %user.username, ended, "signed out other devices");
    Json(app.accounts.sessions_of(&user.username, user.session.as_deref()))
}

/// `DELETE /api/v1/users/{username}/sessions`: sign someone out everywhere.
#[utoipa::path(
    delete,
    operation_id = "accounts_revoke_person",
    path = "/api/v1/users/{username}/sessions",
    tag = "people",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::People),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn revoke_person(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage people") {
        return denied;
    }
    let ended = app.accounts.revoke(&username, |_| true);
    tracing::info!(by = %user.username, %username, ended, "signed someone out everywhere");
    Json(app.accounts.people()).into_response()
}

/// `GET /api/v1/users`
#[utoipa::path(
    get,
    operation_id = "accounts_people",
    path = "/api/v1/users",
    tag = "people",
    responses(
        (status = 200, description = "OK", body = delune_core::api::People),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn people(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage people") {
        return denied;
    }
    Json(app.accounts.people()).into_response()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PermissionsUpdate {
    permissions: Permissions,
}

/// `PUT /api/v1/users/{username}/permissions`
#[utoipa::path(
    put,
    operation_id = "accounts_set_permissions",
    path = "/api/v1/users/{username}/permissions",
    tag = "people",
    params(
        ("username" = String, Path),
    ),
    request_body = PermissionsUpdate,
    responses(
        (status = 200, description = "OK", body = delune_core::api::People),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn set_permissions(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
    Json(update): Json<PermissionsUpdate>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage people") {
        return denied;
    }
    let accounts = &app.accounts;
    let mut state = accounts.lock();
    let Some(record) = state.users.get_mut(&username) else {
        return error(StatusCode::NOT_FOUND, "no-such-person", "Nobody by that name has signed in yet.");
    };
    if record.admin {
        return error(
            StatusCode::CONFLICT,
            "admin",
            "Admins can always do everything. Change their role in Navidrome.",
        );
    }
    record.permissions = update.permissions;
    tracing::info!(by = %user.username, %username, permissions = ?update.permissions, "permissions changed");
    crate::events::changed(&app, crate::events::Topic::People);
    accounts.save(&mut state);
    drop(state);
    Json(accounts.people()).into_response()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ApprovalUpdate {
    require_approval: bool,
}

/// `PUT /api/v1/users/approval`
#[utoipa::path(
    put,
    operation_id = "accounts_set_approval",
    path = "/api/v1/users/approval",
    tag = "people",
    request_body = ApprovalUpdate,
    responses(
        (status = 200, description = "OK", body = delune_core::api::People),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn set_approval(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(update): Json<ApprovalUpdate>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change settings") {
        return denied;
    }
    let accounts = &app.accounts;
    let mut state = accounts.lock();
    state.require_approval = update.require_approval;
    accounts.save(&mut state);
    drop(state);
    Json(accounts.people()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_mode_is_an_admin() {
        let accounts = Accounts::in_memory(None);
        let user = accounts.authenticate(None).unwrap();
        assert!(user.admin && user.permissions.manage && user.can_import);
    }

    #[test]
    fn sessions_carry_navidrome_roles_and_member_permissions() {
        let accounts = Accounts::in_memory(Some("http://navidrome".into()));
        assert!(accounts.authenticate(None).is_none());
        assert!(accounts.authenticate(Some("made-up")).is_none());

        let (token, _) = accounts.signed_in("sam", false);
        let sam = accounts.authenticate(Some(&token)).unwrap();
        assert_eq!(sam.permissions, Permissions::MEMBER);
        assert!(sam.can_import, "imports don't need approval by default");
        assert!(!sam.can_see(Some("alex")) && sam.can_see(Some("sam")) && !sam.can_see(None));

        accounts.lock().require_approval = true;
        assert!(!accounts.authenticate(Some(&token)).unwrap().can_import);

        let (admin_token, _) = accounts.signed_in("alex", true);
        let alex = accounts.authenticate(Some(&admin_token)).unwrap();
        assert!(alex.can_import && alex.can_see(Some("sam")));

        accounts.sign_out(&token);
        assert!(accounts.authenticate(Some(&token)).is_none());
    }

    #[test]
    fn sessions_can_be_listed_and_revoked() {
        let accounts = Accounts::in_memory(Some("http://navidrome".into()));
        let firefox = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
        let (laptop, here) = accounts.signed_in_on("sam", false, Some(describe_device(firefox)));
        let (phone, _) = accounts.signed_in_on("sam", false, Some("Safari on iPhone".into()));
        let (tui, _) = accounts.signed_in("sam", false);
        let (alex, _) = accounts.signed_in("alex", true);

        let listed = accounts.sessions_of("sam", here.session.as_deref());
        assert_eq!(listed.len(), 3);
        assert!(listed[0].current && listed[0].device == "Firefox on Linux");
        assert!(listed.iter().all(|s| !laptop.contains(&s.id)), "ids aren't tokens");

        let phone_id = accounts.authenticate(Some(&phone)).unwrap().session.unwrap();
        assert_eq!(accounts.revoke("sam", |id| id == phone_id), 1);
        assert!(accounts.authenticate(Some(&phone)).is_none());
        assert_eq!(accounts.revoke("alex", |id| id == phone_id), 0, "only your own sessions");

        let current = here.session.unwrap();
        assert_eq!(accounts.revoke("sam", |id| id != current), 1);
        assert!(accounts.authenticate(Some(&tui)).is_none());
        assert!(accounts.authenticate(Some(&laptop)).is_some());
        assert!(accounts.authenticate(Some(&alex)).is_some());
        assert_eq!(accounts.people().people.iter().find(|p| p.username == "sam").unwrap().sessions, 1);
    }

    #[test]
    fn describes_devices() {
        let chrome_android = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Mobile Safari/537.36";
        assert_eq!(describe_device(chrome_android), "Chrome on Android");
        let safari_mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15";
        assert_eq!(describe_device(safari_mac), "Safari on macOS");
        assert_eq!(describe_device("delune/0.1.0"), "Terminal UI");
        assert_eq!(describe_device("curl/8"), "Another app");
    }

    #[test]
    fn only_token_hashes_are_stored() {
        let accounts = Accounts::in_memory(Some("http://navidrome".into()));
        let (token, _) = accounts.signed_in("sam", false);
        let stored = serde_json::to_string(&*accounts.lock()).unwrap();
        assert!(!stored.contains(&token));
    }

    #[test]
    fn locks_out_after_repeated_failures() {
        let accounts = Accounts::in_memory(Some("http://navidrome".into()));
        for _ in 0..MAX_FAILURES {
            assert!(!accounts.locked_out("Sam"));
            accounts.record_failure("sam");
        }
        assert!(accounts.locked_out("SAM"));
        accounts.clear_failures("sam");
        assert!(!accounts.locked_out("sam"));
    }

    #[test]
    fn reads_tokens_from_cookies_and_bearer_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("theme=night; delune_session=abc123; other=1"));
        assert_eq!(token_from(&headers).as_deref(), Some("abc123"));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer xyz"));
        assert_eq!(token_from(&headers).as_deref(), Some("xyz"));
        let mut empty = HeaderMap::new();
        empty.insert(header::COOKIE, HeaderValue::from_static("delune_session="));
        assert_eq!(token_from(&empty), None);
    }
}
