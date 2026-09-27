// Wypiekanie terenu wysokiej rozdzielczości z danych LiDAR:
//   PL – GUGiK NMT (siatka 1 m, WCS w EPSG:2180, przeliczane lokalnie do Web Mercatora),
//   CZ – ČÚZK DMR 5G (ArcGIS ImageServer bezpośrednio w Web Mercatorze),
//   reszta (SK, UA) – kafle Terrarium jako tło, z łagodnym przejściem na granicy pokrycia.
// Wynik: piramida kafli public/data/<region>/tiles/{z}/{x}/{y}.png (259×259, wysokość w RGB),
// indeks tiles/index.json, nowy dem.bin (zoom+1) oraz wysokości szlaków i punktów z modelu HD.
//
//   node scripts/bake-hd.mjs            – wszystkie regiony
//   node scripts/bake-hd.mjs tatry      – jeden region
import fs from 'node:fs';
import path from 'node:path';
import { PNG } from 'pngjs';
import { fromArrayBuffer } from 'geotiff';
import { toPuwg } from './lib/puwg.mjs';
import { COVERAGE, areaKm2, inRing } from './lib/area.mjs';

import { CACHE_DIR as CACHE, DATA_DIR as OUT, progress, selectRegions } from './lib/common.mjs';
const UA = 'gran-trail-atlas/1.0 (https://github.com/BlackSingularities/gran-3d)';
const regions = selectRegions(process.argv);
const CLEAN = process.argv.includes('--clean-cache') || process.env.GRAN_CLEAN_CACHE === '1';
const bigFiles = [];
fs.mkdirSync(CACHE, { recursive: true });

const lon2px = (lon, z) => ((lon + 180) / 360) * 256 * 2 ** z;
const lat2px = (lat, z) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 256 * 2 ** z;
};
const px2lon = (px, z) => (px / (256 * 2 ** z)) * 360 - 180;
const px2lat = (py, z) => (Math.atan(Math.sinh(Math.PI - (2 * Math.PI * py) / (256 * 2 ** z))) * 180) / Math.PI;
const WM = 20037508.342789244;
const px2mx = (px, z) => (px / (256 * 2 ** z)) * 2 * WM - WM;
const py2my = (py, z) => WM - (py / (256 * 2 ** z)) * 2 * WM;
const log = (...a) => console.log('  ', ...a);

async function fetchCached(url, file) {
  const p = path.join(CACHE, file);
  if (fs.existsSync(p) && fs.statSync(p).size > 0) return fs.readFileSync(p);
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const res = await fetch(url, { headers: { 'User-Agent': UA } });
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      const buf = Buffer.from(await res.arrayBuffer());
      fs.writeFileSync(p, buf);
      return buf;
    } catch (e) {
      console.warn(`  ! ${file}: ${e.message} (próba ${attempt + 1})`);
      await new Promise((r) => setTimeout(r, 4000 * (attempt + 1)));
    }
  }
  throw new Error(`Nie udało się pobrać ${url}`);
}

async function readTiff(buf) {
  const tiff = await fromArrayBuffer(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.length));
  const im = await tiff.getImage();
  const [data] = await im.readRasters();
  return { w: im.getWidth(), h: im.getHeight(), data, bbox: im.getBoundingBox() };
}

async function pool(jobs, n, fn) {
  let i = 0;
  await Promise.all(Array.from({ length: n }, async () => {
    while (i < jobs.length) await fn(jobs[i++]);
  }));
}

// ---------------------------------------------------------------- tło: Terrarium z13
async function coarseSampler(region, frame) {
  const z = 13;
  const k = 2 ** (z - frame.zf);
  const x0 = Math.floor(frame.X0 * k) - 2, x1 = Math.ceil(frame.X1 * k) + 2;
  const y0 = Math.floor(frame.Y0 * k) - 2, y1 = Math.ceil(frame.Y1 * k) + 2;
  const W = x1 - x0, H = y1 - y0;
  const g = new Float32Array(W * H);
  const jobs = [];
  for (let ty = Math.floor(y0 / 256); ty <= Math.floor((y1 - 1) / 256); ty++)
    for (let tx = Math.floor(x0 / 256); tx <= Math.floor((x1 - 1) / 256); tx++) jobs.push([tx, ty]);
  await pool(jobs, 8, async ([tx, ty]) => {
    const png = PNG.sync.read(await fetchCached(`https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${tx}/${ty}.png`, `terrarium-${z}-${tx}-${ty}.png`));
    for (let y = 0; y < 256; y++) {
      const gy = ty * 256 + y - y0;
      if (gy < 0 || gy >= H) continue;
      for (let x = 0; x < 256; x++) {
        const gx = tx * 256 + x - x0;
        if (gx < 0 || gx >= W) continue;
        const i = (y * 256 + x) * 4;
        g[gy * W + gx] = Math.max(0, png.data[i] * 256 + png.data[i + 1] + png.data[i + 2] / 256 - 32768);
      }
    }
  });
  log(`tło Terrarium z${z}: ${jobs.length} kafli`);
  // próbka w punkcie siatki zf (konwencja krawędziowa) → środek piksela z13
  return (X, Y) => {
    let x = X * k - 0.5 - x0, y = Y * k - 0.5 - y0;
    x = Math.max(0, Math.min(W - 1.001, x));
    y = Math.max(0, Math.min(H - 1.001, y));
    const xi = x | 0, yi = y | 0, fx = x - xi, fy = y - yi, i = yi * W + xi;
    return (g[i] * (1 - fx) + g[i + 1] * fx) * (1 - fy) + (g[i + W] * (1 - fx) + g[i + W + 1] * fx) * fy;
  };
}

// ---------------------------------------------------------------- PL: GUGiK NMT (EPSG:2180)
async function lidarPL(region, frame, F, M) {
  const res = frame.zf >= 15 ? 3 : frame.zf >= 14 ? 6 : 12;
  // zasięg regionu w PUWG-1992
  let e0 = Infinity, e1 = -Infinity, n0 = Infinity, n1 = -Infinity;
  for (let i = 0; i <= 20; i++) for (let j = 0; j <= 20; j++) {
    const lon = region.bbox[0] + ((region.bbox[2] - region.bbox[0]) * i) / 20;
    const lat = region.bbox[1] + ((region.bbox[3] - region.bbox[1]) * j) / 20;
    const [e, n] = toPuwg(lon, lat);
    e0 = Math.min(e0, e); e1 = Math.max(e1, e); n0 = Math.min(n0, n); n1 = Math.max(n1, n);
  }
  e0 = Math.floor((e0 - 200) / res) * res; n0 = Math.floor((n0 - 200) / res) * res;
  e1 = Math.ceil((e1 + 200) / res) * res; n1 = Math.ceil((n1 + 200) / res) * res;
  const GW = Math.round((e1 - e0) / res), GH = Math.round((n1 - n0) / res);
  const grid = new Float32Array(GW * GH); // wiersz 0 = północ
  // serwer składa odpowiedź tylko z ograniczonej liczby arkuszy – fragmenty ≤ 5 km dają pełne pokrycie
  const CH = Math.round(5000 / res);
  const jobs = [];
  for (let cy = 0; cy < GH; cy += CH) for (let cx = 0; cx < GW; cx += CH) jobs.push([cx, cy]);
  let done = 0;
  await pool(jobs, 4, async ([cx, cy]) => {
    const w = Math.min(CH, GW - cx), h = Math.min(CH, GH - cy);
    const be0 = e0 + cx * res, be1 = be0 + w * res;
    const bn1 = n1 - cy * res, bn0 = bn1 - h * res;
    const url = `https://mapy.geoportal.gov.pl/wss/service/PZGIK/NMT/GRID1/WCS/DigitalTerrainModelFormatTIFF?SERVICE=WCS&VERSION=1.0.0&REQUEST=GetCoverage&COVERAGE=DTM_PL-KRON86-NH_TIFF&CRS=EPSG:2180&FORMAT=image/tiff&INTERPOLATION=bilinear&BBOX=${be0},${bn0},${be1},${bn1}&WIDTH=${w}&HEIGHT=${h}`;
    const fname = `nmt-pl-${res}-${be0}-${bn0}-${w}x${h}.tif`;
    bigFiles.push(fname);
    const t = await readTiff(await fetchCached(url, fname));
    for (let y = 0; y < h; y++) {
      const src = t.data.subarray(y * t.w, y * t.w + w);
      grid.set(src, (cy + y) * GW + cx);
    }
    done++;
  });
  // przeliczenie do siatki Web Mercator (próbki w narożnikach pikseli zf)
  let covered = 0;
  for (let j = 0; j < frame.H; j++) {
    const lat = px2lat(frame.Y0 + j, frame.zf);
    for (let i = 0; i < frame.W; i++) {
      const idx = j * frame.W + i;
      if (M[idx]) continue;
      const lon = px2lon(frame.X0 + i, frame.zf);
      const [E, N] = toPuwg(lon, lat);
      const gx = (E - e0) / res - 0.5, gy = (n1 - N) / res - 0.5;
      const xi = Math.floor(gx), yi = Math.floor(gy);
      if (xi < 0 || yi < 0 || xi >= GW - 1 || yi >= GH - 1) continue;
      const fx = gx - xi, fy = gy - yi, g0 = yi * GW + xi;
      const a = grid[g0], b = grid[g0 + 1], c = grid[g0 + GW], d = grid[g0 + GW + 1];
      if (a <= 1 || b <= 1 || c <= 1 || d <= 1) continue; // 0 = brak danych (poza Polską)
      F[idx] = (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy;
      M[idx] = 1;
      covered++;
    }
  }
  return covered;
}

// ---------------------------------------------------------------- CZ: ČÚZK DMR 5G (Web Mercator)
async function lidarCZ(region, frame, F, M) {
  const CH = 4000;
  const jobs = [];
  for (let cy = 0; cy < frame.H; cy += CH) for (let cx = 0; cx < frame.W; cx += CH) jobs.push([cx, cy]);
  let covered = 0, done = 0;
  await pool(jobs, 3, async ([cx, cy]) => {
    const w = Math.min(CH, frame.W - cx), h = Math.min(CH, frame.H - cy);
    // środki pikseli obrazu = narożniki pikseli zf
    const X = frame.X0 + cx - 0.5, Y = frame.Y0 + cy - 0.5;
    const bb = [px2mx(X, frame.zf), py2my(Y + h, frame.zf), px2mx(X + w, frame.zf), py2my(Y, frame.zf)].map((v) => v.toFixed(3)).join(',');
    const url = `https://ags.cuzk.gov.cz/arcgis/rest/services/3D/dmr5g_wm/ImageServer/exportImage?bbox=${bb}&bboxSR=3857&imageSR=3857&size=${w},${h}&format=tiff&pixelType=F32&interpolation=RSP_BilinearInterpolation&noData=-9999&f=image`;
    const fname = `dmr5g-${frame.zf}-${X}-${Y}-${w}x${h}.tif`;
    bigFiles.push(fname);
    const t = await readTiff(await fetchCached(url, fname));
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const v = t.data[y * t.w + x];
      const idx = (cy + y) * frame.W + cx + x;
      if (v > -100 && !M[idx]) { F[idx] = v; M[idx] = 1; covered++; }
    }
    done++;
  });
  return covered;
}

// ---------------------------------------------------------------- łączenie
function boxBlur(src, W, H, r) {
  const tmp = new Float32Array(W * H), out = new Float32Array(W * H);
  const n = 2 * r + 1;
  for (let y = 0; y < H; y++) {
    let s = 0;
    const o = y * W;
    for (let x = -r; x <= r; x++) s += src[o + Math.min(W - 1, Math.max(0, x))];
    for (let x = 0; x < W; x++) {
      tmp[o + x] = s / n;
      s += src[o + Math.min(W - 1, x + r + 1)] - src[o + Math.max(0, x - r)];
    }
  }
  for (let x = 0; x < W; x++) {
    let s = 0;
    for (let y = -r; y <= r; y++) s += tmp[Math.min(H - 1, Math.max(0, y)) * W + x];
    for (let y = 0; y < H; y++) {
      out[y * W + x] = s / n;
      s += tmp[Math.min(H - 1, y + r + 1) * W + x] - tmp[Math.max(0, y - r) * W + x];
    }
  }
  return out;
}

function downsample(src, W, H) {
  const W2 = (W - 1) / 2 + 1, H2 = (H - 1) / 2 + 1;
  const out = new Float32Array(W2 * H2);
  const wt = [0.25, 0.5, 0.25];
  for (let y = 0; y < H2; y++) for (let x = 0; x < W2; x++) {
    let s = 0;
    for (let b = -1; b <= 1; b++) {
      const yy = Math.min(H - 1, Math.max(0, 2 * y + b));
      for (let a = -1; a <= 1; a++) {
        const xx = Math.min(W - 1, Math.max(0, 2 * x + a));
        s += src[yy * W + xx] * wt[a + 1] * wt[b + 1];
      }
    }
    out[y * W2 + x] = s;
  }
  return { data: out, W: W2, H: H2 };
}

function maxpool(src, W, H) {
  const W2 = (W - 1) / 2 + 1, H2 = (H - 1) / 2 + 1;
  const out = new Uint8Array(W2 * H2);
  for (let y = 0; y < H2; y++) for (let x = 0; x < W2; x++) {
    let m = 0;
    for (let b = -1; b <= 1 && !m; b++) for (let a = -1; a <= 1 && !m; a++) {
      const yy = 2 * y + b, xx = 2 * x + a;
      if (yy >= 0 && xx >= 0 && yy < H && xx < W && src[yy * W + xx]) m = 1;
    }
    out[y * W2 + x] = m;
  }
  return out;
}

function encodeTile(level, tx, ty) {
  const S = 259;
  const png = new PNG({ width: S, height: S, colorType: 2 });
  png.data = Buffer.alloc(S * S * 4);
  let mn = Infinity, mx = -Infinity;
  for (let j = 0; j < S; j++) {
    const gy = Math.min(level.H - 1, Math.max(0, ty * 256 + j - 1 - level.Y0));
    for (let i = 0; i < S; i++) {
      const gx = Math.min(level.W - 1, Math.max(0, tx * 256 + i - 1 - level.X0));
      const h = level.data[gy * level.W + gx];
      if (i > 0 && j > 0 && i < S - 1 && j < S - 1) { if (h < mn) mn = h; if (h > mx) mx = h; }
      const v = Math.max(0, Math.min(16777215, Math.round((h + 1000) * 10)));
      const o = (j * S + i) * 4;
      png.data[o] = v >> 16;
      png.data[o + 1] = (v >> 8) & 255;
      png.data[o + 2] = v & 255;
      png.data[o + 3] = 255;
    }
  }
  return { buf: PNG.sync.write(png, { colorType: 2, deflateLevel: 9, filterType: 4 }), mn, mx };
}

// ---------------------------------------------------------------- główny przebieg (blokami)
// Obszar dzielimy na bloki = kafle poziomu bazowego (zr). Każdy blok niezależnie pobiera LiDAR,
// łączy go z tłem i zapisuje swoje kafle; w pamięci globalnie trzymamy tylko model poziomu zr+1.
// Dzięki temu pamięć nie zależy od wielkości obszaru.

/** Rozdzielczość LiDAR dopasowana do powierzchni (czas pobierania i rozmiar danych). */
function fineZoom(hd, km2) {
  const z = km2 > 4000 ? 13 : km2 > 1500 ? 14 : 15;
  return Math.min(hd.zoom, z);
}

function bilin(data, W, H, x, y) {
  x = Math.max(0, Math.min(W - 1.001, x));
  y = Math.max(0, Math.min(H - 1.001, y));
  const xi = x | 0, yi = y | 0, fx = x - xi, fy = y - yi, i = yi * W + xi;
  return (data[i] * (1 - fx) + data[i + 1] * fx) * (1 - fy) + (data[i + W] * (1 - fx) + data[i + W + 1] * fx) * fy;
}

/** Łączenie LiDAR z tłem w ramce bloku (splot znormalizowany przy granicy pokrycia). */
function fuse(F, M, C, W, H, zf) {
  const mpp = 3 * 2 ** (15 - zf);
  const Wm = new Float32Array(M.length), Dm = new Float32Array(M.length);
  for (let i = 0; i < M.length; i++) if (M[i]) { Wm[i] = 1; Dm[i] = F[i] - C[i]; }
  const R2 = Math.max(2, Math.round(90 / mpp));
  const bw = boxBlur(boxBlur(Wm, W, H, R2), W, H, R2);
  const bd = boxBlur(boxBlur(Dm, W, H, R2), W, H, R2);
  for (let i = 0; i < M.length; i++) {
    if (M[i]) continue;
    const reach = Math.min(1, bw[i] * 4);
    F[i] = C[i] + (bw[i] > 1e-4 ? bd[i] / bw[i] : 0) * reach;
  }
  const R = Math.max(1, Math.round(12 / mpp));
  const edge = boxBlur(Wm, W, H, R);
  const sm = boxBlur(F, W, H, R);
  for (let i = 0; i < M.length; i++) {
    const e = edge[i];
    if (e > 0.02 && e < 0.98) {
      const t = 1 - Math.abs(e - 0.5) * 2;
      F[i] = F[i] * (1 - t * 0.7) + sm[i] * t * 0.7;
    }
  }
}

for (const region of regions) {
  const hd = region.hd;
  if (!hd) continue;
  const zr = region.zoom;
  const zg = zr + 1;
  const km2 = areaKm2(region.bbox);
  const zf = Math.max(zg, fineZoom(hd, km2));
  console.log(`\n▲ ${region.name} (LiDAR z${zf}, ${Math.round(km2)} km², źródła: ${hd.lidar.join(', ')})`);
  const [w, s, e, n] = region.bbox;
  const px0 = Math.floor(lon2px(w, zr)), px1 = Math.ceil(lon2px(e, zr));
  const py0 = Math.floor(lat2px(n, zr)), py1 = Math.ceil(lat2px(s, zr));
  const k = 2 ** (zf - zr);
  const frame = { zf, X0: px0 * k, Y0: py0 * k, X1: px1 * k, Y1: py1 * k };
  progress(0.02, 'Model globalny (tło)');
  const coarse = await coarseSampler(region, frame);

  // model poziomu zg (zr+1) – cały obszar w pamięci
  const gW = (px1 - px0) * 2 + 1, gH = (py1 - py0) * 2 + 1, gX0 = px0 * 2, gY0 = py0 * 2;
  const kg = 2 ** (zf - zg);
  const G = new Float32Array(gW * gH);
  for (let j = 0; j < gH; j++) for (let i = 0; i < gW; i++) G[j * gW + i] = coarse((gX0 + i) * kg, (gY0 + j) * kg);

  // punkty do przeliczenia wysokości (szlaki, szczyty) – przypisane do bloków
  const dir = path.join(OUT, region.id);
  const trails = JSON.parse(fs.readFileSync(path.join(dir, 'trails.json'), 'utf8'));
  const pois = JSON.parse(fs.readFileSync(path.join(dir, 'pois.json'), 'utf8'));
  const refs = new Map();
  const addRef = (lon, lat, set) => {
    const X = lon2px(lon, zf), Y = lat2px(lat, zf);
    set(bilin(G, gW, gH, X / kg - gX0, Y / kg - gY0));
    const key = `${Math.floor(X / (256 * k))},${Math.floor(Y / (256 * k))}`;
    if (!refs.has(key)) refs.set(key, []);
    refs.get(key).push({ X, Y, set });
  };
  for (const nd of trails.nodes) addRef(nd[0], nd[1], (v) => (nd[2] = +v.toFixed(1)));
  for (const ed of trails.edges) for (let i = 0; i < ed.g.length; i += 3) addRef(ed.g[i], ed.g[i + 1], (v) => (ed.g[i + 2] = +v.toFixed(1)));
  for (const p of pois) addRef(p.lon, p.lat, (v) => (p.d = +v.toFixed(1)));

  // bloki z możliwym pokryciem LiDAR
  const lidarCountries = COVERAGE.filter((c) => c.lidar && hd.lidar.includes(c.lidar));
  const blocks = [];
  const mppR = (156543.034 * Math.cos((((s + n) / 2) * Math.PI) / 180)) / 2 ** zr;
  const bufPx = Math.ceil(5000 / mppR);
  for (let ty = Math.floor(py0 / 256); ty <= Math.floor((py1 - 1) / 256); ty++) {
    for (let tx = Math.floor(px0 / 256); tx <= Math.floor((px1 - 1) / 256); tx++) {
      // test pokrycia na siatce 9×9 z zapasem 5 km (uproszczone granice bywają przesunięte);
      // bloki bez danych i tak odpadają po pobraniu
      const lw = px2lon(tx * 256 - bufPx, zr), le = px2lon(tx * 256 + 256 + bufPx, zr);
      const ln = px2lat(ty * 256 - bufPx, zr), ls = px2lat(ty * 256 + 256 + bufPx, zr);
      let hit = false;
      for (let j = 0; j <= 8 && !hit; j++) for (let i = 0; i <= 8 && !hit; i++) {
        const lon = lw + ((le - lw) * i) / 8, lat = ls + ((ln - ls) * j) / 8;
        if (lidarCountries.some((c) => inRing(lon, lat, c.ring))) hit = true;
      }
      if (hit) blocks.push([tx, ty]);
    }
  }
  const tdir = path.join(dir, 'tiles');
  fs.rmSync(tdir, { recursive: true, force: true });
  const index = { minZ: zr, maxZ: zf, tiles: {} };
  let bytes = 0, count = 0, withData = 0;
  const writeTile = (L, z, tx, ty) => {
    const { buf, mn, mx } = encodeTile(L, tx, ty);
    const p = path.join(tdir, String(z), String(tx));
    fs.mkdirSync(p, { recursive: true });
    fs.writeFileSync(path.join(p, `${ty}.png`), buf);
    index.tiles[`${z}/${tx}/${ty}`] = [Math.round(mn * 10) / 10, Math.round(mx * 10) / 10];
    bytes += buf.length;
    count++;
  };
  log(`bloki z LiDAR: ${blocks.length}`);

  const MARGIN = 64;
  const B = 256 * k;
  let bi = 0;
  for (const [tx, ty] of blocks) {
    bi++;
    progress(0.08 + (bi / Math.max(1, blocks.length)) * 0.8, `LiDAR: blok ${bi}/${blocks.length}`);
    const bf = { zf, X0: tx * B - MARGIN, Y0: ty * B - MARGIN, W: B + 2 * MARGIN + 1, H: B + 2 * MARGIN + 1 };
    bf.X1 = bf.X0 + bf.W;
    bf.Y1 = bf.Y0 + bf.H;
    const pseudo = { bbox: [px2lon(bf.X0, zf), px2lat(bf.Y1, zf), px2lon(bf.X1, zf), px2lat(bf.Y0, zf)] };
    const F = new Float32Array(bf.W * bf.H);
    const M = new Uint8Array(bf.W * bf.H);
    let covered = 0;
    for (const src of hd.lidar) {
      if (src === 'pl') covered += await lidarPL(pseudo, bf, F, M);
      if (src === 'cz') covered += await lidarCZ(pseudo, bf, F, M);
    }
    if (CLEAN) for (const f of bigFiles.splice(0)) fs.rmSync(path.join(CACHE, f), { force: true });
    if (!covered) continue;
    withData++;
    const C = new Float32Array(F.length);
    for (let j = 0; j < bf.H; j++) for (let i = 0; i < bf.W; i++) C[j * bf.W + i] = coarse(bf.X0 + i, bf.Y0 + j);
    fuse(F, M, C, bf.W, bf.H, zf);

    // piramida bloku: zf … zg
    let L = { data: F, W: bf.W, H: bf.H, X0: bf.X0, Y0: bf.Y0 };
    for (let z = zf; z >= zg; z--) {
      const n = 2 ** (z - zr);
      if (z > zg) {
        for (let j = 0; j < n; j++) for (let i = 0; i < n; i++) writeTile(L, z, tx * n + i, ty * n + j);
        const d = downsample(L.data, L.W, L.H);
        L = { data: d.data, W: d.W, H: d.H, X0: L.X0 / 2, Y0: L.Y0 / 2 };
      } else {
        // wnętrze bloku na poziomie zg → model globalny
        for (let j = 0; j < L.H; j++) {
          const gy = L.Y0 + j - gY0;
          if (gy < 0 || gy >= gH) continue;
          const by = L.Y0 + j - ty * 512;
          if (by < 0 || by > 512) continue;
          for (let i = 0; i < L.W; i++) {
            const bx = L.X0 + i - tx * 512;
            if (bx < 0 || bx > 512) continue;
            const gx = L.X0 + i - gX0;
            if (gx < 0 || gx >= gW) continue;
            G[gy * gW + gx] = L.data[j * L.W + i];
          }
        }
      }
    }
    // szlaki i szczyty w tym bloku – z pełnej rozdzielczości
    for (const r of refs.get(`${tx},${ty}`) ?? []) r.set(bilin(F, bf.W, bf.H, r.X - bf.X0, r.Y - bf.Y0));
  }
  log(`bloki z danymi LiDAR: ${withData}/${blocks.length}`);

  // poziomy zg i zr z modelu globalnego (wszystkie kafle obszaru)
  progress(0.9, 'Kafle poziomów zgrubnych');
  const LG = { data: G, W: gW, H: gH, X0: gX0, Y0: gY0 };
  const d = downsample(G, gW, gH);
  const LR = { data: d.data, W: d.W, H: d.H, X0: px0, Y0: py0 };
  for (const [L, z] of [[LR, zr], [LG, zg]]) {
    for (let ty = Math.floor(L.Y0 / 256); ty <= Math.floor((L.Y0 + L.H - 2) / 256); ty++)
      for (let tx = Math.floor(L.X0 / 256); tx <= Math.floor((L.X0 + L.W - 2) / 256); tx++) writeTile(L, z, tx, ty);
  }
  fs.writeFileSync(path.join(tdir, 'index.json'), JSON.stringify(index));
  log(`kafle: ${count}, ${(bytes / 1024 / 1024).toFixed(1)} MB`);

  // model analityczny (zg, środki komórek)
  const gw = (px1 - px0) * 2, gh = (py1 - py0) * 2;
  const q = new Uint16Array(gw * gh);
  let mn = Infinity, mx = -Infinity;
  for (let y = 0; y < gh; y++) for (let x = 0; x < gw; x++) {
    const i = y * gW + x;
    const h = (G[i] + G[i + 1] + G[i + gW] + G[i + gW + 1]) / 4;
    q[y * gw + x] = Math.round(Math.max(0, h) * 10);
    if (h < mn) mn = h;
    if (h > mx) mx = h;
  }
  fs.writeFileSync(path.join(dir, 'dem.bin'), Buffer.from(q.buffer));
  const meta = JSON.parse(fs.readFileSync(path.join(dir, 'meta.json'), 'utf8'));
  Object.assign(meta, {
    width: gw, height: gh, zoom: zg, px0: px0 * 2, py0: py0 * 2, min: mn, max: mx,
    hd: { minZ: zr, maxZ: zf, lidar: hd.lidar, ortho: hd.ortho },
    baked: new Date().toISOString(),
    sources: [
      'Terrarium elevation tiles (Mapzen / AWS Open Data)',
      ...(hd.lidar.includes('pl') ? ['NMT GUGiK (LiDAR, geoportal.gov.pl)'] : []),
      ...(hd.lidar.includes('cz') ? ['DMR 5G © ČÚZK'] : []),
      '© OpenStreetMap contributors (ODbL)',
    ],
  });
  fs.writeFileSync(path.join(dir, 'meta.json'), JSON.stringify(meta));
  fs.writeFileSync(path.join(dir, 'trails.json'), JSON.stringify(trails));
  fs.writeFileSync(path.join(dir, 'pois.json'), JSON.stringify(pois));
  log(`dem.bin: ${gw}×${gh}, ${mn.toFixed(0)}–${mx.toFixed(0)} m`);
  const peak = [...pois].filter((p) => p.t === 'peak').sort((a, b) => (b.e ?? b.d) - (a.e ?? a.d))[0];
  if (peak) log(`kontrola: ${peak.n} OSM ${peak.e} m / model HD ${peak.d} m`);
  fs.writeFileSync(path.join(dir, 'install.json'), JSON.stringify({ level: 'hd', lidarZoom: zf, date: new Date().toISOString() }));
  progress(1, 'Gotowe');
}
