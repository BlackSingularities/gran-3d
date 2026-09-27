// GRAŃ – serwer do samodzielnego hostowania.
// Serwuje aplikację, dane pobranych pasm (katalog data/) oraz API menedżera map,
// które pobiera i wypieka wybrane pasma w tle (skrypty z katalogu scripts/).
//
//   npm run dev          – tryb deweloperski (Vite jako middleware, HMR)
//   npm start            – produkcja (wymaga wcześniejszego `npm run build`)
//
// Zmienne środowiskowe:
//   PORT (5190), HOST (0.0.0.0), GRAN_DATA_DIR (./data), GRAN_CACHE_DIR (./.cache),
//   GRAN_ADMIN_TOKEN – jeśli ustawiony, pobieranie i usuwanie pasm wymaga tego hasła,
//   GRAN_KEEP_CACHE=1 – nie usuwaj dużych plików LiDAR z bufora po instalacji.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { CACHE_DIR, DATA_DIR, ROOT } from '../scripts/lib/common.mjs';
import { LIMITS, deriveRegion, slug, validate } from '../scripts/lib/area.mjs';

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

const catalog = () => JSON.parse(fs.readFileSync(path.join(ROOT, 'public', 'catalog.json'), 'utf8')).regions;

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

/** Starsze instalacje (pasma z katalogu) → region.json w nowym formacie. */
function migrate() {
  for (const e of fs.readdirSync(DATA_DIR, { withFileTypes: true })) {
    if (!e.isDirectory() || e.name.startsWith('.')) continue;
    const dir = path.join(DATA_DIR, e.name);
    if (fs.existsSync(path.join(dir, 'region.json')) || !fs.existsSync(path.join(dir, 'meta.json'))) continue;
    const preset = catalog().find((r) => r.id === e.name);
    if (!preset) continue;
    const hd = fs.existsSync(path.join(dir, 'tiles', 'index.json'));
    const def = { ...preset, quality: hd ? 'high' : 'normal', created: new Date().toISOString() };
    if (!hd) delete def.hd;
    fs.writeFileSync(path.join(dir, 'region.json'), JSON.stringify(def, null, 2));
  }
}

function listAreas() {
  const out = [];
  for (const e of fs.readdirSync(DATA_DIR, { withFileTypes: true })) {
    if (!e.isDirectory() || e.name.startsWith('.')) continue;
    const dir = path.join(DATA_DIR, e.name);
    const def = readJson(path.join(dir, 'region.json'));
    if (!def) continue;
    const installed = fs.existsSync(path.join(dir, 'meta.json')) && fs.existsSync(path.join(dir, 'trails.json')) && fs.existsSync(path.join(dir, 'install.json'));
    const info = readJson(path.join(dir, 'install.json')) ?? {};
    if (installed && !sizeCache.has(e.name)) sizeCache.set(e.name, dirSize(dir));
    out.push({ def, id: def.id, installed, level: info.level ?? null, date: info.date ?? def.created ?? null, bytes: installed ? sizeCache.get(e.name) : 0 });
  }
  return out.sort((a, b) => String(b.date).localeCompare(String(a.date)));
}

// ------------------------------------------------------------------ kolejka zadań
const queue = [];
let current = null;

function publicJob(j) {
  return j && { id: j.id, level: j.level, state: j.state, progress: j.progress, label: j.label, error: j.error ?? null, log: j.log.slice(-12) };
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
  const def = readJson(path.join(DATA_DIR, job.id, 'region.json'));
  const hd = job.level === 'hd' && def?.hd;
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
  lastFinished = job;
  current = null;
  void runNext();
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
      presets: catalog(),
      job: publicJob(current),
      queue: queue.map((j) => ({ id: j.id, level: j.level })),
      last: publicJob(lastFinished),
    });
  }
  if (req.method !== 'GET' && !authorized(req)) return send(res, 401, { error: 'Wymagane hasło administratora.' });
  // Zgodność z menedżerem gotowych pasm: zapisujemy preset jako obszar,
  // a następnie uruchamiamy tę samą kolejkę co dla obszarów użytkownika.
  if (url.pathname === '/api/install' && req.method === 'POST') {
    const { id, level } = await readBody(req);
    const preset = catalog().find((r) => r.id === id);
    if (!preset) return send(res, 404, { error: 'Nieznane pasmo.' });
    const dir = path.join(DATA_DIR, id);
    const existing = readJson(path.join(dir, 'region.json'));
    const high = level === 'hd' && !!preset.hd;
    const def = { ...preset, ...existing, quality: high ? 'high' : (existing?.quality ?? 'normal'), created: existing?.created ?? new Date().toISOString() };
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'region.json'), JSON.stringify(def, null, 2));
    enqueue(id, high ? 'hd' : 'base');
    return send(res, 202, { ok: true });
  }
  if (url.pathname === '/api/areas' && req.method === 'POST') {
    const { name, bbox, quality } = await readBody(req);
    const q = quality === 'high' ? 'high' : 'normal';
    const err = validate(bbox, q);
    if (err) return send(res, 400, { error: err });
    if (current || queue.length) return send(res, 409, { error: 'Trwa pobieranie innego obszaru – poczekaj na jego koniec.' });
    const clean = String(name ?? '').trim().slice(0, 60);
    const id = slug(clean);
    const def = deriveRegion({ id, name: clean, bbox, quality: q });
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

migrate();
server.listen(PORT, HOST, () => {
  const installed = listAreas().filter((r) => r.installed).map((r) => r.def.name);
  console.log(`\n  ▲ GRAŃ ${DEV ? '(dev)' : ''} → http://localhost:${PORT}`);
  console.log(`  dane: ${DATA_DIR}`);
  console.log(`  pobrane obszary: ${installed.length ? installed.join(', ') : 'brak – wybierz je na mapie w aplikacji'}`);
  if (TOKEN) console.log('  menedżer map chroniony hasłem (GRAN_ADMIN_TOKEN)');
  console.log('');
});
