//! Making one folder one album.
//!
//! Players group tracks into albums by their tags, not their folder: album title,
//! album artist, release date and MusicBrainz release id. Tracks that arrived from
//! different places (a download, beets, another rip) disagree on those, so one folder
//! shows up as several albums, and a track tagged for another album can sit in it.
//!
//! [`look`] says what's inconsistent; [`tidy`] makes every track agree on the album
//! most of them already name, and moves tracks of other albums to those albums'
//! folders. Every tag it changes is recorded, so [`undo`] can put them back.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lofty::config::{ParseOptions, WriteOptions};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag};
use serde::{Deserialize, Serialize};

use crate::merge::{album_folders, album_key};
use crate::trash;

/// What a track says about the album it's on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub album: Option<String>,
    /// Every album artist value, as stored (some taggers store the same name twice).
    pub album_artists: Vec<String>,
    pub artist: Option<String>,
    pub date: Option<String>,
    pub release_date: Option<String>,
    pub release_id: Option<String>,
    pub release_group_id: Option<String>,
    pub release_artist_id: Option<String>,
}

fn primary(file: &mut lofty::file::TaggedFile) -> &mut Tag {
    if file.primary_tag().is_none() {
        let tag_type = file.primary_tag_type();
        file.insert_tag(Tag::new(tag_type));
    }
    file.primary_tag_mut().expect("a primary tag was just inserted")
}

fn clean(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Read a track's album identity, without decoding audio properties.
///
/// # Errors
///
/// When the file can't be read as tagged audio.
pub fn identity(path: &Path) -> Result<Identity, String> {
    let file = Probe::open(path)
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new().read_properties(false))
        .read()
        .map_err(|e| e.to_string())?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return Ok(Identity::default()) };
    Ok(Identity {
        album: clean(tag.album().as_deref()),
        album_artists: tag
            .get_strings(ItemKey::AlbumArtist)
            .flat_map(|v| v.split(';'))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect(),
        artist: clean(tag.artist().as_deref()),
        date: clean(tag.get_string(ItemKey::RecordingDate)),
        release_date: clean(tag.get_string(ItemKey::ReleaseDate)),
        release_id: clean(tag.get_string(ItemKey::MusicBrainzReleaseId)),
        release_group_id: clean(tag.get_string(ItemKey::MusicBrainzReleaseGroupId)),
        release_artist_id: clean(tag.get_string(ItemKey::MusicBrainzReleaseArtistId)),
    })
}

/// The album artist a track really has: its values without repeats, or its artist.
fn album_artist(id: &Identity) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for value in &id.album_artists {
        if !seen.iter().any(|s| s.eq_ignore_ascii_case(value)) {
            seen.push(value);
        }
    }
    if seen.is_empty() { id.artist.clone() } else { Some(seen.join("; ")) }
}

fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for value in values {
        match counts.iter_mut().find(|(v, _)| *v == value) {
            Some((_, n)) => *n += 1,
            None => counts.push((value, 1)),
        }
    }
    // Most common; the first seen wins a tie, so the answer doesn't wobble.
    counts.iter().enumerate().max_by_key(|(i, (_, n))| (*n, std::cmp::Reverse(*i))).map(|(_, (v, _))| (*v).to_owned())
}

/// The album a folder should be, and how its tracks fall short of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub album: String,
    pub album_artist: Option<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    /// Kept only when every track that has one agrees.
    pub release_id: Option<String>,
    /// Likewise; players use it to tell album artists apart.
    pub release_artist_id: Option<String>,
    /// Library-relative tracks whose album tags differ from the above.
    pub retag: Vec<String>,
    /// Library-relative tracks tagged for another album.
    pub strays: Vec<String>,
}

impl Look {
    #[must_use]
    pub fn is_tidy(&self) -> bool {
        self.retag.is_empty() && self.strays.is_empty()
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn audio(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && crate::health::is_audio(p)).collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Work out the album `dir` holds and which tracks disagree with it. `None` when the
/// folder has no readable tagged audio.
#[must_use]
pub fn look(root: &Path, dir: &Path) -> Option<Look> {
    let tracks: Vec<(PathBuf, Identity)> =
        audio(dir).into_iter().filter_map(|p| identity(&p).ok().map(|id| (p, id))).collect();
    if tracks.is_empty() {
        return None;
    }
    // The album most tracks name; the folder's own name settles a tie.
    let folder_key = dir.file_name().and_then(|n| n.to_str()).map(album_key).unwrap_or_default();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (_, id) in &tracks {
        if let Some(album) = &id.album {
            *by_key.entry(album_key(album)).or_default() += 1;
        }
    }
    let main_key = by_key
        .iter()
        .max_by_key(|(k, n)| (**n, **k == folder_key, std::cmp::Reverse((*k).clone())))
        .map_or_else(|| folder_key.clone(), |(k, _)| k.clone());

    let (members, others): (Vec<_>, Vec<_>) =
        tracks.iter().partition(|(_, id)| id.album.as_deref().is_none_or(|a| album_key(a) == main_key));
    let album = most_common(members.iter().filter_map(|(_, id)| id.album.as_deref()))
        .or_else(|| dir.file_name().and_then(|n| n.to_str()).map(str::to_owned))
        .unwrap_or_default();
    let artists: Vec<String> = members.iter().filter_map(|(_, id)| album_artist(id)).collect();
    let main_artist = most_common(artists.iter().map(String::as_str));
    let date = most_common(members.iter().filter_map(|(_, id)| id.date.as_deref()));
    let release_date = most_common(members.iter().filter_map(|(_, id)| id.release_date.as_deref()));
    let ids: Vec<&str> = members.iter().filter_map(|(_, id)| id.release_id.as_deref()).collect();
    // Tracks without the id would still be their own album, so it's all or nothing.
    let release_id = ids
        .first()
        .filter(|first| ids.len() == members.len() && ids.iter().all(|i| i == *first))
        .map(|i| (*i).to_owned());
    let artist_ids: Vec<&str> = members.iter().filter_map(|(_, id)| id.release_artist_id.as_deref()).collect();
    let release_artist_id = artist_ids
        .first()
        .filter(|first| artist_ids.len() == members.len() && artist_ids.iter().all(|i| i == *first))
        .map(|i| (*i).to_owned());

    let retag = members
        .iter()
        .filter(|(_, id)| {
            // The same album artist under several tag names ("ALBUM ARTIST", "ALBUMARTIST")
            // is one artist; only a different one splits the album.
            id.album.as_deref() != Some(album.as_str())
                || album_artist(id) != main_artist
                || (date.is_some() && id.date != date)
                || (release_date.is_some() && id.release_date != release_date)
                || id.release_id != release_id
                || (release_id.is_none() && id.release_group_id.is_some())
                || id.release_artist_id != release_artist_id
        })
        .map(|(p, _)| relative(root, p))
        .collect();
    let strays = others.iter().map(|(p, _)| relative(root, p)).collect();
    Some(Look { album, album_artist: main_artist, date, release_date, release_id, release_artist_id, retag, strays })
}

/// A safe folder name for an album.
fn folder_name(album: &str) -> String {
    let name: String =
        album
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
    let name = name.trim().trim_end_matches(['.', ' ']).to_owned();
    if name.is_empty() || name.starts_with('.') { format!("_{name}") } else { name }
}

/// A track's album tags before [`tidy`] changed them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Before {
    file: String,
    identity: Identity,
}

const TAGS: &str = ".delune-tags";

fn record(root: &Path, batch: &str, before: &Before) -> io::Result<()> {
    use std::io::Write as _;
    let dir = root.join(trash::DIR).join(batch);
    fs::create_dir_all(&dir)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(dir.join(TAGS))?;
    writeln!(file, "{}", serde_json::to_string(before).map_err(io::Error::other)?)
}

fn set(tag: &mut Tag, key: ItemKey, value: Option<&str>) {
    tag.remove_key(key);
    if let Some(value) = value {
        tag.insert_text(key, value.to_owned());
    }
}

fn write(path: &Path, id: &Identity) -> Result<(), String> {
    let mut file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let tag = primary(&mut file);
    set(tag, ItemKey::AlbumTitle, id.album.as_deref());
    tag.remove_key(ItemKey::AlbumArtist);
    for artist in &id.album_artists {
        tag.push(lofty::tag::TagItem::new(ItemKey::AlbumArtist, lofty::tag::ItemValue::Text(artist.clone())));
    }
    set(tag, ItemKey::RecordingDate, id.date.as_deref());
    set(tag, ItemKey::ReleaseDate, id.release_date.as_deref());
    set(tag, ItemKey::MusicBrainzReleaseId, id.release_id.as_deref());
    set(tag, ItemKey::MusicBrainzReleaseGroupId, id.release_group_id.as_deref());
    set(tag, ItemKey::MusicBrainzReleaseArtistId, id.release_artist_id.as_deref());
    file.save_to_path(path, WriteOptions::default()).map_err(|e| e.to_string())
}

/// What [`tidy`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tidied {
    pub retagged: usize,
    /// Tracks moved to the folder of the album they're tagged for.
    pub moved_out: usize,
    /// Tracks of another album with no folder to go to; left where they are.
    pub left: Vec<String>,
}

/// Make `dir` one album: retag what disagrees, move strays to their own album's
/// folder. Changes are recorded in trash batch `batch`.
///
/// # Errors
///
/// When a tag can't be written or a file can't be moved; what was done so far stays
/// recorded in the batch, so [`undo`] can reverse it.
pub fn tidy(root: &Path, dir: &Path, batch: &str) -> io::Result<Tidied> {
    let mut done = Tidied::default();
    let Some(look) = look(root, dir) else { return Ok(done) };

    for stray in &look.strays {
        let path = root.join(stray);
        let Ok(id) = identity(&path) else { continue };
        let (Some(album), Some(artist)) = (id.album.clone(), album_artist(&id)) else {
            done.left.push(stray.clone());
            continue;
        };
        // Its album's folder, or a new one for it beside this album.
        let home = album_folders(root, &artist, &album)
            .into_iter()
            .find(|d| d != dir)
            .or_else(|| dir.parent().map(|artist_dir| artist_dir.join(folder_name(&album))));
        match home {
            Some(home) if home != *dir => {
                fs::create_dir_all(&home)?;
                crate::health::move_in(root, &path, &home, batch, None)?;
                done.moved_out += 1;
            }
            _ => done.left.push(stray.clone()),
        }
    }

    for file in &look.retag {
        let path = root.join(file);
        let before = identity(&path).map_err(io::Error::other)?;
        let wanted = Identity {
            album: Some(look.album.clone()),
            album_artists: look.album_artist.iter().cloned().collect(),
            artist: before.artist.clone(),
            date: look.date.clone().or_else(|| before.date.clone()),
            release_date: look.release_date.clone().or_else(|| before.release_date.clone()),
            release_id: look.release_id.clone(),
            release_group_id: look.release_id.as_ref().and(before.release_group_id.clone()),
            release_artist_id: look.release_artist_id.clone(),
        };
        if wanted == before {
            continue;
        }
        record(root, batch, &Before { file: file.clone(), identity: before })?;
        write(&path, &wanted).map_err(io::Error::other)?;
        done.retagged += 1;
    }
    Ok(done)
}

/// Put back the tags a batch changed (before its files move back).
pub fn undo_tags(root: &Path, batch: &str) {
    let Ok(text) = fs::read_to_string(root.join(trash::DIR).join(batch).join(TAGS)) else { return };
    // Newest first, so a file changed twice ends up as it began.
    for line in text.lines().rev() {
        let Ok(before) = serde_json::from_str::<Before>(line) else { continue };
        let path = root.join(&before.file);
        if path.starts_with(root)
            && path.is_file()
            && let Err(error) = write(&path, &before.identity)
        {
            tracing::warn!(%error, path = %path.display(), "couldn't put tags back");
        }
    }
}

/// Undo a batch: tags first, then the files it moved or put away.
///
/// # Errors
///
/// As for [`trash::restore`].
pub fn undo(root: &Path, batch: &str) -> io::Result<Vec<String>> {
    undo_tags(root, batch);
    trash::restore(root, batch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_album_artists_count_once() {
        let id = Identity { album_artists: vec!["Fred again..".into(), "fred again..".into()], ..Identity::default() };
        assert_eq!(album_artist(&id).as_deref(), Some("Fred again.."));
        let solo = Identity { artist: Some("Kyle".into()), ..Identity::default() };
        assert_eq!(album_artist(&solo).as_deref(), Some("Kyle"));
    }

    #[test]
    fn the_most_common_value_wins_and_ties_keep_the_first() {
        assert_eq!(most_common(["b", "a", "a"].into_iter()).as_deref(), Some("a"));
        assert_eq!(most_common(["b", "a"].into_iter()).as_deref(), Some("b"));
        assert_eq!(most_common(std::iter::empty()), None);
    }

    /// A FLAC file with no audio, tagged as given.
    fn flac(path: &Path, album: &str, album_artists: &[&str], date: &str, release_id: Option<&str>) {
        let mut bytes = b"fLaC".to_vec();
        bytes.extend_from_slice(&[0x80, 0, 0, 34]);
        bytes.extend_from_slice(&[0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0]);
        let packed: u64 = (44_100u64 << 44) | (1 << 41) | (15 << 36);
        bytes.extend_from_slice(&packed.to_be_bytes());
        bytes.extend_from_slice(&[0; 16]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let id = Identity {
            album: Some(album.into()),
            album_artists: album_artists.iter().map(|a| (*a).to_owned()).collect(),
            artist: Some("Fred again..".into()),
            date: Some(date.into()),
            release_id: release_id.map(str::to_owned),
            ..Identity::default()
        };
        write(path, &id).unwrap();
    }

    #[test]
    fn one_folder_becomes_one_album_and_can_go_back() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let usb = root.join("Fred again._/USB (2025)");
        let beets = ["Fred again..", "Fred again.."];
        flac(&usb.join("03 - I Luv U.flac"), "USB", &beets, "2025-12-12", Some("be7a"));
        flac(&usb.join("04 - solo.flac"), "USB", &beets, "2025-12-12", Some("be7a"));
        flac(&usb.join("06 - FEISTY.flac"), "USB", &beets, "2025-12-12", Some("be7a"));
        flac(&usb.join("01 - Lights Burn Dimmer.flac"), "USB", &["Fred again.."], "2026-03-13", None);
        flac(&usb.join("17 - Back 2 Back.flac"), "USB", &["Fred again.."], "2025-12-12", Some("5891"));
        let al3 = "Actual Life 3 (January 1 - September 9 2022)";
        flac(&usb.join("11 - Clara.flac"), al3, &["Fred again.."], "2022-10-28", Some("b29d"));
        let home = root.join("Fred again._").join(format!("{al3} (2022)"));
        flac(&home.join("01 - January 1st 2022.flac"), al3, &["Fred again.."], "2022-10-28", Some("b29d"));

        let look = look(root, &usb).unwrap();
        assert_eq!(look.album, "USB");
        assert_eq!(look.album_artist.as_deref(), Some("Fred again.."));
        assert_eq!(look.date.as_deref(), Some("2025-12-12"));
        assert_eq!(look.release_id, None, "the tracks disagree");
        assert_eq!(look.strays, ["Fred again._/USB (2025)/11 - Clara.flac"]);
        assert_eq!(look.retag.len(), 5);
        let before_clara = identity(&usb.join("11 - Clara.flac")).unwrap();

        let batch = trash::batch_name();
        let done = tidy(root, &usb, &batch).unwrap();
        assert_eq!((done.retagged, done.moved_out), (5, 1));
        assert!(done.left.is_empty());
        assert!(home.join("11 - Clara.flac").exists(), "the stray joined its own album");
        let after = look_of(root, &usb);
        assert!(after.is_tidy(), "{after:?}");
        let solo = identity(&usb.join("04 - solo.flac")).unwrap();
        assert_eq!(solo.album_artists, ["Fred again.."]);
        assert_eq!((solo.date.as_deref(), solo.release_id), (Some("2025-12-12"), None));
        assert_eq!(identity(&home.join("11 - Clara.flac")).unwrap(), before_clara, "strays keep their tags");

        assert_eq!(trash::contents(root, &batch), Vec::<String>::new(), "notes aren't files to put back");
        let listed = &trash::batches(root)[0];
        assert_eq!(listed.changes.iter().filter(|c| c.kind == "retagged").count(), 5);
        assert_eq!(listed.changes.iter().filter(|c| c.kind == "moved").count(), 1);
        undo(root, &batch).unwrap();
        assert!(!root.join(".delune-tags").exists());
        assert!(usb.join("11 - Clara.flac").exists() && !home.join("11 - Clara.flac").exists());
        let solo = identity(&usb.join("04 - solo.flac")).unwrap();
        assert_eq!((solo.album_artists.len(), solo.release_id.as_deref()), (2, Some("be7a")));
        let dimmer = identity(&usb.join("01 - Lights Burn Dimmer.flac")).unwrap();
        assert_eq!(dimmer.date.as_deref(), Some("2026-03-13"));
    }

    #[test]
    fn repeated_album_artist_tags_are_not_a_problem() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = root.join("Radiohead/In Rainbows (2007)");
        let names = ["Radiohead", "Radiohead", "Radiohead"];
        flac(&dir.join("01 - 15 Step.flac"), "In Rainbows", &names, "2007", None);
        flac(&dir.join("02 - Bodysnatchers.flac"), "In Rainbows", &names, "2007", None);
        let look = look_of(root, &dir);
        assert!(look.is_tidy(), "{look:?}");

        // Another album's tracks in the folder still show.
        flac(&dir.join("01 - Everything In Its Right Place.flac"), "Kid A", &["Radiohead"], "2000", None);
        flac(&dir.join("02 - Kid A.flac"), "Kid A", &["Radiohead"], "2000", None);
        flac(&dir.join("03 - Nude.flac"), "In Rainbows", &names, "2007", None);
        let look = look_of(root, &dir);
        assert_eq!(look.album, "In Rainbows");
        assert_eq!(look.strays.len(), 2);
        assert!(look.retag.is_empty());

        // With no Kid A folder yet, they move into a new one beside In Rainbows.
        let batch = trash::batch_name();
        let done = tidy(root, &dir, &batch).unwrap();
        assert_eq!(done.moved_out, 2);
        assert!(root.join("Radiohead/Kid A/02 - Kid A.flac").exists());
        undo(root, &batch).unwrap();
        assert!(dir.join("02 - Kid A.flac").exists());
        assert!(!root.join("Radiohead/Kid A").exists(), "the folder made for them goes again");
    }

    fn look_of(root: &Path, dir: &Path) -> Look {
        look(root, dir).unwrap()
    }
}
