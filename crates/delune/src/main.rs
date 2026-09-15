//! `delune` — one binary for the server and the terminal client.
//!
//! ```text
//! delune serve                 # run the server next to Navidrome
//! delune tui --server URL      # open the terminal UI against a server
//! ```

use std::net::SocketAddr;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "delune", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the delune server (API, web UI, Soulseek client).
    Serve {
        /// Address to listen on.
        #[arg(long, env = "DELUNE_BIND", default_value = "0.0.0.0:7474")]
        bind: SocketAddr,
    },
    /// Open the terminal UI.
    Tui {
        /// URL of a running delune server.
        #[arg(long, env = "DELUNE_SERVER", default_value = "http://localhost:7474")]
        server: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { bind } => {
            init_logging();
            delune_server::serve(bind).await?;
        }
        // No logging to stdout here: it would corrupt the terminal UI.
        Command::Tui { server } => delune_tui::run(server)?,
    }
    Ok(())
}

fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_env("DELUNE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
