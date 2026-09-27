// PUWG-1992 (EPSG:2180): odwzorowanie Gaussa-Krügera na elipsoidzie GRS80, szereg Krügera (dokładność < 1 mm).
const a = 6378137;
const f = 1 / 298.257222101;
const n = f / (2 - f);
const A = (a / (1 + n)) * (1 + (n * n) / 4 + (n ** 4) / 64);
const al = [n / 2 - (2 * n * n) / 3 + (5 * n ** 3) / 16, (13 * n * n) / 48 - (3 * n ** 3) / 5, (61 * n ** 3) / 240];
const k0 = 0.9993, E0 = 500000, N0 = -5300000, lon0 = (19 * Math.PI) / 180;
const c = (2 * Math.sqrt(n)) / (1 + n);

/** lon/lat (stopnie) → [E (easting), N (northing)] w metrach EPSG:2180. */
export function toPuwg(lon, lat) {
  const phi = (lat * Math.PI) / 180, dl = (lon * Math.PI) / 180 - lon0;
  const s = Math.sin(phi);
  const t = Math.sinh(Math.atanh(s) - c * Math.atanh(c * s));
  const xi = Math.atan(t / Math.cos(dl));
  const eta = Math.atanh(Math.sin(dl) / Math.sqrt(1 + t * t));
  let E = eta, N = xi;
  for (let j = 1; j <= 3; j++) {
    E += al[j - 1] * Math.cos(2 * j * xi) * Math.sinh(2 * j * eta);
    N += al[j - 1] * Math.sin(2 * j * xi) * Math.cosh(2 * j * eta);
  }
  return [E0 + k0 * A * E, N0 + k0 * A * N];
}
