// Logika obszarów wybieranych na mapie (wspólna z scripts/lib/area.mjs).
export type Quality = 'normal' | 'high';
export type BBox = [number, number, number, number];

export const LIMITS: Record<Quality, number> = { normal: 1200, high: 300 };

export interface CoverageCountry {
  code: string;
  name: string;
  lidar: string | null;
  ortho: string | null;
  ring: [number, number][];
}

let coverage: CoverageCountry[] = [];
export async function loadCoverage() {
  if (coverage.length) return coverage;
  const j = await (await fetch(`${import.meta.env.BASE_URL}coverage.json`)).json();
  coverage = j.countries;
  return coverage;
}
export const getCoverage = () => coverage;

export function areaKm2([w, s, e, n]: BBox) {
  const lat = ((s + n) / 2) * (Math.PI / 180);
  const wk = (e - w) * 111.32 * Math.cos(lat), hk = (n - s) * 110.57;
  return { km2: wk * hk, wk, hk };
}

export function inRing(lon: number, lat: number, ring: [number, number][]) {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i], [xj, yj] = ring[j];
    if (yi > lat !== yj > lat && lon < ((xj - xi) * (lat - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

export function countryAt(lon: number, lat: number) {
  return coverage.find((c) => inRing(lon, lat, c.ring)) ?? null;
}

/** Udział powierzchni prostokąta w krajach i w zasięgu LiDAR. */
export function coverageOf([w, s, e, n]: BBox) {
  const frac: Record<string, number> = {};
  let any = 0;
  const N = 12;
  for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) {
    const c = countryAt(w + ((i + 0.5) / N) * (e - w), s + ((j + 0.5) / N) * (n - s));
    if (c) {
      frac[c.code] = (frac[c.code] ?? 0) + 1 / (N * N);
      any += 1 / (N * N);
    }
  }
  const lidar = coverage.filter((c) => c.lidar).reduce((s, c) => s + (frac[c.code] ?? 0), 0);
  return { frac, any, lidar };
}

export function validate(bbox: BBox, quality: Quality): string | null {
  const { km2 } = areaKm2(bbox);
  if (km2 < 1) return 'Obszar jest za mały (min. 1 km²).';
  if (km2 > LIMITS[quality]) return `Obszar za duży dla tej jakości – zmniejsz zaznaczenie do ${LIMITS[quality]} km².`;
  const cov = coverageOf(bbox);
  if (cov.any < 0.5) return 'Zaznacz obszar w zasięgu danych: Polska, Czechy lub Słowacja.';
  if (quality === 'high' && cov.lidar < 0.05) return 'Wysoka jakość wymaga danych LiDAR – dostępne w Polsce i Czechach.';
  return null;
}

/** Szacunkowy rozmiar (MB) i czas pobierania (min). */
export function estimate(bbox: BBox, quality: Quality) {
  const { km2 } = areaKm2(bbox);
  const cov = coverageOf(bbox);
  const base = 1 + km2 * (km2 > 500 ? 0.008 : 0.02);
  const mb = quality === 'high' ? base + km2 * cov.lidar * 0.16 : base;
  const min = quality === 'high' ? 1.5 + km2 * cov.lidar * 0.015 : 1 + km2 / 800;
  return { mb, min, lidar: cov.lidar };
}
