//! One SQLite database for everything delune keeps: `<data dir>/delune.db`.
//!
//! Each part of the server (downloads, accounts, the wishlist, settings…) saves its
//! state as a JSON document under its own key, so a change is written in one
//! transaction and a crash can't leave half a file behind. The database also keeps
//! a history per Soulseek user of how downloads from them went, which ranks search
//! results and shows in the release view.
//!
//! Earlier versions kept a JSON file per part. Those are imported the first time
//! the database opens and renamed to `*.json.imported`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use delune_core::api::PeerHistory;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A short id that is unique for the life of the process and sorts by creation time.
#[must_use]
pub fn new_id(prefix: &str) -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{prefix}{:x}{:05x}", now(), n & 0xf_ffff)
}

/// Keys that used to be `<key>.json` in the data directory.
const IMPORTED_FILES: &[&str] = &[
    "accounts",
    "automation",
    "chat",
    "import-options",
    "jobs",
    "naming",
    "notifications",
    "requests",
    "sharing",
    "stats",
    "wishlist",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS documents (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS peers (
    username       TEXT PRIMARY KEY,
    files_done     INTEGER NOT NULL DEFAULT 0,
    files_failed   INTEGER NOT NULL DEFAULT 0,
    bytes          INTEGER NOT NULL DEFAULT 0,
    seconds        INTEGER NOT NULL DEFAULT 0,
    first_seen     INTEGER NOT NULL,
    last_seen      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS share_lists (
    username   TEXT PRIMARY KEY,
    list       BLOB NOT NULL,
    fetched_at INTEGER NOT NULL
);
";

#[derive(Debug)]
pub struct Database {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Default for Database {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl Database {
    /// Open (or create) the database in `data_dir`, importing any old JSON files.
    ///
    /// # Errors
    ///
    /// When the data directory or database can't be created or read.
    pub fn open(data_dir: &Path) -> Result<Self, rusqlite::Error> {
        std::fs::create_dir_all(data_dir).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let path = data_dir.join("delune.db");
        let conn = Connection::open(&path)?;
        restrict(&path);
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        let db = Self { conn: Mutex::new(conn), path: Some(path) };
        db.import_json_files(data_dir);
        Ok(db)
    }

    /// A database that lives only as long as the process, for tests.
    ///
    /// # Panics
    ///
    /// Never in practice: an in-memory SQLite database with a fixed schema always opens.
    #[must_use]
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("an in-memory database always opens");
        conn.execute_batch(SCHEMA).expect("the schema is valid");
        Self { conn: Mutex::new(conn), path: None }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The document saved under `key`, if there is one and it still reads as `T`.
    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let text: Option<String> = self
            .lock()
            .query_row("SELECT value FROM documents WHERE key = ?1", params![key], |row| row.get(0))
            .optional()
            .unwrap_or_else(|error| {
                tracing::warn!(key, %error, "couldn't read from the database");
                None
            });
        let text = text?;
        match serde_json::from_str(&text) {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(key, %error, "saved data no longer reads; starting that part fresh");
                None
            }
        }
    }

    /// Replace the document under `key`. Failures are logged; the in-memory state
    /// carries on and the next save tries again.
    pub fn save<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> bool {
        let text = match serde_json::to_string(value) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(key, %error, "couldn't serialise data to save");
                return false;
            }
        };
        let saved = self.lock().execute(
            "INSERT INTO documents (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, text, now()],
        );
        if let Err(error) = &saved {
            tracing::warn!(key, %error, "couldn't save to the database");
        }
        saved.is_ok()
    }

    /// Bring in `<key>.json` files from before the database, once each.
    fn import_json_files(&self, data_dir: &Path) {
        for key in IMPORTED_FILES {
            let file = data_dir.join(format!("{key}.json"));
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let exists = self
                .lock()
                .query_row("SELECT 1 FROM documents WHERE key = ?1", params![key], |_| Ok(()))
                .optional()
                .ok()
                .flatten()
                .is_some();
            if !exists {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    tracing::warn!(file = %file.display(), "couldn't read an old data file; leaving it in place");
                    continue;
                };
                if !self.save(key, &value) {
                    continue;
                }
                tracing::info!(file = %file.display(), "imported into the database");
            }
            let _ = std::fs::rename(&file, file.with_extension("json.imported"));
        }
    }

    /// Keep `username`'s share list (compressed, as Soulseek sends it) for next time.
    pub fn save_share_list(&self, username: &str, list: &[u8]) {
        let result = self.lock().execute(
            "INSERT INTO share_lists (username, list, fetched_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(username) DO UPDATE SET list = excluded.list, fetched_at = excluded.fetched_at",
            params![username, list, now()],
        );
        if let Err(error) = result {
            tracing::warn!(%username, %error, "couldn't save a share list");
        }
    }

    /// The saved share list for `username` and when it was fetched (Unix seconds).
    #[must_use]
    pub fn share_list(&self, username: &str) -> Option<(Vec<u8>, u64)> {
        self.lock()
            .query_row("SELECT list, fetched_at FROM share_lists WHERE username = ?1", params![username], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .unwrap_or_else(|error| {
                tracing::warn!(%username, %error, "couldn't read a saved share list");
                None
            })
            .map(|(list, at)| (list, u64::try_from(at).unwrap_or(0)))
    }

    /// When `username`'s share list was saved, without reading the list itself.
    #[must_use]
    pub fn share_list_saved_at(&self, username: &str) -> Option<u64> {
        self.lock()
            .query_row("SELECT fetched_at FROM share_lists WHERE username = ?1", params![username], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .ok()
            .flatten()
            .map(|at| u64::try_from(at).unwrap_or(0))
    }

    /// Stop keeping `username`'s share list.
    pub fn forget_share_list(&self, username: &str) {
        if let Err(error) = self.lock().execute("DELETE FROM share_lists WHERE username = ?1", params![username]) {
            tracing::warn!(%username, %error, "couldn't forget a share list");
        }
    }

    /// Note how a finished download's files from `username` went.
    pub fn record_peer(&self, username: &str, files_done: u32, files_failed: u32, bytes: u64, seconds: u64) {
        let at = now();
        let result = self.lock().execute(
            "INSERT INTO peers (username, files_done, files_failed, bytes, seconds, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(username) DO UPDATE SET
                files_done = files_done + excluded.files_done,
                files_failed = files_failed + excluded.files_failed,
                bytes = bytes + excluded.bytes,
                seconds = seconds + excluded.seconds,
                last_seen = excluded.last_seen",
            params![
                username,
                files_done,
                files_failed,
                i64::try_from(bytes).unwrap_or(i64::MAX),
                i64::try_from(seconds).unwrap_or(i64::MAX),
                at
            ],
        );
        if let Err(error) = result {
            tracing::warn!(%username, %error, "couldn't record peer history");
        }
    }

    /// History for each of `usernames` that delune has downloaded from before.
    #[must_use]
    pub fn peers(&self, usernames: &[&str]) -> HashMap<String, PeerHistory> {
        let conn = self.lock();
        let Ok(mut query) = conn.prepare_cached(
            "SELECT files_done, files_failed, bytes, seconds, last_seen FROM peers WHERE username = ?1",
        ) else {
            return HashMap::new();
        };
        usernames
            .iter()
            .filter_map(|username| {
                let history = query
                    .query_row(params![username], |row| {
                        let bytes: i64 = row.get(2)?;
                        let seconds: i64 = row.get(3)?;
                        Ok(PeerHistory {
                            files_done: row.get(0)?,
                            files_failed: row.get(1)?,
                            bytes: u64::try_from(bytes).unwrap_or(0),
                            average_speed: u64::try_from(bytes.checked_div(seconds).unwrap_or(0)).unwrap_or(0),
                            last_seen: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        })
                    })
                    .optional()
                    .ok()
                    .flatten()?;
                Some(((*username).to_owned(), history))
            })
            .collect()
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// The database holds session hashes and settings: readable by delune only.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_documents_and_imports_old_files_once() {
        let dir = std::env::temp_dir().join(format!("delune-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wishlist.json"), r#"[{"id":"w1"}]"#).unwrap();
        std::fs::write(dir.join("naming.json"), "not json").unwrap();

        let db = Database::open(&dir).unwrap();
        let wishlist: serde_json::Value = db.load("wishlist").unwrap();
        assert_eq!(wishlist[0]["id"], "w1");
        assert!(dir.join("wishlist.json.imported").exists() && !dir.join("wishlist.json").exists());
        assert!(dir.join("naming.json").exists(), "unreadable files stay put");

        assert!(db.save("wishlist", &vec!["changed"]));
        drop(db);
        let reopened = Database::open(&dir).unwrap();
        assert_eq!(reopened.load::<Vec<String>>("wishlist").unwrap(), ["changed"]);
        assert_eq!(reopened.load::<Vec<u32>>("wishlist"), None, "a different shape starts fresh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(dir.join("delune.db")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_peer_history() {
        let db = Database::in_memory();
        db.record_peer("moonty", 10, 0, 300_000_000, 60);
        db.record_peer("moonty", 1, 2, 30_000_000, 6);
        let history = db.peers(&["moonty", "stranger"]);
        assert_eq!(history.len(), 1);
        let moonty = &history["moonty"];
        assert_eq!((moonty.files_done, moonty.files_failed, moonty.bytes), (11, 2, 330_000_000));
        assert_eq!(moonty.average_speed, 5_000_000);
    }
}
