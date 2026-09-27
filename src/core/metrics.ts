import { haversine } from './geo';

/** Czas przejścia wg normy DIN 33466 (stosowanej na drogowskazach alpejskich), w godzinach. */
export function dinTime(horiz: number, up: number, down: number) {
  const th = horiz / 4000;
  const tv = up / 300 + down / 500;
  return Math.max(th, tv) + Math.min(th, tv) / 2;
}

/** Aktywny model czasu przejścia i tempo (mnożnik czasu: 0,8 szybkie … 1,25 spokojne). */
export const timeModel: { kind: 'pttk' | 'din'; pace: number } = { kind: 'pttk', pace: 1 };

export const TIME_MODEL_LABEL = { pttk: 'reguła PTTK', din: 'DIN 33466' };

/**
 * Czas przejścia (h). PTTK: 15 min/km + 1 min/10 m podejścia + 1 min/17 m zejścia
 * (zgodne z czasami na tatrzańskich drogowskazach); DIN 33466: normy alpejskie.
 */
export function walkTime(horiz: number, up: number, down: number) {
  const base = timeModel.kind === 'din' ? dinTime(horiz, up, down) : horiz / 4000 + up / 600 + down / 1000;
  return base * timeModel.pace;
}

/** Mnożnik czasu dla skali trudności SAC (T1–T6). */
export const sacFactor = (sac: number) => [1, 1, 1.03, 1.12, 1.28, 1.5, 1.8][sac] ?? 1;

export const SAC_LABEL = ['—', 'T1 turystyczna', 'T2 górska', 'T3 wymagająca', 'T4 wysokogórska', 'T5 wymagająca wysokogórska', 'T6 trudna wysokogórska'];

/** Suma podejść/zejść z histerezą – eliminuje szum modelu terenu. */
export function climb(e: ArrayLike<number>, from = 0, to = e.length - 1, threshold = 2.5) {
  let up = 0, down = 0;
  let ref = e[from];
  for (let i = from + 1; i <= to; i++) {
    const d = e[i] - ref;
    if (d >= threshold) { up += d; ref = e[i]; }
    else if (d <= -threshold) { down -= d; ref = e[i]; }
  }
  // domknięcie ostatniego odcinka
  const d = e[to] - ref;
  if (d > 0) up += d; else down -= d;
  return { up, down };
}

export interface Track {
  n: number;
  lon: Float64Array;
  lat: Float64Array;
  x: Float32Array;
  z: Float32Array;
  e: Float32Array;
  /** skumulowany dystans poziomy */
  d2: Float32Array;
  /** skumulowany dystans rzeczywisty (po powierzchni, 3D) */
  d3: Float32Array;
  /** skumulowany czas (h) */
  t: Float32Array;
  /** indeks koloru szlaku dla punktu (kolory w `palette`) */
  color: Uint8Array;
  sac: Uint8Array;
}

export interface TrackStats {
  len2: number;
  len3: number;
  up: number;
  down: number;
  time: number;
  got: number;
  maxEle: number;
  minEle: number;
  maxEleIdx: number;
  maxSlope: number;
  maxGrade: number;
  avgGrade: number;
  sacMax: number;
  straight: number;
  difficulty: 0 | 1 | 2 | 3;
}

export const TRAIL_COLORS = ['red', 'blue', 'green', 'yellow', 'black', 'other', 'offtrail', 'orange', 'purple', 'brown'] as const;
export type TrailColor = (typeof TRAIL_COLORS)[number];
export const colorIndex = (c: string) => Math.max(0, TRAIL_COLORS.indexOf(c as TrailColor));

export function newTrack(n: number): Track {
  return {
    n,
    lon: new Float64Array(n),
    lat: new Float64Array(n),
    x: new Float32Array(n),
    z: new Float32Array(n),
    e: new Float32Array(n),
    d2: new Float32Array(n),
    d3: new Float32Array(n),
    t: new Float32Array(n),
    color: new Uint8Array(n),
    sac: new Uint8Array(n),
  };
}

/** Wylicza skumulowane dystanse i czas; `timeFactor` np. 1,35 dla marszu poza szlakiem. */
export function finalizeTrack(tr: Track, timeFactor = 1) {
  const { n, lon, lat, e } = tr;
  tr.d2[0] = tr.d3[0] = tr.t[0] = 0;
  for (let i = 1; i < n; i++) {
    const h = haversine(lon[i - 1], lat[i - 1], lon[i], lat[i]);
    const dh = e[i] - e[i - 1];
    tr.d2[i] = tr.d2[i - 1] + h;
    tr.d3[i] = tr.d3[i - 1] + Math.hypot(h, dh);
    const dt = walkTime(h, Math.max(0, dh), Math.max(0, -dh)) * sacFactor(tr.sac[i]) * timeFactor;
    tr.t[i] = tr.t[i - 1] + dt;
  }
  // skalowanie czasu, aby suma była zgodna z modelem liczonym dla całości (bez szumu DEM)
  const { up, down } = climb(e);
  const din = walkTime(tr.d2[n - 1], up, down) * timeFactor;
  let sacW = 0;
  for (let i = 1; i < n; i++) sacW += sacFactor(tr.sac[i]) * (tr.d2[i] - tr.d2[i - 1]);
  const target = din * (tr.d2[n - 1] > 0 ? sacW / tr.d2[n - 1] : 1);
  const k = tr.t[n - 1] > 0 ? target / tr.t[n - 1] : 1;
  for (let i = 1; i < n; i++) tr.t[i] *= k;
  return tr;
}

export function trackStats(tr: Track): TrackStats {
  const { n, e, d2 } = tr;
  const { up, down } = climb(e);
  let maxEle = -Infinity, minEle = Infinity, maxEleIdx = 0, sacMax = 0;
  for (let i = 0; i < n; i++) {
    if (e[i] > maxEle) { maxEle = e[i]; maxEleIdx = i; }
    if (e[i] < minEle) minEle = e[i];
    if (tr.sac[i] > sacMax) sacMax = tr.sac[i];
  }
  // maksymalne nachylenie w oknie ~100 m (odporne na szum modelu i przesunięcia śladu)
  let maxGrade = 0;
  let j = 0;
  for (let i = 0; i < n; i++) {
    while (j < n - 1 && d2[j] - d2[i] < 100) j++;
    const dd = d2[j] - d2[i];
    if (dd >= 70) {
      const g = Math.abs(e[j] - e[i]) / dd;
      if (g > maxGrade) maxGrade = g;
    }
  }
  const len2 = d2[n - 1];
  const straight = haversine(tr.lon[0], tr.lat[0], tr.lon[n - 1], tr.lat[n - 1]);
  const avgGrade = len2 > 0 ? (up + down) / len2 : 0;
  const maxSlope = (Math.atan(maxGrade) * 180) / Math.PI;
  let difficulty: 0 | 1 | 2 | 3 = 0;
  const score = sacMax * 1.2 + maxSlope / 10 + up / 700 + len2 / 12000;
  if (score > 4) difficulty = 1;
  if (score > 6.5) difficulty = 2;
  if (score > 9) difficulty = 3;
  return {
    len2,
    len3: tr.d3[n - 1],
    up,
    down,
    time: tr.t[n - 1],
    got: Math.round(len2 / 1000) + Math.round(up / 100),
    maxEle,
    minEle,
    maxEleIdx,
    maxSlope,
    maxGrade,
    avgGrade,
    sacMax,
    straight,
    difficulty,
  };
}

export const DIFFICULTY = ['łatwa', 'umiarkowana', 'trudna', 'bardzo trudna'];

/** Nachylenie odcinka → klasa koloru (0..5) używana w profilu. */
export function gradeClass(g: number) {
  const a = Math.abs(g);
  if (a < 0.05) return 0;
  if (a < 0.12) return 1;
  if (a < 0.2) return 2;
  if (a < 0.3) return 3;
  if (a < 0.45) return 4;
  return 5;
}
