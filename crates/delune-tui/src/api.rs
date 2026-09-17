//! HTTP calls the TUI makes besides searching. Errors come back as the server's
//! own sentence, ready to show, and say whether the session has ended.

use std::fmt;

use delune_core::api::ApiError;
use reqwest::{Client, Method, Response, StatusCode};
use serde::de::DeserializeOwned;

/// Why a call didn't work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    /// The server no longer accepts this session.
    pub signed_out: bool,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, signed_out: false }
    }
}

pub type Outcome<T> = Result<T, Failure>;

async fn failure(response: Response) -> Failure {
    let status = response.status();
    let message =
        response.json::<ApiError>().await.map_or_else(|_| format!("The server answered {status}."), |e| e.message);
    Failure { message, signed_out: status == StatusCode::UNAUTHORIZED }
}

fn unreachable(e: &reqwest::Error) -> Failure {
    let message = if e.is_connect() {
        "Can't reach the server.".into()
    } else if e.is_timeout() {
        "The server took too long to answer.".into()
    } else {
        e.to_string()
    };
    message.into()
}

/// `GET` and decode JSON.
pub async fn get<T: DeserializeOwned>(http: &Client, url: &str) -> Outcome<T> {
    let response = http.get(url).header("accept", "application/json").send().await.map_err(|e| unreachable(&e))?;
    if !response.status().is_success() {
        return Err(failure(response).await);
    }
    response.json().await.map_err(|e| format!("Unexpected answer from the server: {e}").into())
}

/// `GET` raw bytes, at most `limit` of them.
pub async fn bytes(http: &Client, url: &str, limit: usize) -> Outcome<Vec<u8>> {
    let response = http.get(url).send().await.map_err(|e| unreachable(&e))?;
    if !response.status().is_success() {
        return Err(failure(response).await);
    }
    let body = response.bytes().await.map_err(|e| unreachable(&e))?;
    if body.len() > limit {
        return Err("That picture is too big.".to_owned().into());
    }
    Ok(body.to_vec())
}

/// Send a request with an optional JSON body and decode the JSON answer.
pub async fn call<T: DeserializeOwned>(
    http: &Client,
    method: Method,
    url: &str,
    body: Option<&serde_json::Value>,
) -> Outcome<T> {
    let mut request = http.request(method, url).header("accept", "application/json");
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|e| unreachable(&e))?;
    if !response.status().is_success() {
        return Err(failure(response).await);
    }
    response.json().await.map_err(|e| format!("Unexpected answer from the server: {e}").into())
}

/// Send a request with an optional JSON body; succeed on any 2xx.
pub async fn send(http: &Client, method: Method, url: &str, body: Option<&serde_json::Value>) -> Outcome<()> {
    let mut request = http.request(method, url);
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|e| unreachable(&e))?;
    if response.status().is_success() { Ok(()) } else { Err(failure(response).await) }
}

/// A URL under `base` with query parameters, encoded properly.
#[must_use]
pub fn url_with(base: &str, path: &str, params: &[(&str, &str)]) -> String {
    let joined = format!("{base}{path}");
    match url::Url::parse(&joined) {
        Ok(mut url) => {
            if !params.is_empty() {
                let mut pairs = url.query_pairs_mut();
                for (key, value) in params {
                    pairs.append_pair(key, value);
                }
            }
            url.into()
        }
        Err(_) => joined,
    }
}
