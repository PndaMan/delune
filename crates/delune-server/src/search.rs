//! Live Soulseek search: turning raw peer responses into release candidates, and
//! streaming them to clients as Server-Sent Events.
//!
//! A peer's response is a flat list of file paths. People search for *releases*, so
//! files are grouped by folder, and each folder containing audio becomes one
//! [`Candidate`]. Grouping happens per response as it arrives, so the first results
//! show up within a second or two instead of after the search times out.

use std::collections::BTreeMap;
use std::convert::Infallible;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use delune_core::EntityKind;
use delune_core::{
    Quality,
    api::{ApiError, Candidate, CandidateFile, SearchEvent},
};
use delune_resolve::{
    ResolveError, classify,
    query::{broader_queries, search_query},
};
use delune_soulseek::peer::{SearchResponse, SharedFile};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::AppState;
use crate::accounts::CurrentUser;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
}

/// `GET /api/v1/search?q=…` — a stream of [`SearchEvent`]s. Closing the connection
/// cancels the search.
///
/// A pasted link is resolved first (a `resolved` event says what it points at), then
/// Soulseek is searched for its artist and title. If nothing turns up, broader searches
/// follow: without edition noise and filler words, and for links the title alone.
/// Soulseek requires every word, so store titles and typed queries often carry one
/// that shared folders don't.
pub async fn stream(State(app): State<AppState>, user: CurrentUser, Query(params): Query<SearchParams>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search, "search") {
        return denied;
    }
    let Some(client) = app.soulseek.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "soulseek-not-configured",
            "Soulseek isn't set up. Add a Soulseek username and password to the server configuration.",
        );
    };
    let input = match classify(&params.q) {
        delune_resolve::Query::Text(q) if q.is_empty() => {
            return error(StatusCode::BAD_REQUEST, "empty-query", "Type something to search for.");
        }
        input => input,
    };

    let (tx, rx) = mpsc::channel::<SearchEvent>(64);
    tokio::spawn(async move {
        let (query, fallback) = match input {
            delune_resolve::Query::Text(query) => {
                let fallback = broader_queries(&query);
                (query, fallback)
            }
            delune_resolve::Query::Link(parsed) => match app.resolver.resolve(&parsed).await {
                Ok(link) => {
                    let mut fallback = broader_queries(&link.query);
                    let title_alone = (link.artist.is_some() && link.kind != EntityKind::Artist)
                        .then(|| search_query(None, &link.title))
                        .filter(|f| f.len() >= 4 && *f != link.query && !fallback.contains(f));
                    fallback.extend(title_alone);
                    let query = link.query.clone();
                    let playlist = link.kind == EntityKind::Playlist;
                    if tx.send(SearchEvent::Resolved { link }).await.is_err() {
                        return;
                    }
                    // Playlists aren't one search: clients offer to import them instead.
                    if playlist {
                        let _ = tx.send(SearchEvent::Finished { peers: 0, candidates: 0 }).await;
                        return;
                    }
                    (query, fallback)
                }
                Err(e) => {
                    let message = match e {
                        ResolveError::Unsupported(message) => message,
                        other => {
                            format!("{}. Try typing the artist and album instead.", capitalise(&other.to_string()))
                        }
                    };
                    let _ = tx.send(SearchEvent::Failed { error: ApiError::new("link-unresolved", message) }).await;
                    return;
                }
            },
        };

        let (mut peers, mut total) = (0u32, 0u32);
        for query in std::iter::once(query).chain(fallback) {
            if total > 0 {
                break;
            }
            let Some((p, t)) = run(&app, &client, query, &tx).await else { return };
            peers += p;
            total += t;
        }
        let _ = tx.send(SearchEvent::Finished { peers, candidates: total }).await;
    });

    let events = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        let sse = Event::default().json_data(&event).unwrap_or_else(|_| Event::default().data("{}"));
        Some((Ok::<_, Infallible>(sse), rx))
    });
    Sse::new(events).keep_alive(KeepAlive::default()).into_response()
}

/// One Soulseek search, streamed. `None` when the client went away or the search couldn't start.
async fn run(
    app: &AppState,
    client: &delune_soulseek::Client,
    query: String,
    tx: &mpsc::Sender<SearchEvent>,
) -> Option<(u32, u32)> {
    let mut search = match client.search(&query).await {
        Ok(search) => search,
        Err(e) => {
            let error = ApiError::new("soulseek-unavailable", format!("Can't search right now: {e}."));
            let _ = tx.send(SearchEvent::Failed { error }).await;
            return None;
        }
    };
    let timeout_secs = u32::try_from(app.search_timeout.as_secs()).unwrap_or(u32::MAX);
    tx.send(SearchEvent::Started { query, timeout_secs }).await.ok()?;

    let (mut peers, mut total) = (0u32, 0u32);
    loop {
        let response = tokio::select! {
            response = search.next() => response,
            () = tx.closed() => return None,
        };
        let Some(response) = response else { break };
        peers += 1;
        let items = candidates(&response);
        if items.is_empty() {
            continue;
        }
        total += u32::try_from(items.len()).unwrap_or(u32::MAX);
        tx.send(SearchEvent::Candidates { items }).await.ok()?;
    }
    Some((peers, total))
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// Group one peer's files into release candidates.
#[must_use]
pub fn candidates(response: &SearchResponse) -> Vec<Candidate> {
    let mut folders: BTreeMap<&str, Vec<&SharedFile>> = BTreeMap::new();
    for file in &response.files {
        folders.entry(file.folder()).or_default().push(file);
    }

    folders
        .into_iter()
        .filter_map(|(folder, files)| {
            let entries: Vec<CandidateFile> = files
                .iter()
                .map(|f| {
                    let quality = f.quality();
                    CandidateFile {
                        path: f.path.clone(),
                        name: f.file_name().to_owned(),
                        size: f.size,
                        audio: quality.is_some(),
                        quality_label: quality.map(|q| q.to_string()),
                        quality,
                        duration_secs: f.duration_secs,
                    }
                })
                .collect();

            let audio: Vec<&CandidateFile> = entries.iter().filter(|f| f.audio).collect();
            if audio.is_empty() {
                return None;
            }
            let qualities: Vec<Quality> = audio.iter().filter_map(|f| f.quality).collect();
            let worst = qualities.iter().copied().min_by_key(Quality::rank);
            let mixed_quality = qualities.iter().any(|q| {
                q.codec != qualities[0].codec
                    || q.bit_depth != qualities[0].bit_depth
                    || q.sample_rate != qualities[0].sample_rate
            });
            let duration_secs = audio.iter().map(|f| f.duration_secs).sum::<Option<u32>>();
            let has_cover = entries.iter().any(|f| {
                std::path::Path::new(&f.name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| ["jpg", "jpeg", "png"].iter().any(|img| e.eq_ignore_ascii_case(img)))
            });
            let (title, parent) = display_names(folder);

            Some(Candidate {
                id: format!("{}\u{1f}{folder}", response.username),
                username: response.username.clone(),
                folder: folder.to_owned(),
                title,
                parent,
                audio_files: u32::try_from(audio.len()).unwrap_or(u32::MAX),
                total_bytes: entries.iter().map(|f| f.size).sum(),
                duration_secs,
                quality_label: worst.map(|q| q.to_string()),
                quality_rank: worst.map_or(0, |q| q.rank()),
                quality: worst,
                mixed_quality,
                has_cover,
                free_slot: response.free_slot,
                avg_speed: response.avg_speed,
                queue_length: response.queue_length,
                files: entries,
            })
        })
        .collect()
}

/// Pick a human title for a folder. Disc sub-folders (`CD1`, `Disc 2`) take their
/// name from the album folder above them.
fn display_names(folder: &str) -> (String, Option<String>) {
    let segments: Vec<&str> = folder.split(['\\', '/']).filter(|s| !s.is_empty() && !s.starts_with("@@")).collect();
    let last = segments.last().copied().unwrap_or(folder);
    let parent = segments.len().checked_sub(2).map(|i| segments[i]);
    if is_disc_folder(last)
        && let Some(album) = parent
    {
        let artist = segments.len().checked_sub(3).map(|i| segments[i].to_owned());
        return (format!("{album} ({last})"), artist);
    }
    (last.to_owned(), parent.map(str::to_owned))
}

fn is_disc_folder(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("cd").or_else(|| lower.strip_prefix("disc")).or_else(|| lower.strip_prefix("disk"));
    rest.is_some_and(|r| {
        let r = r.trim_start_matches([' ', '_', '-']);
        !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64, depth: Option<u32>, rate: Option<u32>, duration: Option<u32>) -> SharedFile {
        SharedFile {
            path: path.into(),
            size,
            extension: path.rsplit('.').next().unwrap().into(),
            bitrate_kbps: None,
            duration_secs: duration,
            vbr: false,
            sample_rate: rate,
            bit_depth: depth,
        }
    }

    fn response(files: Vec<SharedFile>) -> SearchResponse {
        SearchResponse {
            username: "moon".into(),
            token: 1,
            files,
            free_slot: true,
            avg_speed: 1_000_000,
            queue_length: 2,
            private_files: vec![],
        }
    }

    #[test]
    fn groups_by_folder_and_skips_folders_without_audio() {
        let r = response(vec![
            file(r"@@moon\Music\Radiohead\OK Computer\01 - Airbag.flac", 30, Some(16), Some(44_100), Some(284)),
            file(
                r"@@moon\Music\Radiohead\OK Computer\02 - Paranoid Android.flac",
                60,
                Some(16),
                Some(44_100),
                Some(386),
            ),
            file(r"@@moon\Music\Radiohead\OK Computer\cover.jpg", 1, None, None, None),
            file(r"@@moon\Music\Radiohead\Scans\booklet.png", 5, None, None, None),
        ]);
        let found = candidates(&r);
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.title, "OK Computer");
        assert_eq!(c.parent.as_deref(), Some("Radiohead"));
        assert_eq!(c.audio_files, 2);
        assert_eq!(c.files.len(), 3);
        assert_eq!(c.total_bytes, 91);
        assert_eq!(c.duration_secs, Some(670));
        assert_eq!(c.quality_label.as_deref(), Some("FLAC 16/44.1"));
        assert!(c.has_cover);
        assert!(!c.mixed_quality);
    }

    #[test]
    fn reports_worst_quality_and_mixed_flag() {
        let r = response(vec![
            file(r"@@moon\Album\01.flac", 1, Some(24), Some(96_000), None),
            file(r"@@moon\Album\02.flac", 1, Some(16), Some(44_100), None),
        ]);
        let c = &candidates(&r)[0];
        assert_eq!(c.quality_label.as_deref(), Some("FLAC 16/44.1"));
        assert!(c.mixed_quality);
        assert_eq!(c.duration_secs, None, "unknown durations don't add up to a total");
    }

    #[test]
    fn disc_folders_use_the_album_name() {
        assert_eq!(
            display_names(r"@@moon\Music\Pink Floyd\The Wall\CD 2"),
            ("The Wall (CD 2)".to_owned(), Some("Pink Floyd".to_owned()))
        );
        assert_eq!(display_names(r"@@moon\Disc1"), ("Disc1".to_owned(), None));
        assert!(!is_disc_folder("CDs"));
        assert!(is_disc_folder("disk_03"));
    }
}
