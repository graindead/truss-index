//! The SNGPKG (.sng) container format: header parsing and unmasking, with no I/O, so both the
//! scanner and the in-browser graph (WebAssembly) can use it. Copied from Truss (src/sng.rs).
//! Format: https://github.com/mdsitton/SngFileFormat

const HEADER_FIXED: usize = 6 + 4 + 16; // "SNGPKG" + version + xor mask

pub struct FileMeta {
    pub name: String,
    pub len: u64,
    pub index: u64,
}

/// The chart file Fretwork rates: notes.* first, then .mid before .chart (as scan-chart picks).
pub fn pick_chart(files: &[FileMeta]) -> Option<&FileMeta> {
    files
        .iter()
        .filter(|f| {
            let n = f.name.to_ascii_lowercase();
            n.ends_with(".chart") || n.ends_with(".mid")
        })
        .min_by_key(|f| {
            let n = f.name.to_ascii_lowercase();
            (!(n == "notes.chart" || n == "notes.mid"), !n.ends_with(".mid"))
        })
}

/// XOR-unmasks `data`, which starts at byte `offset` within its file.
#[inline]
pub fn unmask(data: &mut [u8], mask: &[u8; 16], offset: u64) {
    // The key repeats every 256 bytes: key[i] = mask[i % 16] ^ (i % 256).
    let mut table = [0u8; 256];
    for (i, k) in table.iter_mut().enumerate() {
        *k = mask[i % 16] ^ i as u8;
    }
    let start = (offset % 256) as usize;
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= table[(start + i) & 0xFF];
    }
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.buf.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn string(&mut self, n: usize) -> Option<String> {
        self.take(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }
}

pub type Header = (Vec<FileMeta>, [u8; 16], usize, Vec<(String, String)>);
/// Returns `Ok(None)` if more bytes are needed.
pub fn try_parse_header(buf: &[u8]) -> Result<Option<Header>, String> {
    if buf.len() < HEADER_FIXED + 8 {
        return Ok(None);
    }
    if &buf[..6] != b"SNGPKG" {
        return Err("Not a valid .sng file".into());
    }
    let mut mask = [0u8; 16];
    mask.copy_from_slice(&buf[10..26]);

    let mut c = Cursor { buf, pos: HEADER_FIXED };
    let Some(meta_len) = c.u64() else { return Ok(None) };
    let meta_start = c.pos;
    let Some(meta_end) = meta_start.checked_add(meta_len as usize) else { return Err("Corrupt header".into()) };
    // Need metadata + fileMetaLen
    if buf.len() < meta_end + 8 {
        return Ok(None);
    }
    let meta_count = c.u64().ok_or("Corrupt header")?;
    let mut metadata = Vec::with_capacity(meta_count.min(256) as usize);
    for _ in 0..meta_count {
        let kl = c.i32().ok_or("Corrupt metadata")?.max(0) as usize;
        let k = c.string(kl).ok_or("Corrupt metadata")?;
        let vl = c.i32().ok_or("Corrupt metadata")?.max(0) as usize;
        let v = c.string(vl).ok_or("Corrupt metadata")?;
        metadata.push((k, v));
    }
    c.pos = meta_end;
    let file_meta_len = c.u64().ok_or("Corrupt header")? as usize;
    let fm_start = c.pos;
    let data_start = fm_start + file_meta_len + 8; // + fileDataLen
    if buf.len() < data_start {
        return Ok(None);
    }
    let count = c.u64().ok_or("Corrupt file list")?;
    let mut files = Vec::with_capacity(count.min(1024) as usize);
    for _ in 0..count {
        let nl = c.u8().ok_or("Corrupt file list")? as usize;
        let name = c.string(nl).ok_or("Corrupt file list")?;
        let len = c.u64().ok_or("Corrupt file list")?;
        let index = c.u64().ok_or("Corrupt file list")?;
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err(format!("Refusing unsafe file name in .sng: {name:?}"));
        }
        if name.eq_ignore_ascii_case("song.ini") {
            continue; // generated from metadata instead
        }
        files.push(FileMeta { name, len, index });
    }
    files.sort_by_key(|f| f.index);
    if let Some(first) = files.first()
        && first.index < data_start as u64
    {
        return Err("Corrupt file offsets".into());
    }
    Ok(Some((files, mask, data_start, metadata)))
}
