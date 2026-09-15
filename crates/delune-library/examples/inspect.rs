//! Inspect and verify audio files from the command line.
//!
//! ```sh
//! cargo run --release -p delune-library --example inspect -- ~/Music/album/*.flac
//! ```
//!
//! Prints what delune's review screen would show for each file: format, tags,
//! whether it decodes cleanly, and where its spectrum ends.

use std::path::PathBuf;
use std::time::Instant;

use delune_library::{inspect, verify};

fn main() {
    let paths: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: inspect <audio files>");
        std::process::exit(2);
    }
    for path in paths {
        println!("{}", path.display());
        match inspect::inspect(&path) {
            Ok(info) => {
                let t = &info.tags;
                println!(
                    "  {}  {}s  track {:?}/{:?} disc {:?}  {:?} - {:?} - {:?} ({:?})",
                    info.quality,
                    info.duration_secs,
                    t.track,
                    t.track_total,
                    t.disc,
                    t.artist,
                    t.album,
                    t.title,
                    t.year
                );
                let started = Instant::now();
                match verify::verify(&path, info.quality.codec.is_lossless()) {
                    Ok(v) => println!(
                        "  decoded {:.1}s, {} errors, cutoff {:?} Hz, suspect transcode: {}  ({:.1}s)",
                        v.decoded_secs,
                        v.decode_errors,
                        v.cutoff_hz,
                        v.suspect_transcode,
                        started.elapsed().as_secs_f32()
                    ),
                    Err(error) => println!("  can't verify: {error}"),
                }
            }
            Err(error) => println!("  can't inspect: {error}"),
        }
    }
}
