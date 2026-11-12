//! Writes the static site's data: `charts.json` (one compact row per rated chart) and
//! `meta.json` (counts and the export time), and the watcher's `recent.json` (charts rated since
//! the database snapshot the site was last built from).
//!
//! charts.json: { "instruments": [...], "rows": [[md5, name, artist, album, charter, lengthSec,
//!   modifiedDate, [[inst, d*100, remap, tier, movement*100, spread*100, kicks*100|null, tier2x|null], ...],
//!   genre, year, hasVideo (0/1: the notes come from Encore's {md5}_novideo.sng)], ...] }
//! `inst` indexes "instruments". Numbers are scaled to integers to keep the file small.
//! recent.json: { "checked": time, "since": time, "rows": [same rows] }

use crate::db::Db;
use rusqlite::types::Value as SqlValue;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

pub const INSTRUMENTS: [&str; 6] = ["guitar", "guitarcoop", "rhythm", "bass", "drums", "keys"];

fn x100(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

/// Site rows for the charts matching `filter` (an SQL condition on `charts c`), newest first.
fn rows(db: &Db, filter: &str, params: &[SqlValue]) -> Result<Vec<Value>, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let p = rusqlite::params_from_iter(params.iter());

    let mut by_md5: HashMap<String, Vec<Value>> = HashMap::new();
    {
        let mut st = db
            .conn
            .prepare(&format!(
                "SELECT r.md5, r.instrument, r.d, r.remap, r.tier, r.movement, r.spread, r.kicks, r.tier_2x
                 FROM ratings r JOIN charts c ON c.md5 = r.md5 WHERE {filter}"
            ))
            .map_err(e)?;
        let mut rs = st.query(p.clone()).map_err(e)?;
        while let Some(r) = rs.next().map_err(e)? {
            let inst: String = r.get(1).map_err(e)?;
            let Some(i) = INSTRUMENTS.iter().position(|x| *x == inst) else { continue };
            let kicks: Option<f64> = r.get(7).map_err(e)?;
            let t2x: Option<i64> = r.get(8).map_err(e)?;
            by_md5.entry(r.get(0).map_err(e)?).or_default().push(json!([
                i,
                x100(r.get(2).map_err(e)?),
                r.get::<_, i64>(3).map_err(e)?,
                r.get::<_, i64>(4).map_err(e)?,
                x100(r.get(5).map_err(e)?),
                x100(r.get(6).map_err(e)?),
                kicks.map(x100),
                t2x,
            ]));
        }
    }
    for v in by_md5.values_mut() {
        v.sort_by_key(|r| r[0].as_u64());
    }

    let mut out = Vec::with_capacity(by_md5.len());
    let mut st = db
        .conn
        .prepare(&format!(
            "SELECT c.md5, c.name, c.artist, c.album, c.charter, c.song_length, c.modified_time, c.genre, c.year, c.has_video
             FROM charts c WHERE {filter} ORDER BY c.modified_time DESC"
        ))
        .map_err(e)?;
    let mut rs = st.query(p).map_err(e)?;
    while let Some(r) = rs.next().map_err(e)? {
        let md5: String = r.get(0).map_err(e)?;
        let Some(ratings) = by_md5.remove(&md5) else { continue };
        let s = |i: usize| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default();
        let len: Option<i64> = r.get(5).map_err(e)?;
        let modified = s(6);
        out.push(json!([
            md5,
            s(1),
            s(2),
            s(3),
            strip_tags(&s(4)),
            len.map(|ms| (ms + 500) / 1000).unwrap_or(0),
            modified.get(..10).unwrap_or(""),
            ratings,
            s(7),
            s(8),
            r.get::<_, i64>(9).map_err(e)?,
        ]));
    }
    Ok(out)
}

pub fn export(db: &Db, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let out_rows = rows(db, "c.removed = 0 AND c.rated_at IS NOT NULL", &[])?;
    let charts = json!({ "instruments": INSTRUMENTS, "rows": out_rows });
    let path = out.join("charts.json");
    std::fs::write(&path, serde_json::to_string(&charts).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let meta = json!({
        "exported": crate::now(),
        // When the database this export came from was saved: the watcher's recent.json holds
        // the charts rated after it.
        "snapshot": db.get_meta("snapshot_at"),
        "charts": out_rows.len(),
        "listed": db.count("SELECT COUNT(*) FROM charts WHERE removed = 0"),
        "rated": db.count("SELECT COUNT(*) FROM charts WHERE removed = 0 AND rated_at IS NOT NULL"),
        "formulas": crate::db::FORMULA_VERSION,
    });
    std::fs::write(out.join("meta.json"), meta.to_string()).map_err(|e| e.to_string())?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("exported {} charts to {} ({:.1} MB)", out_rows.len(), path.display(), size as f64 / 1e6);
    Ok(())
}

/// The charts rated since `since` (the snapshot the live site was built from), for the watcher.
/// Returns whether the rows differ from what `out` already held.
pub fn export_recent(db: &Db, since: &str, out: &Path) -> Result<bool, String> {
    let recent = rows(
        db,
        "c.removed = 0 AND c.rated_at IS NOT NULL AND c.rated_at >= ?1",
        &[SqlValue::Text(since.to_string())],
    )?;
    let old_rows = std::fs::read_to_string(out)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("rows").cloned());
    let changed = old_rows.as_ref() != Some(&Value::Array(recent.clone()));
    if changed {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let doc = json!({ "checked": crate::now(), "since": since, "rows": recent });
        std::fs::write(out, doc.to_string()).map_err(|e| e.to_string())?;
    }
    println!("recent: {} charts rated since {since}{}", recent.len(), if changed { " (changed)" } else { "" });
    Ok(changed)
}

/// Charter names often carry Clone Hero colour tags (`<color=#FF0000>Name</color>`).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}
