// GRAŃ – serwer do samodzielnego hostowania.
// Serwuje aplikację, zapamiętane kwadraty terenu (katalog data/) oraz API,
// które pobiera i wypieka wybrane przez użytkownika wycinki w tle.
//
//   npm run dev          – tryb deweloperski (Vite jako middleware, HMR)
//   npm start            – produkcja (wymaga wcześniejszego `npm run build`)
//
// Zmienne środowiskowe:
//   PORT (5190), HOST (0.0.0.0), GRAN_DATA_DIR (./data), GRAN_CACHE_DIR (./.cache),
//   GRAN_ADMIN_TOKEN – jeśli ustawiony, pobieranie i usuwanie kwadratów wymaga tego hasła,
//   GRAN_KEEP_CACHE=1 – nie usuwaj dużych plików LiDAR z bufora po instalacji.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { CACHE_DIR, DATA_DIR, ROOT } from '../scripts/lib/common.mjs';
import { LIMITS, areaKm2, deriveRegion, slug, validate } from '../scripts/lib/area.mjs';
import { getTile, lidarBusy, prefetchBase, tileWaiting } from '../scripts/lib/tileservice.mjs';
import { collectGarbage, touch } from '../scripts/lib/cachegc.mjs';
import { lat2px, lon2px, px2lat, px2lon, UA } from '../scripts/lib/terrain.mjs';

const DEV = process.argv.includes('--dev');
const PORT = Number(process.env.PORT || 5190);
const HOST = process.env.HOST || '0.0.0.0';
const TOKEN = process.env.GRAN_ADMIN_TOKEN || '';
const DIST = path.join(ROOT, 'dist');
fs.mkdirSync(DATA_DIR, { recursive: true });

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.bin': 'application/octet-stream',
  '.woff2': 'font/woff2',
};

// ------------------------------------------------------------------ obszary (katalogi w data/)
const sizeCache = new Map();
function dirSize(dir) {
  let total = 0;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    total += e.isDirectory() ? dirSize(p) : fs.statSync(p).size;
  }
  return total;
}

const readJson = (f) => {
  try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
};

function listAreas() {
  const out = [];
  for (const e of fs.readdirSync(DATA_DIR, { withFileTypes: true })) {
    if (!e.isDirectory() || e.name.startsWith('.')) continue;
    const dir = path.join(DATA_DIR, e.name);
    const def = readJson(path.join(dir, 'region.json'));
    if (!def?.custom || def.custom === 'sector') continue;
    const installed = fs.existsSync(path.join(dir, 'meta.json')) && fs.existsSync(path.join(dir, 'trails.json')) && fs.existsSync(path.join(dir, 'install.json'));
    const info = readJson(path.join(dir, 'install.json')) ?? {};
    if (installed && !sizeCache.has(e.name)) sizeCache.set(e.name, dirSize(dir));
    out.push({ def, id: def.id, installed, level: info.level ?? null, lidarZoom: info.lidarZoom ?? null, date: info.date ?? def.created ?? null, bytes: installed ? sizeCache.get(e.name) : 0 });
  }
  return out.sort((a, b) => String(b.date).localeCompare(String(a.date)));
}

// ------------------------------------------------------------------ kolejka zadań
const queue = [];
let current = null;

function publicJob(j) {
  if (!j) return null;
  const now = Date.now();
  const elapsed = j.started ? (now - j.started) / 1000 : 0;
  // szacowany czas do końca z tempa postępu (po rozgrzewce)
  const eta = j.state === 'running' && j.progress > 0.04 && elapsed > 8 ? (elapsed / j.progress) * (1 - j.progress) : null;
  return {
    id: j.id, level: j.level, state: j.state, progress: j.progress, label: j.label, error: j.error ?? null, log: j.log.slice(-12),
    name: j.name ?? '', res: j.res ?? null, km2: j.km2 ?? null, elapsed, eta, finished: j.finished ?? null,
  };
}

// ------------------------------------------------------------------ sektory (dane okolicy pod kamerą)
// Sektor = kafel z9 (~50×50 km): szlaki, punkty, miejscowości, pokrycie terenu i model do analiz.
const SECTOR_Z = 9;
export function sectorAt(lon, lat) {
  const x = Math.floor(lon2px(lon, SECTOR_Z) / 256), y = Math.floor(lat2px(lat, SECTOR_Z) / 256);
  const bbox = [px2lon(x * 256, SECTOR_Z), px2lat((y + 1) * 256, SECTOR_Z), px2lon((x + 1) * 256, SECTOR_Z), px2lat(y * 256, SECTOR_Z)];
  return { id: `s${SECTOR_Z}-${x}-${y}`, x, y, bbox };
}

function ensureSector(sec) {
  const dir = path.join(DATA_DIR, sec.id);
  const ready = fs.existsSync(path.join(dir, 'install.json'));
  lastSector = sec;
  if (ready) {
    touch(path.join(dir, 'install.json'));
    return { ready: true, def: readJson(path.join(dir, 'region.json')) };
  }
  if (current?.id === sec.id) {
    current.prefetch = false; // użytkownik tu patrzy – zadanie przestaje być „w tle”
    return { ready: false, job: publicJob(current) };
  }
  // sąsiad wypiekany w tle ustępuje miejsca sektorowi pod kamerą
  if (current?.prefetch && current.child) {
    current.cancelled = true;
    current.child.kill();
  }
  const queued = queue.find((j) => j.id === sec.id);
  if (!fs.existsSync(path.join(dir, 'region.json'))) {
    const def = { ...deriveRegion({ id: sec.id, name: '', bbox: sec.bbox.map((v) => +v.toFixed(6)), quality: 'normal' }), custom: 'sector' };
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'region.json'), JSON.stringify(def, null, 2));
  }
  if (queued) {
    queued.prefetch = false;
    queue.splice(queue.indexOf(queued), 1);
    queue.unshift(queued);
  } else {
    // nowszy sektor wypiera czekające (kamera poleciała dalej); własne kwadraty zostają w kolejce
    for (let i = queue.length - 1; i >= 0; i--) if (queue[i].sector && queue[i].id !== sec.id) queue.splice(i, 1);
    queue.unshift({ id: sec.id, level: 'base', sector: true, state: 'queued', progress: 0, label: 'W kolejce', log: [] });
    void runNext();
  }
  return { ready: false, queued: true };
}

let lastSector = null;
/** Gdy kolejka pusta: wypiekanie w tle czterech sąsiadów ostatnio oglądanego sektora (lot w bok bez czekania). */
function prefetchNeighbours() {
  if (current || queue.length || !lastSector) return;
  const { x, y } = lastSector;
  for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
    const n = 2 ** SECTOR_Z;
    const sx = (x + dx + n) % n, sy = y + dy;
    if (sy < 0 || sy >= n) continue;
    const bbox = [px2lon(sx * 256, SECTOR_Z), px2lat((sy + 1) * 256, SECTOR_Z), px2lon((sx + 1) * 256, SECTOR_Z), px2lat(sy * 256, SECTOR_Z)];
    const id = `s${SECTOR_Z}-${sx}-${sy}`;
    const dir = path.join(DATA_DIR, id);
    if (fs.existsSync(path.join(dir, 'install.json'))) continue;
    const def = { ...deriveRegion({ id, name: '', bbox: bbox.map((v) => +v.toFixed(6)), quality: 'normal' }), custom: 'sector' };
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'region.json'), JSON.stringify(def, null, 2));
    queue.push({ id, level: 'base', sector: true, prefetch: true, state: 'queued', progress: 0, label: 'W kolejce', log: [] });
  }
  void runNext();
}

/** Sprzątanie bufora do budżetu (1 GB) – po zadaniach i co 10 minut. */
function gc() {
  const r = collectGarbage();
  if (r?.removed) console.log(`[gran] bufor: ${(r.before / 2 ** 20).toFixed(0)} → ${(r.after / 2 ** 20).toFixed(0)} MB, usunięto ${r.removed} elementów`);
}
setInterval(gc, 10 * 60 * 1000).unref();

// ------------------------------------------------------------------ wyszukiwanie miejsc (Nominatim, 1 zapytanie/s)
const geoCache = new Map();
let geoLast = 0;
async function geocode(q) {
  const key = q.toLowerCase();
  if (geoCache.has(key)) return geoCache.get(key);
  const wait = Math.max(0, geoLast + 1100 - Date.now());
  geoLast = Date.now() + wait;
  if (wait) await new Promise((r) => setTimeout(r, wait));
  const r = await fetch(`https://nominatim.openstreetmap.org/search?format=jsonv2&limit=8&accept-language=pl&q=${encodeURIComponent(q)}`, { headers: { 'User-Agent': UA } });
  if (!r.ok) throw new Error(`Nominatim ${r.status}`);
  const j = (await r.json()).map((p) => ({
    name: p.name || p.display_name.split(',')[0],
    label: p.display_name,
    type: p.type,
    category: p.category,
    lon: +p.lon,
    lat: +p.lat,
    bbox: p.boundingbox ? [+p.boundingbox[2], +p.boundingbox[0], +p.boundingbox[3], +p.boundingbox[1]] : null,
  }));
  geoCache.set(key, j);
  if (geoCache.size > 500) geoCache.delete(geoCache.keys().next().value);
  return j;
}

function enqueue(id, level) {
  if (current?.id === id || queue.some((j) => j.id === id)) return false;
  queue.push({ id, level, state: 'queued', progress: 0, label: 'W kolejce', log: [] });
  void runNext();
  return true;
}

function runStep(job, script, from, to) {
  return new Promise((resolve, reject) => {
    const args = [path.join(ROOT, 'scripts', script), job.id];
    if (script === 'bake-hd.mjs' && process.env.GRAN_KEEP_CACHE !== '1') args.push('--clean-cache');
    const child = spawn(process.execPath, args, {
      cwd: ROOT,
      env: { ...process.env, GRAN_DATA_DIR: DATA_DIR, GRAN_CACHE_DIR: CACHE_DIR },
    });
    job.child = child;
    let buf = '';
    const onData = (d) => {
      buf += d.toString();
      const lines = buf.split(/\r?\n|\r/);
      buf = lines.pop() ?? '';
      for (const line of lines) {
        const m = line.match(/^@@progress ([\d.]+) (.*)$/);
        if (m) {
          job.progress = from + (to - from) * Number(m[1]);
          job.label = m[2];
        } else if (line.trim()) {
          job.log.push(line.trim());
          if (job.log.length > 60) job.log.shift();
        }
      }
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    child.on('close', (code) => {
      job.child = null;
      if (job.cancelled) reject(new Error('Anulowano'));
      else if (code === 0) resolve();
      else reject(new Error(job.log.slice(-1)[0] || `Kod wyjścia ${code}`));
    });
  });
}

async function runNext() {
  if (current || !queue.length) return;
  current = queue.shift();
  const job = current;
  job.state = 'running';
  job.started = Date.now();
  const def = readJson(path.join(DATA_DIR, job.id, 'region.json'));
  const hd = job.level === 'hd' && def?.hd;
  job.name = def?.name || (def?.custom === 'sector' ? 'Dane okolicy' : '');
  job.res = hd ? (def.hd.zoom >= 15 ? 3 : def.hd.zoom >= 14 ? 6 : 12) : null;
  job.km2 = def?.bbox ? Math.round(areaKm2(def.bbox)) : null;
  try {
    await runStep(job, 'bake.mjs', 0, hd ? 0.3 : 1);
    if (hd) await runStep(job, 'bake-hd.mjs', 0.3, 1);
    job.state = 'done';
    job.progress = 1;
    job.label = 'Gotowe';
  } catch (e) {
    job.state = 'error';
    job.error = e.message;
    // nieudany lub anulowany nowy obszar – bez danych nie ma sensu go trzymać
    if (!fs.existsSync(path.join(DATA_DIR, job.id, 'install.json'))) fs.rmSync(path.join(DATA_DIR, job.id), { recursive: true, force: true });
    console.error(`[gran] ${job.id}: ${e.message}`);
  }
  sizeCache.delete(job.id);
  job.finished = Date.now();
  if (!job.prefetch) lastFinished = job;
  current = null;
  gc();
  if (queue.length) void runNext();
  else setTimeout(prefetchNeighbours, 3000);
}
let lastFinished = null;

// ------------------------------------------------------------------ HTTP
function send(res, code, body, type = 'application/json; charset=utf-8') {
  res.writeHead(code, { 'Content-Type': type, 'Cache-Control': 'no-store' });
  res.end(typeof body === 'string' ? body : JSON.stringify(body));
}

function readBody(req) {
  return new Promise((resolve) => {
    let s = '';
    req.on('data', (d) => (s += d));
    req.on('end', () => {
      try { resolve(s ? JSON.parse(s) : {}); } catch { resolve({}); }
    });
  });
}

function authorized(req) {
  if (!TOKEN) return true;
  return (req.headers.authorization || '') === `Bearer ${TOKEN}`;
}

async function api(req, res, url) {
  if (url.pathname === '/api/status' && req.method === 'GET') {
    return send(res, 200, {
      admin: !!TOKEN,
      limits: LIMITS,
      regions: listAreas(),
      job: current?.prefetch ? null : publicJob(current),
      queue: queue.map((j) => ({ id: j.id, level: j.level })),
      last: publicJob(lastFinished),
      lidarBlocks: lidarBusy(),
    });
  }
  if (url.pathname === '/api/sector' && req.method === 'GET') {
    const lon = Number(url.searchParams.get('lon')), lat = Number(url.searchParams.get('lat'));
    if (!Number.isFinite(lon) || !Number.isFinite(lat) || Math.abs(lat) > 84) return send(res, 400, { error: 'Złe współrzędne.' });
    const sec = sectorAt(lon, lat);
    return send(res, 200, { id: sec.id, bbox: sec.bbox, ...ensureSector(sec) });
  }
  if (url.pathname === '/api/geocode' && req.method === 'GET') {
    const q = (url.searchParams.get('q') ?? '').trim().slice(0, 120);
    if (q.length < 2) return send(res, 200, []);
    try {
      return send(res, 200, await geocode(q));
    } catch (e) {
      return send(res, 502, { error: e.message });
    }
  }
  if (req.method !== 'GET' && !authorized(req)) return send(res, 401, { error: 'Wymagane hasło administratora.' });
  if (url.pathname === '/api/areas' && req.method === 'POST') {
    const { name, bbox, quality, res: lidarRes } = await readBody(req);
    const q = quality === 'high' ? 'high' : 'normal';
    const err = validate(bbox, q);
    if (err) return send(res, 400, { error: err });
    if (current || queue.length) return send(res, 409, { error: 'Trwa pobieranie innego obszaru – poczekaj na jego koniec.' });
    const clean = String(name ?? '').trim().slice(0, 60);
    const id = slug(clean);
    const r = [3, 6, 12].includes(Number(lidarRes)) ? Number(lidarRes) : 3;
    const def = deriveRegion({ id, name: clean, bbox, quality: q, res: r });
    fs.mkdirSync(path.join(DATA_DIR, id), { recursive: true });
    fs.writeFileSync(path.join(DATA_DIR, id, 'region.json'), JSON.stringify(def, null, 2));
    enqueue(id, def.hd ? 'hd' : 'base');
    return send(res, 202, { ok: true, id });
  }
  if (url.pathname === '/api/cancel' && req.method === 'POST') {
    const { id } = await readBody(req);
    const qi = queue.findIndex((j) => j.id === id);
    if (qi >= 0) {
      queue.splice(qi, 1);
      if (!fs.existsSync(path.join(DATA_DIR, id, 'install.json'))) fs.rmSync(path.join(DATA_DIR, id), { recursive: true, force: true });
    }
    if (current?.id === id && current.child) {
      current.cancelled = true;
      current.child.kill();
    }
    return send(res, 200, { ok: true });
  }
  const m = url.pathname.match(/^\/api\/regions\/([a-z0-9-]+)$/);
  if (m && req.method === 'PATCH') {
    const f = path.join(DATA_DIR, m[1], 'region.json');
    const def = readJson(f);
    if (!def) return send(res, 404, { error: 'Nie znaleziono obszaru.' });
    const { name } = await readBody(req);
    if (typeof name === 'string' && name.trim()) def.name = name.trim().slice(0, 60);
    fs.writeFileSync(f, JSON.stringify(def, null, 2));
    return send(res, 200, { ok: true });
  }
  if (m && req.method === 'DELETE') {
    if (current?.id === m[1]) return send(res, 409, { error: 'Obszar jest właśnie pobierany.' });
    fs.rmSync(path.join(DATA_DIR, m[1]), { recursive: true, force: true });
    sizeCache.delete(m[1]);
    return send(res, 200, { ok: true });
  }
  return send(res, 404, { error: 'Nie znaleziono.' });
}

function serveFile(res, file, cache) {
  fs.stat(file, (err, st) => {
    if (err || !st.isFile()) return send(res, 404, 'Nie znaleziono', 'text/plain; charset=utf-8');
    res.writeHead(200, {
      'Content-Type': MIME[path.extname(file).toLowerCase()] ?? 'application/octet-stream',
      'Content-Length': st.size,
      'Cache-Control': cache,
    });
    fs.createReadStream(file).pipe(res);
  });
}

/** Bezpieczne złożenie ścieżki (bez wychodzenia poza katalog bazowy). */
function safeJoin(base, rel) {
  const p = path.resolve(base, '.' + path.sep + decodeURIComponent(rel));
  return p.startsWith(path.resolve(base) + path.sep) ? p : null;
}

let vite = null;
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url || '/', 'http://localhost');
  try {
    if (url.pathname.startsWith('/api/')) return await api(req, res, url);
    const tm = url.pathname.match(/^\/tiles\/dem\/(\d+)\/(\d+)\/(\d+)\.png$/);
    if (tm) {
      if (tileWaiting(+tm[1], +tm[2], +tm[3])) {
        res.writeHead(202, { 'Retry-After': '3', 'Cache-Control': 'no-store' });
        return res.end();
      }
      const file = await getTile(+tm[1], +tm[2], +tm[3]);
      if (!file) return send(res, 404, 'Brak kafla', 'text/plain; charset=utf-8');
      if (+tm[1] > 12) touch(file);
      return serveFile(res, file, 'public, max-age=604800');
    }
    if (url.pathname.startsWith('/data/')) {
      const file = safeJoin(DATA_DIR, url.pathname.slice(6));
      if (!file) return send(res, 400, 'Zła ścieżka', 'text/plain; charset=utf-8');
      const immutable = /\/tiles\/\d+\//.test(url.pathname);
      return serveFile(res, file, immutable ? 'public, max-age=86400' : 'no-cache');
    }
    if (vite) return vite.middlewares(req, res);
    let file = safeJoin(DIST, url.pathname === '/' ? 'index.html' : url.pathname.slice(1));
    if (!file || !fs.existsSync(file)) file = path.join(DIST, 'index.html');
    return serveFile(res, file, file.endsWith('index.html') ? 'no-cache' : 'public, max-age=31536000, immutable');
  } catch (e) {
    console.error(e);
    send(res, 500, { error: 'Błąd serwera' });
  }
});

if (DEV) {
  const { createServer } = await import('vite');
  vite = await createServer({ root: ROOT, appType: 'spa', server: { middlewareMode: true, hmr: { server } } });
} else if (!fs.existsSync(path.join(DIST, 'index.html'))) {
  console.error('Brak zbudowanej aplikacji – uruchom najpierw `npm run build`.');
  process.exit(1);
}

server.listen(PORT, HOST, () => {
  const installed = listAreas().filter((r) => r.installed).map((r) => r.def.name);
  console.log(`\n  ▲ GRAŃ ${DEV ? '(dev)' : ''} → http://localhost:${PORT}`);
  console.log(`  dane: ${DATA_DIR}`);
  console.log(`  pobrane kwadraty: ${installed.length ? installed.join(', ') : 'brak – zaznacz pierwszy na mapie w aplikacji'}`);
  if (TOKEN) console.log('  menedżer map chroniony hasłem (GRAN_ADMIN_TOKEN)');
  setTimeout(gc, 5000);
  void prefetchBase(6);
  console.log('');
});
