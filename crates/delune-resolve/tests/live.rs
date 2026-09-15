//! Resolution against the real services. These need the internet and the services'
//! goodwill, so they're ignored by default:
//!
//! ```sh
//! cargo test -p delune-resolve --test live -- --ignored
//! ```

use delune_resolve::{Query, Resolver, classify};

async fn resolve(url: &str) -> delune_core::api::ResolvedLink {
    let Query::Link(parsed) = classify(url) else { panic!("{url} isn't a link") };
    Resolver::new().resolve(&parsed).await.unwrap_or_else(|e| panic!("{url}: {e}"))
}

macro_rules! live {
    ($name:ident, $url:expr, $query:expr) => {
        #[tokio::test]
        #[ignore = "needs the internet"]
        async fn $name() {
            let resolved = resolve($url).await;
            println!("{resolved:#?}");
            assert_eq!(resolved.query, $query);
        }
    };
}

live!(spotify_album, "https://open.spotify.com/album/6dVIqQ8qmQ5GBnJ9shOYGE", "Radiohead OK Computer");
live!(spotify_track, "https://open.spotify.com/track/7c378mlmubSu7NGkLFa4sN", "Radiohead Airbag");
live!(deezer_album, "https://www.deezer.com/album/302127", "Daft Punk Discovery");
live!(apple_album, "https://music.apple.com/us/album/ok-computer/1097861387", "Radiohead OK Computer");
live!(youtube_video, "https://www.youtube.com/watch?v=u5CVsCnxyXg", "Radiohead No Surprises");
live!(
    youtube_music_album,
    "https://music.youtube.com/playlist?list=OLAK5uy_nMr9h2VlS-2PULNz3M3XVXQj_P3C2bqaY",
    "Eminem Revival"
);
live!(soundcloud_track, "https://soundcloud.com/forss/flickermood", "Forss Flickermood");
live!(
    bandcamp_album,
    "https://boardsofcanada.bandcamp.com/album/tomorrows-harvest",
    "Boards of Canada Tomorrow's Harvest"
);
live!(tidal_track, "https://tidal.com/browse/track/77646171", "Beck Paper Tiger");
live!(qobuz_album, "https://www.qobuz.com/us-en/album/ok-computer-radiohead/0634904078164", "Radiohead OK Computer");
live!(
    musicbrainz_release,
    "https://musicbrainz.org/release/b84ee12a-09ef-421b-82de-0441a926375b",
    "Pink Floyd The Dark Side of the Moon"
);

#[tokio::test]
#[ignore = "needs the internet"]
async fn musicbrainz_matches_by_barcode_isrc_and_name() {
    // One resolver, as the server has, so MusicBrainz's rate limit is respected.
    let resolver = Resolver::new();
    let resolve = |url: &'static str| {
        let resolver = &resolver;
        async move {
            let Query::Link(parsed) = classify(url) else { panic!("{url} isn't a link") };
            let link = resolver.resolve(&parsed).await.unwrap_or_else(|e| panic!("{url}: {e}"));
            resolver.match_musicbrainz(&link).await.unwrap_or(link)
        }
    };

    // Deezer gives Discovery's barcode.
    let discovery = resolve("https://www.deezer.com/album/302127").await;
    let found = discovery.musicbrainz.as_ref().expect("matched by barcode");
    println!("{found:#?}");
    assert_eq!(found.matched_by, delune_core::api::MatchedBy::Barcode);
    assert_eq!(discovery.year, Some(2001));

    // A Deezer track gives its ISRC.
    let track = resolve("https://www.deezer.com/track/3135556").await;
    println!("{:#?}", track.musicbrainz);
    assert_eq!(track.musicbrainz.map(|m| m.matched_by), Some(delune_core::api::MatchedBy::Isrc));

    // Spotify gives neither; the exact title and artist still match.
    let okc = resolve("https://open.spotify.com/album/6dVIqQ8qmQ5GBnJ9shOYGE").await;
    println!("{:#?}", okc.musicbrainz);
    assert_eq!(okc.musicbrainz.map(|m| m.matched_by), Some(delune_core::api::MatchedBy::Name));
    assert_eq!(okc.year, Some(1997));
}
