//! Try the import finishing touches on a file: `cargo run --example finish -- track.flac cover.jpg`
use std::path::Path;

use delune_library::extras::{Lyrics, LyricsMode, embed_cover, write_lyrics};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (audio, cover) = (Path::new(&args[0]), args.get(1).map(Path::new));
    if let Some(cover) = cover {
        println!("embed cover: {:?}", embed_cover(audio, cover));
    }
    let lyrics = Lyrics { synced: Some("[00:01.00]hello".into()), plain: Some("hello".into()) };
    println!("lyrics: {:?}", write_lyrics(audio, &lyrics, LyricsMode::Both));
}
