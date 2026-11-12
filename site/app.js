// Truss Index: loads data/charts.json (written by `truss-index export`) and filters it
// in the browser. Row: [md5, name, artist, album, charter, lengthSec, modified, ratings]
// Rating: [inst, d*100, remap, tier, movement*100, spread*100, kicks*100|null, tier2x|null]
// then genre, year.
"use strict";

const PER_PAGE = 100;
const ASSET_V = document.currentScript ? new URL(document.currentScript.src).search : "";
// Encore's own order: most recently added or updated first (charts.json is written in that order).
const DEFAULT_SORT = "mod-desc";
const NAMES = ["Lead guitar", "Co-op guitar", "Rhythm", "Bass", "Drums", "Keys"];
const SHORT = ["Guitar", "Co-op", "Rhythm", "Bass", "Drums", "Keys"];
const $ = (id) => document.getElementById(id);
const els = {
  q: $("q"), inst: $("inst"), scale: $("scale"), tmin: $("tmin"), tmax: $("tmax"), sort: $("sort"),
  reset: $("reset"), count: $("count"), rows: $("rows"), prev: $("prev"), next: $("next"), page: $("page"), meta: $("meta"),
};

let charts = [];   // { row, text, idx } (idx: position in Encore's newest-first order)
let view = [];     // filtered + sorted
let page = 0;
const open = new Set(); // md5s of expanded rows
let last = { inst: -1, tIdx: 2 }; // what the table was last rendered with
let byInst = [];        // per instrument: every chart's D*100, sorted, for percentiles

const fold = (s) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const fmtLen = (s) => (s ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : "");
const tierColor = (t) => `var(--t${Math.min(6, Math.max(0, t))})`;

// ---------- state <-> URL ----------
function readState() {
  const p = new URLSearchParams(location.hash.slice(1));
  els.q.value = p.get("q") || "";
  els.inst.value = p.get("i") || "";
  els.scale.value = p.get("s") === "tier" ? "tier" : "remap";
  els.tmin.value = p.get("min") || "";
  els.tmax.value = p.get("max") || "";
  els.sort.value = p.get("o") || DEFAULT_SORT;
  if (!els.sort.value) els.sort.value = DEFAULT_SORT;
  page = Math.max(0, (parseInt(p.get("p"), 10) || 1) - 1);
}
function writeState() {
  const p = new URLSearchParams();
  if (els.q.value) p.set("q", els.q.value);
  if (els.inst.value) p.set("i", els.inst.value);
  if (els.scale.value !== "remap") p.set("s", els.scale.value);
  if (els.tmin.value) p.set("min", els.tmin.value);
  if (els.tmax.value) p.set("max", els.tmax.value);
  if (els.sort.value !== DEFAULT_SORT) p.set("o", els.sort.value);
  if (page > 0) p.set("p", page + 1);
  const h = p.toString();
  history.replaceState(null, "", h ? `#${h}` : location.pathname + location.search);
}

// ---------- filtering ----------
function apply(resetPage) {
  if (resetPage) page = 0;
  const words = fold(els.q.value).split(/\s+/).filter(Boolean);
  const inst = els.inst.value === "" ? -1 : +els.inst.value;
  const tIdx = els.scale.value === "tier" ? 3 : 2;
  const min = els.tmin.value === "" ? -Infinity : +els.tmin.value;
  const max = els.tmax.value === "" ? Infinity : +els.tmax.value;
  const inRange = (r) => r[tIdx] >= min && r[tIdx] <= max;

  view = [];
  for (const c of charts) {
    if (words.length && !words.every((w) => c.text.includes(w))) continue;
    const rs = c.row[7];
    // The ratings that pass the filters: the chosen instrument's, or every one in range.
    const ok = inst >= 0 ? rs.filter((x) => x[0] === inst && inRange(x)) : rs.filter(inRange);
    if (!ok.length) continue;
    view.push({ c, ok });
  }

  const [field, dir] = els.sort.value.split("-");
  const sgn = dir === "asc" ? 1 : -1;
  const coll = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
  const known = (v) => (v && !/^unknown /i.test(v) ? v : null);
  const maxOf = (ok, i) => Math.max(...ok.map((r) => r[i]));
  // Each sort's value for a chart; null (no album, no drums, ...) always goes last.
  const value = {
    // Hardest by tier first (D scales differ per instrument), then D.
    d: (e) => (inst >= 0 ? e.ok[0][1] : Math.max(...e.ok.map((r) => r[tIdx] * 1e6 + r[1]))),
    mod: (e) => -e.c.idx,
    len: (e) => e.c.row[5] || null,
    // Real years only: classical pieces go back centuries, joke ones ("42069", "20Bruhteen") don't count.
    year: (e) => { const y = parseInt(e.c.row[9], 10); return y >= 1000 && y <= new Date().getFullYear() + 1 ? y : null; },
    move: (e) => maxOf(e.ok, 4),
    spread: (e) => maxOf(e.ok, 5),
    kicks: (e) => e.ok.find((r) => r[0] === 4)?.[6] ?? null,
    name: (e) => e.c.row[1] || null,
    artist: (e) => e.c.row[2] || null,
    album: (e) => known(e.c.row[3]),
    charter: (e) => e.c.row[4] || null,
  }[field] || ((e) => -e.c.idx);
  for (const e of view) e.key = value(e);
  const text = typeof view[0]?.key === "string" || ["name", "artist", "album", "charter"].includes(field);
  view.sort((a, b) => {
    if (a.key == null || b.key == null) return (a.key == null) - (b.key == null) || a.c.idx - b.c.idx;
    const v = text ? coll.compare(a.key, b.key) : a.key - b.key;
    return v * sgn || a.c.idx - b.c.idx;
  });

  const pages = Math.max(1, Math.ceil(view.length / PER_PAGE));
  page = Math.min(page, pages - 1);
  render(inst, tIdx);
  writeState();
}

// ---------- rendering ----------
function chip(r, sel, tIdx) {
  const t = r[tIdx];
  const title = `${NAMES[r[0]]}: D ${(r[1] / 100).toFixed(2)}, tier ${r[2]} (0–6), open-ended tier ${r[3]}`;
  return `<span class="chip${sel ? " sel" : ""}" title="${esc(title)}"><span class="t" style="background:${tierColor(r[2])}">${t}</span>${SHORT[r[0]]} <span>${(r[1] / 100).toFixed(1)}</span></span>`;
}

function detail(r) {
  const parts = [
    `D ${(r[1] / 100).toFixed(2)}`,
    `tier ${r[2]} / open ${r[3]}`,
    `movement ${(r[4] / 100).toFixed(2)}/s`,
    `spread ${(r[5] / 100).toFixed(2)}`,
  ];
  if (r[6] != null) parts.push(`kicks ${(r[6] / 100).toFixed(2)}/s`);
  if (r[7] != null) parts.push(`2x tier ${r[7]}`);
  return `<div class="detail">${parts.join(" · ")}</div>`;
}

// Share of rated charts on this instrument with a lower D.
function percentile(i, d) {
  const a = byInst[i];
  if (!a || a.length < 2) return null;
  let lo = 0, hi = a.length;
  while (lo < hi) { const m = (lo + hi) >> 1; if (a[m] < d) lo = m + 1; else hi = m; }
  return Math.floor((lo / a.length) * 100);
}

function meter(remap) {
  let h = "";
  for (let t = 0; t <= 6; t++) {
    h += `<span class="seg${t <= remap ? " on" : ""}" style="${t <= remap ? `background:${tierColor(t)}` : ""}"></span>`;
  }
  return `<span class="meter" aria-hidden="true">${h}</span>`;
}

function card(r, sel, md5) {
  const key = `${md5}:${r[0]}`;
  const gOpen = graphOpen.has(key);
  const pct = percentile(r[0], r[1]);
  const kind = r[0] === 4 ? "drum" : NAMES[r[0]].toLowerCase();
  const stats = [
    ["Fretwork score (D)", (r[1] / 100).toFixed(2), "The raw difficulty score behind both tiers. Scales differ by instrument."],
    ["Open-ended tier", r[7] != null ? `${r[3]} <span class="dim"><span class="sep">· </span>${r[7]} counting 2x kick</span>` : r[3], "Keeps climbing past 6 for very hard charts."],
    ["Harder than", pct == null ? "–" : `${pct}% <span class="dim">of ${kind} charts</span>`, "Where this D sits among every rated chart on this instrument."],
    ["Movement", `${(r[4] / 100).toFixed(2)} <span class="dim">per second</span>`, r[0] === 4 ? "Average newly struck pads per second." : "Average fret changes per second."],
    ["Spread", (r[5] / 100).toFixed(2), "How much the note rate swings over the song: low is even, high has spikes."],
  ];
  if (r[6] != null) stats.push(["Kicks", `${(r[6] / 100).toFixed(2)} <span class="dim">per second</span>`, "Average single-pedal kicks per second."]);
  return `<section class="card${sel ? " sel" : ""}${gOpen ? " gopen" : ""}" data-key="${key}" tabindex="0" role="button" aria-expanded="${gOpen}"
      aria-label="${NAMES[r[0]]}: ${gOpen ? "hide" : "show"} the difficulty graph">
    <header><span class="t big" style="background:${tierColor(r[2])}">${r[2]}</span>
      <div><b>${NAMES[r[0]]}</b><div class="dim">Tier ${r[2]} of 6</div></div>${meter(r[2])}
      <span class="gtoggle">${gOpen ? "Hide graph ▴" : "Show graph ▾"}</span></header>
    <dl>${stats.map(([k, v, tip]) => `<div title="${esc(tip)}"><dt>${k}</dt><dd>${v}</dd></div>`).join("")}</dl>
    ${gOpen ? `<div class="graph" data-key="${key}">${graphBody(md5, r[0])}</div>` : ""}
  </section>`;
}

function panel(row, inst) {
  const [md5, , , album, charter, len, mod, rs, genre, year, hasVideo] = row;
  videoOf.set(md5, !!hasVideo);
  const sel = inst >= 0 ? rs.find((x) => x[0] === inst) : null;
  const shown = sel ? [sel, ...rs.filter((x) => x !== sel)] : rs;
  const known = (v) => v && !/^unknown /i.test(v); // song.ini's "Unknown Album" etc.
  const facts = [
    known(album) && ["Album", esc(album)],
    known(genre) && ["Genre", esc(genre)],
    known(year) && ["Year", esc(year)],
    charter && ["Charter", esc(charter)],
    len && ["Length", fmtLen(len)],
    mod && ["Updated on Encore", mod],
  ].filter(Boolean);
  return `<div class="panel">
    <div class="facts">${facts.map(([k, v]) => `<span><span class="dim">${k}</span> ${v}</span>`).join("")}
      <a href="https://www.enchor.us/chart/${md5}" target="_blank" rel="noopener">Open on Chorus Encore ↗</a></div>
    <div class="cards">${shown.map((r) => card(r, r === sel, md5)).join("")}</div>
    <p class="dim note">Expert only. Hover a figure for what it means; click an instrument to see its difficulty over the song.</p>
  </div>`;
}

function render(inst, tIdx) {
  last = { inst, tIdx };
  const start = page * PER_PAGE;
  const slice = view.slice(start, start + PER_PAGE);
  els.rows.innerHTML = slice
    .map(({ c }) => {
      const [md5, name, artist, album, charter, len, , rs] = c.row;
      const sel = inst >= 0 ? rs.find((x) => x[0] === inst) : null;
      const shown = sel ? [sel, ...rs.filter((x) => x !== sel)] : rs;
      const isOpen = open.has(md5);
      return `<tr class="row${isOpen ? " open" : ""}" data-md5="${md5}" tabindex="0" aria-expanded="${isOpen}">
        <td class="song"><a href="https://www.enchor.us/chart/${md5}" target="_blank" rel="noopener">${esc(name)}</a>
          <div class="by">${esc(artist)}${album && !/^unknown /i.test(album) ? ` — ${esc(album)}` : ""}</div></td>
        <td class="charter">${esc(charter)}</td>
        <td class="num">${fmtLen(len)}</td>
        <td><div class="chips">${shown.map((r) => chip(r, r === sel, tIdx)).join("")}</div>${sel && !isOpen ? detail(sel) : ""}</td>
      </tr>${isOpen ? `<tr class="more"><td colspan="4">${panel(c.row, inst)}</td></tr>` : ""}`;
    })
    .join("");
  const pages = Math.max(1, Math.ceil(view.length / PER_PAGE));
  els.count.textContent = `${view.length.toLocaleString()} of ${charts.length.toLocaleString()} charts`;
  els.page.textContent = `Page ${page + 1} of ${pages}`;
  els.prev.disabled = page === 0;
  els.next.disabled = page >= pages - 1;
  drawGraphs();
  const [field, dir] = els.sort.value.split("-");
  document.querySelectorAll("th[data-sort]").forEach((th) => {
    th.classList.toggle("on", th.dataset.sort === field);
    th.classList.toggle("asc", dir === "asc");
  });
}

// ---------- difficulty graphs ----------
// Fretwork's curves over the song, computed in the browser: the .sng header and notes come from
// Encore by byte range (as the scanner reads them), and curves.wasm (built from the scanner's
// own Rust parser and src/curves.rs) turns them into per-second series.
const graphOpen = new Set();   // `${md5}:${inst}` with the graph showing
const curveData = new Map();   // md5 -> { step, tracks } | { error }
const curveLoading = new Set();
const videoOf = new Map();     // md5 -> has a video background (notes come from the _novideo file)
const INST_KEYS = ["guitar", "guitarcoop", "rhythm", "bass", "drums", "keys"];
const SERIES = {
  d: { label: "D", color: "var(--accent)", width: 2.4, tip: "Moment-to-moment difficulty: the other lines added together" },
  d2x: { label: "D with 2x kick", color: "var(--t6)", width: 1.8, dash: "5 4", tip: "D counting double-kick notes" },
  notes: { label: "Notes", color: "var(--success)", width: 1.4, tip: "Notes or chords per second" },
  movement: { label: "Movement", color: "var(--warning)", width: 1.4, tip: "Fret changes per second" },
  hands: { label: "Hands", color: "var(--success)", width: 1.4, tip: "Hand hits per second (rolls capped at 4)" },
  travel: { label: "Travel", color: "var(--warning)", width: 1.4, tip: "Newly struck pads per second" },
  kicks: { label: "Kicks", color: "var(--danger)", width: 1.4, tip: "Single-pedal kicks per second" },
};

let wasmP = null;
function wasm() {
  wasmP ||= fetch(`curves.wasm${ASSET_V}`)
    .then((r) => { if (!r.ok) throw new Error(`curves.wasm: HTTP ${r.status}`); return r.arrayBuffer(); })
    .then((b) => WebAssembly.instantiate(b))
    .then((m) => m.instance.exports);
  wasmP.catch(() => { wasmP = null; });
  return wasmP;
}

// Copies the byte arrays into wasm memory, calls fn, and parses its JSON result.
function wcall(ex, fn, ...bufs) {
  const args = [];
  const ptrs = bufs.map((b) => {
    const p = ex.alloc(b.length);
    new Uint8Array(ex.memory.buffer, p, b.length).set(b);
    args.push(p, b.length);
    return [p, b.length];
  });
  try {
    const n = ex[fn](...args);
    const text = new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.out_ptr(), n));
    return JSON.parse(text);
  } finally {
    for (const [p, l] of ptrs) ex.free(p, l);
  }
}

async function getRange(url, start, end) {
  const r = await fetch(url, { headers: { Range: `bytes=${start}-${end - 1}` } });
  if (!r.ok) throw new Error(`Encore answered HTTP ${r.status}`);
  const b = new Uint8Array(await r.arrayBuffer());
  return r.status === 200 ? b.subarray(start, Math.min(end, b.length)) : b;
}

async function loadCurves(md5) {
  const ex = await wasm();
  const url = `https://files.enchor.us/${md5}${videoOf.get(md5) ? "_novideo" : ""}.sng`;
  let want = 64 * 1024, head, h;
  for (;;) {
    head = await getRange(url, 0, want);
    h = wcall(ex, "header", head);
    if (h.error) throw new Error(h.error);
    if (!h.need) break;
    if (head.length < want || want >= 16 << 20) throw new Error("Incomplete .sng file");
    want *= 4;
  }
  const end = h.index + h.len;
  const notes = end <= head.length ? head.subarray(h.index, end) : await getRange(url, h.index, end);
  const res = wcall(ex, "curves", head, notes);
  if (res.error) throw new Error(res.error);
  return res;
}

function graphBody(md5, inst) {
  const d = curveData.get(md5);
  if (!d) {
    if (!curveLoading.has(md5)) {
      curveLoading.add(md5);
      loadCurves(md5)
        .then((res) => curveData.set(md5, res))
        .catch((e) => curveData.set(md5, { error: e.message || String(e) }))
        .finally(() => { curveLoading.delete(md5); render(last.inst, last.tIdx); });
    }
    return `<p class="gmsg dim">Loading the notes from Chorus Encore…</p>`;
  }
  if (d.error) return `<p class="gmsg">Couldn't draw this graph: ${esc(d.error)}</p>`;
  if (!d.tracks[INST_KEYS[inst]]) return `<p class="gmsg dim">No Expert ${NAMES[inst].toLowerCase()} notes found in this chart.</p>`;
  return `<div class="greadout"></div><div class="gplot"></div>`;
}

// The first step giving at most `want` ticks over `span`.
function niceStep(span, want, steps) {
  for (const s of steps) if (span / s <= want) return s;
  return steps[steps.length - 1];
}

function drawGraphs() {
  for (const g of els.rows.querySelectorAll(".graph")) {
    const plot = g.querySelector(".gplot");
    if (!plot) continue;
    const [md5, inst] = g.dataset.key.split(":");
    drawGraph(g, plot, curveData.get(md5), INST_KEYS[+inst]);
  }
}

function drawGraph(g, plot, data, instKey) {
  const tr = data.tracks[instKey];
  const names = Object.keys(SERIES).filter((k) => tr[k]);
  const n = Math.max(...names.map((k) => tr[k].length));
  const stepS = data.step / 1000;
  plot.innerHTML = ""; // measure the space without the old drawing holding it open
  const W = Math.max(240, plot.clientWidth), H = W < 500 ? 190 : 230;
  const m = { l: 34, r: 10, t: 10, b: 24 };
  const iw = W - m.l - m.r, ih = H - m.t - m.b;
  let top = 1;
  for (const k of names) for (const v of tr[k]) if (v > top) top = v;
  const ymax = top * 1.08;
  const yStep = niceStep(ymax, 5, [1, 2, 5, 10, 20, 25, 50, 100, 200]);
  const dur = Math.max(1, (n - 1) * stepS);
  const xStep = niceStep(dur, W < 500 ? 4 : 8, [10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600]);
  const x = (i) => m.l + ((i * stepS) / dur) * iw;
  const y = (v) => m.t + ih - (v / ymax) * ih;
  const every = Math.max(1, Math.floor(n / (iw * 1.5)));

  let svg = `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(names.map((k) => SERIES[k].label).join(", "))} over the song">`;
  for (let v = 0; v <= ymax; v += yStep) {
    svg += `<line x1="${m.l}" x2="${W - m.r}" y1="${y(v)}" y2="${y(v)}" class="grid"/><text x="${m.l - 6}" y="${y(v) + 4}" class="ax" text-anchor="end">${v}</text>`;
  }
  for (let t = 0; t <= dur; t += xStep) {
    svg += `<text x="${m.l + (t / dur) * iw}" y="${H - 6}" class="ax" text-anchor="middle">${fmtLen(t) || "0:00"}</text>`;
  }
  // The D lines are drawn last so they sit on top.
  for (const k of [...names].reverse()) {
    const s = SERIES[k], v = tr[k];
    let d = "";
    for (let i = 0; i < v.length; i += every) d += `${d ? "L" : "M"}${x(i).toFixed(1)},${y(v[i]).toFixed(1)}`;
    svg += `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="${s.width}" stroke-linejoin="round"${s.dash ? ` stroke-dasharray="${s.dash}"` : ""}/>`;
  }
  svg += `<line class="cursor" y1="${m.t}" y2="${m.t + ih}" x1="0" x2="0" visibility="hidden"/>`;
  svg += `<rect x="${m.l}" y="${m.t}" width="${iw}" height="${ih}" fill="transparent" class="hit"/></svg>`;
  plot.innerHTML = svg;

  const read = g.querySelector(".greadout");
  let peak = 0;
  tr.d.forEach((v, i) => { if (v > tr.d[peak]) peak = i; });
  const legend = names.map((k) => `<span title="${esc(SERIES[k].tip)}"><i style="background:${SERIES[k].color}"></i>${SERIES[k].label}</span>`).join("");
  const idle = `${legend}<span class="dim">Peak D ${tr.d[peak].toFixed(1)} at ${fmtLen(Math.round(peak * stepS)) || "0:00"}</span>`;
  read.innerHTML = idle;
  const cursor = plot.querySelector(".cursor");
  const hit = plot.querySelector(".hit");
  hit.addEventListener("pointermove", (e) => {
    const box = hit.getBoundingClientRect();
    const i = Math.max(0, Math.min(n - 1, Math.round((((e.clientX - box.left) / box.width) * dur) / stepS)));
    cursor.setAttribute("x1", x(i));
    cursor.setAttribute("x2", x(i));
    cursor.setAttribute("visibility", "visible");
    read.innerHTML = `<b>${fmtLen(Math.round(i * stepS)) || "0:00"}</b>` + names.map((k) =>
      `<span><i style="background:${SERIES[k].color}"></i>${SERIES[k].label} ${(tr[k][i] ?? 0).toFixed(1)}${k.startsWith("d") ? "" : "/s"}</span>`).join("");
  });
  hit.addEventListener("pointerleave", () => {
    cursor.setAttribute("visibility", "hidden");
    read.innerHTML = idle;
  });
}

let resizeTimer;
addEventListener("resize", () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(drawGraphs, 150); });

function toggleGraph(key) {
  if (graphOpen.has(key)) graphOpen.delete(key); else graphOpen.add(key);
  render(last.inst, last.tIdx);
  els.rows.querySelector(`.card[data-key="${key}"]`)?.focus({ preventScroll: true });
}

// ---------- events ----------
// Clicking a row (anywhere but a link) opens or closes its details; Enter or Space does too.
function toggleRow(tr) {
  const md5 = tr.dataset.md5;
  if (open.has(md5)) open.delete(md5); else open.add(md5);
  render(last.inst, last.tIdx);
  els.rows.querySelector(`tr.row[data-md5="${md5}"]`)?.focus({ preventScroll: true });
}
els.rows.addEventListener("click", (e) => {
  if (e.target.closest("a") || getSelection().toString()) return;
  const cardEl = e.target.closest(".card[data-key]");
  if (cardEl) {
    if (!e.target.closest(".graph")) toggleGraph(cardEl.dataset.key);
    return;
  }
  const tr = e.target.closest("tr.row");
  if (tr) toggleRow(tr);
});
els.rows.addEventListener("keydown", (e) => {
  if (e.key !== "Enter" && e.key !== " ") return;
  if (e.target.matches("tr.row")) { e.preventDefault(); toggleRow(e.target); }
  else if (e.target.matches(".card[data-key]")) { e.preventDefault(); toggleGraph(e.target.dataset.key); }
});

let timer;
els.q.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(() => apply(true), 150); });
for (const el of [els.inst, els.scale, els.tmin, els.tmax, els.sort]) el.addEventListener("change", () => apply(true));
els.tmin.addEventListener("input", () => apply(true));
els.tmax.addEventListener("input", () => apply(true));
els.prev.addEventListener("click", () => { page--; apply(false); scrollTo(0, 0); });
els.next.addEventListener("click", () => { page++; apply(false); scrollTo(0, 0); });
els.reset.addEventListener("click", () => { history.replaceState(null, "", location.pathname); readState(); apply(true); });
document.querySelectorAll("th[data-sort]").forEach((th) =>
  th.addEventListener("click", () => {
    const f = th.dataset.sort;
    const [cur, dir] = els.sort.value.split("-");
    const def = f === "name" || f === "charter" ? "asc" : "desc";
    const next = cur === f ? (dir === "asc" ? "desc" : "asc") : def;
    els.sort.value = `${f}-${next}`;
    if (!els.sort.value) { // combination not in the menu: add it
      const o = new Option(`${th.textContent} (${next})`, `${f}-${next}`);
      els.sort.add(o);
      els.sort.value = o.value;
    }
    apply(true);
  })
);

// A link or hand edit changing only the #state (writeState uses replaceState, which doesn't fire this).
window.addEventListener("hashchange", () => { readState(); if (charts.length) apply(false); });

// ---------- load ----------
(async () => {
  readState();
  try {
    // meta.json is tiny and always fetched fresh; its export time versions the big file, so a
    // new export is never hidden behind a cached charts.json.
    const meta = await fetch("data/meta.json", { cache: "no-cache" }).then((r) => (r.ok ? r.json() : null)).catch(() => null);
    const v = meta ? `?v=${encodeURIComponent(meta.exported)}` : "";
    const data = await fetch(`data/charts.json${v}`).then((r) => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.json(); });
    charts = data.rows.map((row, idx) => ({ row, idx, text: fold(`${row[1]} ${row[2]} ${row[3]} ${row[4]}`) }));
    byInst = NAMES.map(() => []);
    for (const { row } of charts) for (const r of row[7]) byInst[r[0]].push(r[1]);
    for (const a of byInst) a.sort((x, y) => x - y);
    if (meta) els.meta.textContent = `${meta.charts.toLocaleString()} charts rated, updated ${meta.exported.slice(0, 10)}.`;
    apply(false);
  } catch (e) {
    els.count.textContent = `Couldn't load the chart list (${e.message}).`;
  }
})();
