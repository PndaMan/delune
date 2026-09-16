//! Bandcamp for each person: albums to buy there, and what they've already bought.
//!
//! Anyone can see whether an album is on Bandcamp and what it costs; buying happens
//! on Bandcamp's own page. Someone who links their account (by pasting their
//! Bandcamp login cookie once) gets their purchases listed, albums they own marked
//! as theirs, and a button to fetch the files they paid for, which land in Review
//! like any other download.
//!
//! The cookie is a login, so it's kept only in delune's database, never logged, and
//! never sent anywhere but Bandcamp.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_bandcamp::{Client, Error as BandcampError, Purchase, ReleaseDetail};
use delune_core::api::{ApiError, BandcampAccount, BandcampDownload, BandcampOffer, BandcampPurchase, LinkBandcamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::artwork::{names_match, normalize};
use crate::events::{Topic, changed};
use crate::store::Database;

/// How long a looked-up album is remembered.
const OFFER_FOR: Duration = Duration::from_secs(6 * 60 * 60);
/// How often linked accounts' purchases are fetched again.
const SYNC_EVERY: Duration = Duration::from_secs(12 * 60 * 60);
/// The most a purchase download may unpack to.
const MAX_UNPACKED: u64 = 20 << 30;
const FORMATS: &[&str] = &["flac", "mp3-320", "mp3-v0", "alac", "aac-hi", "vorbis", "wav", "aiff-lossless"];

/// One person's linked account.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Linked {
    cookie: String,
    fan_id: u64,
    username: String,
    name: Option<String>,
    #[serde(default)]
    purchases: Vec<Purchase>,
    #[serde(default)]
    synced_at: Option<u64>,
    #[serde(default)]
    problem: Option<String>,
    /// Download jobs started for purchases, by purchase id.
    #[serde(default)]
    jobs: BTreeMap<String, String>,
}

// Written by hand so the cookie can never end up in a log line.
impl fmt::Debug for Linked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Linked")
            .field("cookie", &"<hidden>")
            .field("username", &self.username)
            .field("purchases", &self.purchases.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Bandcamp {
    store: Option<Arc<Database>>,
    client: Client,
    /// Linked accounts, by delune account.
    linked: Mutex<BTreeMap<String, Linked>>,
    offers: Mutex<HashMap<String, (Instant, Option<ReleaseDetail>)>>,
    syncing: Mutex<HashSet<String>>,
}

impl Default for Bandcamp {
    fn default() -> Self {
        Self::with_client(None, Client::default())
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A purchase's id: steady across syncs, unlike Bandcamp's signed download links.
fn purchase_id(purchase: &Purchase) -> String {
    let key = format!("{}\u{0}{}", normalize(&purchase.artist), normalize(&purchase.title));
    hex::encode(&Sha256::digest(key.as_bytes())[..8])
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn bandcamp_error(e: &BandcampError) -> Response {
    match e {
        BandcampError::SignedOut => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bandcamp-signed-out",
            "Bandcamp didn't accept that login. Copy it again while signed in to bandcamp.com.",
        ),
        BandcampError::NotFound => error(StatusCode::NOT_FOUND, "not-on-bandcamp", "That isn't on Bandcamp."),
        BandcampError::Http(_) | BandcampError::Unreadable => {
            error(StatusCode::BAD_GATEWAY, "bandcamp-unreachable", "Couldn't reach Bandcamp just now. Try again soon.")
        }
    }
}

impl Bandcamp {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        Self::with_client(Some(db.clone()), Client::default())
    }

    /// For tests: talk to `client` (usually a local stand-in for Bandcamp).
    #[must_use]
    pub fn with_client(store: Option<Arc<Database>>, client: Client) -> Self {
        let linked = store.as_ref().and_then(|db| db.load("bandcamp")).unwrap_or_default();
        Self { store, client, linked: Mutex::new(linked), offers: Mutex::default(), syncing: Mutex::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Linked>> {
        self.linked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, linked: &BTreeMap<String, Linked>) {
        if let Some(db) = &self.store {
            db.save("bandcamp", linked);
        }
    }

    fn account(&self, who: &str) -> BandcampAccount {
        let syncing = self.syncing.lock().unwrap_or_else(PoisonError::into_inner).contains(who);
        match self.lock().get(who) {
            Some(linked) => BandcampAccount {
                linked: true,
                username: Some(linked.username.clone()),
                name: linked.name.clone(),
                purchases: u32::try_from(linked.purchases.len()).unwrap_or(u32::MAX),
                synced_at: linked.synced_at,
                syncing,
                problem: linked.problem.clone(),
            },
            None => BandcampAccount {
                linked: false,
                username: None,
                name: None,
                purchases: 0,
                synced_at: None,
                syncing: false,
                problem: None,
            },
        }
    }

    fn purchases(&self, who: &str) -> Vec<BandcampPurchase> {
        let linked = self.lock();
        let Some(linked) = linked.get(who) else { return Vec::new() };
        linked
            .purchases
            .iter()
            .map(|p| {
                let id = purchase_id(p);
                BandcampPurchase {
                    job: linked.jobs.get(&id).cloned(),
                    id,
                    title: p.title.clone(),
                    artist: p.artist.clone(),
                    purchased_at: p.purchased_at,
                    art: p.art.clone(),
                    url: p.url.clone(),
                    downloadable: p.download_url.is_some(),
                }
            })
            .collect()
    }

    /// The purchase matching an album, if `who` bought it.
    fn owned(&self, who: &str, artist: &str, title: &str) -> Option<String> {
        let (artist, title) = (normalize(artist), normalize(title));
        let linked = self.lock();
        linked.get(who)?.purchases.iter().find_map(|p| {
            (names_match(&normalize(&p.artist), &artist) && names_match(&normalize(&p.title), &title))
                .then(|| purchase_id(p))
        })
    }

    fn cookie(&self, who: &str) -> Option<String> {
        self.lock().get(who).map(|l| l.cookie.clone())
    }

    /// Check a pasted login with Bandcamp and keep it for `who`.
    async fn link(&self, who: &str, pasted: &str) -> Result<(), BandcampError> {
        let cookie = delune_bandcamp::cookie_header(pasted);
        let fan = self.client.fan(&cookie).await?;
        let mut linked = self.lock();
        linked.insert(
            who.to_owned(),
            Linked { cookie, fan_id: fan.fan_id, username: fan.username, name: fan.name, ..Linked::default() },
        );
        self.save(&linked);
        Ok(())
    }

    fn unlink(&self, who: &str) {
        let mut linked = self.lock();
        linked.remove(who);
        self.save(&linked);
    }

    /// Fetch `who`'s purchases again.
    async fn sync(&self, who: &str) {
        let Some((cookie, fan_id)) = self.lock().get(who).map(|l| (l.cookie.clone(), l.fan_id)) else { return };
        if !self.syncing.lock().unwrap_or_else(PoisonError::into_inner).insert(who.to_owned()) {
            return;
        }
        let result = self.client.purchases(&cookie, fan_id).await;
        {
            let mut linked = self.lock();
            if let Some(account) = linked.get_mut(who) {
                match result {
                    Ok(purchases) => {
                        account.purchases = purchases;
                        account.synced_at = Some(now());
                        account.problem = None;
                    }
                    Err(e) => {
                        tracing::warn!(account = %who, error = %e, "couldn't fetch Bandcamp purchases");
                        account.problem = Some(match e {
                            BandcampError::SignedOut => {
                                "Bandcamp signed delune out. Link your account again.".to_owned()
                            }
                            _ => "Couldn't reach Bandcamp last time; delune will try again.".to_owned(),
                        });
                    }
                }
                self.save(&linked);
            }
        }
        self.syncing.lock().unwrap_or_else(PoisonError::into_inner).remove(who);
    }

    /// The album on Bandcamp, if it's there. Remembered for a few hours either way.
    async fn find(&self, artist: &str, title: &str) -> Result<Option<ReleaseDetail>, BandcampError> {
        let key = format!("{}\u{0}{}", normalize(artist), normalize(title));
        if let Some((at, found)) = self.offers.lock().unwrap_or_else(PoisonError::into_inner).get(&key)
            && at.elapsed() < OFFER_FOR
        {
            return Ok(found.clone());
        }
        let results = self.client.search(&format!("{artist} {title}")).await?;
        let (want_artist, want_title) = (normalize(artist), normalize(title));
        let hit = results.into_iter().find(|r| {
            names_match(&normalize(&r.artist), &want_artist) && names_match(&normalize(&r.title), &want_title)
        });
        let found = match hit {
            Some(hit) => match self.client.release(&hit.url, None).await {
                Ok(detail) => Some(detail),
                Err(BandcampError::NotFound) => None,
                Err(e) => return Err(e),
            },
            None => None,
        };
        let mut offers = self.offers.lock().unwrap_or_else(PoisonError::into_inner);
        if offers.len() > 2000 {
            offers.retain(|_, (at, _)| at.elapsed() < OFFER_FOR);
        }
        offers.insert(key, (Instant::now(), found.clone()));
        Ok(found)
    }
}

/// Keep linked accounts' purchases fresh. Call once at startup.
pub fn start(app: &AppState) {
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let people: Vec<String> = app.bandcamp.lock().keys().cloned().collect();
            for who in people {
                app.bandcamp.sync(&who).await;
            }
            changed(&app, Topic::Bandcamp);
            tokio::time::sleep(SYNC_EVERY).await;
        }
    });
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ReleaseParams {
    artist: String,
    album: String,
}

/// `GET /api/v1/bandcamp/release?artist=…&album=…`
#[utoipa::path(
    get,
    operation_id = "bandcamp_release",
    path = "/api/v1/bandcamp/release",
    tag = "bandcamp",
    params(ReleaseParams),
    responses(
        (status = 200, description = "It's on Bandcamp", body = delune_core::api::BandcampOffer),
        (status = 404, description = "It isn't", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn release(State(app): State<AppState>, user: CurrentUser, Query(params): Query<ReleaseParams>) -> Response {
    let (artist, album) = (params.artist.trim(), params.album.trim());
    if artist.is_empty() || album.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no-album", "Say which album, and by whom.");
    }
    let detail = match app.bandcamp.find(artist, album).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not-on-bandcamp", "That isn't on Bandcamp."),
        Err(e) => return bandcamp_error(&e),
    };
    let purchase = app.bandcamp.owned(&user.username, &detail.artist, &detail.title);
    Json(BandcampOffer {
        url: detail.url,
        title: detail.title,
        artist: detail.artist,
        price: detail.price,
        currency: detail.currency,
        name_your_price: detail.name_your_price,
        owned: purchase.is_some(),
        purchase,
    })
    .into_response()
}

/// `GET /api/v1/bandcamp/account`
#[utoipa::path(
    get,
    operation_id = "bandcamp_account",
    path = "/api/v1/bandcamp/account",
    tag = "bandcamp",
    responses(
        (status = 200, description = "Your linked account, if any", body = delune_core::api::BandcampAccount),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn account(State(app): State<AppState>, user: CurrentUser) -> Json<BandcampAccount> {
    Json(app.bandcamp.account(&user.username))
}

/// `PUT /api/v1/bandcamp/account`: link an account, checking the login with Bandcamp first.
#[utoipa::path(
    put,
    operation_id = "bandcamp_link",
    path = "/api/v1/bandcamp/account",
    tag = "bandcamp",
    request_body = delune_core::api::LinkBandcamp,
    responses(
        (status = 200, description = "Linked; purchases are on their way", body = delune_core::api::BandcampAccount),
        (status = 422, description = "Bandcamp didn't accept the login", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn link(State(app): State<AppState>, user: CurrentUser, Json(body): Json<LinkBandcamp>) -> Response {
    let pasted = body.cookie.trim();
    if pasted.is_empty() || pasted.len() > 64_000 {
        return error(StatusCode::UNPROCESSABLE_ENTITY, "no-cookie", "Paste your Bandcamp login first.");
    }
    if let Err(e) = app.bandcamp.link(&user.username, pasted).await {
        return bandcamp_error(&e);
    }
    tracing::info!(account = %user.username, "linked a Bandcamp account");
    let (app2, who) = (app.clone(), user.username.clone());
    tokio::spawn(async move {
        app2.bandcamp.sync(&who).await;
        changed(&app2, Topic::Bandcamp);
    });
    changed(&app, Topic::Bandcamp);
    let mut account = app.bandcamp.account(&user.username);
    account.syncing = true;
    Json(account).into_response()
}

/// `DELETE /api/v1/bandcamp/account`: forget the login and the purchase list.
#[utoipa::path(
    delete,
    operation_id = "bandcamp_unlink",
    path = "/api/v1/bandcamp/account",
    tag = "bandcamp",
    responses(
        (status = 200, description = "Unlinked", body = delune_core::api::BandcampAccount),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn unlink(State(app): State<AppState>, user: CurrentUser) -> Json<BandcampAccount> {
    app.bandcamp.unlink(&user.username);
    changed(&app, Topic::Bandcamp);
    Json(app.bandcamp.account(&user.username))
}

/// `GET /api/v1/bandcamp/purchases`
#[utoipa::path(
    get,
    operation_id = "bandcamp_purchases",
    path = "/api/v1/bandcamp/purchases",
    tag = "bandcamp",
    responses(
        (status = 200, description = "What you've bought, newest first", body = [delune_core::api::BandcampPurchase]),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn purchases(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<BandcampPurchase>> {
    Json(app.bandcamp.purchases(&user.username))
}

/// `POST /api/v1/bandcamp/purchases/sync`: fetch purchases again now.
#[utoipa::path(
    post,
    operation_id = "bandcamp_sync",
    path = "/api/v1/bandcamp/purchases/sync",
    tag = "bandcamp",
    responses(
        (status = 202, description = "Fetching", body = delune_core::api::BandcampAccount),
        (status = 409, description = "No account linked", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn sync(State(app): State<AppState>, user: CurrentUser) -> Response {
    if app.bandcamp.cookie(&user.username).is_none() {
        return error(StatusCode::CONFLICT, "not-linked", "Link your Bandcamp account first.");
    }
    let (app2, who) = (app.clone(), user.username.clone());
    tokio::spawn(async move {
        app2.bandcamp.sync(&who).await;
        changed(&app2, Topic::Bandcamp);
    });
    let mut account = app.bandcamp.account(&user.username);
    account.syncing = true;
    (StatusCode::ACCEPTED, Json(account)).into_response()
}

/// `POST /api/v1/bandcamp/purchases/{id}/download`: fetch a purchase's files for review.
#[utoipa::path(
    post,
    operation_id = "bandcamp_download",
    path = "/api/v1/bandcamp/purchases/{id}/download",
    tag = "bandcamp",
    params(("id" = String, Path, description = "The purchase")),
    request_body = delune_core::api::BandcampDownload,
    responses(
        (status = 201, description = "Downloading", body = delune_core::api::DownloadJob),
        (status = 404, description = "No such purchase", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed to download", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn download(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<BandcampDownload>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "download") {
        return denied;
    }
    let format = body.format.as_deref().unwrap_or("flac");
    if !FORMATS.contains(&format) {
        return error(StatusCode::BAD_REQUEST, "bad-format", "Bandcamp doesn't offer that format.");
    }
    let found = {
        let linked = app.bandcamp.lock();
        linked.get(&user.username).and_then(|l| {
            let purchase = l.purchases.iter().find(|p| purchase_id(p) == id)?.clone();
            Some((l.cookie.clone(), purchase))
        })
    };
    let Some((cookie, purchase)) = found else {
        return error(StatusCode::NOT_FOUND, "no-purchase", "That isn't among your Bandcamp purchases.");
    };
    let Some(page) = purchase.download_url.clone() else {
        return error(StatusCode::NOT_FOUND, "no-download", "Bandcamp doesn't offer the files for that one.");
    };

    let job =
        crate::downloads::begin_external(&app, "Bandcamp", &purchase.title, Some(&purchase.artist), &user.username);
    {
        let mut linked = app.bandcamp.lock();
        if let Some(account) = linked.get_mut(&user.username) {
            account.jobs.insert(id.clone(), job.id.clone());
        }
        app.bandcamp.save(&linked);
    }
    tracing::info!(id = %job.id, by = %user.username, "fetching a Bandcamp purchase");
    let staging = crate::downloads::staging_dir(&app.data_dir, &job.id);
    let (app2, job_id, format) = (app.clone(), job.id.clone(), format.to_owned());
    tokio::spawn(async move {
        let outcome = fetch(&app2.bandcamp.client, &cookie, &page, &format, &staging).await;
        crate::downloads::finish_external(&app2, &job_id, outcome).await;
        changed(&app2, Topic::Bandcamp);
    });
    changed(&app, Topic::Bandcamp);
    (StatusCode::CREATED, Json(job)).into_response()
}

/// Download a purchase into `staging`, unpacking it when Bandcamp sends a zip.
async fn fetch(client: &Client, cookie: &str, page: &str, format: &str, staging: &Path) -> Result<(), String> {
    let url = client
        .download_url(cookie, page, format)
        .await
        .map_err(|e| format!("Bandcamp didn't hand over the files: {e}."))?;
    tokio::fs::create_dir_all(staging).await.map_err(|e| format!("Couldn't make a folder to download into: {e}"))?;
    let mut response = reqwest::Client::new()
        .get(&url)
        .header(reqwest::header::COOKIE, delune_bandcamp::cookie_header(cookie))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "Bandcamp stopped the download. Try again in a while.".to_owned())?;
    let name = file_name(&response).unwrap_or_else(|| format!("bandcamp.{}", extension(format)));
    let target = staging.join(&name);
    let mut file = std::fs::File::create(&target).map_err(|e| format!("Couldn't save the download: {e}"))?;
    let mut written: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| "The download was cut off.".to_owned())? {
        written += chunk.len() as u64;
        if written > MAX_UNPACKED {
            return Err("That download is far bigger than an album should be.".to_owned());
        }
        file.write_all(&chunk).map_err(|e| format!("Couldn't save the download: {e}"))?;
    }
    drop(file);
    let staging = staging.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if is_zip(&target) {
            unpack(&target, &staging)?;
            let _ = std::fs::remove_file(&target);
        }
        Ok(())
    })
    .await
    .map_err(|_| "Unpacking the download failed.".to_owned())?
}

fn extension(format: &str) -> &'static str {
    match format {
        "mp3-320" | "mp3-v0" => "mp3",
        "alac" | "aac-hi" => "m4a",
        "vorbis" => "ogg",
        "wav" => "wav",
        "aiff-lossless" => "aiff",
        _ => "flac",
    }
}

/// The name Bandcamp gives the file, kept to a plain file name.
fn file_name(response: &reqwest::Response) -> Option<String> {
    let header = response.headers().get(reqwest::header::CONTENT_DISPOSITION)?.to_str().ok()?;
    let name = header.split("filename=").nth(1)?.split(';').next()?.trim().trim_matches('"');
    let name = Path::new(name).file_name()?.to_str()?.trim();
    (!name.is_empty() && !name.starts_with('.')).then(|| name.to_owned())
}

fn is_zip(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"PK\x03\x04"
}

/// Unpack `archive` into `into`, refusing paths that would land outside it.
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("Couldn't open the download: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "The download isn't a readable zip.".to_owned())?;
    let mut total: u64 = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| "The download's zip is damaged.".to_owned())?;
        let Some(relative) = entry.enclosed_name() else { continue };
        let path: PathBuf = into.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        total += entry.size();
        if total > MAX_UNPACKED {
            return Err("That download unpacks to far more than an album should.".to_owned());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&path).map_err(|e| format!("Couldn't unpack the download: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("Couldn't unpack the download: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    /// A stand-in for Bandcamp that knows one fan and one album.
    async fn fake_bandcamp() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let page = format!(
            r#"<div data-tralbum="{{&quot;artist&quot;:&quot;Burial&quot;,&quot;url&quot;:&quot;{base}/album/untrue&quot;,&quot;current&quot;:{{&quot;title&quot;:&quot;Untrue&quot;,&quot;minimum_price&quot;:7.0,&quot;currency&quot;:&quot;GBP&quot;}}}}"></div>"#
        );
        let search_base = base.clone();
        let collection_base = base.clone();
        let download_page = format!(
            r#"<div data-blob="{{&quot;digital_items&quot;:[{{&quot;downloads&quot;:{{&quot;flac&quot;:{{&quot;url&quot;:&quot;{base}/files.zip&quot;}}}}}}]}}"></div>"#
        );
        let zip_bytes = {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("Burial - Untrue - 02 Archangel.flac", options).unwrap();
            zip.write_all(b"fLaC not really").unwrap();
            zip.finish().unwrap().into_inner()
        };
        let router = axum::Router::new()
            .route(
                "/api/bcsearch_public_api/1/autocomplete_elastic",
                post(move || {
                    let base = search_base.clone();
                    async move {
                        Json(serde_json::json!({"auto": {"results": [
                            {"type": "a", "name": "Untrue", "band_name": "Burial", "item_url_path": format!("{base}/album/untrue")}
                        ]}}))
                    }
                }),
            )
            .route("/album/untrue", get(move || async move { axum::response::Html(page) }))
            .route(
                "/api/fan/2/collection_summary",
                get(|headers: axum::http::HeaderMap| async move {
                    let cookie = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or_default();
                    if cookie == "identity=good" {
                        Json(serde_json::json!({"fan_id": 7, "collection_summary": {"username": "aidan", "name": "Aidan"}}))
                            .into_response()
                    } else {
                        Json(serde_json::json!({"error": true})).into_response()
                    }
                }),
            )
            .route(
                "/api/fancollection/1/collection_items",
                post(move || {
                    let base = collection_base.clone();
                    async move {
                        Json(serde_json::json!({
                            "items": [{"item_title": "Untrue", "band_name": "Burial", "sale_item_id": 1, "sale_item_type": "p"}],
                            "redownload_urls": {"p1": format!("{base}/download")},
                            "more_available": false,
                        }))
                    }
                }),
            )
            .route("/download", get(move || async move { axum::response::Html(download_page) }))
            .route(
                "/files.zip",
                get(move || async move {
                    ([(axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"../Untrue.zip\"")], zip_bytes)
                }),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        base
    }

    async fn call(app: &axum::Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn links_an_account_lists_purchases_and_marks_albums_owned() {
        let base = fake_bandcamp().await;
        let db = Arc::new(Database::in_memory());
        let bandcamp = Arc::new(Bandcamp::with_client(Some(db.clone()), Client::new(&base)));
        let data_dir = std::env::temp_dir().join(format!("delune-bandcamp-{}", std::process::id()));
        let state = AppState { bandcamp: bandcamp.clone(), db, data_dir: data_dir.clone(), ..AppState::default() };
        let app = crate::router(state.clone());

        let (status, offer) = call(&app, "GET", "/api/v1/bandcamp/release?artist=Burial&album=Untrue", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((offer["price"].as_f64(), offer["owned"].as_bool()), (Some(7.0), Some(false)));
        assert!(offer["url"].as_str().unwrap().ends_with("/album/untrue"));
        let (status, _) = call(&app, "GET", "/api/v1/bandcamp/release?artist=Burial&album=Nothing", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = call(&app, "PUT", "/api/v1/bandcamp/account", Some(r#"{"cookie":"bad"}"#)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "a login Bandcamp refuses isn't kept");
        let (status, account) =
            call(&app, "PUT", "/api/v1/bandcamp/account", Some(r#"{"cookie":"Cookie: identity=good"}"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(account["username"], "aidan");
        assert!(!account.to_string().contains("good"), "the login is never sent back");

        // The link starts a sync in the background; run one directly to be sure it's done.
        bandcamp.sync(crate::accounts::OPEN_MODE_USER).await;
        let (_, bought) = call(&app, "GET", "/api/v1/bandcamp/purchases", None).await;
        assert_eq!(bought[0]["title"], "Untrue");
        assert_eq!(bought[0]["downloadable"], true);
        let (_, offer) = call(&app, "GET", "/api/v1/bandcamp/release?artist=burial&album=UNTRUE", None).await;
        assert_eq!(offer["owned"], true);
        assert_eq!(offer["purchase"], bought[0]["id"]);

        // Fetching what was bought: the zip is unpacked into the job's folder for review.
        let id = bought[0]["id"].as_str().unwrap().to_owned();
        let (status, _) =
            call(&app, "POST", &format!("/api/v1/bandcamp/purchases/{id}/download"), Some(r#"{"format":"wav-ish"}"#))
                .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "only formats Bandcamp offers");
        let (status, job) = call(&app, "POST", &format!("/api/v1/bandcamp/purchases/{id}/download"), Some("{}")).await;
        assert_eq!(status, StatusCode::CREATED);
        let job_id = job["id"].as_str().unwrap().to_owned();
        let staging = crate::downloads::staging_dir(&data_dir, &job_id);
        let mut ready = false;
        for _ in 0..100 {
            let job = state.downloads.list().into_iter().find(|j| j.id == job_id).unwrap();
            if job.status != delune_core::api::JobStatus::Downloading {
                assert_eq!(job.status, delune_core::api::JobStatus::Ready, "{:?}", job.error);
                assert_eq!(job.files.len(), 1);
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(ready, "the purchase finished downloading");
        assert!(staging.join("Burial - Untrue - 02 Archangel.flac").exists());
        assert!(!staging.join("Untrue.zip").exists(), "the zip is removed once unpacked");
        let (_, bought) = call(&app, "GET", "/api/v1/bandcamp/purchases", None).await;
        assert_eq!(bought[0]["job"], job_id.as_str());
        let _ = std::fs::remove_dir_all(&data_dir);

        let (_, account) = call(&app, "DELETE", "/api/v1/bandcamp/account", None).await;
        assert_eq!(account["linked"], false);
        assert!(bandcamp.cookie(crate::accounts::OPEN_MODE_USER).is_none());
    }

    #[test]
    fn the_login_stays_out_of_logs() {
        let linked = Linked { cookie: "identity=secret".into(), ..Linked::default() };
        assert!(!format!("{linked:?}").contains("secret"));
    }

    #[test]
    fn unpacks_zips_without_escaping_the_folder() {
        let dir = std::env::temp_dir().join(format!("delune-bandcamp-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("album.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("Burial - Untrue - 01 Archangel.flac", options).unwrap();
            zip.write_all(b"fLaC").unwrap();
            zip.start_file("../escaped.flac", options).unwrap();
            zip.write_all(b"nope").unwrap();
            zip.finish().unwrap();
        }
        assert!(is_zip(&archive));
        let out = dir.join("out");
        unpack(&archive, &out).unwrap();
        assert!(out.join("Burial - Untrue - 01 Archangel.flac").exists());
        assert!(!dir.join("escaped.flac").exists(), "paths outside the folder are skipped");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn purchase_ids_ignore_spelling() {
        let a = Purchase {
            title: "Untrue".into(),
            artist: "Burial".into(),
            purchased_at: None,
            art: None,
            url: None,
            download_url: None,
        };
        let b = Purchase { title: "UNTRUE".into(), download_url: Some("x".into()), ..a.clone() };
        assert_eq!(purchase_id(&a), purchase_id(&b));
    }
}
