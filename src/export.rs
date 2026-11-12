//! Writes the static site's data: `charts.json` (one compact row per rated chart) and
//! `meta.json` (counts and the export time).
//!
//! charts.json: { "instruments": [...], "rows": [[md5, name, artist, album, charter, lengthSec,
//!   modifiedDate, [[inst, d*100, remap, tier, movement*100, spread*100, kicks*100|null, tier2x|null], ...],
//!   genre, year], ...] }
//! `inst` indexes "instruments". Numbers are scaled to integers to keep the file small.

use crate::db::Db;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

pub const INSTRUMENTS: [&str; 6] = ["guitar", "guitarcoop", "rhythm", "bass", "drums", "keys"];

fn x100(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

pub fn export(db: &Db, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let e = |e: rusqlite::Error| e.to_string();

    let mut by_md5: HashMap<String, Vec<Value>> = HashMap::new();
    {
        let mut st = db
            .conn
            .prepare("SELECT md5, instrument, d, remap, tier, movement, spread, kicks, tier_2x FROM ratings")
            .map_err(e)?;
        let mut rows = st.query([]).map_err(e)?;
        while let Some(r) = rows.next().map_err(e)? {
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

    let mut out_rows = Vec::with_capacity(by_md5.len());
    let mut st = db
        .conn
        .prepare(
            "SELECT md5, name, artist, album, charter, song_length, modified_time, genre, year FROM charts
             WHERE removed = 0 AND rated_at IS NOT NULL ORDER BY modified_time DESC",
        )
        .map_err(e)?;
    let mut rows = st.query([]).map_err(e)?;
    while let Some(r) = rows.next().map_err(e)? {
        let md5: String = r.get(0).map_err(e)?;
        let Some(ratings) = by_md5.remove(&md5) else { continue };
        let s = |i: usize| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default();
        let len: Option<i64> = r.get(5).map_err(e)?;
        let modified = s(6);
        out_rows.push(json!([
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
        ]));
    }

    let charts = json!({ "instruments": INSTRUMENTS, "rows": out_rows });
    let path = out.join("charts.json");
    std::fs::write(&path, serde_json::to_string(&charts).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let meta = json!({
        "exported": crate::now(),
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
