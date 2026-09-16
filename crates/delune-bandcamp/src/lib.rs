//! # delune-bandcamp
//!
//! Bandcamp, as far as a music library needs it: find a release, see what it costs,
//! and — once someone links their account — list what they've bought and fetch the
//! files they paid for.
//!
//! Bandcamp retired its developer API, so this reads the same public pages and
//! endpoints a browser does:
//!
//! | What                  | Where                                                    |
//! |-----------------------|----------------------------------------------------------|
//! | Search                | `bcsearch_public_api/1/autocomplete_elastic`              |
//! | A release             | `data-tralbum` on the album or track page                 |
//! | Who you are           | `api/fan/2/collection_summary` (needs the cookie)         |
//! | What you've bought    | `api/fancollection/1/collection_items` (needs the cookie) |
//! | Files you've bought   | the download page's `data-blob`, then its `statdownload`  |
//!
//! Nothing here buys anything: Bandcamp takes payment on their own pages, so delune
//! only ever links people there. The cookie is someone's session, so treat it like a
//! password: it goes straight from settings into delune's database and is only ever
//! sent back to Bandcamp.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const SEARCH: &str = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";
const COLLECTION_SUMMARY: &str = "https://bandcamp.com/api/fan/2/collection_summary";
const COLLECTION_ITEMS: &str = "https://bandcamp.com/api/fancollection/1/collection_items";
/// Bandcamp serves bots a thinner page, so ask as a browser would.
const BROWSER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
/// Items asked for per page of a collection.
const PAGE: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't reach Bandcamp: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Bandcamp's answer wasn't what delune expected")]
    Unreadable,
    #[error("Bandcamp didn't accept the login; the cookie may have expired")]
    SignedOut,
    #[error("nothing there")]
    NotFound,
}

/// A release on Bandcamp, as found by a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub title: String,
    pub artist: String,
    /// The album or track page.
    pub url: String,
    pub art: Option<String>,
}

/// What a release costs and holds, from its page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseDetail {
    pub title: String,
    pub artist: String,
    pub url: String,
    pub art: Option<String>,
    /// The digital price, when it has one. Name-your-price albums have a minimum of 0.
    pub price: Option<f64>,
    pub currency: Option<String>,
    /// Name-your-price, so anything at or above `price` works.
    pub name_your_price: bool,
    pub track_titles: Vec<String>,
    /// Whether the signed-in fan already owns it (only meaningful with a cookie).
    pub owned: bool,
}

/// Something someone has bought.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Purchase {
    pub title: String,
    pub artist: String,
    /// Unix seconds, when Bandcamp says.
    pub purchased_at: Option<u64>,
    pub art: Option<String>,
    /// The release's own page.
    pub url: Option<String>,
    /// Where the files can be fetched from again.
    pub download_url: Option<String>,
}

/// Who a cookie belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fan {
    pub fan_id: u64,
    pub username: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Default for Client {
    fn default() -> Self {
        Self::new("https://bandcamp.com")
    }
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

impl Client {
    /// `base` is Bandcamp's address; tests point it at their own server.
    #[must_use]
    pub fn new(base: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(BROWSER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self { http, base: base.trim_end_matches('/').to_owned() }
    }

    /// Releases matching `query`, best first.
    ///
    /// # Errors
    ///
    /// When Bandcamp can't be reached or answers with something unreadable.
    pub async fn search(&self, query: &str) -> Result<Vec<Release>, Error> {
        let body = serde_json::json!({
            "search_text": query,
            "search_filter": "a",
            "full_page": false,
            "fan_id": null,
        });
        let url = format!("{}/api/bcsearch_public_api/1/autocomplete_elastic", self.base);
        let url = if self.base == "https://bandcamp.com" { SEARCH.to_owned() } else { url };
        let response: Value = self.http.post(url).json(&body).send().await?.error_for_status()?.json().await?;
        Ok(releases(&response))
    }

    /// A release page: price, tracklist, and whether the signed-in fan owns it.
    ///
    /// # Errors
    ///
    /// When the page can't be fetched or doesn't carry the release data.
    pub async fn release(&self, url: &str, cookie: Option<&str>) -> Result<ReleaseDetail, Error> {
        let mut request = self.http.get(url);
        if let Some(cookie) = cookie {
            request = request.header(reqwest::header::COOKIE, cookie_header(cookie));
        }
        let response = request.send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::NotFound);
        }
        let page = response.error_for_status()?.text().await?;
        release_detail(&page, url).ok_or(Error::Unreadable)
    }

    /// Who the cookie belongs to.
    ///
    /// # Errors
    ///
    /// [`Error::SignedOut`] when Bandcamp doesn't recognise the cookie.
    pub async fn fan(&self, cookie: &str) -> Result<Fan, Error> {
        let url = if self.base == "https://bandcamp.com" {
            COLLECTION_SUMMARY.to_owned()
        } else {
            format!("{}/api/fan/2/collection_summary", self.base)
        };
        let response = self.http.get(url).header(reqwest::header::COOKIE, cookie_header(cookie)).send().await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(Error::SignedOut);
        }
        let value: Value = response.error_for_status()?.json().await?;
        fan(&value).ok_or(Error::SignedOut)
    }

    /// Everything a fan has bought, newest first.
    ///
    /// # Errors
    ///
    /// When Bandcamp can't be reached, or the cookie has expired.
    pub async fn purchases(&self, cookie: &str, fan_id: u64) -> Result<Vec<Purchase>, Error> {
        let url = if self.base == "https://bandcamp.com" {
            COLLECTION_ITEMS.to_owned()
        } else {
            format!("{}/api/fancollection/1/collection_items", self.base)
        };
        let mut out = Vec::new();
        let mut older_than: Option<String> = None;
        // Bandcamp pages a collection by "older than this token".
        for _ in 0..20 {
            let body = serde_json::json!({
                "fan_id": fan_id,
                "older_than_token": older_than.clone().unwrap_or_else(|| format!("{}::a::", now())),
                "count": PAGE,
            });
            let response =
                self.http.post(&url).header(reqwest::header::COOKIE, cookie_header(cookie)).json(&body).send().await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(Error::SignedOut);
            }
            let value: Value = response.error_for_status()?.json().await?;
            let page = purchases(&value);
            let more = value.get("more_available").and_then(Value::as_bool).unwrap_or(false);
            let token = str_at(&value, "/last_token").map(str::to_owned);
            out.extend(page);
            if !more || token.is_none() || token == older_than {
                break;
            }
            older_than = token;
        }
        Ok(out)
    }

    /// Where the files for one purchase can be fetched, in `format`
    /// (`flac`, `mp3-320`, `mp3-v0`, `aac-hi`, `alac`, `aiff-lossless`, `vorbis`, `wav`).
    ///
    /// # Errors
    ///
    /// When the download page can't be read, or doesn't offer that format.
    pub async fn download_url(&self, cookie: &str, download_page: &str, format: &str) -> Result<String, Error> {
        let page = self
            .http
            .get(download_page)
            .header(reqwest::header::COOKIE, cookie_header(cookie))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let url = download_link(&page, format).ok_or(Error::Unreadable)?;
        // Bandcamp prepares big downloads, answering with JSON until the file is ready.
        for attempt in 0..10u32 {
            let response = self.http.get(&url).header(reqwest::header::COOKIE, cookie_header(cookie)).send().await;
            let Ok(response) = response else { break };
            let text = response.text().await.unwrap_or_default();
            match prepared(&text) {
                Prepared::Ready(ready) => return Ok(ready),
                Prepared::Wait => tokio::time::sleep(Duration::from_secs(2 + u64::from(attempt))).await,
                Prepared::NotJson => return Ok(url),
            }
        }
        Ok(url)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Accepts the `identity` value on its own, or a whole `Cookie:` header pasted from
/// a browser's network tab, and sends back something Bandcamp will accept.
#[must_use]
pub fn cookie_header(pasted: &str) -> String {
    let pasted = pasted.trim().trim_start_matches("Cookie:").trim();
    if pasted.contains("identity=") {
        return pasted.to_owned();
    }
    // A cookies.txt line: tab-separated, with the value last.
    if let Some(line) = pasted.lines().find(|line| line.contains("\tidentity\t"))
        && let Some(value) = line.rsplit('\t').next()
    {
        return format!("identity={}", value.trim());
    }
    format!("identity={pasted}")
}

fn releases(value: &Value) -> Vec<Release> {
    value
        .pointer("/auto/results")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter(|result| str_at(result, "/type") == Some("a"))
                .filter_map(|result| {
                    Some(Release {
                        title: str_at(result, "/name")?.to_owned(),
                        artist: str_at(result, "/band_name")?.to_owned(),
                        url: str_at(result, "/item_url_path").or_else(|| str_at(result, "/item_url_root"))?.to_owned(),
                        art: str_at(result, "/img").map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn fan(value: &Value) -> Option<Fan> {
    let fan_id = value.get("fan_id").and_then(Value::as_u64)?;
    let username = str_at(value, "/collection_summary/username").unwrap_or("you").to_owned();
    Some(Fan { fan_id, username, name: str_at(value, "/collection_summary/name").map(str::to_owned) })
}

fn purchases(value: &Value) -> Vec<Purchase> {
    let downloads = value.get("redownload_urls").and_then(Value::as_object);
    value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let sale_item_id = item.get("sale_item_id").and_then(Value::as_u64);
                    let kind = str_at(item, "/sale_item_type").unwrap_or("p");
                    let key = sale_item_id.map(|id| format!("{kind}{id}"));
                    Some(Purchase {
                        title: str_at(item, "/item_title")?.to_owned(),
                        artist: str_at(item, "/band_name")?.to_owned(),
                        purchased_at: str_at(item, "/purchased").and_then(rfc_seconds),
                        art: str_at(item, "/item_art_url").map(str::to_owned),
                        url: str_at(item, "/item_url").map(str::to_owned),
                        download_url: key.and_then(|key| downloads?.get(&key)?.as_str().map(str::to_owned)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Bandcamp's dates look like `18 Feb 2023 10:44:02 GMT`; the year is enough to sort by.
fn rfc_seconds(date: &str) -> Option<u64> {
    let mut parts = date.split_whitespace();
    let day: u64 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: u64 = parts.next()?.parse().ok()?;
    if !(1970..3000).contains(&year) {
        return None;
    }
    // Days since the epoch, by the civil-from-days algorithm.
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y / 400;
    let year_of_era = y - era * 400;
    let day_of_year = (153 * m + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400)
}

/// The `data-tralbum` blob a release page carries.
fn release_detail(page: &str, url: &str) -> Option<ReleaseDetail> {
    let blob = attribute_json(page, "data-tralbum")?;
    let current = blob.get("current")?;
    let art_id = current.get("art_id").and_then(Value::as_u64).or_else(|| blob.get("art_id")?.as_u64());
    let price = current.get("minimum_price").and_then(Value::as_f64);
    Some(ReleaseDetail {
        title: str_at(current, "/title")?.to_owned(),
        artist: str_at(&blob, "/artist").or_else(|| str_at(current, "/artist"))?.to_owned(),
        url: str_at(&blob, "/url").unwrap_or(url).to_owned(),
        art: art_id.map(|id| format!("https://f4.bcbits.com/img/a{id}_10.jpg")),
        price,
        // The band's currency sits on the page rather than in the blob; packages carry it too.
        currency: str_at(current, "/currency")
            .or_else(|| str_at(&blob, "/currency"))
            .map(str::to_owned)
            .or_else(|| attribute(page, "data-band-currency"))
            .or_else(|| str_at(&blob, "/packages/0/currency").map(str::to_owned)),
        name_your_price: current.get("is_set_price").and_then(Value::as_bool) != Some(true)
            && price.unwrap_or(0.0) >= 0.0
            && current.get("minimum_price_nonzero").and_then(Value::as_f64).is_some(),
        track_titles: blob
            .get("trackinfo")
            .and_then(Value::as_array)
            .map(|tracks| tracks.iter().filter_map(|t| str_at(t, "/title").map(str::to_owned)).collect())
            .unwrap_or_default(),
        owned: blob.get("is_purchased").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The download page's `data-blob` holds a link per format.
fn download_link(page: &str, format: &str) -> Option<String> {
    let blob = attribute_json(page, "data-blob")?;
    let item = blob.get("digital_items")?.as_array()?.first()?;
    let downloads = item.get("downloads")?;
    str_at(downloads, &format!("/{format}/url"))
        .or_else(|| str_at(downloads, "/flac/url"))
        .or_else(|| str_at(downloads, "/mp3-320/url"))
        .map(str::to_owned)
}

enum Prepared {
    Ready(String),
    Wait,
    NotJson,
}

/// Bandcamp's `statdownload` answers with JSONP-ish JSON until the file is zipped.
fn prepared(text: &str) -> Prepared {
    let trimmed = text.trim();
    let json =
        trimmed.strip_prefix("if(window.Downloads){Downloads.statResult(").and_then(|rest| rest.strip_suffix(")};"));
    let Ok(value) = serde_json::from_str::<Value>(json.unwrap_or(trimmed)) else {
        return Prepared::NotJson;
    };
    match str_at(&value, "/result") {
        Some("ok") => match str_at(&value, "/download_url").or_else(|| str_at(&value, "/url")) {
            Some(url) => Prepared::Ready(url.to_owned()),
            None => Prepared::Wait,
        },
        _ => Prepared::Wait,
    }
}

/// A plain attribute's value (`data-band-currency="GBP"`).
fn attribute(page: &str, attribute: &str) -> Option<String> {
    let start = page.find(&format!("{attribute}=\""))? + attribute.len() + 2;
    let end = start + page[start..].find('"')?;
    let value = unescape(page[start..end].trim());
    (!value.is_empty()).then_some(value)
}

/// Read a JSON attribute (`data-tralbum="{…}"`) out of a page.
fn attribute_json(page: &str, attribute: &str) -> Option<Value> {
    let start = page.find(&format!("{attribute}=\""))? + attribute.len() + 2;
    let end = start + page[start..].find('"')?;
    serde_json::from_str(&unescape(&page[start..end])).ok()
}

/// HTML entities Bandcamp escapes its JSON attributes with.
fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_search() {
        let value = serde_json::json!({"auto": {"results": [
            {"type": "a", "name": "Twoism", "band_name": "Boards of Canada", "item_url_path": "https://boardsofcanada.bandcamp.com/album/twoism", "img": "art.jpg"},
            {"type": "b", "name": "Boards of Canada"},
        ]}});
        let found = releases(&value);
        assert_eq!(found.len(), 1, "bands aren't releases");
        assert_eq!(found[0].title, "Twoism");
        assert_eq!(found[0].url, "https://boardsofcanada.bandcamp.com/album/twoism");
    }

    #[test]
    fn reads_a_release_page() {
        let page = r#"<div data-tralbum="{&quot;artist&quot;:&quot;Burial&quot;,&quot;url&quot;:&quot;https://burial.bandcamp.com/album/untrue&quot;,&quot;is_purchased&quot;:true,&quot;current&quot;:{&quot;title&quot;:&quot;Untrue&quot;,&quot;minimum_price&quot;:7.0,&quot;minimum_price_nonzero&quot;:7.0,&quot;currency&quot;:&quot;GBP&quot;,&quot;art_id&quot;:123},&quot;trackinfo&quot;:[{&quot;title&quot;:&quot;Archangel&quot;}]}"></div>"#;
        let detail = release_detail(page, "https://x").unwrap();
        assert_eq!((detail.title.as_str(), detail.artist.as_str()), ("Untrue", "Burial"));
        assert_eq!((detail.price, detail.currency.as_deref()), (Some(7.0), Some("GBP")));
        assert_eq!(detail.track_titles, ["Archangel"]);
        assert!(detail.owned);
        assert!(detail.art.unwrap().contains("a123"));
        assert!(detail.name_your_price, "a minimum with no set price means pay what you like above it");
    }

    #[test]
    fn finds_the_currency_on_the_page() {
        let page = r#"<div data-tralbum="{&quot;artist&quot;:&quot;Boards of Canada&quot;,&quot;current&quot;:{&quot;title&quot;:&quot;Twoism&quot;,&quot;minimum_price&quot;:8.0,&quot;is_set_price&quot;:true}}"></div>
            <script src="https://bandcamp.com/api/currency_data/1/javascript" data-band-currency="GBP"></script>"#;
        let detail = release_detail(page, "https://x").unwrap();
        assert_eq!(detail.currency.as_deref(), Some("GBP"));
        assert!(!detail.name_your_price);
    }

    #[test]
    fn reads_a_collection_page() {
        let value = serde_json::json!({
            "items": [{
                "item_title": "Untrue", "band_name": "Burial", "purchased": "18 Feb 2023 10:44:02 GMT",
                "item_art_url": "art.jpg", "item_url": "https://burial.bandcamp.com/album/untrue",
                "sale_item_id": 42, "sale_item_type": "p",
            }],
            "redownload_urls": {"p42": "https://bandcamp.com/download?id=1&sig=x"},
            "more_available": false,
        });
        let bought = purchases(&value);
        assert_eq!(bought.len(), 1);
        assert_eq!(bought[0].download_url.as_deref(), Some("https://bandcamp.com/download?id=1&sig=x"));
        assert_eq!(bought[0].purchased_at, Some(1_676_678_400));
    }

    #[test]
    fn reads_download_pages() {
        let page = r#"<div data-blob="{&quot;digital_items&quot;:[{&quot;downloads&quot;:{&quot;flac&quot;:{&quot;url&quot;:&quot;https://popplers.bandcamp.com/statdownload/album?id=1&quot;},&quot;mp3-320&quot;:{&quot;url&quot;:&quot;https://x/mp3&quot;}}}]}"></div>"#;
        assert_eq!(download_link(page, "flac").unwrap(), "https://popplers.bandcamp.com/statdownload/album?id=1");
        assert_eq!(
            download_link(page, "wav").unwrap(),
            "https://popplers.bandcamp.com/statdownload/album?id=1",
            "falls back"
        );
        assert!(matches!(
            prepared(r#"if(window.Downloads){Downloads.statResult({"result":"ok","download_url":"https://x/zip"})};"#),
            Prepared::Ready(url) if url == "https://x/zip"
        ));
        assert!(matches!(prepared(r#"{"result":"err"}"#), Prepared::Wait));
        assert!(matches!(prepared("PK\u{3}\u{4}binary"), Prepared::NotJson));
    }

    #[test]
    fn accepts_a_cookie_however_it_was_copied() {
        assert_eq!(cookie_header("abc123"), "identity=abc123");
        assert_eq!(cookie_header("  identity=abc123; session=9  "), "identity=abc123; session=9");
        assert_eq!(cookie_header("Cookie: identity=abc123"), "identity=abc123");
        assert_eq!(cookie_header(".bandcamp.com\tTRUE\t/\tTRUE\t0\tidentity\tabc123"), "identity=abc123");
    }

    #[test]
    fn dates_become_seconds() {
        assert_eq!(rfc_seconds("01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(rfc_seconds("25 Nov 2002 00:00:00 GMT"), Some(1_038_182_400));
        assert_eq!(rfc_seconds("nonsense"), None);
    }
}
