// Logika obszarów wybieranych przez użytkownika (wspólna z src/core/area.ts).
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './common.mjs';

export const LIMITS = { normal: 10000, high: 10000 };
export const COVERAGE = JSON.parse(fs.readFileSync(path.join(ROOT, 'public', 'coverage.json'), 'utf8')).countries;

export function areaKm2([w, s, e, n]) {
  const lat = ((s + n) / 2) * (Math.PI / 180);
  return (e - w) * 111.32 * Math.cos(lat) * (n - s) * 110.57;
}

export function inRing(lon, lat, ring) {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i], [xj, yj] = ring[j];
    if (yi > lat !== yj > lat && lon < ((xj - xi) * (lat - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Udział powierzchni prostokąta w poszczególnych krajach (próbkowanie siatką 12×12). */
export function coverageOf([w, s, e, n]) {
  const frac = {};
  let any = 0;
  const N = 12;
  for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) {
    const lon = w + ((i + 0.5) / N) * (e - w), lat = s + ((j + 0.5) / N) * (n - s);
    const c = COVERAGE.find((k) => inRing(lon, lat, k.ring));
    if (c) { frac[c.code] = (frac[c.code] ?? 0) + 1 / (N * N); any += 1 / (N * N); }
  }
  const lidar = COVERAGE.filter((c) => c.lidar).reduce((s, c) => s + (frac[c.code] ?? 0), 0);
  return { frac, any, lidar };
}

/** Walidacja wyboru; zwraca komunikat błędu albo null. */
export function validate(bbox, quality) {
  if (!Array.isArray(bbox) || bbox.length !== 4 || bbox.some((v) => !Number.isFinite(v))) return 'Nieprawidłowy obszar.';
  const [w, s, e, n] = bbox;
  if (e <= w || n <= s) return 'Nieprawidłowy obszar.';
  const km2 = areaKm2(bbox);
  if (km2 < 1) return 'Obszar jest za mały (min. 1 km²).';
  if (km2 > LIMITS[quality]) return `Obszar za duży: ${Math.round(km2)} km² (limit ${LIMITS[quality]} km²).`;
  const cov = coverageOf(bbox);
  if (cov.any < 0.5) return 'Obszar leży poza obsługiwaną częścią Europy.';
  if (quality === 'high' && cov.lidar <= 0) return 'LiDAR musi obejmować choć część obszaru (Polska lub Czechy).';
  return null;
}

/** Definicja kwadratu (region.json) dla skryptów wypiekających. */
export function deriveRegion({ id, name, bbox, quality }) {
  const cov = coverageOf(bbox);
  const km2 = areaKm2(bbox);
  const present = (code) => (cov.frac[code] ?? 0) > 0.01;
  const lidar = COVERAGE.filter((c) => c.lidar && present(c.code)).sort((a, b) => (cov.frac[b.code] ?? 0) - (cov.frac[a.code] ?? 0)).map((c) => c.lidar);
  const ortho = ['sk', 'cz', 'pl'].filter((o) => COVERAGE.some((c) => c.ortho === o && present(c.code)));
  return {
    id,
    name: name || '',
    subtitle: '',
    custom: true,
    quality,
    created: new Date().toISOString(),
    countries: COVERAGE.filter((c) => present(c.code)).map((c) => c.code),
    bbox: bbox.map((v) => +v.toFixed(5)),
    zoom: km2 > 500 ? 12 : 13,
    ...(quality === 'high' && lidar.length ? { hd: { zoom: 15, lidar, ortho } } : { ortho }),
  };
}

export function slug(name) {
  const base = (name || 'obszar')
    .toLowerCase()
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/ł/g, 'l')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 32) || 'obszar';
  return `${base}-${Math.random().toString(36).slice(2, 6)}`;
}
