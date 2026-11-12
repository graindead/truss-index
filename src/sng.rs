//! Reads the notes file out of a remote .sng (SNGPKG) by HTTP byte range, without downloading
//! the audio. Copied from Truss (src/sng.rs), trimmed to what the scanner needs.
//! Format: https://github.com/mdsitton/SngFileFormat

const HEADER_FIXED: usize = 6 + 4 + 16; // "SNGPKG" + version + xor mask

struct FileMeta {
    name: String,
    len: u64,
    index: u64,
}

/// Why a fetch failed: `retry` is true for errors worth trying again (network, 429, 5xx).
#[derive(Debug)]
pub struct FetchError {
    pub msg: String,
    pub retry: bool,
}

impl FetchError {
    fn fatal(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), retry: false }
    }
}

/// Byte range `start..end` of a remote file. A server that ignores ranges sends the whole file,
/// which is sliced here.
async fn get_range(client: &reqwest::Client, url: &str, start: u64, end: u64) -> Result<Vec<u8>, FetchError> {
    let resp = client
        .get(url)
        .header("Range", format!("bytes={start}-{}", end.saturating_sub(1)))
        .send()
        .await
        .map_err(|e| FetchError { msg: format!("request failed: {e}"), retry: true })?;
    let status = resp.status();
    if !status.is_success() {
        let retry = status.as_u16() == 429 || status.is_server_error();
        return Err(FetchError { msg: format!("HTTP {status}"), retry });
    }
    let whole = status == reqwest::StatusCode::OK;
    let bytes = resp.bytes().await.map_err(|e| FetchError { msg: format!("read failed: {e}"), retry: true })?;
    Ok(if whole { bytes.get(start as usize..(end as usize).min(bytes.len())).unwrap_or_default().to_vec() } else { bytes.to_vec() })
}

/// The metadata and chart files (notes.chart / notes.mid) of a remote .sng, fetched by byte
/// range: only the header and the chart itself come down, not the audio.
pub async fn fetch_remote_notes(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Vec<(String, String)>, Vec<(String, Vec<u8>)>), FetchError> {
    let mut want = 64 * 1024;
    let (head, files, mask, metadata) = loop {
        let head = get_range(client, url, 0, want).await?;
        if let Some((files, mask, _, metadata)) = try_parse_header(&head).map_err(FetchError::fatal)? {
            break (head, files, mask, metadata);
        }
        if (head.len() as u64) < want || want >= 16 << 20 {
            return Err(FetchError::fatal("Incomplete .sng file"));
        }
        want *= 4;
    };
    // Only the chart file that will be rated: notes.* first, then .mid before .chart (as scan-chart picks).
    let pick = files
        .iter()
        .filter(|f| {
            let n = f.name.to_ascii_lowercase();
            n.ends_with(".chart") || n.ends_with(".mid")
        })
        .min_by_key(|f| {
            let n = f.name.to_ascii_lowercase();
            (!(n == "notes.chart" || n == "notes.mid"), !n.ends_with(".mid"))
        });
    let mut out = Vec::new();
    if let Some(f) = pick {
        let end = f.index + f.len;
        // Often the notes sit right after the header, inside the bytes already fetched.
        let mut bytes = if end <= head.len() as u64 {
            head[f.index as usize..end as usize].to_vec()
        } else {
            get_range(client, url, f.index, end).await?
        };
        if bytes.len() as u64 != f.len {
            return Err(FetchError { msg: "Truncated .sng file".into(), retry: true });
        }
        unmask(&mut bytes, &mask, 0);
        out.push((f.name.clone(), bytes));
    }
    Ok((metadata, out))
}

/// XOR-unmasks `data`, which starts at byte `offset` within its file.
#[inline]
fn unmask(data: &mut [u8], mask: &[u8; 16], offset: u64) {
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

type Header = (Vec<FileMeta>, [u8; 16], usize, Vec<(String, String)>);
/// Returns `Ok(None)` if more bytes are needed.
fn try_parse_header(buf: &[u8]) -> Result<Option<Header>, String> {
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
