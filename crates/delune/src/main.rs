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
        /// Where delune keeps staged downloads and its own data.
        #[arg(long, env = "DELUNE_DATA_DIR", default_value = "delune-data")]
        data_dir: std::path::PathBuf,
        #[command(flatten)]
        soulseek: SoulseekArgs,
    },
    /// Open the terminal UI.
    Tui {
        /// URL of a running delune server.
        #[arg(long, env = "DELUNE_SERVER", default_value = "http://localhost:7474")]
        server: String,
    },
}

// Field names become the `--slsk-*` flags, so the shared prefix is the point.
#[allow(clippy::struct_field_names)]
#[derive(Debug, clap::Args)]
#[group(requires_all = ["slsk_username", "slsk_password"], multiple = true)]
struct SoulseekArgs {
    /// Soulseek account name. Search is disabled without one.
    #[arg(long, env = "DELUNE_SLSK_USERNAME")]
    slsk_username: Option<String>,
    /// Soulseek account password.
    #[arg(long, env = "DELUNE_SLSK_PASSWORD", hide_env_values = true)]
    slsk_password: Option<String>,
    /// Port other Soulseek users connect to. Forward it on your router for more
    /// and faster results.
    #[arg(long, env = "DELUNE_SLSK_PORT", default_value_t = 2234)]
    slsk_port: u16,
}

impl SoulseekArgs {
    fn into_config(self) -> Option<delune_soulseek::Config> {
        let (Some(username), Some(password)) = (self.slsk_username, self.slsk_password) else { return None };
        let mut config = delune_soulseek::Config::new(username, password);
        config.listen_port = Some(self.slsk_port);
        Some(config)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { bind, data_dir, soulseek } => {
            init_logging();
            let config = delune_server::ServerConfig { soulseek: soulseek.into_config(), data_dir };
            delune_server::serve(bind, config).await?;
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
