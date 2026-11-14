// Star Power paths: CHOpt (the Truss port of CHOpt by Raymond Wright, GPL-3.0) in the browser.
// The search runs in chopt-worker.js; this file is the section's page: loading a chart (from
// an Encore row's Path button, or dropped files), the options, and drawing the path image.
// Shares app.js's globals (charts, esc, isPathHash, ASSET_V, ...).
"use strict";

const P = {
  view: $("view-path"), chartsView: $("view-charts"), tabCharts: $("tab-charts"), tabPath: $("tab-path"),
  drop: $("drop"), empty: $("drop-empty"), loaded: $("drop-loaded"), name: $("loaded-name"), by: $("loaded-by"),
  src: $("loaded-src"), pick: $("pick"), pick2: $("pick2"), inst: $("p-inst"), diff: $("p-diff"),
  squeeze: $("p-squeeze"), ew: $("p-ew"), lazy: $("p-lazy"), wdelay: $("p-wdelay"), lag: $("p-lag"),
  speed: $("p-speed"), precision: $("p-precision"), lefty: $("p-lefty"), run: $("p-run"), cancel: $("p-cancel"),
  status: $("p-status"), result: $("p-result"), summary: $("p-summary"), save: $("p-save"), image: $("p-image"),
};
const INST_LABEL = { guitar: "Lead guitar", guitarcoop: "Co-op guitar", bass: "Bass", rhythm: "Rhythm", keys: "Keys" };
const DIFF_LABEL = { expert: "Expert", hard: "Hard", medium: "Medium", easy: "Easy" };
const STRIP_ROWS = 6;  // measure rows per drawn strip

let worker = null;
let nextId = 1;
const pending = new Map();
let lastLoad = null;   // the message that loaded the current chart, to reload after a cancel
let song = null;       // {name, artist, charter, tracks}
let image = null;      // {width, height, row}
let running = false;
let loadedMd5 = null;

// The worker (and the chopt.wasm it loads) share this build's ?v= with app.js.
function startWorker() {
  worker = new Worker(`chopt-worker.js${ASSET_V}`);
  worker.onmessage = (e) => {
    const p = pending.get(e.data.id);
    if (!p) return;
    pending.delete(e.data.id);
    if (e.data.error) p.reject(new Error(e.data.error)); else p.resolve(e.data.result);
  };
  worker.onerror = (e) => {
    for (const p of pending.values()) p.reject(new Error(e.message || "The path finder stopped."));
    pending.clear();
  };
}

function ask(msg, transfer) {
  if (!worker) startWorker();
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ ...msg, id }, transfer || []);
  });
}

function stopWorker() {
  worker?.terminate();
  worker = null;
  for (const p of pending.values()) p.reject(new Error("Cancelled"));
  pending.clear();
}

// ---------- options (remembered per browser) ----------
const OPT_KEYS = ["squeeze", "ew", "lazy", "wdelay", "lag", "speed", "precision", "lefty"];
function saveOpts() {
  const o = {};
  for (const k of OPT_KEYS) o[k] = P[k].type === "checkbox" ? P[k].checked : P[k].value;
  try { localStorage.setItem("chopt-options", JSON.stringify(o)); } catch {}
}
function restoreOpts() {
  let o = null;
  try { o = JSON.parse(localStorage.getItem("chopt-options") || "null"); } catch {}
  if (!o) return;
  for (const k of OPT_KEYS) if (k in o) { if (P[k].type === "checkbox") P[k].checked = !!o[k]; else P[k].value = o[k]; }
}
const num = (el, lo, hi, d) => { const v = Number(el.value); return Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d; };
function options() {
  return {
    instrument: P.inst.value, difficulty: P.diff.value,
    squeeze: num(P.squeeze, 0, 100, 100), early_whammy: num(P.ew, 0, 100, 100),
    lazy_whammy_ms: num(P.lazy, 0, 100000, 0), whammy_delay_ms: num(P.wdelay, 0, 100000, 0),
    video_lag_ms: num(P.lag, -200, 200, 0), speed: Math.round(num(P.speed, 5, 5000, 100) / 5) * 5,
    precision: P.precision.checked, lefty: P.lefty.checked,
  };
}

// ---------- view switching ----------
function showView() {
  const path = isPathHash();
  P.view.hidden = !path;
  P.chartsView.hidden = path;
  P.tabPath.classList.toggle("on", path);
  P.tabCharts.classList.toggle("on", !path);
  P.tabCharts.href = chartsHash || "#";
  if (!path) return;
  const md5 = (location.hash.match(/^#path=([0-9a-f]{32})$/) || [])[1];
  if (md5 && md5 !== loadedMd5) whenCharts(() => loadEncore(md5));
}

// Encore links use the chart list (for whether the chart has a video background), which app.js
// may still be loading; after 15 s, go ahead without it.
function whenCharts(fn, tries = 0) {
  if (charts.length || tries > 75) fn();
  else {
    if (!tries) setStatus("Loading the chart list…");
    setTimeout(() => whenCharts(fn, tries + 1), 200);
  }
}

P.tabCharts.addEventListener("click", (e) => {
  e.preventDefault();
  history.pushState(null, "", chartsHash || location.pathname + location.search);
  showView();
  readState();
  if (charts.length) apply(false);
});
addEventListener("hashchange", showView);
addEventListener("popstate", showView);

// ---------- loading ----------
function setStatus(text, isError) {
  P.status.textContent = text;
  P.status.classList.toggle("err", !!isError);
}

function showSong(info, source) {
  song = info;
  P.empty.hidden = true;
  P.loaded.hidden = false;
  P.name.textContent = info.name;
  P.by.textContent = `${info.artist} · charted by ${info.charter}`;
  P.src.textContent = source;
  // Instruments and difficulties the chart has (CHOpt paths 5-fret parts).
  const insts = [...new Set(info.tracks.map((t) => t[0]))];
  const prevInst = P.inst.value;
  P.inst.innerHTML = insts.map((i) => `<option value="${i}">${INST_LABEL[i] || i}</option>`).join("");
  P.inst.value = insts.includes(prevInst) ? prevInst : insts.includes("guitar") ? "guitar" : insts[0] || "";
  fillDiffs();
  P.run.disabled = !insts.length;
  if (!insts.length) setStatus("This chart has no 5-fret part CHOpt can path.", true);
}

function fillDiffs() {
  const diffs = song ? song.tracks.filter((t) => t[0] === P.inst.value).map((t) => t[1]) : [];
  const order = ["expert", "hard", "medium", "easy"].filter((d) => diffs.includes(d));
  const prev = P.diff.value;
  P.diff.innerHTML = order.map((d) => `<option value="${d}">${DIFF_LABEL[d]}</option>`).join("");
  P.diff.value = order.includes(prev) ? prev : order[0] || "";
}
P.inst.addEventListener("change", fillDiffs);

async function load(msg, source, transfer) {
  stopWorker();
  running = false;
  setRunning(false);
  image = null;
  P.result.hidden = true;
  setStatus("Loading the chart…");
  lastLoad = msg;
  try {
    const info = await ask(msg, transfer);
    showSong(info, source);
    if (!P.run.disabled) findPath();
  } catch (e) {
    if (e.message !== "Cancelled") setStatus(`Couldn't load this chart: ${e.message}`, true);
  }
}

function loadEncore(md5) {
  loadedMd5 = md5;
  const c = charts.find((x) => x.row[0] === md5);
  const video = c ? !!c.row[10] : undefined;
  const label = c ? `${c.row[1]} — from Chorus Encore` : "From Chorus Encore";
  load({ type: "encore", md5, video }, label);
}

// Dropped or picked files: a .sng, or a notes.chart / notes.mid with an optional song.ini.
async function loadFiles(files) {
  const lower = (f) => f.name.toLowerCase();
  const sng = files.find((f) => lower(f).endsWith(".sng"));
  const charts_ = files.filter((f) => /\.(chart|mid|midi)$/.test(lower(f)));
  // notes.* first, then .mid before .chart, as Clone Hero picks.
  charts_.sort((a, b) => (/^notes\./.test(lower(b)) - /^notes\./.test(lower(a))) || (/\.midi?$/.test(lower(b)) - /\.midi?$/.test(lower(a))));
  const ini = files.find((f) => lower(f) === "song.ini") || files.find((f) => lower(f).endsWith(".ini"));
  loadedMd5 = null;
  history.replaceState(null, "", "#path");
  if (charts_.length) {
    const notes = await charts_[0].arrayBuffer();
    const iniBytes = ini ? await ini.arrayBuffer() : new ArrayBuffer(0);
    const msg = { type: "files", notes, isMid: /\.midi?$/.test(lower(charts_[0])), ini: iniBytes };
    await load(msg, `${charts_[0].name}${ini ? ` + ${ini.name}` : " (no song.ini: default settings)"}`);
  } else if (sng) {
    await load({ type: "sng", bytes: await sng.arrayBuffer() }, sng.name);
  } else {
    setStatus("Drop a notes.chart, notes.mid or .sng file (plus its song.ini if you have it).", true);
  }
}

// A dropped folder: its files, one level down.
async function filesFromDrop(dt) {
  const out = [];
  const entries = [...(dt.items || [])].map((i) => i.webkitGetAsEntry?.()).filter(Boolean);
  if (!entries.length) return [...dt.files];
  const fileOf = (entry) => new Promise((res) => entry.file(res, () => res(null)));
  const readDir = (dir) => new Promise((res) => {
    const all = [];
    const r = dir.createReader();
    const next = () => r.readEntries((batch) => { if (!batch.length) res(all); else { all.push(...batch); next(); } }, () => res(all));
    next();
  });
  for (const e of entries) {
    if (e.isFile) out.push(await fileOf(e));
    else if (e.isDirectory) for (const c of await readDir(e)) if (c.isFile) out.push(await fileOf(c));
  }
  return out.filter(Boolean);
}

for (const ev of ["dragenter", "dragover"]) {
  P.view.addEventListener(ev, (e) => { e.preventDefault(); P.drop.classList.add("over"); });
}
P.view.addEventListener("dragleave", (e) => { if (!P.view.contains(e.relatedTarget)) P.drop.classList.remove("over"); });
P.view.addEventListener("drop", async (e) => {
  e.preventDefault();
  P.drop.classList.remove("over");
  loadFiles(await filesFromDrop(e.dataTransfer));
});
for (const input of [P.pick, P.pick2]) {
  input.addEventListener("change", () => { if (input.files.length) loadFiles([...input.files]); input.value = ""; });
}

// ---------- the search ----------
function setRunning(on) {
  running = on;
  P.run.hidden = on;
  P.cancel.hidden = !on;
}

let clock = null;
async function findPath() {
  if (!song || !P.inst.value) return;
  saveOpts();
  setRunning(true);
  P.result.hidden = true;
  const t0 = performance.now();
  const tick = () => setStatus(`Finding the best path… ${((performance.now() - t0) / 1000).toFixed(0)} s`);
  tick();
  clearInterval(clock);
  clock = setInterval(tick, 1000);
  try {
    const out = await ask({ type: "run", opts: options() });
    image = out;
    P.summary.textContent = out.summary;
    P.result.hidden = false;
    setStatus(`${INST_LABEL[P.inst.value]} ${DIFF_LABEL[P.diff.value]}: found in ${((performance.now() - t0) / 1000).toFixed(1)} s.`);
    drawImage();
  } catch (e) {
    if (e.message !== "Cancelled") setStatus(`Couldn't find a path: ${e.message}`, true);
  } finally {
    clearInterval(clock);
    setRunning(false);
  }
}
P.run.addEventListener("click", findPath);
P.cancel.addEventListener("click", () => {
  stopWorker();
  clearInterval(clock);
  setRunning(false);
  setStatus("Cancelled.");
  // The worker held the chart; load it again so the next search is ready.
  if (lastLoad) ask(lastLoad).catch(() => {});
});

// ---------- the image ----------
// Drawn in strips as they scroll into view; CHOpt's text is drawn here, in the page's font.
function paintStrip(canvas, y0, res) {
  const ctx = canvas.getContext("2d");
  ctx.putImageData(new ImageData(res.pixels, image.width, canvas.height), 0, 0);
  ctx.textBaseline = "top";
  for (const [x, y, text, colour, height, right] of res.texts) {
    ctx.font = `${Math.round(height * 0.86)}px system-ui, "Segoe UI", Roboto, sans-serif`;
    ctx.fillStyle = colour;
    ctx.textAlign = right ? "right" : "left";
    // Section names can carry Clone Hero formatting tags (<i>Verse 1</i>).
    ctx.fillText(text.replace(/<[^>]*>/g, ""), x, y + height * 0.08);
  }
}

let observer = null;
function drawImage() {
  observer?.disconnect();
  P.image.innerHTML = "";
  const stripH = image.row * STRIP_ROWS;
  const strips = [];
  for (let y0 = 0; y0 < image.height; y0 += stripH) {
    const c = document.createElement("canvas");
    c.width = image.width;
    c.height = Math.min(stripH, image.height - y0);
    c.dataset.y0 = y0;
    P.image.appendChild(c);
    strips.push(c);
  }
  const shown = image;
  observer = new IntersectionObserver((entries) => {
    for (const en of entries) {
      if (!en.isIntersecting || en.target.dataset.done) continue;
      const c = en.target;
      c.dataset.done = "1";
      const y0 = +c.dataset.y0;
      ask({ type: "render", y0, y1: y0 + c.height })
        .then((res) => { if (image === shown) paintStrip(c, y0, res); })
        .catch(() => { delete c.dataset.done; });
    }
  }, { rootMargin: "800px 0px" });
  for (const c of strips) observer.observe(c);
}

// Save the whole image as a PNG (drawn from every strip, rendering the ones not yet shown).
P.save.addEventListener("click", async () => {
  if (!image) return;
  P.save.disabled = true;
  try {
    const full = document.createElement("canvas");
    full.width = image.width;
    full.height = image.height;
    const stripH = image.row * STRIP_ROWS;
    for (let y0 = 0; y0 < image.height; y0 += stripH) {
      const c = document.createElement("canvas");
      c.width = image.width;
      c.height = Math.min(stripH, image.height - y0);
      paintStrip(c, y0, await ask({ type: "render", y0, y1: y0 + c.height }));
      full.getContext("2d").drawImage(c, 0, y0);
    }
    const blob = await new Promise((r) => full.toBlob(r, "image/png"));
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${(song?.name || "path").replace(/[\\/:*?"<>|]+/g, "")} - ${INST_LABEL[P.inst.value]} ${DIFF_LABEL[P.diff.value]}.png`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10000);
  } catch (e) {
    setStatus(`Couldn't save the image: ${e.message}`, true);
  } finally {
    P.save.disabled = false;
  }
});

restoreOpts();
showView();
