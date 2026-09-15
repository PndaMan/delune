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
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{FromRequestParts, Path as UrlPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, AuthMode, LoginRequest, Me, People, Permissions, Person};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppState;

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
    store: Option<PathBuf>,
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
}

/// The person making a request.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub username: String,
    pub admin: bool,
    pub permissions: Permissions,
    pub can_import: bool,
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

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    // The OS random source failing is unrecoverable; refusing to sign anyone in is right.
    getrandom::fill(&mut bytes).expect("the operating system's random number generator is unavailable");
    hex::encode(bytes)
}

impl Accounts {
    /// Load accounts from `data_dir`. `navidrome_url` switches sign-in on.
    #[must_use]
    pub fn open(data_dir: &Path, navidrome_url: Option<String>) -> Self {
        let store = data_dir.join("accounts.json");
        let state = match std::fs::read(&store) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %store.display(), "couldn't read accounts; everyone will need to sign in again");
                Stored::default()
            }),
            Err(_) => Stored::default(),
        };
        if navidrome_url.is_none() {
            tracing::warn!("no Navidrome configured: anyone who can reach delune can use it as an admin");
        }
        Self { navidrome_url, store: Some(store), state: Mutex::new(state), failures: Mutex::default() }
    }

    /// In-memory accounts for tests.
    #[must_use]
    pub fn in_memory(navidrome_url: Option<String>) -> Self {
        Self { navidrome_url, store: None, state: Mutex::default(), failures: Mutex::default() }
    }

    #[must_use]
    pub const fn mode(&self) -> AuthMode {
        if self.navidrome_url.is_some() { AuthMode::Navidrome } else { AuthMode::Open }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Stored> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, state: &mut Stored) {
        let Some(path) = &self.store else { return };
        let cutoff = now().saturating_sub(SESSION_IDLE.as_secs());
        state.sessions.retain(|_, s| s.last_seen >= cutoff);
        let Ok(json) = serde_json::to_vec_pretty(&*state) else { return };
        let tmp = path.with_extension("json.tmp");
        let written = (|| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&tmp, &json)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::rename(&tmp, path)
        })();
        if let Err(error) = written {
            tracing::warn!(%error, path = %path.display(), "couldn't save accounts");
        }
    }

    fn current(record: &UserRecord, require_approval: bool) -> CurrentUser {
        let permissions = if record.admin { Permissions::ALL } else { record.permissions };
        CurrentUser {
            username: record.username.clone(),
            admin: record.admin,
            permissions,
            can_import: permissions.manage || !require_approval || permissions.skip_approval,
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
        let user = Self::current(&record, state.require_approval);
        if save_timer {
            self.save(&mut state);
        }
        Some(user)
    }

    /// Remember a successful Navidrome sign-in and open a session. Returns the token.
    pub(crate) fn signed_in(&self, username: &str, admin: bool) -> (String, CurrentUser) {
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
        state.sessions.insert(hash(&token), Session { username: username.to_owned(), created_at: now, last_seen: now });
        let user = Self::current(&record, state.require_approval);
        self.save(&mut state);
        (token, user)
    }

    fn sign_out(&self, token: &str) {
        let mut state = self.lock();
        if state.sessions.remove(&hash(token)).is_some() {
            self.save(&mut state);
        }
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

fn me(user: &CurrentUser, mode: AuthMode, token: Option<String>) -> Me {
    Me {
        username: user.username.clone(),
        admin: user.admin,
        permissions: user.permissions,
        can_import: user.can_import,
        mode,
        token,
    }
}

/// `GET /api/v1/session`: who is signed in, or `null` when nobody is.
///
/// Answering `200 null` rather than 401 keeps "not signed in yet" from showing up as
/// an error in the browser console on every visit.
pub async fn session(State(app): State<AppState>, headers: HeaderMap) -> Json<Option<Me>> {
    Json(app.accounts.authenticate(token_from(&headers).as_deref()).map(|user| me(&user, app.accounts.mode(), None)))
}

/// `POST /api/v1/session`: sign in with a Navidrome username and password.
pub async fn sign_in(State(app): State<AppState>, headers: HeaderMap, Json(request): Json<LoginRequest>) -> Response {
    let accounts = &app.accounts;
    let Some(url) = accounts.navidrome_url.clone() else {
        // Open mode: nobody to check the password with, and nothing to sign in to.
        return match accounts.authenticate(None) {
            Some(user) => Json(me(&user, AuthMode::Open, None)).into_response(),
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
    let (token, user) = accounts.signed_in(&navidrome_user.username, navidrome_user.admin_role);
    tracing::info!(username = %user.username, admin = user.admin, "signed in");

    let body = me(&user, AuthMode::Navidrome, request.token.then(|| token.clone()));
    let mut response = Json(body).into_response();
    response.headers_mut().insert(header::SET_COOKIE, session_cookie(&token, &headers, SESSION_IDLE.as_secs()));
    response
}

/// `DELETE /api/v1/session`
pub async fn sign_out(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = token_from(&headers) {
        app.accounts.sign_out(&token);
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, session_cookie("", &headers, 0));
    response
}

/// `GET /api/v1/users`
pub async fn people(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage people") {
        return denied;
    }
    Json(app.accounts.people()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct PermissionsUpdate {
    permissions: Permissions,
}

/// `PUT /api/v1/users/{username}/permissions`
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
    accounts.save(&mut state);
    drop(state);
    Json(accounts.people()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApprovalUpdate {
    require_approval: bool,
}

/// `PUT /api/v1/users/approval`
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
