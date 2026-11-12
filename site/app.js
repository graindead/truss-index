// Fretwork Ratings: loads data/charts.json (written by `fretwork-scan export`) and filters it
// in the browser. Row: [md5, name, artist, album, charter, lengthSec, modified, ratings]
// Rating: [inst, d*100, remap, tier, movement*100, spread*100, kicks*100|null, tier2x|null]
"use strict";

const PER_PAGE = 100;
const NAMES = ["Lead guitar", "Co-op guitar", "Rhythm", "Bass", "Drums", "Keys"];
const SHORT = ["Guitar", "Co-op", "Rhythm", "Bass", "Drums", "Keys"];
const $ = (id) => document.getElementById(id);
const els = {
  q: $("q"), inst: $("inst"), scale: $("scale"), tmin: $("tmin"), tmax: $("tmax"), sort: $("sort"),
  reset: $("reset"), count: $("count"), rows: $("rows"), prev: $("prev"), next: $("next"), page: $("page"), meta: $("meta"),
};

let charts = [];   // { row, text }
let view = [];     // filtered + sorted
let page = 0;

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
  els.sort.value = p.get("o") || "d-desc";
  if (!els.sort.value) els.sort.value = "d-desc";
  page = Math.max(0, (parseInt(p.get("p"), 10) || 1) - 1);
}
function writeState() {
  const p = new URLSearchParams();
  if (els.q.value) p.set("q", els.q.value);
  if (els.inst.value) p.set("i", els.inst.value);
  if (els.scale.value !== "remap") p.set("s", els.scale.value);
  if (els.tmin.value) p.set("min", els.tmin.value);
  if (els.tmax.value) p.set("max", els.tmax.value);
  if (els.sort.value !== "d-desc") p.set("o", els.sort.value);
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
    let key;
    if (inst >= 0) {
      const r = rs.find((x) => x[0] === inst);
      if (!r || !inRange(r)) continue;
      key = r[1];
    } else {
      const ok = rs.filter(inRange);
      if (!ok.length) continue;
      // Hardest instrument: by tier first (D scales differ per instrument), then D.
      key = Math.max(...ok.map((r) => r[tIdx] * 1e6 + r[1]));
    }
    view.push({ c, key });
  }

  const [field, dir] = els.sort.value.split("-");
  const sgn = dir === "asc" ? 1 : -1;
  const str = { name: 1, artist: 2, charter: 4 }[field];
  const coll = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
  view.sort((a, b) => {
    let v;
    if (str) v = coll.compare(a.c.row[str], b.c.row[str]);
    else if (field === "len") v = a.c.row[5] - b.c.row[5];
    else if (field === "mod") v = a.c.row[6] < b.c.row[6] ? -1 : a.c.row[6] > b.c.row[6] ? 1 : 0;
    else v = a.key - b.key;
    return v * sgn || coll.compare(a.c.row[1], b.c.row[1]);
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

function render(inst, tIdx) {
  const start = page * PER_PAGE;
  const slice = view.slice(start, start + PER_PAGE);
  els.rows.innerHTML = slice
    .map(({ c }) => {
      const [md5, name, artist, album, charter, len, , rs] = c.row;
      const sel = inst >= 0 ? rs.find((x) => x[0] === inst) : null;
      const shown = sel ? [sel, ...rs.filter((x) => x !== sel)] : rs;
      return `<tr>
        <td class="song"><a href="https://www.enchor.us/chart/${md5}" target="_blank" rel="noopener">${esc(name)}</a>
          <div class="by">${esc(artist)}${album ? ` — ${esc(album)}` : ""}</div></td>
        <td class="charter">${esc(charter)}</td>
        <td class="num">${fmtLen(len)}</td>
        <td><div class="chips">${shown.map((r) => chip(r, r === sel, tIdx)).join("")}</div>${sel ? detail(sel) : ""}</td>
      </tr>`;
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
    charts = data.rows.map((row) => ({ row, text: fold(`${row[1]} ${row[2]} ${row[3]} ${row[4]}`) }));
    if (meta) els.meta.textContent = `${meta.charts.toLocaleString()} charts rated, updated ${meta.exported.slice(0, 10)}.`;
    apply(false);
  } catch (e) {
    els.count.textContent = `Couldn't load the chart list (${e.message}).`;
  }
})();
