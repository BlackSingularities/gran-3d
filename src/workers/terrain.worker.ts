/// <reference lib="webworker" />
// Obliczenia terenowe poza wątkiem głównym: pole widoczności (viewshed) i trasa przez teren (A* po siatce DEM).

const EARTH_R = 6371008.8;
let W = 0, H = 0, MPP = 1;
let DEM: Float32Array = new Float32Array(0);

type Msg =
  | { type: 'init'; w: number; h: number; mpp: number; data: Float32Array }
  | { type: 'viewshed'; id: number; gx: number; gy: number; eye: number; target: number }
  | { type: 'path'; id: number; a: [number, number]; b: [number, number]; maxSlope: number };

self.onmessage = (ev: MessageEvent<Msg>) => {
  const m = ev.data;
  if (m.type === 'init') {
    W = m.w; H = m.h; MPP = m.mpp; DEM = m.data;
    return;
  }
  if (m.type === 'viewshed') {
    const t0 = performance.now();
    const res = viewshed(m.gx, m.gy, m.eye, m.target);
    (self as unknown as Worker).postMessage({ type: 'viewshed', id: m.id, ...res, ms: performance.now() - t0 }, [res.mask.buffer]);
    return;
  }
  if (m.type === 'path') {
    const t0 = performance.now();
    const path = gridPath(m.a, m.b, m.maxSlope);
    (self as unknown as Worker).postMessage({ type: 'path', id: m.id, path, ms: performance.now() - t0 });
  }
};

function bilinear(x: number, y: number) {
  if (x < 0) x = 0; else if (x > W - 1.001) x = W - 1.001;
  if (y < 0) y = 0; else if (y > H - 1.001) y = H - 1.001;
  const x0 = x | 0, y0 = y | 0, fx = x - x0, fy = y - y0, i = y0 * W + x0;
  return (DEM[i] * (1 - fx) + DEM[i + 1] * fx) * (1 - fy) + (DEM[i + W] * (1 - fx) + DEM[i + W + 1] * fx) * fy;
}

/**
 * Widoczność metodą promieni do każdej komórki obwodu (odmiana XDraw),
 * z poprawką na krzywiznę Ziemi i refrakcję.
 * Maska: 0 – niewidoczne, 1..255 – widoczne (wartość = „zapas” kąta, do cieniowania krawędzi).
 */
function viewshed(ox: number, oy: number, eye: number, target: number) {
  const mask = new Uint8Array(W * H);
  const oz = bilinear(ox, oy) + eye;
  const cx = Math.round(ox), cy = Math.round(oy);
  if (cx >= 0 && cy >= 0 && cx < W && cy < H) mask[cy * W + cx] = 255;
  const k = (1 - 0.13) / (2 * EARTH_R);
  const ray = (tx: number, ty: number) => {
    const dx = tx - ox, dy = ty - oy;
    const steps = Math.ceil(Math.max(Math.abs(dx), Math.abs(dy)));
    if (steps < 1) return;
    const sx = dx / steps, sy = dy / steps;
    const stepM = Math.hypot(sx, sy) * MPP;
    let maxTan = -Infinity;
    for (let s = 1; s <= steps; s++) {
      const x = ox + sx * s, y = oy + sy * s;
      const ix = Math.round(x), iy = Math.round(y);
      if (ix < 0 || iy < 0 || ix >= W || iy >= H) break;
      const d = s * stepM;
      const h = DEM[iy * W + ix] - d * d * k;
      const tanT = (h + target - oz) / d;
      const idx = iy * W + ix;
      if (tanT >= maxTan) {
        const margin = Math.min(254, 1 + ((tanT - maxTan) * d) * 2);
        if (mask[idx] < margin) mask[idx] = Math.max(1, margin | 0);
      }
      const tanG = (h - oz) / d;
      if (tanG > maxTan) maxTan = tanG;
    }
  };
  for (let x = 0; x < W; x++) { ray(x, 0); ray(x, H - 1); }
  for (let y = 0; y < H; y++) { ray(0, y); ray(W - 1, y); }
  let count = 0;
  for (let i = 0; i < mask.length; i++) if (mask[i]) count++;
  return { mask, count, areaKm2: (count * MPP * MPP) / 1e6, observer: oz };
}

// ---------------- A* po siatce ----------------
class GridHeap {
  keys: Float64Array;
  vals: Int32Array;
  size = 0;
  constructor(cap: number) {
    this.keys = new Float64Array(cap);
    this.vals = new Int32Array(cap);
  }
  push(k: number, v: number) {
    if (this.size >= this.keys.length) {
      const nk = new Float64Array(this.keys.length * 2); nk.set(this.keys); this.keys = nk;
      const nv = new Int32Array(this.vals.length * 2); nv.set(this.vals); this.vals = nv;
    }
    let i = this.size++;
    const K = this.keys, V = this.vals;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (K[p] <= k) break;
      K[i] = K[p]; V[i] = V[p]; i = p;
    }
    K[i] = k; V[i] = v;
  }
  pop() {
    const K = this.keys, V = this.vals;
    const top = V[0];
    const n = --this.size;
    const lk = K[n], lv = V[n];
    let i = 0;
    for (;;) {
      let c = 2 * i + 1;
      if (c >= n) break;
      if (c + 1 < n && K[c + 1] < K[c]) c++;
      if (K[c] >= lk) break;
      K[i] = K[c]; V[i] = V[c]; i = c;
    }
    K[i] = lk; V[i] = lv;
    return top;
  }
}

/** Czas marszu wg funkcji Toblera (s) dla odcinka o długości poziomej d i przewyższeniu dh. */
function tobler(d: number, dh: number) {
  const v = 6 * Math.exp(-3.5 * Math.abs(dh / d + 0.05)); // km/h
  return (Math.hypot(d, dh) / 1000 / v) * 3600;
}

function gridPath(a: [number, number], b: [number, number], maxSlopeDeg: number) {
  const N = W * H;
  const g = new Float32Array(N).fill(Infinity);
  const from = new Int32Array(N).fill(-1);
  const closed = new Uint8Array(N);
  const maxTan = Math.tan((maxSlopeDeg * Math.PI) / 180);
  const s = Math.round(a[1]) * W + Math.round(a[0]);
  const t = Math.round(b[1]) * W + Math.round(b[0]);
  const tx = t % W, ty = (t / W) | 0;
  const vmax = 6 * Math.exp(-3.5 * 0) / 3.6; // m/s
  const hcost = (i: number) => (Math.hypot((i % W) - tx, ((i / W) | 0) - ty) * MPP) / vmax;
  const heap = new GridHeap(1 << 16);
  g[s] = 0;
  heap.push(hcost(s), s);
  const DX = [1, -1, 0, 0, 1, 1, -1, -1, 2, 2, -2, -2, 1, 1, -1, -1];
  const DY = [0, 0, 1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 2, -2, 2, -2];
  let found = false;
  let iter = 0;
  while (heap.size) {
    const u = heap.pop();
    if (closed[u]) continue;
    closed[u] = 1;
    if (u === t) { found = true; break; }
    if (++iter > 4_000_000) break;
    const ux = u % W, uy = (u / W) | 0;
    const hu = DEM[u];
    for (let k = 0; k < 16; k++) {
      const vx = ux + DX[k], vy = uy + DY[k];
      if (vx < 0 || vy < 0 || vx >= W || vy >= H) continue;
      const v = vy * W + vx;
      if (closed[v]) continue;
      const d = Math.hypot(DX[k], DY[k]) * MPP;
      // dla ruchów „skoczka” sprawdzamy też teren pośrodku
      let dh = DEM[v] - hu;
      if (k >= 8) {
        const mid = bilinear(ux + DX[k] / 2, uy + DY[k] / 2);
        const s1 = Math.abs(mid - hu) / (d / 2), s2 = Math.abs(DEM[v] - mid) / (d / 2);
        if (s1 > maxTan || s2 > maxTan) continue;
      } else if (Math.abs(dh) / d > maxTan) continue;
      const c = tobler(d, dh);
      const ng = g[u] + c;
      if (ng < g[v]) {
        g[v] = ng;
        from[v] = u;
        heap.push(ng + hcost(v), v);
      }
    }
  }
  if (!found) return null;
  const path: [number, number][] = [];
  for (let v = t; v !== -1; v = from[v]) path.push([v % W, (v / W) | 0]);
  path.reverse();
  // wygładzenie zygzaków siatki
  let p = path;
  for (let it = 0; it < 3; it++) {
    const q: [number, number][] = [p[0]];
    for (let i = 1; i < p.length - 1; i++) {
      q.push([(p[i - 1][0] + 2 * p[i][0] + p[i + 1][0]) / 4, (p[i - 1][1] + 2 * p[i][1] + p[i + 1][1]) / 4]);
    }
    q.push(p[p.length - 1]);
    p = q;
  }
  p[0] = [a[0], a[1]];
  p[p.length - 1] = [b[0], b[1]];
  return p;
}
