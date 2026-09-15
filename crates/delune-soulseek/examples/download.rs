//! Search the live network and download one track from the best-placed peer.
//!
//! ```sh
//! DELUNE_SLSK_USERNAME=me DELUNE_SLSK_PASSWORD=secret \
//!   cargo run -p delune-soulseek --example download -- ./downloads "artist album"
//! ```
//!
//! Picks a peer with a free upload slot and a FLAC file, downloads the smallest
//! audio file from that folder, and prints progress. For checking the transfer code
//! against real clients.

use std::path::PathBuf;
use std::time::Duration;

use delune_soulseek::{Client, Config, DownloadRequest, DownloadState, SessionState};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: download <dir> <query>"));
    let query = args.collect::<Vec<_>>().join(" ");
    let username = std::env::var("DELUNE_SLSK_USERNAME").expect("set DELUNE_SLSK_USERNAME");
    let password = std::env::var("DELUNE_SLSK_PASSWORD").expect("set DELUNE_SLSK_PASSWORD");

    let mut config = Config::new(username, password);
    config.search_timeout = Duration::from_secs(10);
    let client = Client::start(config);
    let mut state = client.state();
    let _ = tokio::time::timeout(Duration::from_secs(20), state.wait_for(|s| matches!(s, SessionState::Online { .. })))
        .await;

    let mut search = client.search(&query).await.expect("search");
    let mut candidates = Vec::new();
    while let Some(response) = search.next().await {
        if !response.free_slot || response.queue_length > 0 {
            continue;
        }
        if let Some(file) = response
            .files
            .iter()
            .filter(|f| f.extension.eq_ignore_ascii_case("flac") || f.path.to_ascii_lowercase().ends_with(".flac"))
            .min_by_key(|f| f.size)
        {
            candidates.push((response.avg_speed, response.username.clone(), file.path.clone(), file.size));
        }
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
    println!("{} peers with a free slot and FLAC files", candidates.len());

    for (speed, peer, path, size) in candidates.into_iter().take(3) {
        let name = path.rsplit(['\\', '/']).next().unwrap_or("track.flac").to_owned();
        println!("\ntrying {peer} ({speed} B/s): {name} ({size} bytes)");
        let download =
            client.download(DownloadRequest { username: peer, filename: path, destination: out.join(&name) });
        let mut state = download.state();
        let deadline = tokio::time::sleep(Duration::from_secs(120));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                changed = state.changed() => {
                    if changed.is_err() { break; }
                    let current = state.borrow().clone();
                    println!("  {current:?}");
                    if current.is_finished() { break; }
                }
                () = &mut deadline => { println!("  still not finished after 2 minutes; moving on"); download.cancel(); break; }
            }
        }
        if matches!(download.finished().await, DownloadState::Completed { .. }) {
            println!("saved to {}", out.join(&name).display());
            return;
        }
    }
    std::process::exit(1);
}
