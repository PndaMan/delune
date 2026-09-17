//! `delune-tui`: the terminal client, for any machine that can reach your delune.
//! `delune tui` (or just `delune`) does the same.
//!
//! ```sh
//! delune-tui                 # the server you used last time
//! delune-tui myserver        # finds delune on that host (next to Navidrome is fine)
//! delune-tui https://music.example.com/delune
//! ```

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "delune-tui", version, about = "Terminal client for delune", long_about = None)]
struct Cli {
    #[command(flatten)]
    args: delune_tui::cli::Args,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    delune_tui::cli::run(Cli::parse().args).await
}
