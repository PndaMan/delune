//! Search the real Soulseek network from the command line.
//!
//! ```sh
//! DELUNE_SLSK_USERNAME=me DELUNE_SLSK_PASSWORD=secret \
//!   cargo run -p delune-soulseek --example search -- "boards of canada geogaddi"
//! ```
//!
//! Logs in, runs one search, and prints a line per responding peer. Useful for
//! checking protocol changes against the live network.

use std::time::Duration;

use delune_soulseek::{Client, Config, SessionState};

#[tokio::main]
async fn main() {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let query = if query.is_empty() { "radiohead ok computer".to_owned() } else { query };
    let username = std::env::var("DELUNE_SLSK_USERNAME").expect("set DELUNE_SLSK_USERNAME");
    let password = std::env::var("DELUNE_SLSK_PASSWORD").expect("set DELUNE_SLSK_PASSWORD");

    let mut config = Config::new(username, password);
    config.search_timeout = Duration::from_secs(15);
    let client = Client::start(config);

    let mut state = client.state();
    let online = tokio::time::timeout(
        Duration::from_secs(20),
        state.wait_for(|s| matches!(s, SessionState::Online { .. } | SessionState::Stopped(_))),
    )
    .await;
    let current = client.state().borrow().clone();
    println!("session: {current:?}");
    if online.is_err() || !matches!(current, SessionState::Online { .. }) {
        std::process::exit(1);
    }

    let mut search = client.search(&query).await.expect("search");
    println!("searching for {query:?} (token {})", search.token());
    let (mut peers, mut files) = (0, 0);
    while let Some(response) = search.next().await {
        peers += 1;
        files += response.files.len();
        let best = response
            .files
            .iter()
            .filter_map(delune_soulseek::peer::SharedFile::quality)
            .max_by_key(delune_core::Quality::rank);
        println!(
            "{:>4} files  {:<14} slot={} speed={:>7}B/s queue={:<3} {}",
            response.files.len(),
            best.map(|q| q.to_string()).unwrap_or_default(),
            u8::from(response.free_slot),
            response.avg_speed,
            response.queue_length,
            response.username,
        );
    }
    println!("done: {peers} peers, {files} files");
}
