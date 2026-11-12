// Truss Index: loads data/charts.json (written by `fretwork-scan export`) and filters it
// in the browser. Row: [md5, name, artist, album, charter, lengthSec, modified, ratings]
// Rating: [inst, d*100, remap, tier, movement*100, spread*100, kicks*100|null, tier2x|null]
// then genre, year.
"use strict";

const PER_PAGE = 100;
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

function card(r, sel) {
  const pct = percentile(r[0], r[1]);
  const kind = r[0] === 4 ? "drum" : NAMES[r[0]].toLowerCase();
  const stats = [
    ["Fretwork score (D)", (r[1] / 100).toFixed(2), "The raw difficulty score behind both tiers. Scales differ by instrument."],
    ["Open-ended tier", r[7] != null ? `${r[3]} <span class="dim">· ${r[7]} counting 2x kick</span>` : r[3], "Keeps climbing past 6 for very hard charts."],
    ["Harder than", pct == null ? "–" : `${pct}% <span class="dim">of rated ${kind} charts</span>`, "Where this D sits among every rated chart on this instrument."],
    ["Movement", `${(r[4] / 100).toFixed(2)} <span class="dim">per second</span>`, r[0] === 4 ? "Average newly struck pads per second." : "Average fret changes per second."],
    ["Spread", (r[5] / 100).toFixed(2), "How much the note rate swings over the song: low is even, high has spikes."],
  ];
  if (r[6] != null) stats.push(["Kicks", `${(r[6] / 100).toFixed(2)} <span class="dim">per second</span>`, "Average single-pedal kicks per second."]);
  return `<section class="card${sel ? " sel" : ""}">
    <header><span class="t big" style="background:${tierColor(r[2])}">${r[2]}</span>
      <div><b>${NAMES[r[0]]}</b><div class="dim">Tier ${r[2]} of 6</div></div>${meter(r[2])}</header>
    <dl>${stats.map(([k, v, tip]) => `<div title="${esc(tip)}"><dt>${k}</dt><dd>${v}</dd></div>`).join("")}</dl>
  </section>`;
}

function panel(row, inst) {
  const [md5, , , album, charter, len, mod, rs, genre, year] = row;
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
    <div class="cards">${shown.map((r) => card(r, r === sel)).join("")}</div>
    <p class="dim note">Expert only. Hover a figure for what it means.</p>
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
  const [field, dir] = els.sort.value.split("-");
  document.querySelectorAll("th[data-sort]").forEach((th) => {
    th.classList.toggle("on", th.dataset.sort === field);
    th.classList.toggle("asc", dir === "asc");
  });
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
  const tr = e.target.closest("tr.row");
  if (tr) toggleRow(tr);
});
els.rows.addEventListener("keydown", (e) => {
  if ((e.key === "Enter" || e.key === " ") && e.target.matches("tr.row")) { e.preventDefault(); toggleRow(e.target); }
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
