import { DEG } from './geo';

/**
 * Położenie Słońca (algorytm NOAA, dokładność ~0,01°).
 * Zwraca azymut (0 = N, zgodnie z zegarem) i wysokość nad horyzontem w stopniach.
 */
export function sunPosition(date: Date, lat: number, lon: number) {
  const jd = date.getTime() / 86400000 + 2440587.5;
  const t = (jd - 2451545) / 36525;
  const L0 = (280.46646 + t * (36000.76983 + t * 0.0003032)) % 360;
  const M = 357.52911 + t * (35999.05029 - 0.0001537 * t);
  const e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
  const C =
    Math.sin(M * DEG) * (1.914602 - t * (0.004817 + 0.000014 * t)) +
    Math.sin(2 * M * DEG) * (0.019993 - 0.000101 * t) +
    Math.sin(3 * M * DEG) * 0.000289;
  const trueLong = L0 + C;
  const omega = 125.04 - 1934.136 * t;
  const lambda = trueLong - 0.00569 - 0.00478 * Math.sin(omega * DEG);
  const eps0 = 23 + (26 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60) / 60;
  const eps = eps0 + 0.00256 * Math.cos(omega * DEG);
  const decl = Math.asin(Math.sin(eps * DEG) * Math.sin(lambda * DEG));
  const y = Math.tan((eps / 2) * DEG) ** 2;
  const eqTime =
    4 / DEG *
    (y * Math.sin(2 * L0 * DEG) -
      2 * e * Math.sin(M * DEG) +
      4 * e * y * Math.sin(M * DEG) * Math.cos(2 * L0 * DEG) -
      0.5 * y * y * Math.sin(4 * L0 * DEG) -
      1.25 * e * e * Math.sin(2 * M * DEG));
  const minutes = date.getUTCHours() * 60 + date.getUTCMinutes() + date.getUTCSeconds() / 60;
  const tst = (minutes + eqTime + 4 * lon + 1440) % 1440;
  let ha = tst / 4 - 180;
  if (ha < -180) ha += 360;
  const latR = lat * DEG;
  const cosZen = Math.sin(latR) * Math.sin(decl) + Math.cos(latR) * Math.cos(decl) * Math.cos(ha * DEG);
  const zen = Math.acos(Math.max(-1, Math.min(1, cosZen)));
  let az = Math.acos(
    Math.max(-1, Math.min(1, (Math.sin(latR) * Math.cos(zen) - Math.sin(decl)) / (Math.cos(latR) * Math.sin(zen))))
  ) / DEG;
  az = ha > 0 ? (az + 180) % 360 : (540 - az) % 360;
  let alt = 90 - zen / DEG;
  // refrakcja atmosferyczna w pobliżu horyzontu
  if (alt > -0.575) alt += 1.02 / Math.tan((alt + 10.3 / (alt + 5.11)) * DEG) / 60;
  return { azimuth: az, altitude: alt };
}

/** Przesunięcie strefy Europe/Warsaw (min) dla podanej daty. */
export function warsawOffsetMin(date: Date) {
  const f = new Intl.DateTimeFormat('en-US', { timeZone: 'Europe/Warsaw', timeZoneName: 'shortOffset' });
  const part = f.formatToParts(date).find((p) => p.type === 'timeZoneName')?.value ?? 'GMT+1';
  const m = part.match(/GMT([+-]\d+)(?::(\d+))?/);
  return m ? Number(m[1]) * 60 + Math.sign(Number(m[1])) * Number(m[2] ?? 0) : 60;
}

/** Data w czasie polskim: dzień (YYYY-MM-DD) + godzina dziesiętna → Date (UTC). */
export function warsawDate(day: string, hour: number) {
  const [y, mo, d] = day.split('-').map(Number);
  const guess = new Date(Date.UTC(y, mo - 1, d, 12));
  const off = warsawOffsetMin(guess);
  return new Date(Date.UTC(y, mo - 1, d, 0, 0) + (hour * 60 - off) * 60000);
}

/** Wschód i zachód (godziny lokalne) – wyszukiwanie numeryczne. */
export function sunTimes(day: string, lat: number, lon: number) {
  const alt = (h: number) => sunPosition(warsawDate(day, h), lat, lon).altitude + 0.833;
  const find = (a: number, b: number) => {
    let fa = alt(a);
    for (let h = a + 0.25; h <= b; h += 0.25) {
      const fb = alt(h);
      if (Math.sign(fa) !== Math.sign(fb)) {
        let lo = h - 0.25, hi = h;
        for (let i = 0; i < 20; i++) {
          const m = (lo + hi) / 2;
          if (Math.sign(alt(m)) === Math.sign(alt(lo))) lo = m; else hi = m;
        }
        return (lo + hi) / 2;
      }
      fa = fb;
    }
    return NaN;
  };
  const rise = find(0, 12.5);
  const set = find(12.5, 24);
  let noon = 12, best = -90;
  for (let h = 9; h <= 15; h += 0.05) {
    const a = alt(h);
    if (a > best) { best = a; noon = h; }
  }
  return { rise, set, noon, maxAlt: best - 0.833 };
}

export const fmtHour = (h: number) => {
  if (!Number.isFinite(h)) return '—';
  const m = Math.round(h * 60);
  return `${String(Math.floor(m / 60) % 24).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
};

/** Wektor kierunku do Słońca w układzie świata (x – wschód, y – góra, z – południe). */
export function sunVector(az: number, alt: number): [number, number, number] {
  const a = az * DEG, h = alt * DEG;
  return [Math.sin(a) * Math.cos(h), Math.sin(h), -Math.cos(a) * Math.cos(h)];
}

/** Sezonowa granica śniegu (m) dla Karpat/Sudetów – przybliżenie klimatologiczne. */
export function seasonalSnowline(date: Date) {
  const doy = (Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()) - Date.UTC(date.getUTCFullYear(), 0, 0)) / 86400000;
  // węzły: [dzień roku, wysokość granicy śniegu]
  const knots: [number, number][] = [
    [0, 700], [45, 650], [80, 1000], [110, 1500], [140, 1950], [170, 2350], [200, 2900],
    [260, 2900], [290, 2150], [315, 1600], [335, 1100], [366, 700],
  ];
  for (let i = 1; i < knots.length; i++) {
    if (doy <= knots[i][0]) {
      const [d0, v0] = knots[i - 1], [d1, v1] = knots[i];
      return v0 + ((v1 - v0) * (doy - d0)) / (d1 - d0);
    }
  }
  return 700;
}
