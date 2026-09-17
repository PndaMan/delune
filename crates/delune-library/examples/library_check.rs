//! Run the library check over a music folder and print what it finds, without
//! changing anything: `cargo run -p delune-library --example library_check -- <dir>`.

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: library_check <music folder>");
        std::process::exit(2);
    };
    let scan = delune_library::health::scan(std::path::Path::new(&root));
    println!("{} albums, {} tracks, {} findings", scan.albums, scan.tracks, scan.findings.len());
    for finding in &scan.findings {
        println!("\n{:?} ({} files): {}", finding.kind, finding.files, finding.folders.join("  |  "));
        if let Some(look) = &finding.album {
            println!(
                "  album {:?} by {:?}, date {:?}, release {:?}",
                look.album, look.album_artist, look.date, look.release_id
            );
            for file in look.retag.iter().take(3) {
                println!("  retag {file}");
            }
            for file in look.strays.iter().take(5) {
                println!("  stray {file}");
            }
        }
        for copies in finding.duplicates.iter().take(3) {
            println!("  twice: {}", copies.join("  /  "));
        }
    }
}
