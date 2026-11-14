// Runs CHOpt (chopt.wasm, the Truss port of CHOpt by Raymond Wright, GPL-3.0) off the page's
// thread, so a long search never freezes the site. Cancelling terminates this worker.
//
// Messages in:  {id, type: "encore", md5, video} | {id, type: "sng", bytes}
//               {id, type: "files", notes, isMid, ini} | {id, type: "run", opts} | {id, type: "render", y0, y1}
// Messages out: {id, result} | {id, error}
"use strict";

const V = new URL(self.location.href).search;
let exP = null;

function wasm() {
  exP ||= fetch(`chopt.wasm${V}`)
    .then((r) => { if (!r.ok) throw new Error(`chopt.wasm: HTTP ${r.status}`); return r.arrayBuffer(); })
    .then((b) => WebAssembly.instantiate(b))
    .then((m) => m.instance.exports);
  return exP;
}

// Calls an export; Uint8Array arguments are copied in as (pointer, length).
function call(ex, fn, ...args) {
  const flat = [];
  const owned = [];
  for (const a of args) {
    if (a instanceof Uint8Array) {
      const p = ex.alloc(a.length);
      new Uint8Array(ex.memory.buffer, p, a.length).set(a);
      owned.push([p, a.length]);
      flat.push(p, a.length);
    } else {
      flat.push(a);
    }
  }
  try {
    return ex[fn](...flat);
  } finally {
    for (const [p, l] of owned) ex.free(p, l);
  }
}

function json(ex, n) {
  return JSON.parse(new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.out_ptr(), n)));
}

function check(v) {
  if (v && v.error) throw new Error(v.error);
  return v;
}

async function getRange(url, start, end) {
  const r = await fetch(url, { headers: { Range: `bytes=${start}-${end - 1}` } });
  if (!r.ok) throw new Error(`Chorus Encore answered HTTP ${r.status}`);
  const b = new Uint8Array(await r.arrayBuffer());
  return r.status === 200 ? b.subarray(start, Math.min(end, b.length)) : b;
}

// The chart's header and notes file from Encore, by byte range (as the scanner reads them).
async function loadEncore(ex, md5, video) {
  const tryUrl = async (url) => {
    let want = 64 * 1024, head, h;
    for (;;) {
      head = await getRange(url, 0, want);
      h = check(json(ex, call(ex, "header", head)));
      if (!h.need) break;
      if (head.length < want || want >= 16 << 20) throw new Error("Incomplete .sng file");
      want *= 4;
    }
    const end = h.index + h.len;
    const notes = end <= head.length ? head.subarray(h.index, end) : await getRange(url, h.index, end);
    return check(json(ex, call(ex, "load_sng", head, notes)));
  };
  const base = `https://files.enchor.us/${md5}`;
  try {
    return await tryUrl(`${base}${video ? "_novideo" : ""}.sng`);
  } catch (e) {
    // Not in the index (e.g. a link to a chart added since): try the other file name.
    if (video !== undefined) throw e;
    return await tryUrl(`${base}_novideo.sng`);
  }
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    const ex = await wasm();
    let result;
    if (m.type === "encore") {
      result = await loadEncore(ex, m.md5, m.video);
    } else if (m.type === "sng") {
      const b = new Uint8Array(m.bytes);
      result = check(json(ex, call(ex, "load_sng", b, b)));
    } else if (m.type === "files") {
      result = check(json(ex, call(ex, "load_files", new Uint8Array(m.notes), m.isMid ? 1 : 0, new Uint8Array(m.ini || []))));
    } else if (m.type === "run") {
      const opts = new TextEncoder().encode(JSON.stringify(m.opts));
      result = check(json(ex, call(ex, "run", opts)));
    } else if (m.type === "render") {
      const n = ex.render(m.y0, m.y1);
      const pixels = new Uint8ClampedArray(n);
      pixels.set(new Uint8Array(ex.memory.buffer, ex.out_ptr(), n));
      const texts = JSON.parse(new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.txt_ptr(), ex.txt_len())) || "[]");
      self.postMessage({ id: m.id, result: { pixels, texts } }, [pixels.buffer]);
      return;
    }
    self.postMessage({ id: m.id, result });
  } catch (err) {
    self.postMessage({ id: m.id, error: err.message || String(err) });
  }
};
