//! SQLite store: every chart Encore lists, and the Fretwork ratings of the ones rated so far.

use crate::encore::Chart;
use crate::fretwork::Rating;
use rusqlite::{Connection, OptionalExtension, params};

/// Bump when the rating maths changes; charts rated with an older version are rated again.
pub const FORMULA_VERSION: i64 = 3;
/// After this many failed attempts a chart is skipped until `--retry-failed`.
pub const MAX_FAILS: i64 = 3;

pub struct Db {
    pub conn: Connection,
}

pub struct Pending {
    pub md5: String,
    pub has_video: bool,
}

impl Db {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS charts (
                 md5 TEXT PRIMARY KEY,
                 chart_id INTEGER,
                 name TEXT, artist TEXT, album TEXT, genre TEXT, year TEXT, charter TEXT,
                 song_length INTEGER,          -- ms
                 modified_time TEXT,           -- ISO 8601 from Encore
                 has_video INTEGER NOT NULL DEFAULT 0,
                 instruments TEXT,             -- comma separated, from Encore
                 first_seen TEXT NOT NULL,
                 last_seen TEXT NOT NULL,
                 removed INTEGER NOT NULL DEFAULT 0,
                 rated_at TEXT,
                 formula_version INTEGER,
                 fail_count INTEGER NOT NULL DEFAULT 0,
                 last_error TEXT
             );
             CREATE INDEX IF NOT EXISTS charts_modified ON charts(modified_time);
             CREATE TABLE IF NOT EXISTS ratings (
                 md5 TEXT NOT NULL,
                 instrument TEXT NOT NULL,
                 d REAL NOT NULL,
                 remap INTEGER NOT NULL,       -- 0-6, spread like official tiers
                 tier INTEGER NOT NULL,        -- open-ended
                 movement REAL NOT NULL,
                 spread REAL NOT NULL,
                 kicks REAL,                   -- drums: single-pedal kicks per second
                 tier_2x INTEGER,              -- drums: open-ended tier counting 2x kick
                 PRIMARY KEY (md5, instrument)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS failures (
                 md5 TEXT NOT NULL,
                 at TEXT NOT NULL,
                 error TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute("INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn known(&self, md5: &str) -> bool {
        self.conn.query_row("SELECT 1 FROM charts WHERE md5 = ?1", [md5], |_| Ok(())).optional().ok().flatten().is_some()
    }

    /// Inserts or refreshes listed charts (one transaction per page).
    pub fn upsert_listed(&mut self, charts: &[Chart], now: &str) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO charts (md5, chart_id, name, artist, album, genre, year, charter, song_length,
                         modified_time, has_video, instruments, first_seen, last_seen, removed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, 0)
                     ON CONFLICT(md5) DO UPDATE SET
                         chart_id = excluded.chart_id, name = excluded.name, artist = excluded.artist,
                         album = excluded.album, genre = excluded.genre, year = excluded.year,
                         charter = excluded.charter, song_length = excluded.song_length,
                         modified_time = excluded.modified_time, has_video = excluded.has_video,
                         instruments = excluded.instruments, last_seen = excluded.last_seen, removed = 0",
                )
                .map_err(|e| e.to_string())?;
            for c in charts {
                if c.md5.is_empty() {
                    continue;
                }
                let instruments = c.notes_data.as_ref().map(|n| n.instruments.join(","));
                st.execute(params![
                    c.md5,
                    c.chart_id,
                    c.name,
                    c.artist,
                    c.album,
                    c.genre,
                    c.year,
                    c.charter,
                    c.song_length,
                    c.modified_time,
                    c.has_video_background,
                    instruments,
                    now
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Charts still listed that a complete catalog listing (started at `run_start`) didn't see.
    pub fn mark_removed(&self, run_start: &str) -> Result<usize, String> {
        self.conn
            .execute("UPDATE charts SET removed = 1 WHERE removed = 0 AND last_seen < ?1", [run_start])
            .map_err(|e| e.to_string())
    }

    /// Charts that still need rating, newest first.
    pub fn pending(&self, limit: Option<usize>, retry_failed: bool) -> Result<Vec<Pending>, String> {
        let sql = format!(
            "SELECT md5, has_video FROM charts
             WHERE removed = 0 AND (rated_at IS NULL OR formula_version < {FORMULA_VERSION})
               AND ({} fail_count < {MAX_FAILS})
             ORDER BY modified_time DESC LIMIT ?1",
            if retry_failed { "1 OR" } else { "" }
        );
        let mut st = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let lim = limit.map(|l| l as i64).unwrap_or(-1);
        let rows = st
            .query_map([lim], |r| Ok(Pending { md5: r.get(0)?, has_video: r.get(1)? }))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn save_ratings(&mut self, md5: &str, ratings: &[(String, Rating)], now: &str) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM ratings WHERE md5 = ?1", [md5]).map_err(|e| e.to_string())?;
        for (inst, r) in ratings {
            tx.execute(
                "INSERT INTO ratings (md5, instrument, d, remap, tier, movement, spread, kicks, tier_2x)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![md5, inst, r.d, r.remap, r.tier, r.movement, r.spread, r.kicks, r.tier_2x],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE charts SET rated_at = ?2, formula_version = ?3, fail_count = 0, last_error = NULL WHERE md5 = ?1",
            params![md5, now, FORMULA_VERSION],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Records a failed attempt. `permanent` failures (a broken file) skip straight to MAX_FAILS.
    pub fn save_failure(&self, md5: &str, error: &str, permanent: bool, now: &str) -> Result<(), String> {
        self.conn
            .execute("INSERT INTO failures (md5, at, error) VALUES (?1, ?2, ?3)", params![md5, now, error])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE charts SET fail_count = CASE WHEN ?3 THEN ?4 ELSE fail_count + 1 END, last_error = ?2 WHERE md5 = ?1",
                params![md5, error, permanent, MAX_FAILS],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
    }
}
