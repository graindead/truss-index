//! Reads the notes file out of a remote .sng (SNGPKG) by HTTP byte range, without downloading
//! the audio. Copied from Truss (src/sng.rs), trimmed to what the scanner needs.
//! Format: https://github.com/mdsitton/SngFileFormat

use crate::sngpkg::{pick_chart, try_parse_header, unmask};

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
    let pick = pick_chart(&files);
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
