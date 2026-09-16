//! `delune-tui`: the terminal client, for any machine that can reach your delune.
//!
//! ```sh
//! delune-tui                 # the server you used last time
//! delune-tui myserver        # finds delune on that host (next to Navidrome is fine)
//! delune-tui https://music.example.com/delune
//! ```

use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::Parser;
use delune_tui::Exit;
use delune_tui::connect::{self, Saved};

#[derive(Debug, Parser)]
#[command(name = "delune-tui", version, about = "Terminal client for delune", long_about = None)]
struct Cli {
    /// delune's address, the host it runs on, or your Navidrome's address.
    /// Remembered for next time.
    #[arg(env = "DELUNE_SERVER")]
    server: Option<String>,
    /// Navidrome username, when the server has accounts. Asked for if needed.
    #[arg(long, env = "DELUNE_USERNAME")]
    username: Option<String>,
    /// Navidrome password. Prefer the prompt; flags end up in shell history.
    #[arg(long, env = "DELUNE_PASSWORD", hide_env_values = true)]
    password: Option<String>,
    /// Forget the remembered server and sign-in, then exit.
    #[arg(long)]
    forget: bool,
}

fn ask(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut saved = Saved::load();
    if cli.forget {
        if let Some(path) = Saved::path() {
            let _ = std::fs::remove_file(path);
        }
        println!("Forgotten. Next time, delune-tui asks where your server is.");
        return Ok(());
    }
    if !io::stdout().is_terminal() {
        bail!("delune-tui needs a terminal.");
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
            delune_tui::auth::signed_in(&server, username.take(), password.take(), saved.token.as_deref()).await?;
        saved.server = Some(server.clone());
        saved.token = token;
        if let Err(e) = saved.save() {
            eprintln!("Couldn't remember the server for next time: {e}");
        }
        match delune_tui::run(server.clone(), &http)? {
            Exit::Quit => return Ok(()),
            Exit::SignedOut => {
                if last_sign_in.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
                    saved.token = None;
                    let _ = saved.save();
                    bail!("{server} keeps refusing the sign-in. Check the address, or run delune-tui --forget.");
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
