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
import { COVERAGE, areaKm2, inRing } from './lib/area.mjs';

import { CACHE_DIR as CACHE, DATA_DIR as OUT, progress, selectRegions } from './lib/common.mjs';
const regions = selectRegions(process.argv);
const CLEAN = process.argv.includes('--clean-cache') || process.env.GRAN_CLEAN_CACHE === '1';
import { bigFiles, lon2px, lat2px, px2lon, px2lat, log, coarseSampler, lidarPL, lidarCZ, downsample, encodeTile, bilin, fuse } from './lib/terrain.mjs';

for (const region of regions) {
  const hd = region.hd;
  if (!hd) continue;
  const zr = region.zoom;
  const zg = zr + 1;
  const km2 = areaKm2(region.bbox);
  // rozdzielczość LiDAR wybrana przez użytkownika (15 ≈ 3 m, 14 ≈ 6 m, 13 ≈ 12 m)
  const zf = Math.max(zg, hd.zoom);
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
