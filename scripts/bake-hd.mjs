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
  const res = frame.zf >= 15 ? 3 : 6;
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
  log(`PL NMT: ${GW}×${GH} px po ${res} m, ${jobs.length} fragmentów WCS`);
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
    process.stdout.write(`\r   PL: ${done}/${jobs.length}   `);
    progress((done / jobs.length) * 0.55, `LiDAR GUGiK: ${done}/${jobs.length} fragmentów`);
  });
  process.stdout.write('\n');
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
  log(`PL: pokrycie ${((covered / (frame.W * frame.H)) * 100).toFixed(1)}% siatki`);
}

// ---------------------------------------------------------------- CZ: ČÚZK DMR 5G (Web Mercator)
async function lidarCZ(region, frame, F, M) {
  const CH = 4000;
  const jobs = [];
  for (let cy = 0; cy < frame.H; cy += CH) for (let cx = 0; cx < frame.W; cx += CH) jobs.push([cx, cy]);
  log(`CZ DMR 5G: ${jobs.length} fragmentów`);
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
    process.stdout.write(`\r   CZ: ${done}/${jobs.length}   `);
    progress((done / jobs.length) * 0.3, `LiDAR ČÚZK: ${done}/${jobs.length} fragmentów`);
  });
  process.stdout.write('\n');
  log(`CZ: pokrycie ${((covered / (frame.W * frame.H)) * 100).toFixed(1)}% siatki`);
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

// ---------------------------------------------------------------- główny przebieg
for (const region of regions) {
  const hd = region.hd;
  if (!hd) continue;
  console.log(`\n▲ ${region.name} (HD z${hd.zoom}, LiDAR: ${hd.lidar.join(', ')})`);
  const zr = region.zoom;
  const [w, s, e, n] = region.bbox;
  const px0 = Math.floor(lon2px(w, zr)), px1 = Math.ceil(lon2px(e, zr));
  const py0 = Math.floor(lat2px(n, zr)), py1 = Math.ceil(lat2px(s, zr));
  const k = 2 ** (hd.zoom - zr);
  const frame = { zf: hd.zoom, X0: px0 * k, Y0: py0 * k, X1: px1 * k, Y1: py1 * k, W: (px1 - px0) * k + 1, H: (py1 - py0) * k + 1 };
  log(`siatka HD: ${frame.W}×${frame.H} (${((frame.W * frame.H) / 1e6).toFixed(0)} mln próbek)`);

  const F = new Float32Array(frame.W * frame.H);
  const M = new Uint8Array(frame.W * frame.H);
  for (const src of hd.lidar) {
    if (src === 'pl') await lidarPL(region, frame, F, M);
    if (src === 'cz') await lidarCZ(region, frame, F, M);
  }
  progress(0.6, 'Model globalny (tło)');
  const coarse = await coarseSampler(region, frame);
  progress(0.65, 'Łączenie danych');

  // Łączenie LiDAR z tłem. Poza pokryciem LiDAR dodajemy do tła poprawkę (LiDAR − tło)
  // wygładzoną splotem znormalizowanym – grań na granicy państw nie „opada” do modelu zgrubnego.
  const C = new Float32Array(F.length);
  for (let j = 0; j < frame.H; j++) for (let i = 0; i < frame.W; i++) C[j * frame.W + i] = coarse(frame.X0 + i, frame.Y0 + j);
  const mpp = 3 * 2 ** (15 - hd.zoom);
  const Wm = new Float32Array(M.length), Dm = new Float32Array(M.length);
  for (let i = 0; i < M.length; i++) if (M[i]) { Wm[i] = 1; Dm[i] = F[i] - C[i]; }
  const R2 = Math.round(90 / mpp);
  const bw = boxBlur(boxBlur(Wm, frame.W, frame.H, R2), frame.W, frame.H, R2);
  const bd = boxBlur(boxBlur(Dm, frame.W, frame.H, R2), frame.W, frame.H, R2);
  for (let i = 0; i < M.length; i++) {
    if (M[i]) continue;
    const reach = Math.min(1, bw[i] * 4); // zanika ~200 m od granicy pokrycia
    F[i] = C[i] + (bw[i] > 1e-4 ? bd[i] / bw[i] : 0) * reach;
  }
  // lekkie wygładzenie samej linii szwu
  const R = Math.max(1, Math.round(12 / mpp));
  const edge = boxBlur(Wm, frame.W, frame.H, R);
  const sm = boxBlur(F, frame.W, frame.H, R);
  for (let i = 0; i < M.length; i++) {
    const e = edge[i];
    if (e > 0.02 && e < 0.98) {
      const t = 1 - Math.abs(e - 0.5) * 2;
      F[i] = F[i] * (1 - t * 0.7) + sm[i] * t * 0.7;
    }
  }

  progress(0.72, 'Piramida kafli');
  // piramida poziomów
  const levels = {};
  levels[hd.zoom] = { data: F, mask: M, W: frame.W, H: frame.H, X0: frame.X0, Y0: frame.Y0 };
  for (let z = hd.zoom - 1; z >= zr; z--) {
    const up = levels[z + 1];
    const d = downsample(up.data, up.W, up.H);
    levels[z] = { data: d.data, mask: maxpool(up.mask, up.W, up.H), W: d.W, H: d.H, X0: up.X0 / 2, Y0: up.Y0 / 2 };
  }

  // wybór kafli: niskie poziomy w całości, wysokie tylko z pokryciem LiDAR (+ rodzeństwo)
  const dir = path.join(OUT, region.id);
  const tdir = path.join(dir, 'tiles');
  fs.rmSync(tdir, { recursive: true, force: true });
  const index = { minZ: zr, maxZ: hd.zoom, tiles: {} };
  const full = zr + 1;
  let wanted = new Set();
  for (let z = hd.zoom; z > full; z--) {
    const L = levels[z];
    const next = new Set();
    const tx0 = Math.floor(L.X0 / 256), tx1 = Math.floor((L.X0 + L.W - 2) / 256);
    const ty0 = Math.floor(L.Y0 / 256), ty1 = Math.floor((L.Y0 + L.H - 2) / 256);
    for (let ty = ty0; ty <= ty1; ty++) for (let tx = tx0; tx <= tx1; tx++) {
      let cov = wanted.has(`${tx},${ty}`);
      if (!cov) {
        for (let j = 0; j < 256 && !cov; j += 4) {
          const gy = ty * 256 + j - L.Y0;
          if (gy < 0 || gy >= L.H) continue;
          for (let i = 0; i < 256; i += 4) {
            const gx = tx * 256 + i - L.X0;
            if (gx >= 0 && gx < L.W && L.mask[gy * L.W + gx]) { cov = true; break; }
          }
        }
      }
      if (cov) next.add(`${tx >> 1},${ty >> 1}`);
    }
    // wszystkie dzieci rodziców z pokryciem (pełne czwórki)
    const list = [];
    for (const p of next) {
      const [x, y] = p.split(',').map(Number);
      for (let b = 0; b < 2; b++) for (let a = 0; a < 2; a++) list.push([2 * x + a, 2 * y + b]);
    }
    levels[z].list = list;
    wanted = next;
  }
  for (let z = zr; z <= full; z++) {
    const L = levels[z];
    const list = [];
    for (let ty = Math.floor(L.Y0 / 256); ty <= Math.floor((L.Y0 + L.H - 2) / 256); ty++)
      for (let tx = Math.floor(L.X0 / 256); tx <= Math.floor((L.X0 + L.W - 2) / 256); tx++) list.push([tx, ty]);
    // pełne czwórki także na poziomie „full”, jeśli rodzic istnieje
    L.list = list;
  }
  let bytes = 0, count = 0;
  for (let z = zr; z <= hd.zoom; z++) {
    const L = levels[z];
    for (const [tx, ty] of L.list) {
      const { buf, mn, mx } = encodeTile(L, tx, ty);
      const p = path.join(tdir, String(z), String(tx));
      fs.mkdirSync(p, { recursive: true });
      fs.writeFileSync(path.join(p, `${ty}.png`), buf);
      index.tiles[`${z}/${tx}/${ty}`] = [Math.round(mn * 10) / 10, Math.round(mx * 10) / 10];
      bytes += buf.length;
      count++;
    }
    log(`z${z}: ${L.list.length} kafli`);
    progress(0.75 + ((z - zr + 1) / (hd.zoom - zr + 1)) * 0.18, `Kafle poziomu ${z}`);
  }
  fs.writeFileSync(path.join(tdir, 'index.json'), JSON.stringify(index));
  log(`kafle: ${count}, ${(bytes / 1024 / 1024).toFixed(1)} MB`);

  // sampler HD (konwencja krawędziowa zf) dla szlaków i punktów
  const hdAt = (lon, lat) => {
    let x = lon2px(lon, hd.zoom) - frame.X0, y = lat2px(lat, hd.zoom) - frame.Y0;
    x = Math.max(0, Math.min(frame.W - 1.001, x));
    y = Math.max(0, Math.min(frame.H - 1.001, y));
    const xi = x | 0, yi = y | 0, fx = x - xi, fy = y - yi, i = yi * frame.W + xi;
    return (F[i] * (1 - fx) + F[i + 1] * fx) * (1 - fy) + (F[i + frame.W] * (1 - fx) + F[i + frame.W + 1] * fx) * fy;
  };

  // nowy model analityczny: zoom+1, środki komórek
  const zg = zr + 1;
  const L = levels[zg];
  const gw = (px1 - px0) * 2, gh = (py1 - py0) * 2;
  const q = new Uint16Array(gw * gh);
  let mn = Infinity, mx = -Infinity;
  for (let y = 0; y < gh; y++) for (let x = 0; x < gw; x++) {
    // środek komórki = współrzędna krawędziowa x+0.5 → średnia sąsiednich próbek
    const i = y * L.W + x;
    const h = (L.data[i] + L.data[i + 1] + L.data[i + L.W] + L.data[i + L.W + 1]) / 4;
    q[y * gw + x] = Math.round(Math.max(0, h) * 10);
    if (h < mn) mn = h;
    if (h > mx) mx = h;
  }
  fs.writeFileSync(path.join(dir, 'dem.bin'), Buffer.from(q.buffer));
  const meta = JSON.parse(fs.readFileSync(path.join(dir, 'meta.json'), 'utf8'));
  Object.assign(meta, {
    width: gw, height: gh, zoom: zg, px0: px0 * 2, py0: py0 * 2, min: mn, max: mx,
    hd: { minZ: zr, maxZ: hd.zoom, lidar: hd.lidar, ortho: hd.ortho },
    baked: new Date().toISOString(),
    sources: [
      'Terrarium elevation tiles (Mapzen / AWS Open Data)',
      ...(hd.lidar.includes('pl') ? ['NMT GUGiK (LiDAR, geoportal.gov.pl)'] : []),
      ...(hd.lidar.includes('cz') ? ['DMR 5G © ČÚZK'] : []),
      '© OpenStreetMap contributors (ODbL)',
    ],
  });
  fs.writeFileSync(path.join(dir, 'meta.json'), JSON.stringify(meta));
  log(`dem.bin: ${gw}×${gh}, ${mn.toFixed(0)}–${mx.toFixed(0)} m`);

  // szlaki i punkty: wysokości z modelu HD
  const trails = JSON.parse(fs.readFileSync(path.join(dir, 'trails.json'), 'utf8'));
  for (const nd of trails.nodes) nd[2] = +hdAt(nd[0], nd[1]).toFixed(1);
  for (const ed of trails.edges) for (let i = 0; i < ed.g.length; i += 3) ed.g[i + 2] = +hdAt(ed.g[i], ed.g[i + 1]).toFixed(1);
  fs.writeFileSync(path.join(dir, 'trails.json'), JSON.stringify(trails));
  const pois = JSON.parse(fs.readFileSync(path.join(dir, 'pois.json'), 'utf8'));
  for (const p of pois) p.d = +hdAt(p.lon, p.lat).toFixed(1);
  fs.writeFileSync(path.join(dir, 'pois.json'), JSON.stringify(pois));
  const peak = pois.find((p) => p.t === 'peak');
  if (peak) log(`kontrola: ${peak.n} OSM ${peak.e} m / model HD ${peak.d} m`);
  fs.writeFileSync(path.join(dir, 'install.json'), JSON.stringify({ level: 'hd', date: new Date().toISOString() }));
  if (CLEAN) for (const f of bigFiles.splice(0)) fs.rmSync(path.join(CACHE, f), { force: true });
  progress(1, 'Gotowe');
}
