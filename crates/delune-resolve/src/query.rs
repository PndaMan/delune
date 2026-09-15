//! Turning a resolved release into words Soulseek will match.
//!
//! Soulseek matches every word against file paths, so each extra word is another
//! chance to miss. Store titles carry plenty of words no shared folder has:
//! "(Deluxe Edition)", "- 2011 Remaster", "feat. Someone", "(Official Video)". We
//! drop those, keep the artist and the title, and strip a leading `-`, which
//! Soulseek reads as "exclude this word".

/// Words that mark a bracketed or dashed suffix as edition noise rather than part of the title.
const NOISE: &[&str] = &[
    "remaster",
    "remastered",
    "deluxe",
    "edition",
    "expanded",
    "anniversary",
    "bonus",
    "explicit",
    "clean",
    "version",
    "official",
    "video",
    "audio",
    "lyrics",
    "lyric",
    "visualizer",
    "visualiser",
    "hd",
    "hq",
    "4k",
    "mv",
    "feat",
    "feat.",
    "ft",
    "ft.",
    "featuring",
    "with",
    "prod",
    "prod.",
    "mono",
    "stereo",
    "mix",
    "single",
];

/// "Discovery (Deluxe Edition) [2011 Remaster]" → "Discovery".
///
/// Any bracketed group goes (folder names rarely repeat them), and so does a
/// trailing " - …" part made of edition words, but a dash that belongs to the title
/// ("Selected Ambient Works 85-92", "Pt. 1 - Pt. 2") stays.
#[must_use]
pub fn clean_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0usize;
    for c in title.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }

    let mut parts: Vec<&str> = out.split(" - ").collect();
    while parts.len() > 1 && parts.last().is_some_and(|p| is_noise(p)) {
        parts.pop();
    }
    let mut joined = parts.join(" - ");

    // "Song feat. Someone" without brackets.
    for marker in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        // ASCII-only case folding keeps byte offsets valid for truncation.
        let found = joined.as_bytes().windows(marker.len()).position(|w| w.eq_ignore_ascii_case(marker.as_bytes()));
        if let Some(at) = found {
            joined.truncate(at);
            break;
        }
    }
    collapse(&joined)
}

fn is_noise(part: &str) -> bool {
    let lower = part.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric() && c != '.').any(|word| {
        NOISE.contains(&word) || (word.len() == 4 && word.starts_with(['1', '2']) && word.parse::<u16>().is_ok())
    }) && lower.split_whitespace().count() <= 4
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The Soulseek search for an artist and a title.
#[must_use]
pub fn search_query(artist: Option<&str>, title: &str) -> String {
    let title = clean_title(title);
    let artist = artist.map(clean_title).filter(|a| {
        let lower = a.to_lowercase();
        !a.is_empty() && lower != "various artists" && lower != "various" && !title.to_lowercase().starts_with(&lower)
    });
    let words = match artist {
        Some(artist) => format!("{artist} {title}"),
        None => title,
    };
    words
        .split_whitespace()
        .map(|w| w.trim_start_matches('-'))
        .filter(|w| !w.is_empty() && *w != "&" && *w != "-")
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split a video title like "Radiohead - No Surprises (Official Video)" into artist
/// and title, using the channel name to decide which side is the artist.
///
/// YouTube's auto-generated "Artist - Topic" channels upload plain track titles, so
/// the channel is the artist. Otherwise "Artist - Title" is by far the most common
/// shape; without a dash the channel is the best guess.
#[must_use]
pub fn split_video_title(title: &str, channel: &str) -> (String, String) {
    let channel = channel.trim();
    if let Some(artist) = channel.strip_suffix(" - Topic") {
        return (artist.trim().to_owned(), clean_title(title));
    }
    let channel = channel.strip_suffix("VEVO").unwrap_or(channel).trim();
    let cleaned = clean_title(title);
    match cleaned.split_once(" - ") {
        Some((artist, track)) if !artist.trim().is_empty() && !track.trim().is_empty() => {
            (artist.trim().to_owned(), track.trim().to_owned())
        }
        _ => (channel.to_owned(), cleaned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_edition_noise_but_keeps_real_titles() {
        assert_eq!(clean_title("Discovery (Deluxe Edition) [2011 Remaster]"), "Discovery");
        assert_eq!(clean_title("Wish You Were Here - 2011 Remastered Version"), "Wish You Were Here");
        assert_eq!(clean_title("Paper Tiger - Remastered"), "Paper Tiger");
        assert_eq!(clean_title("Selected Ambient Works 85-92"), "Selected Ambient Works 85-92");
        assert_eq!(clean_title("Shine On You Crazy Diamond - Pts. 1-5"), "Shine On You Crazy Diamond - Pts. 1-5");
        assert_eq!(clean_title("Walk On Water feat. Beyoncé"), "Walk On Water");
        assert_eq!(clean_title("No Surprises (Official Video)"), "No Surprises");
    }

    #[test]
    fn builds_queries_soulseek_can_match() {
        assert_eq!(
            search_query(Some("Radiohead"), "OK Computer OKNOTOK 1997 2017"),
            "Radiohead OK Computer OKNOTOK 1997 2017"
        );
        assert_eq!(search_query(Some("Various Artists"), "Now 100"), "Now 100");
        assert_eq!(search_query(Some("Daft Punk"), "Discovery (Deluxe)"), "Daft Punk Discovery");
        assert_eq!(search_query(Some("Simon & Garfunkel"), "-Bookends"), "Simon Garfunkel Bookends");
        assert_eq!(search_query(Some("Beck"), "Beck - Sea Change"), "Beck Sea Change");
    }

    #[test]
    fn splits_video_titles() {
        assert_eq!(
            split_video_title("Radiohead - No Surprises", "Radiohead"),
            ("Radiohead".into(), "No Surprises".into())
        );
        assert_eq!(split_video_title("Walk On Water", "Eminem - Topic"), ("Eminem".into(), "Walk On Water".into()));
        assert_eq!(
            split_video_title("Adele - Hello (Official Music Video)", "AdeleVEVO"),
            ("Adele".into(), "Hello".into())
        );
        assert_eq!(split_video_title("Flickermood", "Forss"), ("Forss".into(), "Flickermood".into()));
    }
}
