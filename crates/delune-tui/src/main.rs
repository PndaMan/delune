//! `delune-tui`: the terminal client, for any machine that can reach your delune.
//!
//! ```sh
//! delune-tui                 # the server you used last time
//! delune-tui myserver        # finds delune on that host (next to Navidrome is fine)
//! delune-tui https://music.example.com/delune
//! ```

use std::io::{self, BufRead as _, IsTerminal as _, Write as _};

use anyhow::{Result, bail};
use clap::Parser;
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
    let server = if saved.server.as_deref() == Some(given.as_str()) {
        given
    } else {
        println!("Looking for delune at {given}…");
        let found = connect::find(&given).await?;
        println!("Found it at {found}.");
        if saved.server.as_deref() != Some(found.as_str()) {
            // A different server; its sessions are its own.
            saved.token = None;
        }
        found
    };

    let (http, token) =
        delune_tui::auth::signed_in(&server, cli.username, cli.password, saved.token.as_deref()).await?;
    saved.server = Some(server.clone());
    saved.token = token;
    if let Err(e) = saved.save() {
        eprintln!("Couldn't remember the server for next time: {e}");
    }
    delune_tui::run(server, &http)
}
