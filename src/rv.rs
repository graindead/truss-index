//! RhythmVerse charts for the site, from the truss-rv-index cacher's state database (Fretwork
//! ratings computed by the same port as this scanner), plus album/genre/year from Truss's own
//! RhythmVerse cache when available. Both are opened read-only.
//!
//! Rows use the site's row format with the id prefixed `rv:` (RhythmVerse ids are 32 hex
//! characters, like Encore md5s) and the RhythmVerse page URL appended.

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

fn open_ro(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Default)]
struct Extra {
    album: String,
    genre: String,
    year: String,
    length_ms: i64,
}

/// (sort timestamp, site row) for every RhythmVerse chart with at least one rating.
pub fn rows(state: &Path, truss_cache: Option<&Path>) -> Result<Vec<(String, Value)>, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let mut extra: HashMap<String, Extra> = HashMap::new();
    if let Some(p) = truss_cache {
        let c = open_ro(p)?;
        let mut st = c.prepare("SELECT uid, album, genre, year, length_ms FROM charts").map_err(e)?;
        let mut rs = st.query([]).map_err(e)?;
        while let Some(r) = rs.next().map_err(e)? {
            let s = |i: usize| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default();
            extra.insert(
                s(0),
                Extra { album: s(1), genre: s(2), year: s(3), length_ms: r.get::<_, Option<i64>>(4).ok().flatten().unwrap_or(0) },
            );
        }
    }

    let c = open_ro(state)?;
    let mut st = c
        .prepare(
            "SELECT c.file_id, c.modified, c.title, c.artist, c.charter, c.page_url, c.data, q.length_ms
             FROM charts c LEFT JOIN queue q ON q.file_id = c.file_id
             WHERE c.status = 'ok' AND c.data IS NOT NULL",
        )
        .map_err(e)?;
    let mut rs = st.query([]).map_err(e)?;
    let mut out = Vec::new();
    while let Some(r) = rs.next().map_err(e)? {
        let s = |i: usize| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default();
        let id = s(0);
        let Ok(data) = serde_json::from_str::<Value>(&s(6)) else { continue };
        let Some(w) = data.get("w").and_then(|w| w.as_object()) else { continue };
        let mut ratings: Vec<Value> = Vec::new();
        for (inst, v) in w {
            let Some(i) = crate::export::INSTRUMENTS.iter().position(|x| x == inst) else { continue };
            let f = |k: &str| v.get(k).and_then(|x| x.as_f64());
            let (Some(d), Some(remap), Some(tier)) = (f("d"), f("r"), f("t")) else { continue };
            ratings.push(json!([
                i,
                crate::export::x100(d),
                remap as i64,
                tier as i64,
                crate::export::x100(f("mv").unwrap_or(0.0)),
                crate::export::x100(f("sp").unwrap_or(0.0)),
                f("k").map(crate::export::x100),
                f("t2").map(|t| t as i64),
            ]));
        }
        if ratings.is_empty() {
            continue;
        }
        ratings.sort_by_key(|r| r[0].as_u64());
        let ex = extra.remove(&id).unwrap_or_default();
        let len_ms = r.get::<_, Option<i64>>(7).ok().flatten().filter(|&m| m > 0).unwrap_or(ex.length_ms);
        // "YYYY-MM-DD HH:MM:SS" -> ISO, so it sorts alongside Encore's modifiedTime.
        let modified = s(1).replacen(' ', "T", 1);
        out.push((
            modified.clone(),
            json!([
                format!("rv:{id}"),
                s(2),
                s(3),
                ex.album,
                crate::export::strip_tags(&s(4)),
                (len_ms + 500) / 1000,
                modified.get(..10).unwrap_or(""),
                ratings,
                ex.genre,
                ex.year,
                0,
                s(5),
            ]),
        ));
    }
    Ok(out)
}
