import { DEG, lat2px, lon2px, px2lat, px2lon, EARTH_R } from './geo';
import { FRAME_Z, makeFrame, type WorldFrame } from './frame';

export interface DemMeta {
  id: string;
  width: number;
  height: number;
  zoom: number;
  px0: number;
  py0: number;
  min: number;
  max: number;
  scale: number;
}

/**
 * Numeryczny model terenu w siatce Web Mercator.
 * Układy współrzędnych:
 *  - siatka (gx, gy): środki komórek w liczbach całkowitych, gy rośnie na południe,
 *  - świat (x, y, z): metry, x na wschód, z na południe, y = wysokość n.p.m., środek regionu w (0, 0).
 */
export class Dem {
  readonly w: number;
  readonly h: number;
  readonly zoom: number;
  readonly px0: number;
  readonly py0: number;
  readonly min: number;
  readonly max: number;
  readonly data: Float32Array;
  /** metry na komórkę siatki (dla szerokości środka regionu) */
  readonly mpp: number;
  readonly centerLat: number;
  readonly centerLon: number;
  /** ramka świata i położenie jej początku w siatce tego modelu */
  readonly frame: WorldFrame;
  readonly gx0: number;
  readonly gy0: number;

  constructor(meta: DemMeta, raw: Uint16Array, frame?: WorldFrame) {
    this.w = meta.width;
    this.h = meta.height;
    this.zoom = meta.zoom;
    this.px0 = meta.px0;
    this.py0 = meta.py0;
    this.min = meta.min;
    this.max = meta.max;
    this.data = new Float32Array(raw.length);
    for (let i = 0; i < raw.length; i++) this.data[i] = raw[i] * meta.scale;
    this.centerLon = px2lon(this.px0 + this.w / 2, this.zoom);
    this.centerLat = px2lat(this.py0 + this.h / 2, this.zoom);
    // bez ramki: ramka wyśrodkowana na regionie (zachowanie „jednego obszaru”)
    this.frame = frame ?? makeFrame(this.centerLon, this.centerLat);
    const k = 2 ** (FRAME_Z - this.zoom);
    this.mpp = this.frame.mpp20 * k;
    this.gx0 = this.frame.ox / k - this.px0 - 0.5;
    this.gy0 = this.frame.oy / k - this.py0 - 0.5;
    void EARTH_R;
    void DEG;
  }

  get widthM() { return this.w * this.mpp; }
  get heightM() { return this.h * this.mpp; }

  gxToLon(gx: number) { return px2lon(this.px0 + gx + 0.5, this.zoom); }
  gyToLat(gy: number) { return px2lat(this.py0 + gy + 0.5, this.zoom); }
  lonToGx(lon: number) { return lon2px(lon, this.zoom) - this.px0 - 0.5; }
  latToGy(lat: number) { return lat2px(lat, this.zoom) - this.py0 - 0.5; }

  gxToX(gx: number) { return (gx - this.gx0) * this.mpp; }
  gyToZ(gy: number) { return (gy - this.gy0) * this.mpp; }
  xToGx(x: number) { return x / this.mpp + this.gx0; }
  zToGy(z: number) { return z / this.mpp + this.gy0; }

  lonLatToWorld(lon: number, lat: number): [number, number] {
    return [this.gxToX(this.lonToGx(lon)), this.gyToZ(this.latToGy(lat))];
  }
  worldToLonLat(x: number, z: number): [number, number] {
    return [this.gxToLon(this.xToGx(x)), this.gyToLat(this.zToGy(z))];
  }

  inside(x: number, z: number, margin = 0) {
    const gx = this.xToGx(x), gy = this.zToGy(z);
    return gx >= margin && gy >= margin && gx <= this.w - 1 - margin && gy <= this.h - 1 - margin;
  }

  at(ix: number, iy: number) {
    ix = ix < 0 ? 0 : ix >= this.w ? this.w - 1 : ix;
    iy = iy < 0 ? 0 : iy >= this.h ? this.h - 1 : iy;
    return this.data[iy * this.w + ix];
  }

  sample(gx: number, gy: number) {
    const x = gx < 0 ? 0 : gx > this.w - 1.0001 ? this.w - 1.0001 : gx;
    const y = gy < 0 ? 0 : gy > this.h - 1.0001 ? this.h - 1.0001 : gy;
    const x0 = x | 0, y0 = y | 0;
    const fx = x - x0, fy = y - y0;
    const i = y0 * this.w + x0;
    const d = this.data;
    const a = d[i], b = d[i + 1], c = d[i + this.w], e = d[i + this.w + 1];
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + e * fx) * fy;
  }

  sampleWorld(x: number, z: number) { return this.sample(this.xToGx(x), this.zToGy(z)); }
  sampleLonLat(lon: number, lat: number) { return this.sample(this.lonToGx(lon), this.latToGy(lat)); }

  /** Gradient terenu [dh/dx, dh/dz] w m/m (x – wschód, z – południe). */
  gradient(gx: number, gy: number): [number, number] {
    const e = 1;
    const dx = (this.sample(gx + e, gy) - this.sample(gx - e, gy)) / (2 * e * this.mpp);
    const dz = (this.sample(gx, gy + e) - this.sample(gx, gy - e)) / (2 * e * this.mpp);
    return [dx, dz];
  }

  /** Nachylenie w stopniach. */
  slope(gx: number, gy: number) {
    const [dx, dz] = this.gradient(gx, gy);
    return Math.atan(Math.hypot(dx, dz)) / DEG;
  }

  /** Ekspozycja stoku (kierunek, w który opada teren) w stopniach, 0 = N. */
  aspect(gx: number, gy: number) {
    const [dx, dz] = this.gradient(gx, gy);
    // wektor spadku: (-dx, -dz) w (wschód, południe) → azymut
    const east = -dx, north = dz;
    return ((Math.atan2(east, north) / DEG) + 360) % 360;
  }

  /**
   * Przecięcie promienia (w przestrzeni sceny z przewyższeniem `exag`) z terenem.
   * Zwraca punkt w metrach świata (y = wysokość rzeczywista) lub null.
   */
  raycast(ox: number, oy: number, oz: number, dx: number, dy: number, dz: number, exag: number, fine?: (x: number, z: number) => number) {
    const H = fine ?? ((x: number, z: number) => this.sampleWorld(x, z));
    const top = this.max * exag + 50;
    let t = 0;
    // przesunięcie startu do górnej płaszczyzny ograniczającej
    if (oy > top) {
      if (dy >= 0) return null;
      t = (oy - top) / -dy;
    }
    const maxT = t + Math.max(this.widthM, this.heightM) * 3;
    const minStep = this.mpp * 0.35;
    let prevT = t;
    let prevGap = Infinity;
    for (let i = 0; i < 4000 && t < maxT; i++) {
      const x = ox + dx * t, y = oy + dy * t, z = oz + dz * t;
      const gx = this.xToGx(x), gy = this.zToGy(z);
      if (gx < -1 || gy < -1 || gx > this.w || gy > this.h) {
        if (dy >= 0 || y < this.min * exag - 100) return null;
        t += Math.max(minStep, this.mpp * 4);
        continue;
      }
      const ground = H(x, z) * exag;
      const gap = y - ground;
      if (gap <= 0) {
        // bisekcja między prevT i t
        let lo = prevT, hi = t;
        if (prevGap === Infinity) lo = Math.max(0, t - minStep);
        for (let k = 0; k < 18; k++) {
          const m = (lo + hi) / 2;
          const yy = oy + dy * m;
          const g = H(ox + dx * m, oz + dz * m) * exag;
          if (yy - g > 0) lo = m; else hi = m;
        }
        const hx = ox + dx * hi, hz = oz + dz * hi;
        return { x: hx, z: hz, y: H(hx, hz), t: hi };
      }
      prevT = t;
      prevGap = gap;
      t += Math.max(minStep, gap * 0.4);
    }
    return null;
  }

  /**
   * Profil terenu wzdłuż odcinka (w metrach świata) – do linii wzroku i pomiarów.
   */
  profile(x0: number, z0: number, x1: number, z1: number, step = this.mpp * 0.5, fine?: (x: number, z: number) => number) {
    const H = fine ?? ((x: number, z: number) => this.sampleWorld(x, z));
    const len = Math.hypot(x1 - x0, z1 - z0);
    const n = Math.max(2, Math.ceil(len / step) + 1);
    const d = new Float32Array(n), e = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const f = i / (n - 1);
      d[i] = f * len;
      e[i] = H(x0 + (x1 - x0) * f, z0 + (z1 - z0) * f);
    }
    return { d, e, len };
  }
}
