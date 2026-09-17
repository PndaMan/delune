//! After an import: embedded artwork and lyrics, then a Navidrome rescan.
//!
//! Lyrics come from [LRCLIB](https://lrclib.net), an open lyrics database. Each
//! track is looked up by title and artist, and the entry whose duration is closest
//! wins; synced (timed) lyrics are preferred. Instrumentals are skipped. This runs
//! after the files have moved, so an import never waits on it, and Navidrome is asked
//! to rescan once it's done so it picks up the lyrics too.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::{Json, extract::State, response::IntoResponse, response::Response};
use delune_library::extras::{self, Lyrics, LyricsMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

const LRCLIB: &str = "https://lrclib.net/api/search";
const USER_AGENT: &str = concat!("delune/", env!("CARGO_PKG_VERSION"), " (https://github.com/PndaMan/delune)");

/// `GET/PUT /api/v1/import-options`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ImportOptions {
    pub lyrics: LyricsMode,
    pub embed_cover: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { lyrics: LyricsMode::Sidecar, embed_cover: true }
    }
}

#[derive(Debug, Default)]
pub struct Finishing {
    store: Option<Arc<Database>>,
    options: Mutex<ImportOptions>,
}

impl Finishing {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let options = db.load("import-options").unwrap_or_default();
        Self { store: Some(db.clone()), options: Mutex::new(options) }
    }

    #[must_use]
    pub fn options(&self) -> ImportOptions {
        *self.options.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One imported track to finish.
#[derive(Debug, Clone)]
pub struct Imported {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
}

const IMAGE_NAMES: &[&str] = &["cover", "folder", "front", "album"];
const IMAGE_TYPES: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// A cover image already in `folder`, by the names players look for.
fn cover_in(folder: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(folder).ok()?;
    entries.flatten().map(|e| e.path()).find(|p| {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
        IMAGE_NAMES.contains(&stem.as_str()) && IMAGE_TYPES.contains(&ext.as_str())
    })
}

/// Make sure the album has a cover: the one the download brought, one already in its
/// folder, or else the album's cover from Deezer, saved as `cover.<ext>`.
async fn ensure_cover(app: &AppState, folder: &Path, album: &AlbumName, brought: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(cover) = brought {
        return Some(cover);
    }
    let dir = folder.to_path_buf();
    if let Ok(Some(existing)) = tokio::task::spawn_blocking(move || cover_in(&dir)).await {
        return Some(existing);
    }
    let Some((bytes, content_type)) = app.artwork.cover_image(&album.artist, &album.title).await else {
        tracing::info!(album = %album.title, artist = %album.artist, "no cover art found for an import");
        return None;
    };
    let ext = match content_type.as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        _ => "jpg",
    };
    let path = folder.join(format!("cover.{ext}"));
    match tokio::fs::write(&path, &bytes).await {
        Ok(()) => {
            tracing::info!(album = %album.title, "saved cover art from Deezer");
            Some(path)
        }
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "couldn't save cover art");
            None
        }
    }
}

/// Make sure an album folder in the library has a cover, and that its tracks carry it.
/// The album's name comes from its tracks' tags, or the folder names.
pub(crate) async fn cover_folder(app: &AppState, folder: &Path) {
    let dir = folder.to_path_buf();
    let found = tokio::task::spawn_blocking(move || {
        let tracks: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|entries| {
                entries.flatten().map(|e| e.path()).filter(|p| delune_library::inspect::inspect(p).is_ok()).collect()
            })
            .unwrap_or_default();
        let identity = tracks.first().and_then(|t| delune_library::tidy::identity(t).ok());
        (tracks, identity)
    })
    .await;
    let Ok((tracks, identity)) = found else { return };
    if tracks.is_empty() {
        return;
    }
    let folder_name = |p: &Path| p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    let album = AlbumName {
        artist: identity
            .as_ref()
            .and_then(|i| i.album_artists.first().cloned().or_else(|| i.artist.clone()))
            .unwrap_or_else(|| folder.parent().map(folder_name).unwrap_or_default()),
        title: identity.and_then(|i| i.album).unwrap_or_else(|| folder_name(folder)),
    };
    let Some(cover) = ensure_cover(app, folder, &album, None).await else { return };
    let _ = tokio::task::spawn_blocking(move || {
        for track in tracks {
            if let Err(error) = extras::embed_cover(&track, &cover) {
                tracing::debug!(%error, path = %track.display(), "couldn't embed artwork");
            }
        }
    })
    .await;
}

/// The album imported tracks belong to.
#[derive(Debug, Clone)]
pub struct AlbumName {
    pub artist: String,
    pub title: String,
}

/// Make sure the album has artwork, fetch lyrics for freshly imported tracks, then
/// rescan.
pub fn finish(app: &AppState, tracks: Vec<Imported>, cover: Option<PathBuf>, album: AlbumName) {
    let app = app.clone();
    tokio::spawn(async move {
        let options = app.finishing.options();
        let folder = tracks.first().and_then(|t| t.path.parent().map(Path::to_path_buf));
        let cover = match folder {
            Some(folder) => ensure_cover(&app, &folder, &album, cover).await,
            None => cover,
        };
        if options.embed_cover
            && let Some(cover) = cover
        {
            let paths: Vec<PathBuf> = tracks.iter().map(|t| t.path.clone()).collect();
            let _ = tokio::task::spawn_blocking(move || {
                for path in paths {
                    if let Err(error) = extras::embed_cover(&path, &cover) {
                        tracing::debug!(%error, path = %path.display(), "couldn't embed artwork");
                    }
                }
            })
            .await;
        }
        // Show the album straight away; lyrics can follow.
        rescan(&app).await;
        if options.lyrics != LyricsMode::Off {
            let http = app.music_http.clone();
            let mut found = 0;
            for track in &tracks {
                let duration = {
                    let path = track.path.clone();
                    tokio::task::spawn_blocking(move || delune_library::inspect::properties(&path).ok().map(|(_, d)| d))
                        .await
                        .ok()
                        .flatten()
                };
                if let Some(lyrics) = lookup(&http, &track.title, &track.artist, duration).await {
                    let (path, mode) = (track.path.clone(), options.lyrics);
                    let written = tokio::task::spawn_blocking(move || extras::write_lyrics(&path, &lyrics, mode)).await;
                    if matches!(written, Ok(Ok(_))) {
                        found += 1;
                    }
                }
                // Be gentle with a free service.
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            tracing::info!(tracks = tracks.len(), found, "lyrics fetched");
            if found > 0 {
                rescan(&app).await;
            }
        }
    });
}

/// Ask Navidrome to pick up new files, trying again for a few minutes if it's down.
pub(crate) async fn rescan(app: &AppState) {
    let Some(navidrome) = &app.navidrome else { return };
    for wait in [0, 10, 30, 90, 180] {
        tokio::time::sleep(Duration::from_secs(wait)).await;
        match navidrome.start_scan(false).await {
            Ok(_) => return,
            Err(error) => tracing::warn!(%error, retry_in = wait, "Navidrome didn't start a scan"),
        }
    }
    tracing::warn!("imported, but Navidrome never started a scan; it will find the files on its next one");
}

/// Words for one song from LRCLIB, or `None` when it doesn't have it.
pub(crate) async fn lyrics_for(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    duration: Option<u32>,
) -> Option<Lyrics> {
    lookup(http, title, artist, duration).await
}

/// Search LRCLIB by title and artist. When the artist is unknown or wrong (a folder
/// name like "Music", say), fall back to the title alone, trusting only a match on length.
async fn lookup(http: &reqwest::Client, title: &str, artist: &str, duration: Option<u32>) -> Option<Lyrics> {
    let artist = artist.trim();
    if !artist.is_empty()
        && let Some(found) = search(http, &[("track_name", title), ("artist_name", artist)], duration).await
    {
        return Some(found);
    }
    duration?;
    search(http, &[("track_name", title)], duration).await
}

async fn search(http: &reqwest::Client, query: &[(&str, &str)], duration: Option<u32>) -> Option<Lyrics> {
    let response = http
        .get(LRCLIB)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .query(query)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    let results: Vec<Value> = response.json().await.ok()?;
    pick(&results, duration)
}

/// The entry closest in length (within ten seconds), preferring synced lyrics.
fn pick(results: &[Value], duration: Option<u32>) -> Option<Lyrics> {
    let distance = |entry: &Value| {
        let length = entry.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        duration.map_or(0.0, |d| (f64::from(d) - length).abs())
    };
    let text = |entry: &Value, key: &str| {
        entry.get(key).and_then(Value::as_str).map(str::to_owned).filter(|s| !s.trim().is_empty())
    };
    let mut candidates: Vec<&Value> = results
        .iter()
        .filter(|e| !e.get("instrumental").and_then(Value::as_bool).unwrap_or(false))
        .filter(|e| distance(e) <= 10.0)
        .collect();
    candidates.sort_by(|a, b| {
        text(b, "syncedLyrics")
            .is_some()
            .cmp(&text(a, "syncedLyrics").is_some())
            .then(distance(a).total_cmp(&distance(b)))
    });
    let best = candidates.first()?;
    let lyrics = Lyrics { synced: text(best, "syncedLyrics"), plain: text(best, "plainLyrics") };
    (lyrics.synced.is_some() || lyrics.plain.is_some()).then_some(lyrics)
}

/// `GET /api/v1/import-options`
#[utoipa::path(
    get,
    operation_id = "finishing_get_options",
    path = "/api/v1/import-options",
    tag = "settings",
    responses(
        (status = 200, description = "OK", body = ImportOptions),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn get_options(State(app): State<AppState>, _user: CurrentUser) -> Json<ImportOptions> {
    Json(app.finishing.options())
}

/// `PUT /api/v1/import-options`
#[utoipa::path(
    put,
    operation_id = "finishing_set_options",
    path = "/api/v1/import-options",
    tag = "settings",
    request_body = ImportOptions,
    responses(
        (status = 200, description = "OK", body = ImportOptions),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn set_options(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(options): Json<ImportOptions>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change import settings") {
        return denied;
    }
    *app.finishing.options.lock().unwrap_or_else(PoisonError::into_inner) = options;
    if let Some(db) = &app.finishing.store {
        db.save("import-options", &options);
    }
    crate::events::changed(&app, crate::events::Topic::ImportOptions);
    Json(options).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_synced_lyrics_of_the_right_length() {
        let results: Vec<Value> = serde_json::from_str(
            r#"[
                {"duration": 300.0, "syncedLyrics": "[00:01.00]wrong length", "plainLyrics": "wrong length"},
                {"duration": 149.0, "syncedLyrics": null, "plainLyrics": "plain only"},
                {"duration": 151.0, "syncedLyrics": "[00:01.00]right", "plainLyrics": "right"},
                {"duration": 149.0, "instrumental": true}
            ]"#,
        )
        .unwrap();
        let lyrics = pick(&results, Some(149)).unwrap();
        assert_eq!(lyrics.synced.as_deref(), Some("[00:01.00]right"));
        assert!(pick(&results[3..], Some(149)).is_none(), "instrumentals have no lyrics");
    }
}
