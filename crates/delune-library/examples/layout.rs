//! Print the naming template an existing library follows.
//!
//! ```sh
//! cargo run -p delune-library --example layout -- /srv/music
//! ```

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: layout <music folder>");
        std::process::exit(2);
    };
    let samples = delune_library::layout::sample(std::path::Path::new(&root), 300);
    match delune_library::layout::detect(&samples) {
        Some(layout) => {
            println!("{}", layout.template);
            println!("track padding {}, multi-disc {:?}", layout.options.track_padding, layout.options.multi_disc);
            println!("fits {} of {} files, such as:", layout.matching, layout.sampled);
            for example in layout.examples {
                println!("  {example}");
            }
        }
        None => println!("No layout recognised in {} files; are they tagged?", samples.len()),
    }
}
