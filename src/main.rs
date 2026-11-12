//! truss-index: the Truss Index scanner. Lists the Chorus Encore catalog, rates each chart's
//! Expert tracks with Fretwork's maths (by Staycation, MIT), stores the results in SQLite and
//! exports them for the static site.
//!
//!   truss-index scan   [--db PATH] [--limit N] [--jobs N] [--full-list] [--list-pages N]
//!                        [--no-list] [--retry-failed]
//!                        [--overlap-hours N]
//!   truss-index export [--db PATH] [--out DIR]
//!   truss-index export-recent --since TIME [--db PATH] [--out FILE]
//!   truss-index stats  [--db PATH]

#[allow(dead_code)] // copied whole from Truss; the scanner only needs the notes
mod chart;
mod db;
mod encore;
mod export;
mod fretwork;
mod ini;
mod sng;
mod sngpkg;

use db::Db;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const USAGE: &str = "usage:
  truss-index scan   [--db PATH] [--limit N] [--jobs N] [--full-list] [--list-pages N] [--no-list] [--retry-failed]
                     [--overlap-hours N]
  truss-index export [--db PATH] [--out DIR]
  truss-index export-recent --since TIME [--db PATH] [--out FILE]   (charts rated since TIME, for the watcher)
  truss-index stats  [--db PATH]";

/// Encore's newest-first order can lag a little behind uploads, so an incremental listing keeps
/// going this far past the last listing's newest chart (default; the watcher uses less).
const OVERLAP_HOURS: i64 = 72;
/// Give up the run after this many failures in a row (Encore is probably down).
const MAX_CONSECUTIVE_FAILS: usize = 40;

struct Args {
    cmd: String,
    db: PathBuf,
    out: PathBuf,
    limit: Option<usize>,
    jobs: usize,
    full_list: bool,
    list_pages: Option<u32>,
    no_list: bool,
    retry_failed: bool,
    overlap_hours: i64,
    since: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(USAGE)?;
    let mut a = Args {
        cmd,
        db: PathBuf::from("data/fretwork.db"),
        out: PathBuf::from("site/data"),
        limit: None,
        jobs: 4,
        full_list: false,
        list_pages: None,
        no_list: false,
        retry_failed: false,
        overlap_hours: OVERLAP_HOURS,
        since: None,
    };
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--db" => a.db = val()?.into(),
            "--out" => a.out = val()?.into(),
            "--limit" => a.limit = Some(val()?.parse().map_err(|_| "--limit: not a number")?),
            "--jobs" => a.jobs = val()?.parse::<usize>().map_err(|_| "--jobs: not a number")?.clamp(1, 32),
            "--list-pages" => a.list_pages = Some(val()?.parse().map_err(|_| "--list-pages: not a number")?),
            "--full-list" => a.full_list = true,
            "--no-list" => a.no_list = true,
            "--retry-failed" => a.retry_failed = true,
            "--overlap-hours" => a.overlap_hours = val()?.parse::<i64>().map_err(|_| "--overlap-hours: not a number")?.max(0),
            "--since" => a.since = Some(val()?),
            _ => return Err(format!("unknown option {flag}\n{USAGE}")),
        }
    }
    Ok(a)
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("truss-index-scanner/", env!("CARGO_PKG_VERSION"), " (+https://github.com/graindead/truss-index)"))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(8)
        .build()
        .expect("failed to build HTTP client")
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let result = match args.cmd.as_str() {
        "scan" => scan(&args).await,
        "export" => Db::open(&args.db).and_then(|db| export::export(&db, &args.out)),
        "export-recent" => match &args.since {
            None => Err("export-recent needs --since TIME".to_string()),
            Some(since) => {
                // --out defaults to the site data folder; here it names a file.
                let out = if args.out.extension().is_some() { args.out.clone() } else { PathBuf::from("live/recent.json") };
                Db::open(&args.db).and_then(|db| export::export_recent(&db, since, &out)).map(|_| ())
            }
        },
        "stats" => Db::open(&args.db).map(|db| stats(&db)),
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn stats(db: &Db) {
    println!("charts listed:   {}", db.count("SELECT COUNT(*) FROM charts WHERE removed = 0"));
    println!("charts rated:    {}", db.count("SELECT COUNT(*) FROM charts WHERE removed = 0 AND rated_at IS NOT NULL"));
    println!("with ratings:    {}", db.count("SELECT COUNT(DISTINCT md5) FROM ratings"));
    println!("failed (gave up): {}", db.count(&format!("SELECT COUNT(*) FROM charts WHERE fail_count >= {}", db::MAX_FAILS)));
    println!("removed from Encore: {}", db.count("SELECT COUNT(*) FROM charts WHERE removed = 1"));
    println!("last listing watermark: {}", db.get_meta("list_watermark").unwrap_or_else(|| "-".into()));
}

async fn scan(args: &Args) -> Result<(), String> {
    let mut db = Db::open(&args.db)?;
    let client = client();
    if !args.no_list {
        list_catalog(&mut db, &client, args).await?;
    }
    rate_pending(&mut db, &client, args).await?;
    stats(&db);
    Ok(())
}

/// Pages through the catalog, newest first, recording every chart. Incremental unless
/// `--full-list` or there's no previous complete listing: stops once a page has nothing new and
/// nothing modified since the last listing.
async fn list_catalog(db: &mut Db, client: &reqwest::Client, args: &Args) -> Result<(), String> {
    let run_start = now();
    let watermark = if args.full_list { None } else { db.get_meta("list_watermark") };
    let cutoff = watermark.as_deref().and_then(|w| chrono::DateTime::parse_from_rfc3339(w).ok()).map(|w| {
        (w - chrono::Duration::hours(args.overlap_hours)).to_utc().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    });
    match &cutoff {
        Some(c) => println!("listing Encore charts modified since {c}"),
        None => println!("listing the whole Encore catalog"),
    }
    let mut newest: Option<String> = None;
    let mut seen = 0u64;
    let mut found = 0u64;
    let mut complete = false;
    let mut page = 1u32;
    loop {
        if args.list_pages.is_some_and(|max| page > max) {
            println!("stopped listing after {} pages (--list-pages)", page - 1);
            break;
        }
        let p = with_retries(|| encore::page(client, page)).await?;
        found = p.found;
        if p.data.is_empty() {
            complete = true;
            break;
        }
        if newest.is_none() {
            newest = p.data.iter().filter_map(|c| c.modified_time.clone()).max();
        }
        let fresh = p.data.iter().any(|c| {
            !db.known(&c.md5) || match (&cutoff, &c.modified_time) {
                (Some(cut), Some(m)) => m >= cut,
                _ => true,
            }
        });
        seen += p.data.len() as u64;
        db.upsert_listed(&p.data, &now())?;
        if page % 20 == 1 {
            println!("  page {page}: {seen} charts so far (Encore says {found})");
        }
        if cutoff.is_some() && !fresh {
            complete = true;
            println!("  page {page} had nothing new; incremental listing done");
            break;
        }
        page += 1;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    println!("listed {seen} charts");
    if complete {
        if let Some(n) = newest {
            db.set_meta("list_watermark", &n)?;
        }
        // Only a full pass that saw (nearly) everything can say what was removed.
        if cutoff.is_none() && seen as f64 >= found as f64 * 0.98 {
            let gone = db.mark_removed(&run_start)?;
            if gone > 0 {
                println!("{gone} charts are no longer on Encore");
            }
            db.set_meta("last_full_list", &run_start)?;
        }
    }
    Ok(())
}

/// Retries a catalog request with growing pauses.
async fn with_retries<T, F, Fut>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut wait = 2;
    for attempt in 1.. {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < 5 && !e.starts_with("FATAL ") => {
                eprintln!("  {e}; retrying in {wait}s");
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait *= 3;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

enum Outcome {
    Rated(Vec<(String, fretwork::Rating)>),
    Failed { error: String, permanent: bool },
}

async fn rate_pending(db: &mut Db, client: &reqwest::Client, args: &Args) -> Result<(), String> {
    let pending = db.pending(args.limit, args.retry_failed)?;
    let total = pending.len();
    if total == 0 {
        println!("nothing to rate");
        return Ok(());
    }
    println!("rating {total} charts, {} at a time", args.jobs);
    // When any request is told to slow down, every worker waits until this moment.
    let pause_until = Arc::new(Mutex::new(Instant::now()));
    let started = Instant::now();
    let mut results = futures::stream::iter(pending)
        .map(|p| {
            let client = client.clone();
            let pause = pause_until.clone();
            async move {
                let out = rate_one(&client, &p.md5, p.has_video, &pause).await;
                (p.md5, out)
            }
        })
        .buffer_unordered(args.jobs);
    let (mut done, mut ok, mut failed, mut streak) = (0usize, 0usize, 0usize, 0usize);
    while let Some((md5, outcome)) = results.next().await {
        done += 1;
        match outcome {
            Outcome::Rated(r) => {
                db.save_ratings(&md5, &r, &now())?;
                ok += 1;
                streak = 0;
            }
            Outcome::Failed { error, permanent } => {
                eprintln!("  {md5}: {error}");
                db.save_failure(&md5, &error, permanent, &now())?;
                failed += 1;
                if !permanent {
                    streak += 1;
                }
                if streak >= MAX_CONSECUTIVE_FAILS {
                    return Err(format!("{streak} failures in a row; stopping (progress is saved, run again later)"));
                }
            }
        }
        if done % 50 == 0 || done == total {
            let rate = done as f64 / started.elapsed().as_secs_f64().max(0.001);
            println!("  {done}/{total} ({ok} rated, {failed} failed, {rate:.1}/s)");
        }
    }
    Ok(())
}

async fn rate_one(client: &reqwest::Client, md5: &str, has_video: bool, pause: &Mutex<Instant>) -> Outcome {
    let url = encore::Chart::sng_url(md5, has_video);
    let mut wait = Duration::from_secs(2);
    let mut attempt = 0;
    let (meta, files) = loop {
        let until = *pause.lock().await;
        if until > Instant::now() {
            tokio::time::sleep_until(until.into()).await;
        }
        attempt += 1;
        match sng::fetch_remote_notes(client, &url).await {
            Ok(v) => break v,
            Err(e) if e.retry && attempt < 4 => {
                // Back off everyone, not just this worker.
                let mut p = pause.lock().await;
                *p = (*p).max(Instant::now() + wait);
                drop(p);
                wait *= 3;
            }
            Err(e) => return Outcome::Failed { error: e.msg, permanent: !e.retry },
        }
    };
    match tokio::task::spawn_blocking(move || rate_files(&meta, &files)).await {
        Ok(Ok(r)) => Outcome::Rated(r),
        Ok(Err(e)) => Outcome::Failed { error: e, permanent: true },
        Err(e) => Outcome::Failed { error: format!("rating crashed: {e}"), permanent: true },
    }
}

/// Picks the chart file the way scan-chart does (notes.* first, .mid before .chart), parses it
/// with the .sng's song.ini values, and rates every Expert track Fretwork knows.
fn rate_files(meta: &[(String, String)], files: &[(String, Vec<u8>)]) -> Result<Vec<(String, fretwork::Rating)>, String> {
    let lower = |n: &str| n.rsplit(['/', '\\']).next().unwrap_or(n).to_ascii_lowercase();
    let mut charts: Vec<&(String, Vec<u8>)> = files.iter().collect();
    charts.sort_by_key(|(n, _)| {
        let n = lower(n);
        (!(n == "notes.chart" || n == "notes.mid"), !n.ends_with(".mid"))
    });
    let (name, data) = charts.first().ok_or("no notes.chart or notes.mid")?;
    let mods = ini::modifiers_from_ini(meta);
    let parsed = chart::parse_chart_file(data, lower(name).ends_with(".mid"), &mods)?;
    Ok(parsed
        .tracks
        .iter()
        .filter(|t| t.difficulty == "expert")
        .filter_map(|t| Some((t.instrument.to_string(), fretwork::rate(t.instrument, t)?)))
        .collect())
}
