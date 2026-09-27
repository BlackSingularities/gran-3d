// Numeryczne modele terenu z lotniczego skanowania laserowego dla Alp i okolic:
//  FR  – IGN RGE ALTI 1 m (WMS geopf, float32 w Web Mercatorze)
//  SI  – ARSO DMR 1 m (ArcGIS ImageServer, eksport w Web Mercatorze)
//  ITB – Prowincja Bolzano (Południowy Tyrol) DTM 0,5 m (GeoServer WCS 1.0.0, Web Mercator)
//  AT  – BEV ALS DTM 1 m (Cloud Optimized GeoTIFF, EPSG:3035 – czytany tylko potrzebny wycinek)
//  CH  – swisstopo swissALTI3D 2 m (STAC, kafle 1 km w LV95 / EPSG:2056)
// Każda funkcja wypełnia siatkę bloku (F – wysokości, M – maska pokrycia) i zwraca liczbę pikseli.
import { fromUrl } from 'geotiff';
import { bigFiles, boxBlur, fetchCached, pool, px2lat, px2lon, px2mx, py2my, readTiff, UA } from './terrain.mjs';

// ---------------------------------------------------------------- wspólne: eksport w Web Mercatorze
async function mercatorChunks(frame, F, M, { chunk, conc, name, url, bil = false, valid = (v) => v > -100 && v < 9000 }) {
  const jobs = [];
  for (let cy = 0; cy < frame.H; cy += chunk) for (let cx = 0; cx < frame.W; cx += chunk) jobs.push([cx, cy]);
  let covered = 0;
  await pool(jobs, conc, async ([cx, cy]) => {
    const w = Math.min(chunk, frame.W - cx), h = Math.min(chunk, frame.H - cy);
    // środki pikseli obrazu = narożniki pikseli zf
    const X = frame.X0 + cx - 0.5, Y = frame.Y0 + cy - 0.5;
    const bb = [px2mx(X, frame.zf), py2my(Y + h, frame.zf), px2mx(X + w, frame.zf), py2my(Y, frame.zf)].map((v) => v.toFixed(3)).join(',');
    const fname = `${name}-${frame.zf}-${X}-${Y}-${w}x${h}.${bil ? 'bil' : 'tif'}`;
    bigFiles.push(fname);
    let buf;
    try {
      buf = await fetchCached(url(bb, w, h), fname);
    } catch (e) {
      console.warn(`  ! ${name}: ${e.message}`);
      return;
    }
    let data, tw;
    if (bil) {
      if (buf.length < w * h * 4) return; // odpowiedź z błędem (XML)
      data = new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + w * h * 4));
      tw = w;
    } else {
      if (buf[0] === 0x3c) return; // XML zamiast obrazu
      const t = await readTiff(buf);
      data = t.data;
      tw = t.w;
    }
    if (tw !== w) data = Float32Array.from({ length: w * h }, (_, i) => data[Math.floor(i / w) * tw + (i % w)]);
    data = unstep(data, w, h, valid);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const v = data[y * w + x];
      const idx = (cy + y) * frame.W + cx + x;
      if (valid(v) && !M[idx]) { F[idx] = v; M[idx] = 1; covered++; }
    }
  });
  return covered;
}

/**
 * Część serwerów (IGN) poza pełnym pokryciem oddaje zgrubny model próbkowany „najbliższym
 * sąsiadem” – wysokości idą schodkami (te same wartości w kilku sąsiednich pikselach), co przy
 * cieniowaniu daje pasy. Wykrywamy takie miejsca (gęstość powtórzeń) i tylko tam wygładzamy.
 */
function unstep(d, w, h, valid) {
  const rep = new Float32Array(w * h);
  let any = 0;
  for (let y = 0; y < h; y++) for (let x = 1; x < w - 1; x++) {
    const i = y * w + x, v = d[i];
    if (valid(v) && (v === d[i - 1] || v === d[i + 1] || (y > 0 && v === d[i - w]) || (y < h - 1 && v === d[i + w]))) { rep[i] = 1; any++; }
  }
  if (any < w * h * 0.02) return d;
  const dens = boxBlur(rep, w, h, 4);
  // wygładzenie ważone ważnością próbek (brak danych nie „wlewa się” do średniej)
  const vv = new Float32Array(w * h), ww = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) if (valid(d[i])) { vv[i] = d[i]; ww[i] = 1; }
  const out = new Float32Array(d);
  for (const r of [3]) {
    const sv = boxBlur(vv, w, h, r), sw = boxBlur(ww, w, h, r);
    for (let i = 0; i < w * h; i++) {
      if (!valid(d[i]) || sw[i] < 0.5) continue;
      const k = Math.min(1, Math.max(0, (dens[i] - 0.25) / 0.25)); // 0 = oryginał, 1 = wygładzone
      if (k > 0) out[i] = d[i] * (1 - k) + (sv[i] / sw[i]) * k;
    }
  }
  return out;
}

export const lidarFR = (region, frame, F, M) =>
  mercatorChunks(frame, F, M, {
    chunk: 2048, conc: 3, name: 'ign', bil: true,
    url: (bb, w, h) => `https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES&STYLES=&CRS=EPSG:3857&BBOX=${bb}&WIDTH=${w}&HEIGHT=${h}&FORMAT=image/x-bil;bits=32`,
  });

export const lidarSI = (region, frame, F, M) =>
  mercatorChunks(frame, F, M, {
    chunk: 2400, conc: 3, name: 'arso',
    url: (bb, w, h) => `https://gis.arso.gov.si/arcgis/rest/services/Slovenija_DMR_D96TM/ImageServer/exportImage?bbox=${bb}&bboxSR=3857&imageSR=3857&size=${w},${h}&format=tiff&pixelType=F32&interpolation=RSP_BilinearInterpolation&noData=-9999&f=image`,
  });

export const lidarBZ = (region, frame, F, M) =>
  mercatorChunks(frame, F, M, {
    chunk: 2400, conc: 3, name: 'bz',
    url: (bb, w, h) => `https://geoservices9.civis.bz.it/geoserver/wcs?service=WCS&version=1.0.0&request=GetCoverage&coverage=p_bz-Elevation:DigitalTerrainModel-0.5m&format=GeoTIFF&CRS=EPSG:3857&RESPONSE_CRS=EPSG:3857&BBOX=${bb}&WIDTH=${w}&HEIGHT=${h}&INTERPOLATION=bilinear`,
  });

// ---------------------------------------------------------------- wspólne: siatka w innym układzie
/** Próbkuje siatkę rzutowaną (wiersz 0 = północ) w pikselach bloku; proj(lon, lat) → [E, N]. */
function sampleProjected(frame, F, M, grid, proj) {
  const { E0, N1, res, W, H, data } = grid;
  let covered = 0;
  for (let j = 0; j < frame.H; j++) {
    const lat = px2lat(frame.Y0 + j, frame.zf);
    for (let i = 0; i < frame.W; i++) {
      const idx = j * frame.W + i;
      if (M[idx]) continue;
      const [E, N] = proj(px2lon(frame.X0 + i, frame.zf), lat);
      const gx = (E - E0) / res - 0.5, gy = (N1 - N) / res - 0.5;
      const xi = Math.floor(gx), yi = Math.floor(gy);
      if (xi < 0 || yi < 0 || xi >= W - 1 || yi >= H - 1) continue;
      const fx = gx - xi, fy = gy - yi, g = yi * W + xi;
      const a = data[g], b = data[g + 1], c = data[g + W], d = data[g + W + 1];
      if (!(a > -100 && b > -100 && c > -100 && d > -100)) continue; // brak danych (NaN / −9999)
      F[idx] = (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy;
      M[idx] = 1;
      covered++;
    }
  }
  return covered;
}

/** Zasięg bloku w układzie rzutowanym (z zapasem). */
function projExtent(frame, proj, pad) {
  let e0 = Infinity, e1 = -Infinity, n0 = Infinity, n1 = -Infinity;
  for (let j = 0; j <= 8; j++) for (let i = 0; i <= 8; i++) {
    const [E, N] = proj(px2lon(frame.X0 + (frame.W * i) / 8, frame.zf), px2lat(frame.Y0 + (frame.H * j) / 8, frame.zf));
    e0 = Math.min(e0, E); e1 = Math.max(e1, E); n0 = Math.min(n0, N); n1 = Math.max(n1, N);
  }
  return [e0 - pad, n0 - pad, e1 + pad, n1 + pad];
}

// ---------------------------------------------------------------- AT: ETRS89-LAEA (EPSG:3035)
const A = 6378137, EC = 0.0818191908426215, E2 = EC * EC;
const qf = (s) => (1 - E2) * (s / (1 - E2 * s * s) - (1 / (2 * EC)) * Math.log((1 - EC * s) / (1 + EC * s)));
const QP = qf(1), PHI0 = (52 * Math.PI) / 180, LAM0 = (10 * Math.PI) / 180;
const B0 = Math.asin(qf(Math.sin(PHI0)) / QP), RQ = A * Math.sqrt(QP / 2);
const DD = (A * Math.cos(PHI0)) / (Math.sqrt(1 - E2 * Math.sin(PHI0) ** 2) * RQ * Math.cos(B0));
export function toLaea(lon, lat) {
  const b = Math.asin(qf(Math.sin((lat * Math.PI) / 180)) / QP), dl = (lon * Math.PI) / 180 - LAM0;
  const B = RQ * Math.sqrt(2 / (1 + Math.sin(B0) * Math.sin(b) + Math.cos(B0) * Math.cos(b) * Math.cos(dl)));
  return [4321000 + B * DD * Math.cos(b) * Math.sin(dl), 3210000 + (B / DD) * (Math.cos(B0) * Math.sin(b) - Math.sin(B0) * Math.cos(b) * Math.cos(dl))];
}

const cogCache = new Map();
function openCog(url) {
  let p = cogCache.get(url);
  if (!p) {
    p = fromUrl(url, { headers: { 'User-Agent': UA }, blockSize: 1 << 20, cacheSize: 200 }).catch((e) => {
      cogCache.delete(url);
      throw e;
    });
    cogCache.set(url, p);
  }
  return p;
}

export async function lidarAT(region, frame, F, M) {
  const RES = 2; // poziom podglądu COG ~2 m – wystarcza dla siatki 3 m
  const [e0, n0, e1, n1] = projExtent(frame, toLaea, 50).map((v) => Math.round(v / RES) * RES);
  const W = Math.round((e1 - e0) / RES), H = Math.round((n1 - n0) / RES);
  const data = new Float32Array(W * H).fill(NaN);
  const S = 50000;
  const tiles = [];
  for (let ty = Math.floor(n0 / S); ty <= Math.floor(n1 / S); ty++) for (let tx = Math.floor(e0 / S); tx <= Math.floor(e1 / S); tx++) tiles.push([tx * S, ty * S]);
  await pool(tiles, 2, async (t) => {
    // serwer BEV bywa chwilowo nieosiągalny – do 3 prób
    for (let k = 0; k < 3; k++) {
      try {
        return await atTile(t);
      } catch (e) {
        if (k === 2) throw e;
        await new Promise((r) => setTimeout(r, 2000 * (k + 1)));
      }
    }
  });
  return sampleProjected(frame, F, M, { E0: e0, N1: n1, res: RES, W, H, data }, toLaea);

  async function atTile([TE, TN]) {
    let tif;
    const url = `https://data.bev.gv.at/download/ALS/DTM/20190915/CRS3035RES50000mN${TN}E${TE}.tif`;
    try {
      tif = await openCog(url);
    } catch (e) {
      if (/40[34]/.test(String(e.message))) return; // arkusza nie ma (poza Austrią)
      throw e;
    }
    // obraz o rozdzielczości najbliższej 2 m
    const n = await tif.getImageCount();
    let im = await tif.getImage(0);
    for (let k = 1; k < n; k++) {
      const c = await tif.getImage(k);
      if (S / c.getWidth() > RES * 1.01) break;
      im = c;
    }
    const r = S / (im.getWidth() - 1);
    // okno w pikselach arkusza (wiersz 0 = północ, środek piksela 0 = TE, TN+S)
    const x0 = Math.max(0, Math.floor((e0 - TE) / r) - 1), x1 = Math.min(im.getWidth(), Math.ceil((e1 - TE) / r) + 2);
    const y0 = Math.max(0, Math.floor((TN + S - n1) / r) - 1), y1 = Math.min(im.getHeight(), Math.ceil((TN + S - n0) / r) + 2);
    if (x1 <= x0 || y1 <= y0) return;
    const [win] = await im.readRasters({ window: [x0, y0, x1, y1] });
    const ww = x1 - x0;
    for (let j = 0; j < H; j++) {
      const N = n1 - (j + 0.5) * RES;
      const sy = (TN + S - N) / r - y0;
      const yi = Math.round(sy);
      if (yi < 0 || yi >= y1 - y0) continue;
      for (let i = 0; i < W; i++) {
        const E = e0 + (i + 0.5) * RES;
        const xi = Math.round((E - TE) / r - x0);
        if (xi < 0 || xi >= ww) continue;
        const v = win[yi * ww + xi];
        if (v > -100) data[j * W + i] = v;
      }
    }
  }
}

// ---------------------------------------------------------------- CH: LV95 (EPSG:2056), wzory przybliżone swisstopo (~1 m)
export function toLv95(lon, lat) {
  const p = (lat * 3600 - 169028.66) / 10000, l = (lon * 3600 - 26782.5) / 10000;
  return [
    2600072.37 + 211455.93 * l - 10938.51 * l * p - 0.36 * l * p * p - 44.54 * l ** 3,
    1200147.07 + 308807.95 * p + 3745.25 * l * l + 76.63 * p * p - 194.56 * l * l * p + 119.79 * p ** 3,
  ];
}

export async function lidarCH(region, frame, F, M) {
  const RES = 2;
  const bbox = [px2lon(frame.X0, frame.zf), px2lat(frame.Y0 + frame.H, frame.zf), px2lon(frame.X0 + frame.W, frame.zf), px2lat(frame.Y0, frame.zf)];
  // kafle 1 km z katalogu STAC – dla każdego tylko najnowszy rocznik
  const best = new Map();
  let next = `https://data.geo.admin.ch/api/stac/v0.9/collections/ch.swisstopo.swissalti3d/items?bbox=${bbox.map((v) => v.toFixed(5)).join(',')}&limit=100`;
  for (let page = 0; next && page < 20; page++) {
    const r = await fetch(next, { headers: { 'User-Agent': UA } });
    if (!r.ok) throw new Error(`STAC ${r.status}`);
    const j = await r.json();
    for (const f of j.features) {
      const m = f.id.match(/_(\d{4})_(\d+-\d+)$/);
      if (!m) continue;
      const asset = Object.values(f.assets).find((a) => a['eo:gsd'] === RES && a.href.endsWith('.tif'));
      if (!asset) continue;
      const prev = best.get(m[2]);
      if (!prev || prev.year < +m[1]) best.set(m[2], { year: +m[1], href: asset.href });
    }
    next = j.links.find((l) => l.rel === 'next')?.href ?? null;
  }
  if (!best.size) return 0;
  const [e0, n0, e1, n1] = projExtent(frame, toLv95, 50).map((v) => Math.floor(v / 1000) * 1000);
  const E1 = e1 + 1000, N1 = n1 + 1000;
  const W = (E1 - e0) / RES, H = (N1 - n0) / RES;
  const data = new Float32Array(W * H).fill(NaN);
  await pool([...best.entries()], 6, async ([key, { href }]) => {
    const [ke, kn] = key.split('-').map((v) => +v * 1000);
    if (ke < e0 || kn < n0 || ke >= E1 || kn >= N1) return;
    const fname = `swissalti3d-${key}-${RES}.tif`;
    bigFiles.push(fname);
    let t;
    try {
      t = await readTiff(await fetchCached(href, fname));
    } catch (e) {
      console.warn(`  ! swissALTI3D ${key}: ${e.message}`);
      return;
    }
    const ox = (ke - e0) / RES, oy = (N1 - (kn + 1000)) / RES;
    for (let y = 0; y < t.h; y++) for (let x = 0; x < t.w; x++) {
      const gx = ox + x, gy = oy + y;
      if (gx < W && gy < H) data[gy * W + gx] = t.data[y * t.w + x];
    }
  });
  return sampleProjected(frame, F, M, { E0: e0, N1, res: RES, W, H, data }, toLv95);
}

// ---------------------------------------------------------------- rejestr wszystkich źródeł LiDAR
import { lidarCZ, lidarPL } from './terrain.mjs';
export const LIDAR_SOURCES = {
  pl: { fn: lidarPL, credit: 'NMT GUGiK (LiDAR, geoportal.gov.pl)' },
  cz: { fn: lidarCZ, credit: 'DMR 5G © ČÚZK' },
  fr: { fn: lidarFR, credit: 'RGE ALTI® © IGN' },
  si: { fn: lidarSI, credit: 'DMR © ARSO (LiDAR Slovenije)' },
  bz: { fn: lidarBZ, credit: 'DTM © Autonome Provinz Bozen – Südtirol' },
  at: { fn: lidarAT, credit: 'ALS DTM © BEV (CC BY 4.0)' },
  ch: { fn: lidarCH, credit: 'swissALTI3D © swisstopo' },
};
