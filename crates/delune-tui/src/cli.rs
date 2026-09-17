//! Starting the terminal client: finding the server and signing in.

use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::time::{Duration, Instant};

use crate::Exit;
use crate::connect::{self, Saved};
use anyhow::{Result, bail};

/// How to reach the server, for `delune-tui` and `delune tui`.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct Args {
    /// delune's address, the host it runs on, or your Navidrome's address.
    /// Remembered for next time.
    #[arg(env = "DELUNE_SERVER")]
    pub server: Option<String>,
    /// Navidrome username, when the server has accounts. Asked for if needed.
    #[arg(long, env = "DELUNE_USERNAME")]
    pub username: Option<String>,
    /// Navidrome password. Prefer the prompt; flags end up in shell history.
    #[arg(long, env = "DELUNE_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,
    /// Forget the remembered server and sign-in, then exit.
    #[arg(long)]
    pub forget: bool,
}

fn ask(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

/// Find the server, sign in, and run the terminal UI until the person quits.
///
/// # Errors
///
/// When there's no terminal, the server can't be found, or signing in keeps failing.
pub async fn run(cli: Args) -> Result<()> {
    let mut saved = Saved::load();
    if cli.forget {
        if let Some(path) = Saved::path() {
            let _ = std::fs::remove_file(path);
        }
        println!("Forgotten. Next time, the terminal client asks where your server is.");
        return Ok(());
    }
    if !io::stdout().is_terminal() {
        bail!("The terminal client needs a terminal.");
    }

    let given = match (cli.server, &saved.server) {
        (Some(server), _) => server,
        (None, Some(server)) => server.clone(),
        (None, None) => {
            println!("Where does delune run? Its address, the host name, or your Navidrome's address all work.");
            ask("Server: ")?
        }
    };
    let mut server = if saved.server.as_deref() == Some(given.as_str()) {
        // Remembered: check it hasn't moved (say, to https), or signing in can't stick.
        connect::settle(&given).await
    } else {
        println!("Looking for delune at {given}…");
        let found = connect::find(&given).await?;
        println!("Found it at {found}.");
        found
    };
    if saved.server.as_deref() != Some(server.as_str()) && !same_server(saved.server.as_deref(), &server) {
        // A different server; its sessions are its own.
        saved.token = None;
    }
    server = server.trim_end_matches('/').to_owned();

    let (mut username, mut password) = (cli.username, cli.password);
    let mut last_sign_in: Option<Instant> = None;
    loop {
        let (http, token) =
            crate::auth::signed_in(&server, username.take(), password.take(), saved.token.as_deref()).await?;
        saved.server = Some(server.clone());
        saved.token = token;
        if let Err(e) = saved.save() {
            eprintln!("Couldn't remember the server for next time: {e}");
        }
        match crate::run(server.clone(), &http)? {
            Exit::Quit => return Ok(()),
            Exit::SignedOut => {
                if last_sign_in.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
                    saved.token = None;
                    let _ = saved.save();
                    bail!("{server} keeps refusing the sign-in. Check the address, or run it with --forget.");
                }
                last_sign_in = Some(Instant::now());
                println!("Your session ended. Sign in again to carry on.");
                saved.token = None;
            }
        }
    }
}

/// The same host, reached another way (`http` then `https`).
fn same_server(before: Option<&str>, now: &str) -> bool {
    let host = |url: &str| {
        url.split_once("://").map_or(url, |(_, rest)| rest).split(['/', ':']).next().unwrap_or("").to_owned()
    };
    before.is_some_and(|b| host(b) == host(now))
}
