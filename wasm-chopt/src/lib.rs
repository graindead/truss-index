//! CHOpt in the browser: the Truss port of CHOpt (src/chopt, GPL-3.0, by Raymond Wright) as
//! WebAssembly, driven from site/chopt-worker.js. No wasm-bindgen: plain exports over linear
//! memory, with the loaded song and the last path kept here between calls.
//!
//!   alloc(len) -> ptr, dealloc(ptr, len), out_ptr(), txt_ptr(), txt_len()  (not "free": that would
//!   clash with the C library's free when the tests build natively on Linux)
//!   header(ptr, len) -> n          JSON: {"need":true} | {"error"} | {"index","len","mid"}  (.sng header)
//!   load_sng(hptr, hlen, cptr, clen) -> n    a .sng header + its (masked) notes file
//!   load_files(cptr, clen, is_mid, iptr, ilen) -> n   a notes.chart/.mid and song.ini (may be empty)
//!       both answer JSON {"name","artist","charter","tracks":[[instrument, difficulty], ...]} | {"error"}
//!   run(optsptr, optslen) -> n     JSON options in, {"summary","width","height","row"} | {"error"} out
//!   render(y0, y1) -> n            RGBA pixels of rows [y0, y1) of the path image at out_ptr();
//!                                  the strip's width is 1024; its text, as JSON, at txt_ptr()/txt_len()

#[allow(dead_code)]
#[path = "../../src/chopt/mod.rs"]
mod chopt;
#[path = "../../src/sngpkg.rs"]
mod sngpkg;

use chopt::{Difficulty, Instrument, Options, PathImage, Song};
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static TXT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static SONG: Mutex<Option<Song>> = Mutex::new(None);
static IMAGE: Mutex<Option<PathImage>> = Mutex::new(None);

fn set_out(b: Vec<u8>) -> usize {
    let mut o = OUT.lock().unwrap();
    *o = b;
    o.len()
}

fn set_json(v: serde_json::Value) -> usize {
    set_out(v.to_string().into_bytes())
}

fn error(e: impl std::fmt::Display) -> usize {
    set_json(json!({ "error": e.to_string() }))
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.lock().unwrap().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn txt_ptr() -> *const u8 {
    TXT.lock().unwrap().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn txt_len() -> usize {
    TXT.lock().unwrap().len()
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
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(ptr, 0, len.max(1))) };
}

unsafe fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

fn instrument_key(i: Instrument) -> &'static str {
    match i {
        Instrument::Guitar => "guitar",
        Instrument::GuitarCoop => "guitarcoop",
        Instrument::Bass => "bass",
        Instrument::Rhythm => "rhythm",
        Instrument::Keys => "keys",
    }
}

fn difficulty_key(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
    }
}

fn store(song: Result<Song, String>) -> usize {
    match song {
        Err(e) => error(e),
        Ok(song) => {
            let tracks: Vec<_> = song.tracks.keys().map(|(i, d)| json!([instrument_key(*i), difficulty_key(*d)])).collect();
            let info = json!({
                "name": chopt::strip_tags(&song.name),
                "artist": chopt::strip_tags(&song.artist),
                "charter": chopt::strip_tags(&song.charter),
                "tracks": tracks,
            });
            *SONG.lock().unwrap() = Some(song);
            *IMAGE.lock().unwrap() = None;
            set_json(info)
        }
    }
}

/// Where the notes file is in a .sng, or whether more of the header is needed.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn header(ptr: *const u8, len: usize) -> usize {
    let head = unsafe { slice(ptr, len) };
    match sngpkg::try_parse_header(head) {
        Err(e) => error(e),
        Ok(None) => set_json(json!({ "need": true })),
        Ok(Some((files, _, _, _))) => match sngpkg::pick_chart(&files) {
            None => error("This chart has no notes.chart or notes.mid."),
            Some(f) => set_json(json!({
                "index": f.index,
                "len": f.len,
                "mid": f.name.to_ascii_lowercase().ends_with(".mid"),
            })),
        },
    }
}

/// Loads a chart from a .sng: `head` holds the header (for the mask and song.ini values),
/// `notes` the still-masked notes file. A whole .sng can be passed as both.
///
/// # Safety
/// Both pointer/length pairs must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn load_sng(hptr: *const u8, hlen: usize, cptr: *const u8, clen: usize) -> usize {
    let head = unsafe { slice(hptr, hlen) };
    let notes = unsafe { slice(cptr, clen) };
    store((|| {
        let (files, mask, _, metadata) = sngpkg::try_parse_header(head)?.ok_or("Incomplete .sng file")?;
        let f = sngpkg::pick_chart(&files).ok_or("This chart has no notes.chart or notes.mid.")?;
        // A whole .sng: take the notes out of it.
        let notes = if notes.len() as u64 != f.len && notes.len() as u64 >= f.index + f.len {
            &notes[f.index as usize..(f.index + f.len) as usize]
        } else {
            notes
        };
        let mut data = notes.to_vec();
        sngpkg::unmask(&mut data, &mask, 0);
        let mut ini = String::from("[song]\n");
        for (k, v) in &metadata {
            ini.push_str(&format!("{k} = {v}\n"));
        }
        chopt::load(&data, f.name.to_ascii_lowercase().ends_with(".mid"), ini.as_bytes())
    })())
}

/// Loads a notes.chart or notes.mid, with its song.ini (empty if there isn't one).
///
/// # Safety
/// Both pointer/length pairs must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn load_files(cptr: *const u8, clen: usize, is_mid: u32, iptr: *const u8, ilen: usize) -> usize {
    let notes = unsafe { slice(cptr, clen) };
    let ini = unsafe { slice(iptr, ilen) };
    store(chopt::load(notes, is_mid != 0, ini))
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct RunOptions {
    instrument: String,
    difficulty: String,
    precision: bool,
    squeeze: u32,
    early_whammy: u32,
    lazy_whammy_ms: u32,
    whammy_delay_ms: u32,
    video_lag_ms: i32,
    speed: u32,
    lefty: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        let d = Options::default();
        RunOptions {
            instrument: "guitar".into(),
            difficulty: "expert".into(),
            precision: d.precision,
            squeeze: d.squeeze,
            early_whammy: d.early_whammy,
            lazy_whammy_ms: d.lazy_whammy_ms,
            whammy_delay_ms: d.whammy_delay_ms,
            video_lag_ms: d.video_lag_ms,
            speed: d.speed,
            lefty: d.lefty,
        }
    }
}

/// Finds the best path for the loaded song.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn run(ptr: *const u8, len: usize) -> usize {
    let raw = unsafe { slice(ptr, len) };
    let o: RunOptions = match serde_json::from_slice(raw) {
        Ok(o) => o,
        Err(e) => return error(format!("Bad options: {e}")),
    };
    let instrument = match o.instrument.as_str() {
        "guitar" => Instrument::Guitar,
        "guitarcoop" => Instrument::GuitarCoop,
        "bass" => Instrument::Bass,
        "rhythm" => Instrument::Rhythm,
        "keys" => Instrument::Keys,
        other => return error(format!("CHOpt can't path {other}.")),
    };
    let difficulty = match o.difficulty.as_str() {
        "easy" => Difficulty::Easy,
        "medium" => Difficulty::Medium,
        "hard" => Difficulty::Hard,
        _ => Difficulty::Expert,
    };
    let opts = Options {
        instrument,
        difficulty,
        precision: o.precision,
        squeeze: o.squeeze,
        early_whammy: o.early_whammy,
        lazy_whammy_ms: o.lazy_whammy_ms,
        whammy_delay_ms: o.whammy_delay_ms,
        video_lag_ms: o.video_lag_ms,
        speed: o.speed,
        lefty: o.lefty,
    };
    let Some(song) = SONG.lock().unwrap().clone() else { return error("No chart is loaded.") };
    // Cancelling terminates the worker, so this never needs setting.
    let cancel = AtomicBool::new(false);
    match chopt::run(song, &opts, &cancel) {
        Err(e) => error(e),
        Ok(out) => {
            let height = chopt::draw::image_height(&out.image);
            *IMAGE.lock().unwrap() = Some(out.image);
            set_json(json!({
                "summary": out.summary,
                "width": chopt::draw::IMAGE_WIDTH,
                "height": height,
                "row": chopt::draw::ROW_HEIGHT,
            }))
        }
    }
}

/// Draws rows [y0, y1) of the last path image.
#[unsafe(no_mangle)]
pub extern "C" fn render(y0: u32, y1: u32) -> usize {
    let img = IMAGE.lock().unwrap();
    let Some(img) = img.as_ref() else { return 0 };
    let Some((_, _, pixels, texts)) = chopt::draw::render(img, y0, y1) else { return 0 };
    let t: Vec<_> = texts
        .into_iter()
        .map(|t| json!([t.x, t.y, t.text, format!("rgb({},{},{})", t.colour[0], t.colour[1], t.colour[2]), t.height, t.right]))
        .collect();
    *TXT.lock().unwrap() = serde_json::to_vec(&t).unwrap_or_default();
    set_out(pixels)
}

#[cfg(test)]
mod tests {
    /// TRUSS_TEST_SNG=<file.sng>: load and path it natively, printing the summary (to compare
    /// with the browser build).
    #[test]
    fn path_sng_file() {
        let Ok(p) = std::env::var("TRUSS_TEST_SNG") else { return };
        let data = std::fs::read(p).unwrap();
        let n = unsafe { super::load_sng(data.as_ptr(), data.len(), data.as_ptr(), data.len()) };
        println!("{}", String::from_utf8_lossy(&super::OUT.lock().unwrap()[..n]));
        let opts = br#"{"instrument":"guitar","difficulty":"expert"}"#;
        let n = unsafe { super::run(opts.as_ptr(), opts.len()) };
        println!("{}", String::from_utf8_lossy(&super::OUT.lock().unwrap()[..n]));
    }
}
