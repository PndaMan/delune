//! Signing the TUI in.
//!
//! The server says whether it needs a sign-in: `GET /api/v1/session` answers with the
//! signed-in person (always someone in open mode) or `null`. When it's `null`, the TUI asks
//! for a username and password (or takes them from `DELUNE_USERNAME` and
//! `DELUNE_PASSWORD`), exchanges them for a session token, and sends that token as
//! a bearer header from then on. The password is never stored.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use delune_core::api::{ApiError, LoginRequest, Me};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

/// Lets the server list this as "Terminal UI" among someone's devices.
const USER_AGENT: &str = concat!("delune-tui/", env!("CARGO_PKG_VERSION"));

/// An HTTP client that is signed in to `base`, prompting for credentials if needed.
///
/// # Errors
///
/// When the server can't be reached or the credentials are refused.
pub async fn signed_in_client(
    base: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<reqwest::Client> {
    let anonymous = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let probe = match anonymous.get(format!("{base}/api/v1/session")).send().await {
        Ok(response) => response.json::<Option<Me>>().await,
        // Unreachable: carry on unsigned and let the UI show the problem.
        Err(_) => return Ok(anonymous),
    };
    if !matches!(probe, Ok(None)) {
        return Ok(anonymous);
    }

    let username = match username {
        Some(name) => name,
        None => prompt("Navidrome username: ")?,
    };
    let password = match password {
        Some(password) => password,
        None => prompt_hidden("Password: ")?,
    };
    let response = anonymous
        .post(format!("{base}/api/v1/session"))
        .json(&LoginRequest { username, password, token: true })
        .send()
        .await
        .context("couldn't reach the delune server")?;
    if !response.status().is_success() {
        let error = response.json::<ApiError>().await.map_or_else(|_| "Sign-in failed.".to_owned(), |e| e.message);
        bail!(error);
    }
    let me: Me = response.json().await.context("unexpected sign-in response")?;
    let token = me.token.context("the server didn't return a session token")?;

    let mut headers = HeaderMap::new();
    let mut value = HeaderValue::from_str(&format!("Bearer {token}")).context("invalid session token")?;
    value.set_sensitive(true);
    headers.insert(AUTHORIZATION, value);
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).default_headers(headers).build()?)
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

/// Read a line without echoing it.
fn prompt_hidden(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    terminal::enable_raw_mode()?;
    let mut secret = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(anyhow::anyhow!("cancelled"));
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char(c) => secret.push(c),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e.into()),
        }
    };
    terminal::disable_raw_mode()?;
    println!();
    result.map(|()| secret)
}
