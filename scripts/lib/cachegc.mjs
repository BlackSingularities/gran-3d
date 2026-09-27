// Sprzątanie bufora serwera: dane podstawowe zostają na stałe, reszta ma budżet (domyślnie 1 GB)
// i przy przekroczeniu usuwane są najdawniej używane elementy.
//
//  na stałe:   kafle wysokości z ≤ 12 (model globalny), obszary zaznaczone przez użytkownika (data/obszar-*, pasma)
//  w budżecie: bloki LiDAR (kafle z13–z15 + znacznik), kafle z13 poza LiDAR, sektory (data/s9-*),
//              surowe pobrania w .cache (Terrarium, OSM, GeoTIFF)
import fs from 'node:fs';
import path from 'node:path';
import { CACHE_DIR, DATA_DIR } from './common.mjs';

export const CACHE_BUDGET = Number(process.env.GRAN_CACHE_MB || 1024) * 1024 * 1024;
const DEM = path.join(CACHE_DIR, 'dem');
/** elementów użytych w ostatnich minutach nie ruszamy (mogą być właśnie czytane) */
const GRACE = 10 * 60 * 1000;

const statOr = (p) => {
  try {
    return fs.statSync(p);
  } catch {
    return null;
  }
};

function dirSize(dir) {
  let n = 0;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    n += e.isDirectory() ? dirSize(p) : statOr(p)?.size ?? 0;
  }
  return n;
}

/** Oznaczenie użycia (czas modyfikacji jako „ostatnio używane”); tanie, wołane przy serwowaniu. */
const touched = new Map();
export function touch(p) {
  const now = Date.now();
  if ((touched.get(p) ?? 0) > now - 60_000) return;
  touched.set(p, now);
  if (touched.size > 20000) touched.clear();
  try {
    const t = new Date(now);
    fs.utimesSync(p, t, t);
  } catch {
    /* plik mógł zniknąć */
  }
}

/** Lista elementów w budżecie: { size, used, remove() }. */
function collect() {
  const items = [];
  // surowe pobrania
  for (const e of fs.readdirSync(CACHE_DIR, { withFileTypes: true })) {
    if (!e.isFile()) continue;
    const p = path.join(CACHE_DIR, e.name);
    const st = statOr(p);
    if (st) items.push({ size: st.size, used: st.mtimeMs, remove: () => fs.rmSync(p, { force: true }) });
  }
  // bloki LiDAR: znacznik + kafle z13–z15 jako jedna całość
  const bdir = path.join(DEM, 'blocks');
  const inBlock = new Set();
  if (fs.existsSync(bdir)) {
    for (const f of fs.readdirSync(bdir)) {
      const m = f.match(/^(\d+)-(\d+)\.json$/);
      if (!m) continue;
      const bx = +m[1], by = +m[2];
      const files = [path.join(bdir, f)];
      for (let z = 13; z <= 15; z++) {
        const n = 2 ** (z - 12);
        for (let j = 0; j < n; j++) for (let i = 0; i < n; i++) {
          const p = path.join(DEM, String(z), String(bx * n + i), `${by * n + j}.png`);
          inBlock.add(p);
          files.push(p);
        }
      }
      let size = 0, used = 0;
      for (const p of files) {
        const st = statOr(p);
        if (st) { size += st.size; used = Math.max(used, st.mtimeMs); }
      }
      items.push({ size, used, remove: () => files.forEach((p) => fs.rmSync(p, { force: true })) });
    }
  }
  // pozostałe kafle z ≥ 13 (spoza bloków)
  for (let z = 13; z <= 15; z++) {
    const zd = path.join(DEM, String(z));
    if (!fs.existsSync(zd)) continue;
    for (const x of fs.readdirSync(zd)) {
      const xd = path.join(zd, x);
      for (const f of fs.readdirSync(xd)) {
        const p = path.join(xd, f);
        if (inBlock.has(p)) continue;
        const st = statOr(p);
        if (st) items.push({ size: st.size, used: st.mtimeMs, remove: () => fs.rmSync(p, { force: true }) });
      }
    }
  }
  // sektory (dane okolicy)
  if (fs.existsSync(DATA_DIR)) {
    for (const d of fs.readdirSync(DATA_DIR)) {
      if (!/^s\d+-\d+-\d+$/.test(d)) continue;
      const dir = path.join(DATA_DIR, d);
      const st = statOr(path.join(dir, 'install.json'));
      if (!st) continue; // w trakcie wypiekania
      items.push({ size: dirSize(dir), used: st.mtimeMs, remove: () => fs.rmSync(dir, { recursive: true, force: true }) });
    }
  }
  return items;
}

let running = false;
/** Usuwa najdawniej używane elementy, aż bufor zmieści się w budżecie. Zwraca statystykę. */
export function collectGarbage(budget = CACHE_BUDGET) {
  if (running) return null;
  running = true;
  try {
    const items = collect();
    let total = items.reduce((s, it) => s + it.size, 0);
    const before = total;
    let removed = 0;
    if (total > budget) {
      const now = Date.now();
      items.sort((a, b) => a.used - b.used);
      for (const it of items) {
        if (total <= budget * 0.9) break; // zapas, żeby nie sprzątać po każdym pliku
        if (now - it.used < GRACE) continue;
        try {
          it.remove();
          total -= it.size;
          removed++;
        } catch {
          /* zajęty – następnym razem */
        }
      }
    }
    return { before, after: total, removed };
  } finally {
    running = false;
  }
}
