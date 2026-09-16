//! # delune-soundcloud
//!
//! SoundCloud, as far as a music library needs it: who an artist is, what they've
//! put out lately, and whether a track is one they give away.
//!
//! SoundCloud no longer hands out API keys, so this only reads what anyone can:
//!
//! | What                | Where                                                          |
//! |---------------------|----------------------------------------------------------------|
//! | An artist's profile | the `user` entry in their page's `__sc_hydration` data           |
//! | Their newest tracks | the RSS feed SoundCloud publishes for every account              |
//! | One track           | the `sound` entry in the track page's `__sc_hydration` data      |
//!
//! Nothing here streams or downloads audio. A track the artist gives away says so
//! (`downloadable`, or a "free download" link of their own), and delune sends people
//! there, or to a fetch command its admin set up.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// SoundCloud serves bots a thinner page, so ask as a browser would.
const BROWSER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't reach SoundCloud: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SoundCloud's answer wasn't what delune expected")]
    Unreadable,
    #[error("nothing there")]
    NotFound,
}

/// An artist's profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: u64,
    /// The name they go by.
    pub name: String,
    /// The part of their address after `soundcloud.com/`.
    pub permalink: String,
    pub url: String,
    pub avatar: Option<String>,
    pub followers: u64,
    pub tracks: u64,
    pub verified: bool,
    pub description: Option<String>,
}

/// A track from an artist's feed, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedTrack {
    pub id: u64,
    pub title: String,
    pub url: String,
    /// Unix seconds.
    pub published_at: Option<u64>,
    pub duration_secs: Option<u32>,
    pub artwork: Option<String>,
}

/// One track's page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub url: String,
    pub artwork: Option<String>,
    pub duration_secs: Option<u32>,
    /// The artist switched on SoundCloud's own download button.
    pub downloadable: bool,
    /// The artist's own link, when it's labelled as a free download.
    pub free_download_link: Option<String>,
    /// The album it's from, when the label says.
    pub album: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    site: String,
    feeds: String,
}

impl Default for Client {
    fn default() -> Self {
        Self::new("https://soundcloud.com", "https://feeds.soundcloud.com")
    }
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

impl Client {
    /// `site` and `feeds` are SoundCloud's addresses; tests point them elsewhere.
    #[must_use]
    pub fn new(site: &str, feeds: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(BROWSER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self { http, site: site.trim_end_matches('/').to_owned(), feeds: feeds.trim_end_matches('/').to_owned() }
    }

    async fn page(&self, path: &str) -> Result<String, Error> {
        let response = self.http.get(format!("{}/{}", self.site, path.trim_start_matches('/'))).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::NotFound);
        }
        Ok(response.error_for_status()?.text().await?)
    }

    /// The artist at `soundcloud.com/{permalink}`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there's nobody there.
    pub async fn profile(&self, permalink: &str) -> Result<Profile, Error> {
        if permalink.is_empty() || permalink.contains(['/', '?', '#']) {
            return Err(Error::NotFound);
        }
        let page = self.page(permalink).await?;
        profile(&page).ok_or(Error::Unreadable)
    }

    /// The artist's newest tracks, from the feed SoundCloud publishes for them.
    ///
    /// # Errors
    ///
    /// When the feed can't be fetched.
    pub async fn feed(&self, user_id: u64) -> Result<Vec<FeedTrack>, Error> {
        let url = format!("{}/users/soundcloud:users:{user_id}/sounds.rss", self.feeds);
        let response = self.http.get(url).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::NotFound);
        }
        Ok(feed(&response.error_for_status()?.text().await?))
    }

    /// The track at `soundcloud.com/{artist}/{track}`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there's no such track.
    pub async fn track(&self, path: &str) -> Result<Track, Error> {
        let path = path.trim_matches('/');
        if path.split('/').count() != 2 {
            return Err(Error::NotFound);
        }
        let page = self.page(path).await?;
        track(&page).ok_or(Error::Unreadable)
    }
}

/// Addresses to try for an artist known only by name: `Fred again..` → `fredagain`, …
#[must_use]
pub fn permalink_guesses(name: &str) -> Vec<String> {
    let lower = name.trim().to_lowercase();
    let plain: String = lower.chars().filter(char::is_ascii_alphanumeric).collect();
    if plain.len() < 2 {
        return Vec::new();
    }
    let dashed =
        lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let mut guesses = vec![plain.clone(), dashed, format!("{plain}music"), format!("{plain}official")];
    guesses.retain(|g| g.len() >= 3);
    guesses.dedup();
    guesses
}

/// The `__sc_hydration` entry named `name`.
fn hydration(page: &str, name: &str) -> Option<Value> {
    let start = page.find("__sc_hydration")?;
    let from = start + page[start..].find('[')?;
    let end = from + page[from..].find(";</script>")?;
    let entries: Vec<Value> = serde_json::from_str(&page[from..end]).ok()?;
    entries.into_iter().find(|e| str_at(e, "/hydratable") == Some(name))?.get("data").cloned()
}

fn larger_art(url: &str) -> String {
    // SoundCloud's "large" is 100px; the same image comes bigger under another name.
    url.replace("-large.", "-t500x500.")
}

fn profile(page: &str) -> Option<Profile> {
    let user = hydration(page, "user")?;
    let permalink = str_at(&user, "/permalink")?.to_owned();
    Some(Profile {
        id: user.get("id")?.as_u64()?,
        name: str_at(&user, "/username").unwrap_or(&permalink).to_owned(),
        url: str_at(&user, "/permalink_url")
            .map_or_else(|| format!("https://soundcloud.com/{permalink}"), str::to_owned),
        permalink,
        avatar: str_at(&user, "/avatar_url").map(larger_art),
        followers: user.get("followers_count").and_then(Value::as_u64).unwrap_or(0),
        tracks: user.get("track_count").and_then(Value::as_u64).unwrap_or(0),
        verified: user.get("verified").and_then(Value::as_bool).unwrap_or(false),
        description: str_at(&user, "/description").map(str::to_owned),
    })
}

fn track(page: &str) -> Option<Track> {
    let sound = hydration(page, "sound")?;
    let purchase_title = str_at(&sound, "/purchase_title").unwrap_or_default().to_lowercase();
    let free = purchase_title.contains("free") || purchase_title.contains("download");
    Some(Track {
        id: sound.get("id")?.as_u64()?,
        title: str_at(&sound, "/title")?.to_owned(),
        artist: str_at(&sound, "/publisher_metadata/artist").or_else(|| str_at(&sound, "/user/username"))?.to_owned(),
        url: str_at(&sound, "/permalink_url")?.to_owned(),
        artwork: str_at(&sound, "/artwork_url").or_else(|| str_at(&sound, "/user/avatar_url")).map(larger_art),
        duration_secs: sound
            .get("full_duration")
            .or_else(|| sound.get("duration"))
            .and_then(Value::as_u64)
            .and_then(|ms| u32::try_from(ms / 1000).ok()),
        downloadable: sound.get("downloadable").and_then(Value::as_bool).unwrap_or(false)
            && sound.get("has_downloads_left").and_then(Value::as_bool).unwrap_or(true),
        free_download_link: free.then(|| str_at(&sound, "/purchase_url").map(str::to_owned)).flatten(),
        album: str_at(&sound, "/publisher_metadata/album_title").map(str::to_owned),
    })
}

/// The text inside `<tag>…</tag>` in `xml`, unescaped.
fn element<'a>(xml: &'a str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let body = start + xml[start..].find('>')? + 1;
    if xml[start..body].ends_with("/>") {
        return None;
    }
    let end = body + xml[body..].find(&format!("</{tag}>"))?;
    let text: &'a str = xml[body..end].trim();
    let text = text.strip_prefix("<![CDATA[").and_then(|t| t.strip_suffix("]]>")).unwrap_or(text);
    Some(unescape(text))
}

/// The value of `attribute` on the first `<tag …>` in `xml`.
fn attribute(xml: &str, tag: &str, attribute: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag} "))?;
    let end = start + xml[start..].find('>')?;
    let tag = &xml[start..end];
    let from = tag.find(&format!("{attribute}=\""))? + attribute.len() + 2;
    let to = from + tag[from..].find('"')?;
    Some(unescape(&tag[from..to]))
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

fn feed(xml: &str) -> Vec<FeedTrack> {
    xml.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let item = &item[..item.find("</item>")?];
            let guid = element(item, "guid")?;
            let id = guid.rsplit('/').next()?.parse().ok()?;
            Some(FeedTrack {
                id,
                title: element(item, "title")?,
                url: element(item, "link")?,
                published_at: element(item, "pubDate").as_deref().and_then(rfc2822_seconds),
                duration_secs: element(item, "itunes:duration").as_deref().and_then(clock_seconds),
                artwork: attribute(item, "itunes:image", "href"),
            })
        })
        .collect()
}

/// `00:01:41` → 101.
fn clock_seconds(clock: &str) -> Option<u32> {
    clock.split(':').try_fold(0u32, |total, part| Some(total * 60 + part.parse::<u32>().ok()?))
}

/// `Fri, 14 Nov 2025 21:38:51 +0000` → Unix seconds (the offset is always UTC in feeds).
fn rfc2822_seconds(date: &str) -> Option<u64> {
    let mut parts = date.split_whitespace().skip_while(|p| p.ends_with(','));
    let day: u64 = parts.next()?.parse().ok()?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| Some(*m) == parts.clone().next())? as u64
        + 1;
    parts.next();
    let year: u64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':').map(|p| p.parse::<u64>().ok());
    let (h, m, s) = (clock.next()??, clock.next()??, clock.next().flatten().unwrap_or(0));
    if !(1970..3000).contains(&year) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since the epoch, by the civil-from-days algorithm.
    let (y, mo) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y / 400;
    let year_of_era = y - era * 400;
    let day_of_year = (153 * mo + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_PAGE: &str = r#"<script>window.__sc_hydration = [{"hydratable":"anonymousId","data":"x"},{"hydratable":"user","data":{"id":637643742,"username":"Fred again..","permalink":"fredagain","permalink_url":"https://soundcloud.com/fredagain","avatar_url":"https://i1.sndcdn.com/avatars-abc-large.jpg","followers_count":719034,"track_count":188,"verified":true,"description":"working"}}];</script>"#;

    const TRACK_PAGE: &str = r#"<script>window.__sc_hydration = [{"hydratable":"sound","data":{"id":42,"title":"Leavemealone","permalink_url":"https://soundcloud.com/fredagain/leavemealone","artwork_url":"https://i1.sndcdn.com/artworks-x-large.jpg","duration":30000,"full_duration":213000,"downloadable":true,"has_downloads_left":true,"purchase_url":"https://hypeddit.com/x","purchase_title":"FREE DOWNLOAD","publisher_metadata":{"artist":"Fred again.., Baby Keem","album_title":"Leavemealone"},"user":{"username":"Fred again.."}}}];</script>"#;

    const FEED: &str = r#"<rss><channel><title>Fred again..</title><link>https://x</link>
        <item>
          <guid isPermaLink="false">tag:soundcloud,2010:tracks/2212761875</guid>
          <title>Bounce / Killa P &amp; friends</title>
          <pubDate>Fri, 14 Nov 2025 21:38:51 +0000</pubDate>
          <link>https://soundcloud.com/fredagain/bounce</link>
          <itunes:duration>00:01:41</itunes:duration>
          <itunes:image href="https://i1.sndcdn.com/artworks-y-t3000x3000.jpg"/>
        </item><item>
        </item></channel></rss>"#;

    #[test]
    fn reads_a_profile() {
        let p = profile(USER_PAGE).unwrap();
        assert_eq!((p.id, p.name.as_str(), p.permalink.as_str()), (637_643_742, "Fred again..", "fredagain"));
        assert_eq!((p.followers, p.tracks, p.verified), (719_034, 188, true));
        assert_eq!(p.avatar.as_deref(), Some("https://i1.sndcdn.com/avatars-abc-t500x500.jpg"));
        assert!(profile("<html>nothing</html>").is_none());
    }

    #[test]
    fn reads_a_track() {
        let t = track(TRACK_PAGE).unwrap();
        assert_eq!((t.id, t.title.as_str(), t.artist.as_str()), (42, "Leavemealone", "Fred again.., Baby Keem"));
        assert_eq!(t.duration_secs, Some(213), "the whole track, not the preview");
        assert!(t.downloadable);
        assert_eq!(t.free_download_link.as_deref(), Some("https://hypeddit.com/x"));
        assert_eq!(t.album.as_deref(), Some("Leavemealone"));

        let bought = TRACK_PAGE.replace("FREE DOWNLOAD", "Buy on Beatport");
        assert_eq!(track(&bought).unwrap().free_download_link, None, "a shop link isn't a free download");
    }

    #[test]
    fn reads_a_feed() {
        let tracks = feed(FEED);
        assert_eq!(tracks.len(), 1, "empty items are skipped");
        let t = &tracks[0];
        assert_eq!(t.id, 2_212_761_875);
        assert_eq!(t.title, "Bounce / Killa P & friends");
        assert_eq!(t.published_at, Some(1_763_156_331));
        assert_eq!(t.duration_secs, Some(101));
        assert_eq!(t.artwork.as_deref(), Some("https://i1.sndcdn.com/artworks-y-t3000x3000.jpg"));
    }

    #[test]
    fn guesses_addresses_from_names() {
        assert_eq!(
            permalink_guesses("Fred again.."),
            ["fredagain", "fred-again", "fredagainmusic", "fredagainofficial"]
        );
        assert_eq!(permalink_guesses("Burial")[0], "burial");
        assert!(permalink_guesses("!!").is_empty());
    }

    #[tokio::test]
    #[ignore = "talks to SoundCloud"]
    async fn live_profile_and_feed() {
        let client = Client::default();
        let profile = client.profile("fredagain").await.unwrap();
        assert!(profile.followers > 1000);
        assert!(!client.feed(profile.id).await.unwrap().is_empty());
    }
}
