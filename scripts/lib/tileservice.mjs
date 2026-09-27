// Kafle wysokości na żądanie dla całej Ziemi (model „Google Earth”).
//  - z ≤ 13 poza zasięgiem LiDAR: Terrarium (Mapzen / AWS) przepróbkowane do formatu silnika (259×259, ramka 1 px),
//  - z 13–15 w Polsce i Czechach: blok 6×6 km (kafel z12) wypiekany z LiDAR (GUGiK / ČÚZK) jednorazowo,
//    wszystkie jego kafle z13–z15 trafiają do bufora.
import fs from 'node:fs';
import path from 'node:path';
import { PNG } from 'pngjs';
import { CACHE_DIR } from './common.mjs';
import { COVERAGE, inRing } from './area.mjs';
import { LIDAR_SOURCES } from './alps.mjs';
import {
  bigFiles, coarseSampler, downsample, encodeTile, fetchCached, fuse, lidarCZ, lidarPL, px2lat, px2lon,
} from './terrain.mjs';

export const TILE_DIR = path.join(CACHE_DIR, 'dem');
export const BASE_MAX_Z = 13;
export const LIDAR_MAX_Z = 15;
const BLOCK_Z = 12;
const MARGIN = 64;

const tilePath = (z, x, y) => path.join(TILE_DIR, String(z), String(x), `${y}.png`);

// ------------------------------------------------------------------ Terrarium
const terraCache = new Map();
async function terrarium(z, x, y) {
  const n = 2 ** z;
  x = ((x % n) + n) % n;
  if (y < 0 || y >= n) return null;
  const key = `${z}/${x}/${y}`;
  let p = terraCache.get(key);
  if (!p) {
    p = (async () => {
      const png = PNG.sync.read(await fetchCached(`https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${x}/${y}.png`, `terrarium-${z}-${x}-${y}.png`));
      const out = new Float32Array(256 * 256);
      for (let i = 0; i < out.length; i++) {
        const o = i * 4;
        out[i] = png.data[o] * 256 + png.data[o + 1] + png.data[o + 2] / 256 - 32768;
      }
      return out;
    })();
    terraCache.set(key, p);
    if (terraCache.size > 400) terraCache.delete(terraCache.keys().next().value);
  }
  return p;
}

/** Kafel z modelu globalnego: próbki w narożnikach pikseli (konwencja silnika). */
async function baseTile(z, x, y) {
  const zs = Math.min(z, BASE_MAX_Z);
  const k = 2 ** (zs - z);
  const S = 259;
  const data = new Float32Array(S * S);
  // potrzebne kafle Terrarium (zwykle 1–4) – najpierw wszystkie, potem szybka pętla
  const X0 = (x * 256 - 1) * k - 0.5, X1 = (x * 256 + 258) * k - 0.5;
  const Y0 = (y * 256 - 1) * k - 0.5, Y1 = (y * 256 + 258) * k - 0.5;
  const tiles = new Map();
  for (let ty = Math.floor(Y0 / 256); ty <= Math.floor((Y1 + 1) / 256); ty++)
    for (let tx = Math.floor(X0 / 256); tx <= Math.floor((X1 + 1) / 256); tx++) tiles.set(`${tx},${ty}`, await terrarium(zs, tx, ty));
  const at = (px, py) => {
    const t = tiles.get(`${Math.floor(px / 256)},${Math.floor(py / 256)}`);
    return t ? t[(((py % 256) + 256) % 256) * 256 + (((px % 256) + 256) % 256)] : 0;
  };
  for (let j = 0; j < S; j++) {
    const Y = (y * 256 + j - 1) * k - 0.5;
    const y0 = Math.floor(Y), fy = Y - y0;
    for (let i = 0; i < S; i++) {
      const X = (x * 256 + i - 1) * k - 0.5;
      const x0 = Math.floor(X), fx = X - x0;
      const v = (at(x0, y0) * (1 - fx) + at(x0 + 1, y0) * fx) * (1 - fy) + (at(x0, y0 + 1) * (1 - fx) + at(x0 + 1, y0 + 1) * fx) * fy;
      // morze: poziom 0 (batymetria nie jest potrzebna)
      data[j * S + i] = Math.max(0, v);
    }
  }
  return encodeTile({ data, W: S, H: S, X0: x * 256 - 1, Y0: y * 256 - 1 }, x, y).buf;
}

// ------------------------------------------------------------------ zasięg LiDAR
const LIDAR = COVERAGE.filter((c) => c.lidar);
/** Czy kafel (z zapasem ~3 km) może mieć dane LiDAR. */
export function lidarNear(z, x, y) {
  const buf = 3000 / (40075016 / 2 ** z / 256 * Math.cos((px2lat((y + 0.5) * 256, z) * Math.PI) / 180));
  const w = px2lon(x * 256 - buf, z), e = px2lon(x * 256 + 256 + buf, z);
  const n = px2lat(y * 256 - buf, z), s = px2lat(y * 256 + 256 + buf, z);
  for (let j = 0; j <= 6; j++) for (let i = 0; i <= 6; i++) {
    const lon = w + ((e - w) * i) / 6, lat = s + ((n - s) * j) / 6;
    if (LIDAR.some((c) => inRing(lon, lat, c.ring))) return true;
  }
  return false;
}

// ------------------------------------------------------------------ blok LiDAR
const blocks = new Map();
// Kolejka bloków: kilka naraz, pierwszeństwo ma to, o co klient pytał najświeżej (miejsce pod kamerą);
// bloki, o które nikt nie pyta od WANT_TTL, wypadają z kolejki (kamera poleciała dalej).
const CONCURRENT = 4;
const WANT_TTL = 20_000;
let running = 0;
const waiting = new Map(); // key → { resolve, wanted }
const wanted = new Map(); // key → czas ostatniego zapytania
function slot(key) {
  if (running < CONCURRENT) { running++; return Promise.resolve(true); }
  return new Promise((resolve) => waiting.set(key, { resolve }));
}
function release() {
  running--;
  const now = Date.now();
  for (const [k, w] of waiting) {
    if (now - (wanted.get(k) ?? 0) > WANT_TTL) { waiting.delete(k); w.resolve(false); }
  }
  let best = null, bt = -1;
  for (const [k] of waiting) { const t = wanted.get(k) ?? 0; if (t > bt) { bt = t; best = k; } }
  if (best) { const w = waiting.get(best); waiting.delete(best); running++; w.resolve(true); }
}
/** Klient wciąż potrzebuje bloku (ponowne zapytanie o kafel). */
function want(key) { wanted.set(key, Date.now()); }

const blockMarker = (bx, by) => path.join(TILE_DIR, 'blocks', `${bx}-${by}.json`);

/** Wypieka blok z12 (6×6 km) z LiDAR: kafle z13–z15 do bufora. Zwraca true, gdy LiDAR pokrył blok. */
export function lidarBlock(bx, by) {
  const key = `${bx},${by}`;
  if (fs.existsSync(blockMarker(bx, by))) return Promise.resolve(JSON.parse(fs.readFileSync(blockMarker(bx, by), 'utf8')).lidar);
  let p = blocks.get(key);
  if (p) return p;
  const info = { key, lon: px2lon((bx + 0.5) * 256, BLOCK_Z), lat: px2lat((by + 0.5) * 256, BLOCK_Z), stage: 'w kolejce', sources: [], started: Date.now() };
  blockInfo.set(key, info);
  want(key);
  p = (async () => {
    if (!(await slot(key))) {
      // porzucony w kolejce – przy następnym zapytaniu zacznie od nowa
      blocks.delete(key);
      blockInfo.delete(key);
      return false;
    }
    info.stage = 'pobieranie';
    info.started = Date.now();
    try {
      const zf = LIDAR_MAX_Z;
      const k = 2 ** (zf - BLOCK_Z);
      const B = 256 * k;
      const bf = { zf, X0: bx * B - MARGIN, Y0: by * B - MARGIN, W: B + 2 * MARGIN + 1, H: B + 2 * MARGIN + 1 };
      bf.X1 = bf.X0 + bf.W;
      bf.Y1 = bf.Y0 + bf.H;
      const pseudo = { bbox: [px2lon(bf.X0, zf), px2lat(bf.Y1, zf), px2lon(bf.X1, zf), px2lat(bf.Y0, zf)] };
      const F = new Float32Array(bf.W * bf.H);
      const M = new Uint8Array(bf.W * bf.H);
      const lon = px2lon(bf.X0 + bf.W / 2, zf), lat = px2lat(bf.Y0 + bf.H / 2, zf);
      let covered = 0;
      // źródła: wszystkie kraje z LiDAR w bloku (z zapasem), najpierw kraj środka bloku
      const found = new Set();
      const own = LIDAR.find((c) => inRing(lon, lat, c.ring));
      if (own) found.add(own.lidar);
      for (let j = -1; j <= 7; j++) for (let i = -1; i <= 7; i++) {
        const plon = px2lon(bf.X0 + (bf.W * i) / 6, zf), plat = px2lat(bf.Y0 + (bf.H * j) / 6, zf);
        for (const c of LIDAR) if (inRing(plon, plat, c.ring)) found.add(c.lidar);
      }
      info.sources = [...found];
      for (const src of found) {
        info.stage = `pobieranie ${src.toUpperCase()}`;
        try {
          covered += await LIDAR_SOURCES[src].fn(pseudo, bf, F, M);
        } catch (e) {
          console.warn(`[gran] LiDAR ${src} blok ${key}: ${e.message}`);
        }
      }
      for (const f of bigFiles.splice(0)) fs.rmSync(path.join(CACHE_DIR, f), { force: true });
      if (covered) {
        info.stage = 'łączenie i zapis kafli';
        const coarse = await coarseSampler(null, bf);
        const C = new Float32Array(F.length);
        for (let j = 0; j < bf.H; j++) for (let i = 0; i < bf.W; i++) C[j * bf.W + i] = Math.max(0, coarse(bf.X0 + i, bf.Y0 + j));
        fuse(F, M, C, bf.W, bf.H, zf);
        let L = { data: F, W: bf.W, H: bf.H, X0: bf.X0, Y0: bf.Y0 };
        for (let z = zf; z > BLOCK_Z; z--) {
          const n = 2 ** (z - BLOCK_Z);
          for (let j = 0; j < n; j++) for (let i = 0; i < n; i++) {
            const tx = bx * n + i, ty = by * n + j;
            const file = tilePath(z, tx, ty);
            fs.mkdirSync(path.dirname(file), { recursive: true });
            fs.writeFileSync(file, encodeTile(L, tx, ty).buf);
          }
          const d = downsample(L.data, L.W, L.H);
          L = { data: d.data, W: d.W, H: d.H, X0: L.X0 / 2, Y0: L.Y0 / 2 };
        }
      }
      fs.mkdirSync(path.dirname(blockMarker(bx, by)), { recursive: true });
      fs.writeFileSync(blockMarker(bx, by), JSON.stringify({ lidar: covered > 0, date: new Date().toISOString() }));
      return covered > 0;
    } finally {
      release();
      blocks.delete(key);
      blockInfo.delete(key);
    }
  })();
  blocks.set(key, p);
  return p;
}

const blockInfo = new Map();
/** Stan bloków LiDAR w trakcie wypiekania (do paska postępu). */
export const lidarBusy = () => blocks.size;
export const lidarJobs = () =>
  [...blockInfo.values()].map((b) => ({ lon: +b.lon.toFixed(3), lat: +b.lat.toFixed(3), stage: b.stage, sources: b.sources, elapsed: (Date.now() - b.started) / 1000 }));

/**
 * Czy kafel czeka na wypiekanie bloku LiDAR (trwa to od kilku sekund do minut).
 * Wtedy uruchamiamy pracę w tle i od razu odpowiadamy „spróbuj później” – klient
 * w tym czasie pokazuje kafel nadrzędny i nie blokuje kolejki innych kafli.
 */
export function tileWaiting(z, x, y) {
  if (z <= BLOCK_Z || z > LIDAR_MAX_Z || fs.existsSync(tilePath(z, x, y))) return false;
  const s = 2 ** (z - BLOCK_Z);
  const bx = Math.floor(x / s), by = Math.floor(y / s);
  if (blocks.has(`${bx},${by}`)) want(`${bx},${by}`);
  else if (fs.existsSync(blockMarker(bx, by))) return false;
  if (!lidarNear(z, x, y)) return false;
  getTile(z, x, y).catch(() => {});
  return true;
}

const pending = new Map();
/** Ścieżka do pliku kafla (generuje go, jeśli trzeba). */
export async function getTile(z, x, y) {
  if (z < 0 || z > LIDAR_MAX_Z || x < 0 || y < 0 || x >= 2 ** z || y >= 2 ** z) return null;
  const file = tilePath(z, x, y);
  if (fs.existsSync(file)) return file;
  const key = `${z}/${x}/${y}`;
  if (pending.has(key)) return pending.get(key);
  const p = (async () => {
    try {
      if (z > BLOCK_Z && lidarNear(z, x, y)) {
        const s = 2 ** (z - BLOCK_Z);
        const bx = Math.floor(x / s), by = Math.floor(y / s);
        const lidar = await lidarBlock(bx, by);
        if (fs.existsSync(file)) return file;
        // blok porzucony w kolejce (nikt o niego nie pytał) – bez zapisu zgrubnego kafla na stałe
        if (!fs.existsSync(blockMarker(bx, by))) return 'retry';
        // blok częściowo usunięty przez sprzątanie bufora – wypiekamy go od nowa
        if (lidar) {
          fs.rmSync(blockMarker(bx, by), { force: true });
          await lidarBlock(bx, by);
          if (fs.existsSync(file)) return file;
        }
      }
      const buf = await baseTile(z, x, y);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, buf);
      return file;
    } finally {
      pending.delete(key);
    }
  })();
  pending.set(key, p);
  return p;
}

/**
 * Model podstawowy: kafle z4–z6 całej Ziemi pobierane raz, w tle (ok. 5 tys. kafli).
 * Dzięki nim widok globu i start w dowolnym miejscu są natychmiastowe.
 */
export async function prefetchBase(maxZ = 6, onDone = () => {}) {
  const jobs = [];
  for (let z = 4; z <= maxZ; z++) for (let y = 0; y < 2 ** z; y++) for (let x = 0; x < 2 ** z; x++) if (!fs.existsSync(tilePath(z, x, y))) jobs.push([z, x, y]);
  if (!jobs.length) return onDone(0);
  console.log(`[gran] model podstawowy: pobieram ${jobs.length} kafli w tle`);
  let done = 0;
  const worker = async () => {
    while (jobs.length) {
      const [z, x, y] = jobs.shift();
      try {
        await getTile(z, x, y);
      } catch {
        /* spróbujemy przy następnym starcie */
      }
      if (++done % 500 === 0) console.log(`[gran] model podstawowy: ${done} kafli`);
    }
  };
  await Promise.all(Array.from({ length: 4 }, worker));
  onDone(done);
}
