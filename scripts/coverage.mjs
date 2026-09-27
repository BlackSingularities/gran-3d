// Generuje public/coverage.json: uproszczone granice krajów, w których GRAŃ ma źródła danych
// (Natural Earth 1:50m, domena publiczna). Uruchamiane jednorazowo przy zmianie zasięgu.
import fs from 'node:fs';
import path from 'node:path';
import { CACHE_DIR, ROOT } from './lib/common.mjs';

const SRC = 'https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson';
const file = path.join(CACHE_DIR, 'ne50.geojson');
if (!fs.existsSync(file)) fs.writeFileSync(file, Buffer.from(await (await fetch(SRC)).arrayBuffer()));
const ne = JSON.parse(fs.readFileSync(file, 'utf8'));

const COUNTRIES = {
  POL: { code: 'PL', name: 'Polska', lidar: 'pl', ortho: 'pl' },
  CZE: { code: 'CZ', name: 'Czechy', lidar: 'cz', ortho: 'cz' },
  SVK: { code: 'SK', name: 'Słowacja', lidar: null, ortho: 'sk' },
};

// Douglas–Peucker
function simplify(pts, tol) {
  if (pts.length < 4) return pts;
  const keep = new Uint8Array(pts.length);
  keep[0] = keep[pts.length - 1] = 1;
  const stack = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop();
    let md = 0, mi = -1;
    const [ax, ay] = pts[a], [bx, by] = pts[b];
    const dx = bx - ax, dy = by - ay, L = Math.hypot(dx, dy) || 1e-12;
    for (let i = a + 1; i < b; i++) {
      const d = Math.abs((pts[i][0] - ax) * dy - (pts[i][1] - ay) * dx) / L;
      if (d > md) { md = d; mi = i; }
    }
    if (md > tol) { keep[mi] = 1; stack.push([a, mi], [mi, b]); }
  }
  return pts.filter((_, i) => keep[i]).map(([x, y]) => [+x.toFixed(4), +y.toFixed(4)]);
}

const out = [];
for (const f of ne.features) {
  const iso = f.properties.ADM0_A3 || f.properties.ISO_A3;
  const c = COUNTRIES[iso];
  if (!c) continue;
  const polys = f.geometry.type === 'Polygon' ? [f.geometry.coordinates] : f.geometry.coordinates;
  // tylko główny obszar lądowy (największy pierścień)
  const rings = polys.map((p) => p[0]).sort((a, b) => b.length - a.length);
  // pierścień zamknięty: upraszczamy dwie połówki osobno (początek = koniec)
  const r = rings[0];
  const mid = r.length >> 1;
  const ring = [...simplify(r.slice(0, mid + 1), 0.01).slice(0, -1), ...simplify(r.slice(mid), 0.01)];
  out.push({ ...c, ring });
}
fs.writeFileSync(path.join(ROOT, 'public', 'coverage.json'), JSON.stringify({ source: 'Natural Earth 1:50m (public domain)', countries: out }));
console.log(out.map((c) => `${c.code}: ${c.ring.length} pkt`).join(', '));
