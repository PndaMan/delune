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

// Parsed once at startup, so the size difference between variants doesn't matter.
#[allow(clippy::large_enum_variant)]
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
        #[command(flatten)]
        library: LibraryArgs,
        #[command(flatten)]
        navidrome: NavidromeArgs,
    },
    /// Open the terminal UI.
    Tui {
        /// URL of a running delune server.
        #[arg(long, env = "DELUNE_SERVER", default_value = "http://localhost:7474")]
        server: String,
        /// Navidrome username, when the server has accounts. Asked for if missing.
        #[arg(long, env = "DELUNE_USERNAME")]
        username: Option<String>,
        /// Navidrome password. Prefer the prompt; flags end up in shell history.
        #[arg(long, env = "DELUNE_PASSWORD", hide_env_values = true)]
        password: Option<String>,
    },
}

// Field names become the `--slsk-*` flags, so the shared prefix is the point.
#[allow(clippy::struct_field_names)]
#[derive(Debug, clap::Args)]
struct SoulseekArgs {
    /// Soulseek account name. Search is disabled without one.
    #[arg(long, env = "DELUNE_SLSK_USERNAME", requires = "slsk_password")]
    slsk_username: Option<String>,
    /// Soulseek account password.
    #[arg(long, env = "DELUNE_SLSK_PASSWORD", hide_env_values = true, requires = "slsk_username")]
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

#[derive(Debug, clap::Args)]
struct LibraryArgs {
    /// Your music folder (the one Navidrome scans). Approved imports are moved here.
    #[arg(long, env = "DELUNE_LIBRARY_DIR")]
    library_dir: Option<std::path::PathBuf>,
    /// How imported files are named, until naming is saved from the Settings page
    /// (which can also match an existing library).
    #[arg(long, env = "DELUNE_NAMING_TEMPLATE", default_value = delune_server::review::DEFAULT_TEMPLATE)]
    naming_template: String,
}

// Field names become the `--navidrome-*` flags.
#[allow(clippy::struct_field_names)]
#[derive(Debug, clap::Args)]
#[group(requires_all = ["navidrome_url", "navidrome_username", "navidrome_password"], multiple = true)]
struct NavidromeArgs {
    /// Navidrome's address, such as `http://localhost:4533`. Enables library checks and rescans.
    #[arg(long, env = "DELUNE_NAVIDROME_URL")]
    navidrome_url: Option<String>,
    /// A Navidrome admin account (rescans need admin rights).
    #[arg(long, env = "DELUNE_NAVIDROME_USERNAME")]
    navidrome_username: Option<String>,
    #[arg(long, env = "DELUNE_NAVIDROME_PASSWORD", hide_env_values = true)]
    navidrome_password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { bind, data_dir, soulseek, library, navidrome } => {
            init_logging();
            let template = delune_library::Template::parse(&library.naming_template)
                .map_err(|e| anyhow::anyhow!("DELUNE_NAMING_TEMPLATE is invalid: {e}"))?;
            let navidrome = match (navidrome.navidrome_url, navidrome.navidrome_username, navidrome.navidrome_password)
            {
                (Some(url), Some(username), Some(password)) => {
                    Some((url, delune_navidrome::Credentials { username, password }))
                }
                _ => None,
            };
            let config = delune_server::ServerConfig {
                soulseek: soulseek.into_config(),
                data_dir,
                library: delune_server::review::LibrarySettings {
                    library_dir: library.library_dir,
                    template,
                    options: delune_library::NamingOptions::default(),
                },
                navidrome,
            };
            delune_server::serve(bind, config).await?;
        }
        // No logging to stdout here: it would corrupt the terminal UI.
        Command::Tui { server, username, password } => {
            let http = delune_tui::auth::signed_in_client(server.trim_end_matches('/'), username, password).await?;
            delune_tui::run(server, &http)?;
        }
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

    #[test]
    fn soulseek_port_alone_is_fine_but_half_an_account_is_not() {
        assert!(Cli::try_parse_from(["delune", "serve", "--slsk-port", "2235"]).is_ok());
        assert!(Cli::try_parse_from(["delune", "serve", "--slsk-username", "moon"]).is_err());
        assert!(Cli::try_parse_from(["delune", "serve", "--slsk-password", "secret"]).is_err());
        assert!(
            Cli::try_parse_from(["delune", "serve", "--slsk-username", "moon", "--slsk-password", "secret"]).is_ok()
        );
    }
}
