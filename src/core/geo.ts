export const EARTH_R = 6371008.8;
export const DEG = Math.PI / 180;

export const lon2px = (lon: number, z: number) => ((lon + 180) / 360) * 256 * 2 ** z;
export const lat2px = (lat: number, z: number) => {
  const r = lat * DEG;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 256 * 2 ** z;
};
export const px2lon = (px: number, z: number) => (px / (256 * 2 ** z)) * 360 - 180;
export const px2lat = (py: number, z: number) => {
  const n = Math.PI - (2 * Math.PI * py) / (256 * 2 ** z);
  return Math.atan(Math.sinh(n)) / DEG;
};

export function haversine(lon1: number, lat1: number, lon2: number, lat2: number) {
  const dLat = (lat2 - lat1) * DEG;
  const dLon = (lon2 - lon1) * DEG;
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(lat1 * DEG) * Math.cos(lat2 * DEG) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_R * Math.asin(Math.min(1, Math.sqrt(h)));
}

/** Azymut w stopniach (0 = północ, zgodnie z ruchem wskazówek zegara). */
export function bearing(lon1: number, lat1: number, lon2: number, lat2: number) {
  const y = Math.sin((lon2 - lon1) * DEG) * Math.cos(lat2 * DEG);
  const x =
    Math.cos(lat1 * DEG) * Math.sin(lat2 * DEG) -
    Math.sin(lat1 * DEG) * Math.cos(lat2 * DEG) * Math.cos((lon2 - lon1) * DEG);
  return ((Math.atan2(y, x) / DEG) + 360) % 360;
}

/** Spadek linii wzroku z powodu krzywizny Ziemi z uwzględnieniem refrakcji (k = 0,13). */
export const curvatureDrop = (d: number) => ((d * d) / (2 * EARTH_R)) * (1 - 0.13);

// ---------- formatowanie (pl-PL) ----------
const nf0 = new Intl.NumberFormat('pl-PL', { maximumFractionDigits: 0 });
const nf1 = new Intl.NumberFormat('pl-PL', { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const nf2 = new Intl.NumberFormat('pl-PL', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

export const fmtInt = (v: number) => nf0.format(v);
export const fmt1 = (v: number) => nf1.format(v);

export function fmtDist(m: number) {
  if (!Number.isFinite(m)) return '—';
  if (m < 1000) return `${nf0.format(m)} m`;
  if (m < 10000) return `${nf2.format(m / 1000)} km`;
  return `${nf1.format(m / 1000)} km`;
}

export const fmtEle = (m: number) => (Number.isFinite(m) ? `${nf0.format(m)} m` : '—');

export function fmtTime(h: number) {
  if (!Number.isFinite(h)) return '—';
  const total = Math.round(h * 60 / 5) * 5;
  const hh = Math.floor(total / 60);
  const mm = total % 60;
  if (hh === 0) return `${mm} min`;
  return mm ? `${hh} h ${String(mm).padStart(2, '0')} min` : `${hh} h`;
}

export function fmtSigned(m: number, unit = 'm') {
  const s = m > 0.5 ? '+' : m < -0.5 ? '−' : '±';
  return `${s}${nf0.format(Math.abs(m))} ${unit}`;
}

export function fmtDms(v: number, pos: string, neg: string) {
  const a = Math.abs(v);
  const d = Math.floor(a);
  const mFloat = (a - d) * 60;
  const m = Math.floor(mFloat);
  const s = (mFloat - m) * 60;
  return `${d}°${String(m).padStart(2, '0')}′${nf1.format(s).padStart(4, '0')}″${v >= 0 ? pos : neg}`;
}

export const fmtCoords = (lon: number, lat: number) => `${fmtDms(lat, 'N', 'S')}  ${fmtDms(lon, 'E', 'W')}`;
export const fmtDecimal = (lon: number, lat: number) => `${lat.toFixed(5)}, ${lon.toFixed(5)}`;

const DIRS = ['N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE', 'S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW'];
export const compassDir = (deg: number) => DIRS[Math.round((((deg % 360) + 360) % 360) / 22.5) % 16];

export const clamp = (v: number, a: number, b: number) => (v < a ? a : v > b ? b : v);
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
