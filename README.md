# Truss Index

A static website, part of the [Truss](https://github.com/graindead/truss) family, listing every
[Chorus Encore](https://www.enchor.us) chart with its
[Fretwork](https://github.com/Staycation44/fretwork) difficulty rating. Search, sort, filter by
instrument and tier, and jump to the chart on enchor.us.

It has two parts:

- **`truss-index`** (Rust, in `src/`) pages through the Encore catalog. For each chart it
  downloads only the notes file out of the `.sng` using HTTP byte ranges, not the audio. It rates
  every Expert track with Fretwork's maths and stores the results in SQLite (`data/fretwork.db`).
- **`site/`** is a plain HTML/CSS/JS page. It loads `site/data/charts.json`, which
  `truss-index export` writes, and does all searching in the browser. No build step and no
  server, so it runs on GitHub Pages.

- **`wasm/`** builds the difficulty graphs. Clicking an instrument in a chart's details fetches
  that chart's header and notes from Encore in the browser, by byte range like the scanner (about
  100 KB). `curves.wasm` then computes Fretwork's curves over the song: notes and movement per
  second for 5-fret, or hands, travel and kicks for drums, smoothed over 2 s, with D as their sum
  (Fretwork's `functions/curves.py`). It is built from the scanner's own `chart.rs`, `ini.rs`,
  `sngpkg.rs` and `curves.rs`, so the graphs come from the same code as the ratings. No curve data
  is stored or published.

The chart parser (`src/chart.rs`), the `.sng` reader (`src/sng.rs`), the ini handling
(`src/ini.rs`) and the Fretwork port (`src/fretwork.rs`) are copied from
[Truss](https://github.com/graindead/truss).

## Running the scanner

Needs Rust 1.85+ (edition 2024).

```bash
cargo build --release
```

Trial run: list two catalog pages (~500 charts) and rate 300 of them.

```bash
./target/release/truss-index scan --list-pages 2 --limit 300
```

Full scan: list the whole catalog (~96,000 charts, ~390 requests) and rate everything. At the
default 4 charts at a time this takes roughly 80 minutes.

```bash
./target/release/truss-index scan
```

Options:

| Option | Meaning |
| --- | --- |
| `--db PATH` | Database file (default `data/fretwork.db`) |
| `--jobs N` | Charts fetched at once, 1–32 (default 4; the first full scan ran at 32) |
| `--limit N` | Rate at most N charts this run, newest first |
| `--list-pages N` | Stop listing after N catalog pages (for trials) |
| `--full-list` | List the whole catalog even if a previous listing exists, and mark charts Encore no longer has as removed |
| `--no-list` | Skip listing; only rate charts already in the database |
| `--retry-failed` | Try charts again that failed 3 times |

`truss-index stats` prints counts. `truss-index export [--out site/data]` writes the site data.

### Resuming and keeping up to date

- Each chart's ratings are saved as soon as it's rated. Stopping and re-running picks up where it
  left off, because charts already rated are skipped.
- Listing is incremental after the first complete pass. Encore lists newest-modified charts
  first, so the scanner stops at the first page that has no unknown md5s and nothing modified since
  the last listing (with 3 days of overlap). An updated chart has a new md5, so it's picked up and
  rated as a new entry.
- Charts Encore no longer lists are only detected by a full pass (`--full-list`). They're marked
  `removed` and left out of the export.
- If `db::FORMULA_VERSION` is bumped after a change to the maths, every chart is rated again on
  the next run.

### Being polite to Encore

- By default only 4 charts are fetched at once. Each one is 2–3 small ranged requests: the `.sng`
  header, then the notes file.
- Listing waits 300 ms between pages.
- A 429, 5xx or network error pauses every worker, with waits of 2 s, 6 s and then 18 s.
- A chart is retried up to 3 times. After that it's skipped until `--retry-failed`.
- Broken files (bad header, no notes file, parse error) count as permanent failures straight away.
- Every failure is logged in the `failures` table, and the latest one is kept on the chart row.
- After 40 failures in a row the run stops, since Encore is probably down. Progress is kept.

### Database

| Table | Contents |
| --- | --- |
| `charts` | md5, chart_id, name, artist, album, genre, year, charter, song_length (ms), modified_time, has_video, instruments, first/last seen, removed, rated_at, formula_version, fail_count, last_error |
| `ratings` | one row per chart and instrument: `d`, `remap` (0–6), `tier` (open-ended), `movement`, `spread`, `kicks` and `tier_2x` (drums) |
| `failures` | md5, time, error for every failed attempt |
| `meta` | listing watermark and last full listing time |

Fretwork rates Expert 5-fret guitar, co-op, rhythm, bass, keys and drums. Charts with only
6-fret or vocals are stored as rated but have no rating rows, and are left out of the site.

## Viewing the site locally

```bash
./target/release/truss-index export
```

Build the graph module (needs `rustup target add wasm32-unknown-unknown` once):

```bash
cargo build --release --manifest-path wasm/Cargo.toml --target wasm32-unknown-unknown
```

```bash
cp wasm/target/wasm32-unknown-unknown/release/truss_index_curves.wasm site/curves.wasm
```

```bash
python -m http.server 8765 --directory site
```

Then open http://localhost:8765. Opening `index.html` as a file won't work, because browsers
block `fetch` from `file://`.

The filters are kept in the URL hash, e.g. `#i=4&min=3&max=4` for drums at tier 3–4, so a
filtered view can be shared as a link.

## Deploying to GitHub Pages

`.github/workflows/update.yml` runs every hour at :05 (GitHub often starts scheduled runs a few
minutes late), and can also be started by hand from the Actions tab. Each run takes about 1.5
minutes:

1. downloads the database from a release tagged `data` (too large and changes too often to commit)
2. builds the scanner and scans incrementally, rating up to 20,000 charts. The 05:xx UTC run on
   Sundays also does a full listing to find removed charts
3. uploads the database back to the `data` release
4. exports `site/data` and deploys `site/` to Pages

GitHub switches off scheduled workflows in public repos after 60 days without a commit. These runs
don't commit (the database goes to the release), so when the last commit is 50 days old a run
pushes an empty "Keep the hourly update schedule active" commit.

To set it up:

1. Push the repo to GitHub.
2. Under **Settings → Pages**, set **Source** to **GitHub Actions**.
3. Seed the database. Either run the full scan locally and upload it:

   ```bash
   gzip -k data/fretwork.db
   ```

   ```bash
   gh release create data data/fretwork.db.gz --title "Scanner database" --notes "SQLite database used by the hourly update."
   ```

   Or start the workflow by hand a few times. Each run rates up to 20,000 charts within GitHub's
   6-hour job limit.

With the full catalog (95,358 rated charts), `charts.json` is about 16 MB, or about 6 MB as Pages serves it gzipped. That
is well inside Pages' limits (100 MB per file, 1 GB per site).

## Credits

- **Fretwork** by Staycation, https://github.com/Staycation44/fretwork (MIT licence). The
  difficulty formulas (v3) are theirs; this project only ports the maths to Rust.
- **Chorus Encore**, https://www.enchor.us. The chart catalog and files come from Encore's
  public API and file host. Not affiliated with Encore. Charts belong to their charters.

## Licence

GPL-3.0, like Truss, which the parser code is copied from. See `LICENSE`.
