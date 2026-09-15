//! HTTP calls the TUI makes besides searching. Errors come back as the server's
//! own sentence, ready to show.

use delune_core::api::ApiError;
use reqwest::{Client, Method, Response};
use serde::de::DeserializeOwned;

async fn failure(response: Response) -> String {
    let status = response.status();
    response.json::<ApiError>().await.map_or_else(|_| format!("The server answered {status}."), |e| e.message)
}

fn unreachable(e: &reqwest::Error) -> String {
    if e.is_connect() { "Can't reach the server.".into() } else { e.to_string() }
}

/// `GET` and decode JSON.
pub async fn get<T: DeserializeOwned>(http: &Client, url: &str) -> Result<T, String> {
    let response = http.get(url).header("accept", "application/json").send().await.map_err(|e| unreachable(&e))?;
    if !response.status().is_success() {
        return Err(failure(response).await);
    }
    response.json().await.map_err(|e| format!("Unexpected answer from the server: {e}"))
}

/// Send a request with an optional JSON body; succeed on any 2xx.
pub async fn send(http: &Client, method: Method, url: &str, body: Option<&serde_json::Value>) -> Result<(), String> {
    let mut request = http.request(method, url);
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|e| unreachable(&e))?;
    if response.status().is_success() { Ok(()) } else { Err(failure(response).await) }
}
