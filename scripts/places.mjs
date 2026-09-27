// Dodaje nazwy miejscowości (OSM place=city/town/village/hamlet) do już pobranych obszarów,
// bez ponownego wypiekania terenu. Nowe obszary dostają je automatycznie w scripts/bake.mjs.
//   node scripts/places.mjs            – wszystkie obszary w data/
//   node scripts/places.mjs <id> …     – wybrane
import fs from 'node:fs';
import path from 'node:path';
import { DATA_DIR } from './lib/common.mjs';

const UA = 'gran-trail-atlas/1.0 (https://github.com/BlackSingularities/gran-3d)';
const lon2px = (lon, z) => ((lon + 180) / 360) * 256 * 2 ** z;
const lat2px = (lat, z) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 256 * 2 ** z;
};

async function overpass(q) {
  for (const url of ['https://overpass-api.de/api/interpreter', 'https://overpass.kumi.systems/api/interpreter']) {
    for (let i = 0; i < 3; i++) {
      try {
        const r = await fetch(url, { method: 'POST', headers: { 'User-Agent': UA, 'Content-Type': 'application/x-www-form-urlencoded' }, body: 'data=' + encodeURIComponent(q) });
        if (r.ok) return await r.json();
      } catch { /* ponów */ }
      await new Promise((r) => setTimeout(r, 3000 * (i + 1)));
    }
  }
  throw new Error('Overpass nie odpowiada');
}

const ids = process.argv.slice(2).filter((a) => !a.startsWith('-'));
const dirs = ids.length ? ids : fs.readdirSync(DATA_DIR).filter((d) => fs.existsSync(path.join(DATA_DIR, d, 'region.json')));
for (const id of dirs) {
  const dir = path.join(DATA_DIR, id);
  const def = JSON.parse(fs.readFileSync(path.join(dir, 'region.json'), 'utf8'));
  const meta = JSON.parse(fs.readFileSync(path.join(dir, 'meta.json'), 'utf8'));
  const raw = fs.readFileSync(path.join(dir, 'dem.bin'));
  const dem = new Uint16Array(raw.buffer, raw.byteOffset, raw.length / 2);
  const sample = (lon, lat) => {
    const x = Math.max(0, Math.min(meta.width - 1, Math.round(lon2px(lon, meta.zoom) - meta.px0 - 0.5)));
    const y = Math.max(0, Math.min(meta.height - 1, Math.round(lat2px(lat, meta.zoom) - meta.py0 - 0.5)));
    return dem[y * meta.width + x] * (meta.scale ?? 0.1);
  };
  const [w, s, e, n] = def.bbox;
  const j = await overpass(`[out:json][timeout:120];node["place"~"^(city|town|village|hamlet)$"]["name"](${s},${w},${n},${e});out body;`);
  const pois = JSON.parse(fs.readFileSync(path.join(dir, 'pois.json'), 'utf8')).filter((p) => p.t !== 'place');
  let added = 0;
  for (const el of j.elements) {
    const t = el.tags || {};
    if (el.lon < w || el.lon > e || el.lat < s || el.lat > n) continue;
    pois.push({
      t: 'place', n: t['name:pl'] || t.name, lon: +el.lon.toFixed(6), lat: +el.lat.toFixed(6),
      e: null, d: +sample(el.lon, el.lat).toFixed(1), w: t.wikipedia || '', o: `node/${el.id}`,
      k: t.place, pop: parseInt(String(t.population || '').replace(/\D/g, ''), 10) || 0,
    });
    added++;
  }
  fs.writeFileSync(path.join(dir, 'pois.json'), JSON.stringify(pois));
  console.log(`${def.name || id}: ${added} miejscowości`);
  await new Promise((r) => setTimeout(r, 1500));
}
