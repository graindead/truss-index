//! The Chorus Encore catalog (api.enchor.us). An empty search lists every chart, newest
//! `modifiedTime` first.

use serde::Deserialize;

pub const API_URL: &str = "https://api.enchor.us";
pub const FILES_URL: &str = "https://files.enchor.us";
/// The API serves up to about 250 charts a page.
pub const PER_PAGE: u32 = 250;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NotesData {
    pub instruments: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Chart {
    pub md5: String,
    #[serde(rename = "chartId")]
    pub chart_id: Option<i64>,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub charter: Option<String>,
    /// Milliseconds.
    pub song_length: Option<i64>,
    #[serde(rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(rename = "hasVideoBackground")]
    pub has_video_background: bool,
    #[serde(rename = "notesData")]
    pub notes_data: Option<NotesData>,
}

impl Chart {
    /// Where the chart's .sng lives. Charts with a video background have a smaller copy without
    /// it, which has the same notes.
    pub fn sng_url(md5: &str, has_video: bool) -> String {
        format!("{FILES_URL}/{md5}{}.sng", if has_video { "_novideo" } else { "" })
    }
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub found: u64,
    pub data: Vec<Chart>,
}

/// One page (1-based) of the whole catalog.
pub async fn page(client: &reqwest::Client, page: u32) -> Result<Page, String> {
    let body = serde_json::json!({
        "search": "",
        "per_page": PER_PAGE,
        "page": page,
        "instrument": null,
        "difficulty": null,
        "drumType": null,
        "drumsReviewed": false,
        "sort": null,
        "source": "api",
    });
    let resp = client
        .post(format!("{API_URL}/search"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("search request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        // Retrying won't fix a rejected request; only 429 and server errors are worth another go.
        let retry = status.as_u16() == 429 || status.is_server_error();
        return Err(format!("{}search page {page}: HTTP {status}", if retry { "" } else { "FATAL " }));
    }
    resp.json::<Page>().await.map_err(|e| format!("search page {page}: bad response: {e}"))
}
