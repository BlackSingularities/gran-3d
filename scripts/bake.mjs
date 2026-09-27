// Przygotowuje dane regionów: model terenu (DEM), graf szlaków i punkty (szczyty, przełęcze, schroniska).
// Źródła: kafle Terrarium (AWS Open Data / Mapzen) oraz OpenStreetMap przez Overpass API.
//   node scripts/bake.mjs            – wszystkie regiony
//   node scripts/bake.mjs tatry      – wybrany region
import fs from 'node:fs';
import path from 'node:path';
import { PNG } from 'pngjs';

import { CACHE_DIR as CACHE, DATA_DIR as OUT, progress, selectRegions } from './lib/common.mjs';
const UA = 'gran-trail-atlas/1.0 (https://github.com/BlackSingularities/gran-3d)';
const OVERPASS = ['https://overpass-api.de/api/interpreter', 'https://overpass.kumi.systems/api/interpreter'];

const regions = selectRegions(process.argv);

fs.mkdirSync(CACHE, { recursive: true });

// ---------- projekcja Web Mercator ----------
const lon2px = (lon, z) => ((lon + 180) / 360) * 256 * 2 ** z;
const lat2px = (lat, z) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 256 * 2 ** z;
};

async function fetchCached(url, file, init) {
  const p = path.join(CACHE, file);
  if (fs.existsSync(p)) return fs.readFileSync(p);
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      const res = await fetch(url, { ...init, headers: { 'User-Agent': UA, ...(init?.headers ?? {}) } });
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      const buf = Buffer.from(await res.arrayBuffer());
      fs.writeFileSync(p, buf);
      return buf;
    } catch (e) {
      console.warn(`  ! ${file}: ${e.message} (próba ${attempt + 1})`);
      await new Promise((r) => setTimeout(r, 3000 * (attempt + 1)));
    }
  }
  throw new Error(`Nie udało się pobrać ${url}`);
}

// ---------- DEM ----------
async function bakeDem(region) {
  const { bbox, zoom: z } = region;
  const [w, s, e, n] = bbox;
  const px0 = Math.floor(lon2px(w, z));
  const px1 = Math.ceil(lon2px(e, z));
  const py0 = Math.floor(lat2px(n, z));
  const py1 = Math.ceil(lat2px(s, z));
  const width = px1 - px0;
  const height = py1 - py0;
  const elev = new Float32Array(width * height);

  const tx0 = Math.floor(px0 / 256), tx1 = Math.floor((px1 - 1) / 256);
  const ty0 = Math.floor(py0 / 256), ty1 = Math.floor((py1 - 1) / 256);
  const jobs = [];
  for (let ty = ty0; ty <= ty1; ty++) for (let tx = tx0; tx <= tx1; tx++) jobs.push([tx, ty]);
  console.log(`  DEM ${width}×${height} px, ${jobs.length} kafli z${z}`);

  let done = 0;
  const worker = async () => {
    while (jobs.length) {
      const [tx, ty] = jobs.shift();
      const buf = await fetchCached(
        `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${tx}/${ty}.png`,
        `terrarium-${z}-${tx}-${ty}.png`
      );
      const png = PNG.sync.read(buf);
      for (let y = 0; y < 256; y++) {
        const gy = ty * 256 + y - py0;
        if (gy < 0 || gy >= height) continue;
        for (let x = 0; x < 256; x++) {
          const gx = tx * 256 + x - px0;
          if (gx < 0 || gx >= width) continue;
          const i = (y * 256 + x) * 4;
          elev[gy * width + gx] = png.data[i] * 256 + png.data[i + 1] + png.data[i + 2] / 256 - 32768;
        }
      }
      done++;
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));

  let min = Infinity, max = -Infinity;
  const q = new Uint16Array(width * height);
  for (let i = 0; i < elev.length; i++) {
    const v = Math.max(0, elev[i]);
    elev[i] = v;
    if (v < min) min = v;
    if (v > max) max = v;
    q[i] = Math.round(v * 10);
  }
  console.log(`  wysokości ${min.toFixed(0)}–${max.toFixed(0)} m n.p.m.`);
  return { width, height, px0, py0, zoom: z, min, max, elev, q };
}

function makeSampler(dem) {
  const { width, height, px0, py0, zoom, elev } = dem;
  return (lon, lat) => {
    const x = Math.min(width - 1.001, Math.max(0, lon2px(lon, zoom) - px0 - 0.5));
    const y = Math.min(height - 1.001, Math.max(0, lat2px(lat, zoom) - py0 - 0.5));
    const x0 = Math.floor(x), y0 = Math.floor(y);
    const fx = x - x0, fy = y - y0;
    const i = y0 * width + x0;
    const a = elev[i], b = elev[i + 1], c = elev[i + width], d = elev[i + width + 1];
    return a * (1 - fx) * (1 - fy) + b * fx * (1 - fy) + c * (1 - fx) * fy + d * fx * fy;
  };
}

// ---------- Overpass ----------
async function overpass(query, file) {
  let lastErr;
  for (const url of OVERPASS) {
    try {
      const buf = await fetchCached(url, file, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: 'data=' + encodeURIComponent(query),
      });
      return JSON.parse(buf.toString('utf8'));
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr;
}

const COLOR_WORDS = ['red', 'blue', 'green', 'yellow', 'black', 'orange', 'purple', 'white', 'brown'];
const HEX_TO_WORD = { '#ff0000': 'red', '#0000ff': 'blue', '#008000': 'green', '#00ff00': 'green', '#ffff00': 'yellow', '#000000': 'black' };

function trailColor(tags) {
  const sym = tags['osmc:symbol'];
  if (sym) {
    const parts = sym.split(':');
    const fg = parts[2]?.split('_')[0];
    if (fg && COLOR_WORDS.includes(fg) && fg !== 'white') return fg;
    if (COLOR_WORDS.includes(parts[0]) && parts[0] !== 'white') return parts[0];
  }
  const c = (tags.colour || tags.color || '').toLowerCase();
  if (COLOR_WORDS.includes(c)) return c;
  if (HEX_TO_WORD[c]) return HEX_TO_WORD[c];
  return 'other';
}

const SAC = { hiking: 1, mountain_hiking: 2, demanding_mountain_hiking: 3, alpine_hiking: 4, demanding_alpine_hiking: 5, difficult_alpine_hiking: 6 };

async function bakeOsm(region, dem) {
  const [w, s, e, n] = region.bbox;
  const bb = `${s},${w},${n},${e}`;
  const trailsQ = `[out:json][timeout:240];
relation["route"~"^(hiking|foot)$"](${bb})->.r;
way(r.r)->.w;
.r out body;
.w out body;
.w >;
out skel qt;`;
  const poiQ = `[out:json][timeout:120];
(
  node["natural"~"^(peak|saddle|volcano)$"]["name"](${bb});
  node["mountain_pass"="yes"]["name"](${bb});
  nwr["tourism"~"^(alpine_hut|wilderness_hut)$"](${bb});
  nwr["amenity"="shelter"]["name"](${bb});
  nwr["natural"="water"]["name"](${bb});
  node["waterway"="waterfall"]["name"](${bb});
  node["natural"="cave_entrance"]["name"](${bb});
  node["tourism"="viewpoint"]["name"](${bb});
);
out center tags;`;

  const tj = await overpass(trailsQ, `osm-trails-${region.id}.json`);
  await new Promise((r) => setTimeout(r, 1500));
  const pj = await overpass(poiQ, `osm-poi-${region.id}.json`);

  const nodes = new Map();
  const ways = new Map();
  const rels = [];
  for (const el of tj.elements) {
    if (el.type === 'node') nodes.set(el.id, [el.lon, el.lat]);
    else if (el.type === 'way') ways.set(el.id, el);
    else if (el.type === 'relation') rels.push(el);
  }

  // trasy (relacje) → lista kolorów i nazw dla każdej drogi
  const routes = [];
  const wayRoutes = new Map();
  for (const r of rels) {
    const t = r.tags || {};
    const idx = routes.length;
    routes.push({
      id: r.id,
      name: t['name:pl'] || t.name || t.ref || `Szlak ${r.id}`,
      ref: t.ref || '',
      color: trailColor(t),
      network: t.network || '',
      operator: t.operator || '',
    });
    for (const m of r.members || []) {
      if (m.type !== 'way') continue;
      if (!wayRoutes.has(m.ref)) wayRoutes.set(m.ref, new Set());
      wayRoutes.get(m.ref).add(idx);
    }
  }

  const inside = ([lon, lat]) => lon > w + 0.002 && lon < e - 0.002 && lat > s + 0.002 && lat < n - 0.002;

  // rozbij drogi na odcinki wewnątrz obszaru
  const runs = [];
  for (const way of ways.values()) {
    let cur = [];
    for (const nid of way.nodes) {
      const c = nodes.get(nid);
      if (c && inside(c)) cur.push(nid);
      else {
        if (cur.length > 1) runs.push({ way, ids: cur });
        cur = [];
      }
    }
    if (cur.length > 1) runs.push({ way, ids: cur });
  }

  // węzły grafu: końce odcinków i punkty wspólne dla wielu dróg
  const use = new Map();
  for (const r of runs) for (const id of r.ids) use.set(id, (use.get(id) || 0) + 1);
  for (const r of runs) {
    use.set(r.ids[0], (use.get(r.ids[0]) || 0) + 2);
    use.set(r.ids[r.ids.length - 1], (use.get(r.ids[r.ids.length - 1]) || 0) + 2);
  }

  const sample = makeSampler(dem);
  const gNodes = [];
  const gIndex = new Map();
  const nodeIdx = (id) => {
    if (!gIndex.has(id)) {
      const [lon, lat] = nodes.get(id);
      gIndex.set(id, gNodes.length);
      gNodes.push([+lon.toFixed(6), +lat.toFixed(6), +sample(lon, lat).toFixed(1)]);
    }
    return gIndex.get(id);
  };

  const R = 6371008.8;
  const dist = (a, b) => {
    const dLat = ((b[1] - a[1]) * Math.PI) / 180;
    const dLon = ((b[0] - a[0]) * Math.PI) / 180;
    const la = (a[1] * Math.PI) / 180, lb = (b[1] * Math.PI) / 180;
    const h = Math.sin(dLat / 2) ** 2 + Math.cos(la) * Math.cos(lb) * Math.sin(dLon / 2) ** 2;
    return 2 * R * Math.asin(Math.sqrt(h));
  };
  const step = region.zoom >= 13 ? 8 : 12;

  const edges = [];
  const seen = new Set();
  for (const { way, ids } of runs) {
    const t = way.tags || {};
    const rs = [...(wayRoutes.get(way.id) || [])];
    const colors = [...new Set(rs.map((i) => routes[i].color))];
    let start = 0;
    for (let i = 1; i < ids.length; i++) {
      if (use.get(ids[i]) > 1 || i === ids.length - 1) {
        const seg = ids.slice(start, i + 1);
        const key = seg[0] < seg[seg.length - 1] ? seg.join(',') : [...seg].reverse().join(',');
        start = i;
        if (seen.has(key)) continue;
        seen.add(key);
        // zagęszczenie geometrii i próbkowanie wysokości z DEM
        const g = [];
        for (let k = 0; k < seg.length; k++) {
          const c = nodes.get(seg[k]);
          if (k > 0) {
            const p = nodes.get(seg[k - 1]);
            const d = dist(p, c);
            const m = Math.floor(d / step);
            for (let j = 1; j <= m; j++) {
              const f = j / (m + 1);
              const lon = p[0] + (c[0] - p[0]) * f, lat = p[1] + (c[1] - p[1]) * f;
              g.push(+lon.toFixed(6), +lat.toFixed(6), +sample(lon, lat).toFixed(1));
            }
          }
          g.push(+c[0].toFixed(6), +c[1].toFixed(6), +sample(c[0], c[1]).toFixed(1));
        }
        edges.push({
          a: nodeIdx(seg[0]),
          b: nodeIdx(seg[seg.length - 1]),
          g,
          r: rs,
          c: colors.length ? colors : ['other'],
          sac: SAC[t.sac_scale] || 0,
          ow: t.oneway === 'yes' ? 1 : t.oneway === '-1' ? -1 : 0,
          name: t['name:pl'] || t.name || '',
          hw: t.highway || '',
          way: way.id,
        });
      }
    }
  }
  const usedRoutes = new Set(edges.flatMap((e) => e.r));
  console.log(`  szlaki: ${routes.length} tras (${usedRoutes.size} w obszarze), ${edges.length} krawędzi, ${gNodes.length} węzłów`);

  // punkty
  const pois = [];
  for (const el of pj.elements) {
    const t = el.tags || {};
    const lon = el.lon ?? el.center?.lon;
    const lat = el.lat ?? el.center?.lat;
    if (lon == null || !inside([lon, lat])) continue;
    let type;
    if (t.natural === 'peak' || t.natural === 'volcano') type = 'peak';
    else if (t.natural === 'saddle' || t.mountain_pass === 'yes') type = 'saddle';
    else if (t.tourism === 'alpine_hut') type = 'hut';
    else if (t.tourism === 'wilderness_hut' || t.amenity === 'shelter') type = 'shelter';
    else if (t.natural === 'water') type = 'lake';
    else if (t.waterway === 'waterfall') type = 'waterfall';
    else if (t.natural === 'cave_entrance') type = 'cave';
    else if (t.tourism === 'viewpoint') type = 'viewpoint';
    else continue;
    const name = t['name:pl'] || t.name;
    if (!name && type !== 'hut') continue;
    const osmEle = parseFloat(String(t.ele || '').replace(',', '.'));
    pois.push({
      t: type,
      n: name || 'Schronisko',
      lon: +lon.toFixed(6),
      lat: +lat.toFixed(6),
      e: Number.isFinite(osmEle) ? Math.round(osmEle) : null,
      d: +sample(lon, lat).toFixed(1),
      w: t.wikipedia || '',
      o: `${el.type}/${el.id}`,
    });
  }
  pois.sort((a, b) => (b.e ?? b.d) - (a.e ?? a.d));
  console.log(`  punkty: ${pois.length} (${pois.filter((p) => p.t === 'peak').length} szczytów)`);

  return {
    trails: { nodes: gNodes, edges, routes },
    pois,
  };
}


// ---------- pokrycie terenu (maska rastrowa z poligonów OSM) ----------
async function bakeLandcover(region, dem) {
  const [w, s, e, n] = region.bbox;
  const bb = `${s},${w},${n},${e}`;
  const q = `[out:json][timeout:300];
(
  way["natural"="water"](${bb});
  relation["natural"="water"](${bb});
  way["waterway"="riverbank"](${bb});
  way["landuse"="forest"](${bb});
  relation["landuse"="forest"](${bb});
  way["natural"="wood"](${bb});
  relation["natural"="wood"](${bb});
  way["natural"~"^(scree|bare_rock|shingle|cliff)$"](${bb});
  relation["natural"~"^(scree|bare_rock)$"](${bb});
  way["natural"="scrub"](${bb});
  relation["natural"="scrub"](${bb});
);
out geom;`;
  const j = await overpass(q, `osm-land-${region.id}.json`);
  const S = 2; // nadpróbkowanie względem siatki DEM
  const W = dem.width * S, H = dem.height * S;
  const img = new Uint8Array(W * H * 3);
  const toPx = (lon, lat) => [(lon2px(lon, dem.zoom) - dem.px0) * S, (lat2px(lat, dem.zoom) - dem.py0) * S];

  const fill = (rings, ch, val) => {
    // parzysto-nieparzyste wypełnianie liniami skanowania dla jednego obiektu (obsługuje dziury)
    const segs = [];
    let minY = Infinity, maxY = -Infinity;
    for (const ring of rings) {
      for (let i = 0; i < ring.length - 1; i++) {
        const a = ring[i], b = ring[i + 1];
        if (a[1] === b[1]) continue;
        segs.push([a[0], a[1], b[0], b[1]]);
        minY = Math.min(minY, a[1], b[1]);
        maxY = Math.max(maxY, a[1], b[1]);
      }
    }
    const y0 = Math.max(0, Math.floor(minY)), y1 = Math.min(H - 1, Math.ceil(maxY));
    const xs = [];
    for (let y = y0; y <= y1; y++) {
      const cy = y + 0.5;
      xs.length = 0;
      for (const [ax, ay, bx, by] of segs) {
        if ((ay <= cy && by > cy) || (by <= cy && ay > cy)) xs.push(ax + ((cy - ay) / (by - ay)) * (bx - ax));
      }
      xs.sort((p, q) => p - q);
      for (let k = 0; k + 1 < xs.length; k += 2) {
        const xa = Math.max(0, Math.ceil(xs[k] - 0.5)), xb = Math.min(W - 1, Math.floor(xs[k + 1] - 0.5));
        for (let x = xa; x <= xb; x++) {
          const i = (y * W + x) * 3 + ch;
          if (img[i] < val) img[i] = val;
        }
      }
    }
  };

  let counts = { water: 0, forest: 0, rock: 0, scrub: 0 };
  for (const el of j.elements) {
    const t = el.tags || {};
    let ch, val;
    if (t.natural === 'water' || t.waterway === 'riverbank') { ch = 0; val = 255; counts.water++; }
    else if (t.landuse === 'forest' || t.natural === 'wood') { ch = 1; val = 255; counts.forest++; }
    else if (t.natural === 'scrub') { ch = 2; val = 110; counts.scrub++; }
    else if (['scree', 'bare_rock', 'shingle', 'cliff'].includes(t.natural)) { ch = 2; val = 255; counts.rock++; }
    else continue;
    const rings = [];
    if (el.type === 'way' && el.geometry) {
      const g = el.geometry.map((p) => toPx(p.lon, p.lat));
      if (t.natural === 'cliff') continue; // linie – pomijamy
      if (g.length > 2) {
        if (g[0][0] !== g[g.length - 1][0] || g[0][1] !== g[g.length - 1][1]) g.push(g[0]);
        rings.push(g);
      }
    } else if (el.type === 'relation') {
      for (const m of el.members || []) {
        if (m.type !== 'way' || !m.geometry) continue;
        rings.push(m.geometry.map((p) => toPx(p.lon, p.lat)));
      }
    }
    if (rings.length) fill(rings, ch, val);
  }
  console.log(`  pokrycie: ${counts.water} wód, ${counts.forest} lasów, ${counts.scrub} zarośli, ${counts.rock} skał/piargów`);
  const png = new PNG({ width: W, height: H, colorType: 2 });
  png.data = Buffer.alloc(W * H * 4);
  for (let i = 0; i < W * H; i++) {
    png.data[i * 4] = img[i * 3];
    png.data[i * 4 + 1] = img[i * 3 + 1];
    png.data[i * 4 + 2] = img[i * 3 + 2];
    png.data[i * 4 + 3] = 255;
  }
  return PNG.sync.write(png, { colorType: 2, deflateLevel: 9 });
}

// ---------- główna pętla ----------
for (const region of regions) {
  console.log(`\n▲ ${region.name}`);
  progress(0.02, 'Model terenu (Terrarium)');
  const dem = await bakeDem(region);
  progress(0.3, 'Szlaki i punkty z OpenStreetMap');
  const osm = await bakeOsm(region, dem);
  progress(0.6, 'Pokrycie terenu z OpenStreetMap');
  await new Promise((r) => setTimeout(r, 1500));
  const land = await bakeLandcover(region, { ...dem, width: dem.width, height: dem.height });
  progress(0.92, 'Zapis danych');
  const dir = path.join(OUT, region.id);
  fs.mkdirSync(dir, { recursive: true });
  // podstawowy pakiet zastępuje ewentualne kafle LiDAR (inny model analityczny)
  fs.rmSync(path.join(dir, 'tiles'), { recursive: true, force: true });
  fs.writeFileSync(path.join(dir, 'dem.bin'), Buffer.from(dem.q.buffer));
  fs.writeFileSync(
    path.join(dir, 'meta.json'),
    JSON.stringify({
      id: region.id,
      width: dem.width,
      height: dem.height,
      zoom: dem.zoom,
      px0: dem.px0,
      py0: dem.py0,
      min: dem.min,
      max: dem.max,
      scale: 0.1,
      baked: new Date().toISOString(),
      sources: ['Terrarium elevation tiles (Mapzen / AWS Open Data)', '© OpenStreetMap contributors (ODbL)'],
    })
  );
  fs.writeFileSync(path.join(dir, 'trails.json'), JSON.stringify(osm.trails));
  fs.writeFileSync(path.join(dir, 'pois.json'), JSON.stringify(osm.pois));
  fs.writeFileSync(path.join(dir, 'landcover.png'), land);
  fs.writeFileSync(path.join(dir, 'install.json'), JSON.stringify({ level: 'base', date: new Date().toISOString() }));
  // obszar użytkownika: najwyższe szczyty jako opis (i nazwa, jeśli nie podano)
  const rf = path.join(dir, 'region.json');
  if (fs.existsSync(rf)) {
    const def = JSON.parse(fs.readFileSync(rf, 'utf8'));
    const peaks = osm.pois.filter((p) => p.t === 'peak' && p.n).sort((a, b) => (b.e ?? b.d) - (a.e ?? a.d));
    const names = [...new Set(peaks.map((p) => p.n))].slice(0, 3);
    if (peaks[0]) def.peak = { name: peaks[0].n, ele: Math.round(peaks[0].e ?? peaks[0].d) };
    if (!def.subtitle) def.subtitle = names.length ? names.join(', ') : 'obszar bez nazwanych szczytów';
    if (!def.name) def.name = peaks[0] ? `Okolice: ${peaks[0].n}` : 'Nowy obszar';
    fs.writeFileSync(rf, JSON.stringify(def, null, 2));
  }
  progress(1, 'Gotowe');
  const size = (f) => (fs.statSync(path.join(dir, f)).size / 1024 / 1024).toFixed(2) + ' MB';
  console.log(`  zapisano: dem ${size('dem.bin')}, szlaki ${size('trails.json')}, punkty ${size('pois.json')}, pokrycie ${size('landcover.png')}`);
  await new Promise((r) => setTimeout(r, 2000));
}
