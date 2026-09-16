//! Finding the delune server, and remembering it.
//!
//! People know where their Navidrome is; fewer remember delune's port. So the
//! client takes whatever it's given — delune's address, a bare host name, or the
//! Navidrome address — and tries the places delune usually lives next to it. The
//! server that answered, and the session it gave, are kept in
//! `~/.config/delune/tui.toml` (readable only by you) so the next start is instant.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use delune_core::api::Health;
use serde::{Deserialize, Serialize};
use url::Url;

/// delune's usual port.
const PORT: u16 = 7474;

/// What the client remembers between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// A session token from signing in; the password itself is never kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl Saved {
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("delune").join("tui.toml"))
    }

    #[must_use]
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save, readable only by the owner.
    ///
    /// # Errors
    ///
    /// When the file can't be written.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = format!(
            "# Where delune-tui connects, kept between runs.\n{}",
            toml::to_string(self).map_err(std::io::Error::other)?
        );
        #[cfg(unix)]
        {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            let mut file =
                std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
            // `mode` only applies to new files; an older one may be readable by others.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(text.as_bytes())
        }
        #[cfg(not(unix))]
        std::fs::write(path, text)
    }
}

/// Addresses delune might be at, given what someone typed.
#[must_use]
pub fn candidates(input: &str) -> Vec<String> {
    let input = input.trim().trim_end_matches('/');
    let with_scheme = if input.contains("://") { input.to_owned() } else { format!("http://{input}") };
    let Ok(url) = Url::parse(&with_scheme) else { return Vec::new() };
    // IPv6 hosts come back already in brackets.
    let Some(host) = url.host_str() else { return Vec::new() };
    let exact = with_scheme.trim_end_matches('/').to_owned();
    let mut found = vec![exact];
    // Next to Navidrome, or wherever the host is: delune's own port, then behind the
    // same web server at /delune, as a reverse proxy often puts it.
    // Asked for https, so never fall back to plain http.
    let scheme = url.scheme();
    found.push(format!("{scheme}://{host}:{PORT}"));
    found.push(format!("{scheme}://{host}/delune"));
    if scheme == "http" {
        found.push(format!("https://{host}/delune"));
    }
    found.dedup();
    found
}

async fn is_delune(http: &reqwest::Client, base: &str) -> bool {
    let Ok(response) = http.get(format!("{base}/api/v1/health")).send().await else { return false };
    response.json::<Health>().await.is_ok_and(|h| h.name == "delune")
}

/// The first of [`candidates`] where a delune server answers.
///
/// # Errors
///
/// When none of them does.
pub async fn find(input: &str) -> Result<String> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(4)).build()?;
    let tried = candidates(input);
    if tried.is_empty() {
        bail!("“{input}” isn't an address. Try the host delune or Navidrome runs on, like myserver or 192.168.1.20.");
    }
    for base in &tried {
        if is_delune(&http, base).await {
            return Ok(base.clone());
        }
    }
    bail!("No delune server answered. Tried:\n  {}", tried.join("\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tries_the_usual_places() {
        assert_eq!(
            candidates("music.home:4533"),
            [
                "http://music.home:4533",
                "http://music.home:7474",
                "http://music.home/delune",
                "https://music.home/delune"
            ]
        );
        assert_eq!(
            candidates("https://delune.example.com/"),
            ["https://delune.example.com", "https://delune.example.com:7474", "https://delune.example.com/delune"],
            "https stays https"
        );
        assert_eq!(candidates("http://[::1]:4533")[1], "http://[::1]:7474");
        assert!(candidates("").is_empty());
        assert!(candidates("   ").is_empty());
    }

    #[tokio::test]
    async fn finds_a_server_that_answers_as_delune() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let health = serde_json::json!({"status": "ok", "name": "delune", "version": "test"});
        let router = health.to_string();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break };
                let body = router.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let (status, body) = if request.starts_with("GET /api/v1/health") {
                        ("200 OK", body)
                    } else {
                        ("404 Not Found", String::new())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        assert_eq!(find(&format!("127.0.0.1:{port}")).await.unwrap(), format!("http://127.0.0.1:{port}"));
        assert!(find("no-such-host.invalid").await.is_err());
    }
}
