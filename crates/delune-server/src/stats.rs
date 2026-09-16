//! The library at a glance: how big it is, how good it sounds, what's in it, and how
//! it has grown through delune.
//!
//! Navidrome has no statistics endpoint, so delune reads every song (a page at a time)
//! and counts. That's a few seconds for a large library, so the answer is kept for an
//! hour and shared by everyone who asks meanwhile.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    body::Body,
    extract::{Path as UrlPath, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, JobStatus, LibraryStats, RecentAlbum, StatShare};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::AppState;
use crate::accounts::CurrentUser;

const KEEP_FOR: Duration = Duration::from_secs(60 * 60);
const PAGE: u32 = 500;
/// Stop counting after this many songs rather than hammer a huge server.
const MAX_SONGS: u32 = 400_000;

static CACHE: Mutex<Option<(Instant, Arc<LibraryStats>)>> = Mutex::const_new(None);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// "FLAC 24/96"-style tiers for a song, from what Navidrome reports.
fn quality_of(song: &delune_navidrome::Song) -> (String, &'static str) {
    let suffix = song.suffix.as_deref().unwrap_or("").to_ascii_lowercase();
    let lossless = matches!(suffix.as_str(), "flac" | "alac" | "wav" | "aif" | "aiff" | "ape" | "wv" | "dsf" | "dff")
        || (suffix == "m4a" && song.bit_depth.is_some_and(|d| d > 0));
    if lossless {
        let depth = song.bit_depth.unwrap_or(16);
        let rate = song.sampling_rate.unwrap_or(44_100);
        if depth > 16 || rate > 48_000 { ("Hi-res".to_owned(), "hires") } else { ("CD quality".to_owned(), "lossless") }
    } else {
        match song.bit_rate.unwrap_or(0) {
            0 => ("Unknown".to_owned(), "lossy"),
            b if b >= 256 => ("Lossy, 256 kbps and up".to_owned(), "lossy"),
            _ => ("Lossy, under 256 kbps".to_owned(), "lossy"),
        }
    }
}

fn top(counts: HashMap<String, (u32, u64)>, limit: usize) -> Vec<StatShare> {
    let mut shares: Vec<StatShare> =
        counts.into_iter().map(|(label, (count, bytes))| StatShare { label, count, bytes, tier: None }).collect();
    shares.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    shares.truncate(limit);
    shares
}

async fn compute(app: &AppState) -> Result<LibraryStats, String> {
    let navidrome = app.navidrome.as_ref().ok_or("No Navidrome is connected.")?;
    let mut stats = LibraryStats { computed_at: now(), ..LibraryStats::default() };

    let mut albums: HashMap<String, (String, Option<u16>)> = HashMap::new();
    let mut artists: HashSet<String> = HashSet::new();
    let mut qualities: HashMap<(String, &'static str), (u32, u64)> = HashMap::new();
    let mut genres: HashMap<String, (u32, u64)> = HashMap::new();
    let mut offset = 0;
    loop {
        let page = navidrome.songs(offset, PAGE).await.map_err(|e| format!("Navidrome didn't answer: {e}."))?;
        let got = u32::try_from(page.len()).unwrap_or(u32::MAX);
        for song in &page {
            stats.songs += 1;
            let bytes = song.size.unwrap_or(0);
            stats.bytes += bytes;
            stats.seconds += u64::from(song.duration.unwrap_or(0));
            let entry = qualities.entry(quality_of(song)).or_default();
            entry.0 += 1;
            entry.1 += bytes;
            if let Some(genre) = song.genre.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
                let entry = genres.entry(genre.to_owned()).or_default();
                entry.0 += 1;
                entry.1 += bytes;
            }
            if let Some(artist) = &song.artist {
                artists.insert(artist.to_lowercase());
            }
            let album_key = song.album_id.clone().unwrap_or_else(|| format!("{:?}\u{1f}{:?}", song.artist, song.album));
            albums.entry(album_key).or_insert_with(|| (song.artist.clone().unwrap_or_default(), song.year));
        }
        offset += got;
        if got < PAGE || offset >= MAX_SONGS {
            break;
        }
    }
    stats.albums = u32::try_from(albums.len()).unwrap_or(u32::MAX);
    stats.artists = u32::try_from(artists.len()).unwrap_or(u32::MAX);

    let order = |tier: &str| match tier {
        "hires" => 0,
        "lossless" => 1,
        _ => 2,
    };
    let mut quality_shares: Vec<StatShare> = qualities
        .into_iter()
        .map(|((label, tier), (count, bytes))| StatShare { label, count, bytes, tier: Some(tier.to_owned()) })
        .collect();
    quality_shares.sort_by(|a, b| {
        order(a.tier.as_deref().unwrap_or("")).cmp(&order(b.tier.as_deref().unwrap_or(""))).then(b.count.cmp(&a.count))
    });
    stats.qualities = quality_shares;
    stats.genres = top(genres, 8);

    let mut by_artist: HashMap<String, (u32, u64)> = HashMap::new();
    let mut by_decade: BTreeMap<u16, u32> = BTreeMap::new();
    for (artist, year) in albums.values() {
        if !artist.is_empty() {
            by_artist.entry(artist.clone()).or_default().0 += 1;
        }
        if let Some(year) = year.filter(|y| *y >= 1900) {
            *by_decade.entry(year / 10 * 10).or_default() += 1;
        }
    }
    stats.top_artists = top(by_artist, 10);
    stats.decades = by_decade
        .into_iter()
        .map(|(decade, count)| StatShare { label: format!("{decade}s"), count, bytes: 0, tier: None })
        .collect();

    // How the library grew through delune.
    let mut by_month: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_person: HashMap<String, (u32, u64)> = HashMap::new();
    for job in app.downloads.list().into_iter().filter(|j| j.status == JobStatus::Imported) {
        let at = job.imported_at.unwrap_or(job.created_at);
        if let Ok(stamp) = jiff::Timestamp::from_second(i64::try_from(at).unwrap_or(0)) {
            let date = stamp.to_zoned(jiff::tz::TimeZone::UTC).date();
            *by_month.entry(format!("{:04}-{:02}", date.year(), date.month())).or_default() += 1;
        }
        let who = job.requested_by.clone().unwrap_or_else(|| "someone".into());
        let entry = by_person.entry(who).or_default();
        entry.0 += 1;
        entry.1 += job.total_bytes;
    }
    stats.imports =
        by_month.into_iter().map(|(month, count)| StatShare { label: month, count, bytes: 0, tier: None }).collect();
    stats.importers = top(by_person, 10);

    let db = app.db.clone();
    stats.top_peers = tokio::task::spawn_blocking(move || db.top_peers(10))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(label, count, bytes)| StatShare { label, count, bytes, tier: None })
        .collect();
    Ok(stats)
}

#[derive(Debug, Deserialize)]
pub struct StatsParams {
    /// Count again rather than use the last answer.
    #[serde(default)]
    fresh: bool,
}

/// `GET /api/v1/stats`
#[utoipa::path(
    get,
    operation_id = "stats",
    path = "/api/v1/stats",
    tag = "library",
    params(("fresh" = Option<bool>, Query, description = "Count again now")),
    responses(
        (status = 200, description = "OK", body = delune_core::api::LibraryStats),
        (status = 503, description = "No Navidrome", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn stats(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<StatsParams>) -> Response {
    // One count at a time; anyone arriving meanwhile gets its answer.
    let mut cache = CACHE.lock().await;
    if !params.fresh
        && let Some((at, stats)) = cache.as_ref()
        && at.elapsed() < KEEP_FOR
    {
        return Json(stats.as_ref().clone()).into_response();
    }
    match compute(&app).await {
        Ok(stats) => {
            let stats = Arc::new(stats);
            *cache = Some((Instant::now(), stats.clone()));
            Json(stats.as_ref().clone()).into_response()
        }
        Err(message) => error(StatusCode::SERVICE_UNAVAILABLE, "stats-unavailable", &message),
    }
}

fn added_at(created: Option<&str>) -> Option<u64> {
    created?.parse::<jiff::Timestamp>().ok().and_then(|t| u64::try_from(t.as_second()).ok())
}

/// `GET /api/v1/library/recent`: albums added lately, newest first.
#[utoipa::path(
    get,
    operation_id = "library_recent",
    path = "/api/v1/library/recent",
    tag = "library",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::RecentAlbum>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn recent(State(app): State<AppState>, _user: CurrentUser) -> Json<Vec<RecentAlbum>> {
    let Some(navidrome) = &app.navidrome else { return Json(Vec::new()) };
    let albums = navidrome.newest_albums(18).await.unwrap_or_default();
    Json(
        albums
            .into_iter()
            .map(|a| RecentAlbum {
                cover: a.cover_art.as_ref().map(|c| format!("/api/v1/library/cover/{}", urlencode(c))),
                added_at: added_at(a.created.as_deref()),
                id: a.id,
                title: a.name,
                artist: a.artist,
                year: a.year,
            })
            .collect(),
    )
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `GET /api/v1/library/cover/{id}`: a library album's cover, from Navidrome.
#[utoipa::path(
    get,
    operation_id = "library_cover",
    path = "/api/v1/library/cover/{id}",
    tag = "library",
    params(("id" = String, Path, description = "Navidrome's cover art id")),
    responses(
        (status = 200, description = "The image", content_type = "image/*"),
        (status = 404, description = "No cover", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn cover(State(app): State<AppState>, _user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let Some(navidrome) = &app.navidrome else {
        return error(StatusCode::NOT_FOUND, "no-cover", "No Navidrome is connected.");
    };
    match navidrome.cover_art(&id, 300).await {
        Ok((bytes, content_type)) => (
            [(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "private, max-age=86400".to_owned())],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => error(StatusCode::NOT_FOUND, "no-cover", "That album has no cover."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(suffix: &str, depth: Option<u8>, rate: Option<u32>, bitrate: Option<u32>) -> delune_navidrome::Song {
        serde_json::from_value(serde_json::json!({
            "id": "s", "title": "t", "suffix": suffix, "bitDepth": depth, "samplingRate": rate, "bitRate": bitrate,
        }))
        .unwrap()
    }

    #[test]
    fn sorts_songs_into_quality_tiers() {
        assert_eq!(quality_of(&song("flac", Some(24), Some(96_000), None)).1, "hires");
        assert_eq!(quality_of(&song("flac", Some(16), Some(44_100), None)).1, "lossless");
        assert_eq!(quality_of(&song("mp3", None, None, Some(320))).0, "Lossy, 256 kbps and up");
        assert_eq!(quality_of(&song("mp3", None, None, Some(128))).0, "Lossy, under 256 kbps");
        assert_eq!(added_at(Some("2026-09-16T10:00:00Z")), Some(1_789_552_800));
        assert_eq!(added_at(Some("nonsense")), None);
    }
}
