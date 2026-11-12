//! WebAssembly entry points for the site's difficulty graphs. The browser fetches a chart's
//! .sng header and notes from Encore by byte range; this module finds the notes file in the
//! header, parses the notes with the scanner's own parser, and returns Fretwork's curves as JSON.
//!
//! No wasm-bindgen: plain exports over linear memory.
//!   alloc(len) -> ptr, free(ptr, len)
//!   header(ptr, len) -> out_len    JSON: {"need":true} | {"error":..} | {"index":..,"len":..,"mid":..}
//!   curves(hptr, hlen, cptr, clen) -> out_len    JSON: {"step":250,"tracks":{"guitar":{"d":[..],..}}} | {"error":..}
//!   out_ptr() -> ptr of the last JSON result

#[allow(dead_code)]
#[path = "../../src/chart.rs"]
mod chart;
#[path = "../../src/curves.rs"]
mod curves;
#[allow(dead_code)]
#[path = "../../src/fretwork.rs"]
mod fretwork;
#[path = "../../src/ini.rs"]
mod ini;
#[path = "../../src/sngpkg.rs"]
mod sngpkg;

use std::fmt::Write;
use std::sync::Mutex;

static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn set_out(s: String) -> usize {
    let mut o = OUT.lock().unwrap();
    *o = s.into_bytes();
    o.len()
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.lock().unwrap().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` must come from `alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(ptr, 0, len.max(1))) };
}

unsafe fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Where the notes file is, or whether more of the header is needed.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn header(ptr: *const u8, len: usize) -> usize {
    let head = unsafe { slice(ptr, len) };
    set_out(match sngpkg::try_parse_header(head) {
        Err(e) => format!("{{\"error\":{}}}", json_str(&e)),
        Ok(None) => "{\"need\":true}".into(),
        Ok(Some((files, _, _, _))) => match sngpkg::pick_chart(&files) {
            None => "{\"error\":\"This chart has no notes.chart or notes.mid.\"}".into(),
            Some(f) => format!(
                "{{\"index\":{},\"len\":{},\"mid\":{}}}",
                f.index,
                f.len,
                f.name.to_ascii_lowercase().ends_with(".mid")
            ),
        },
    })
}

/// Fretwork's curves for every Expert track Fretwork rates. `head` is the .sng header (for the
/// mask and song.ini values); `chart` is the still-masked notes file.
///
/// # Safety
/// Both pointer/length pairs must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn curves(hptr: *const u8, hlen: usize, cptr: *const u8, clen: usize) -> usize {
    let head = unsafe { slice(hptr, hlen) };
    let chart_bytes = unsafe { slice(cptr, clen) };
    set_out(match build(head, chart_bytes) {
        Ok(s) => s,
        Err(e) => format!("{{\"error\":{}}}", json_str(&e)),
    })
}

fn build(head: &[u8], chart_bytes: &[u8]) -> Result<String, String> {
    let (files, mask, _, metadata) = sngpkg::try_parse_header(head)?.ok_or("Incomplete .sng header")?;
    let f = sngpkg::pick_chart(&files).ok_or("This chart has no notes.chart or notes.mid.")?;
    let mut data = chart_bytes.to_vec();
    sngpkg::unmask(&mut data, &mask, 0);
    let mods = ini::modifiers_from_ini(&metadata);
    let parsed = chart::parse_chart_file(&data, f.name.to_ascii_lowercase().ends_with(".mid"), &mods)?;
    let mut out = format!("{{\"step\":{},\"tracks\":{{", fretwork::STEP_MS);
    let mut first = true;
    for t in parsed.tracks.iter().filter(|t| t.difficulty == "expert") {
        let Some(c) = curves::curves(t.instrument, t) else { continue };
        if out.contains(&format!("\"{}\":", t.instrument)) {
            continue; // a duplicate track: the first one counts, as in the ratings
        }
        if !first {
            out.push(',');
        }
        first = false;
        let _ = write!(out, "\"{}\":{{", t.instrument);
        for (i, (name, v)) in c.series.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{name}\":[");
            for (j, x) in v.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                // Two decimals is plenty for a graph and keeps the JSON small.
                let r = (x * 100.0).round() / 100.0;
                let _ = write!(out, "{r}");
            }
            out.push(']');
        }
        out.push('}');
    }
    out.push_str("}}");
    Ok(out)
}
