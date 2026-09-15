//! # delune-navidrome
//!
//! A small, typed client for the parts of the Subsonic API delune needs from
//! Navidrome:
//!
//! | Need                              | Endpoint                 |
//! |-----------------------------------|--------------------------|
//! | Check URL + credentials           | `ping`                   |
//! | Log users in, detect admins       | `getUser`                |
//! | "Already in library?"             | `search3`                |
//! | Pick up imported files            | `startScan`, `getScanStatus` |
//!
//! Navidrome has no upload endpoint, so delune runs on the same host and writes to
//! the music folder directly; this client only *tells* Navidrome about it.
//!
//! Authentication uses the Subsonic token scheme: `t = md5(password + salt)` with a
//! fresh random salt per request, so the password itself is never sent. It is still
//! a password-equivalent secret — always use HTTPS for remote servers.

use md5::{Digest, Md5};
use serde::Deserialize;
use std::fmt::Write as _;
use url::Url;

/// Subsonic API version we speak. Navidrome implements 1.16.1.
pub const API_VERSION: &str = "1.16.1";
/// Sent as the `c` (client) parameter; shows up in Navidrome's player list.
pub const CLIENT_NAME: &str = "delune";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid Navidrome URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("could not reach Navidrome: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Navidrome rejected the request ({code}): {message}")]
    Api { code: u32, message: String },
    #[error("unexpected response from server — is this really Navidrome/Subsonic?")]
    Unexpected,
}

impl Error {
    /// Subsonic error 40: wrong username or password.
    #[must_use]
    pub const fn is_bad_credentials(&self) -> bool {
        matches!(self, Self::Api { code: 40, .. })
    }
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// A Navidrome server connection for one user.
#[derive(Debug, Clone)]
pub struct Client {
    base: Url,
    creds: Credentials,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: &str, creds: Credentials) -> Result<Self, Error> {
        let mut base = Url::parse(base_url)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let http = reqwest::Client::builder().user_agent(concat!("delune/", env!("CARGO_PKG_VERSION"))).build()?;
        Ok(Self { base, creds, http })
    }

    /// Check that the URL points at a Subsonic server and the credentials work.
    pub async fn ping(&self) -> Result<ServerInfo, Error> {
        let body: Envelope<Empty> = self.get("ping", &[]).await?;
        body.into_result().map(|(info, _)| info)
    }

    pub async fn user(&self, username: &str) -> Result<User, Error> {
        let body: Envelope<UserBody> = self.get("getUser", &[("username", username)]).await?;
        body.into_result().map(|(_, b)| b.user)
    }

    /// Search albums and songs. Navidrome also matches MusicBrainz IDs here, which is
    /// how delune checks "do we already own this release?".
    pub async fn search(&self, query: &str, albums: u32, songs: u32) -> Result<SearchResult, Error> {
        let (albums, songs) = (albums.to_string(), songs.to_string());
        let params = [("query", query), ("artistCount", "0"), ("albumCount", &albums), ("songCount", &songs)];
        let body: Envelope<SearchBody> = self.get("search3", &params).await?;
        body.into_result().map(|(_, b)| b.search_result3)
    }

    /// Ask Navidrome to scan for changes. Requires an admin user.
    pub async fn start_scan(&self, full: bool) -> Result<ScanStatus, Error> {
        let body: Envelope<ScanBody> =
            self.get("startScan", &[("fullScan", if full { "true" } else { "false" })]).await?;
        body.into_result().map(|(_, b)| b.scan_status)
    }

    /// One album with its songs.
    pub async fn album(&self, id: &str) -> Result<AlbumWithSongs, Error> {
        let body: Envelope<AlbumBody> = self.get("getAlbum", &[("id", id)]).await?;
        body.into_result().map(|(_, b)| b.album)
    }

    /// Albums in name order, a page at a time (`getAlbumList2`).
    pub async fn albums(&self, offset: u32, size: u32) -> Result<Vec<Album>, Error> {
        let (offset, size) = (offset.to_string(), size.min(500).to_string());
        let params = [("type", "alphabeticalByName"), ("offset", offset.as_str()), ("size", size.as_str())];
        let body: Envelope<AlbumListBody> = self.get("getAlbumList2", &params).await?;
        body.into_result().map(|(_, b)| b.album_list2.album)
    }

    pub async fn scan_status(&self) -> Result<ScanStatus, Error> {
        let body: Envelope<ScanBody> = self.get("getScanStatus", &[]).await?;
        body.into_result().map(|(_, b)| b.scan_status)
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<T, Error> {
        let url = self.base.join(&format!("rest/{endpoint}"))?;
        let salt = random_salt();
        let token = auth_token(&self.creds.password, &salt);
        let auth = [
            ("u", self.creds.username.as_str()),
            ("t", token.as_str()),
            ("s", salt.as_str()),
            ("v", API_VERSION),
            ("c", CLIENT_NAME),
            ("f", "json"),
        ];
        let response = self.http.get(url).query(&auth).query(params).send().await?.error_for_status()?;
        response.json::<T>().await.map_err(|_| Error::Unexpected)
    }
}

/// `md5(password + salt)` as lowercase hex.
#[must_use]
pub fn auth_token(password: &str, salt: &str) -> String {
    Md5::digest(format!("{password}{salt}").as_bytes()).iter().fold(String::with_capacity(32), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn random_salt() -> String {
    format!("{:016x}", rand::random::<u64>())
}

// ---- Response types -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    #[serde(rename = "subsonic-response")]
    response: Response<T>,
}

#[derive(Debug, Deserialize)]
struct Response<T> {
    status: String,
    error: Option<ApiError>,
    #[serde(flatten)]
    info: ServerInfo,
    #[serde(flatten)]
    body: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: u32,
    message: Option<String>,
}

impl<T> Envelope<T> {
    fn into_result(self) -> Result<(ServerInfo, T), Error> {
        let r = self.response;
        if r.status != "ok" {
            let err = r.error.ok_or(Error::Unexpected)?;
            return Err(Error::Api { code: err.code, message: err.message.unwrap_or_default() });
        }
        let body = r.body.ok_or(Error::Unexpected)?;
        Ok((r.info, body))
    }
}

/// What the server says about itself on every response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub server_version: Option<String>,
    #[serde(default)]
    pub open_subsonic: bool,
}

impl ServerInfo {
    #[must_use]
    pub fn is_navidrome(&self) -> bool {
        self.server_type.as_deref() == Some("navidrome")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub username: String,
    #[serde(default)]
    pub admin_role: bool,
    pub email: Option<String>,
}

/// Body of responses that carry nothing beyond [`ServerInfo`], like `ping`.
#[derive(Debug, Deserialize)]
struct Empty {}

#[derive(Debug, Deserialize)]
struct UserBody {
    user: User,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    pub album: Vec<Album>,
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlbumListBody {
    #[serde(default)]
    album_list2: AlbumList,
}

#[derive(Debug, Default, Deserialize)]
struct AlbumList {
    #[serde(default)]
    album: Vec<Album>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchBody {
    #[serde(default)]
    search_result3: SearchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub song_count: Option<u32>,
    pub music_brainz_id: Option<String>,
}

/// `getAlbum`: the album and its songs in disc and track order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumWithSongs {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    #[serde(default)]
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize)]
struct AlbumBody {
    album: AlbumWithSongs,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub path: Option<String>,
    pub suffix: Option<String>,
    pub bit_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub sampling_rate: Option<u32>,
    pub duration: Option<u32>,
    pub track: Option<u32>,
    pub disc_number: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub scanning: bool,
    #[serde(default)]
    pub count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanBody {
    scan_status: ScanStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_subsonic_docs_example() {
        // From the Subsonic API documentation: password "sesame", salt "c19b2d".
        assert_eq!(auth_token("sesame", "c19b2d"), "26719a1196d2a940705a59634eb18eab");
    }

    #[test]
    fn salts_differ() {
        assert_ne!(random_salt(), random_salt());
    }

    #[test]
    fn parses_ping() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.64.0 (abc)","openSubsonic":true}}"#;
        let env: Envelope<Empty> = serde_json::from_str(json).unwrap();
        let (info, _) = env.into_result().unwrap();
        assert!(info.is_navidrome());
        assert!(info.open_subsonic);
    }

    #[test]
    fn parses_api_error() {
        let json = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","error":{"code":40,"message":"Wrong username or password"}}}"#;
        let env: Envelope<Empty> = serde_json::from_str(json).unwrap();
        assert!(env.into_result().unwrap_err().is_bad_credentials());
    }

    #[test]
    fn parses_search3() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome",
          "searchResult3":{"album":[{"id":"a1","name":"OK Computer","artist":"Radiohead","year":1997,"songCount":12}],
          "song":[{"id":"s1","title":"Airbag","suffix":"flac","bitRate":900,"bitDepth":16,"samplingRate":44100}]}}}"#;
        let env: Envelope<SearchBody> = serde_json::from_str(json).unwrap();
        let (_, body) = env.into_result().unwrap();
        assert_eq!(body.search_result3.album[0].name, "OK Computer");
        assert_eq!(body.search_result3.song[0].bit_depth, Some(16));
    }

    #[test]
    fn parses_get_album() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"al1","name":"Twoism","artist":"Boards of Canada","year":1995,
          "song":[{"id":"s1","title":"Sixtyniner","track":1,"discNumber":1,"suffix":"flac","bitDepth":24,"samplingRate":96000}]}}}"#;
        let env: Envelope<AlbumBody> = serde_json::from_str(json).unwrap();
        let (_, body) = env.into_result().unwrap();
        assert_eq!(body.album.song[0].track, Some(1));
        assert_eq!(body.album.year, Some(1995));
    }

    #[test]
    fn empty_search_is_not_an_error() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{}}}"#;
        let env: Envelope<SearchBody> = serde_json::from_str(json).unwrap();
        assert_eq!(env.into_result().unwrap().1.search_result3, SearchResult::default());
    }

    #[test]
    fn base_url_keeps_subpath() {
        let c =
            Client::new("https://example.com/navidrome", Credentials { username: "u".into(), password: "p".into() })
                .unwrap();
        assert_eq!(c.base.join("rest/ping").unwrap().as_str(), "https://example.com/navidrome/rest/ping");
    }
}
