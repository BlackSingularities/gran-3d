import type { Dem } from './dem';
import { haversine } from './geo';
import { climb, colorIndex, finalizeTrack, newTrack, sacFactor, trackStats, walkTime, type Track, type TrackStats } from './metrics';
import type { RawTrails } from './region';

export interface Edge {
  id: number;
  a: number;
  b: number;
  n: number;
  lon: Float64Array;
  lat: Float64Array;
  x: Float32Array;
  z: Float32Array;
  e: Float32Array;
  /** skumulowany dystans poziomy wzdłuż krawędzi */
  cum: Float32Array;
  len2: number;
  len3: number;
  up: number;
  down: number;
  colors: string[];
  routes: number[];
  sac: number;
  oneway: number;
  name: string;
  way: number;
}

export interface GNode {
  lon: number;
  lat: number;
  e: number;
  x: number;
  z: number;
  edges: number[];
}

export interface Snap {
  edge: number;
  /** pozycja wzdłuż krawędzi w jednostkach indeksu (i + t) */
  p: number;
  x: number;
  z: number;
  e: number;
  lon: number;
  lat: number;
  dist: number;
}

export type CostMode = 'time' | 'distance' | 'ascent' | 'easy';

export const MODE_LABEL: Record<CostMode, string> = {
  time: 'Najszybsza',
  distance: 'Najkrótsza',
  ascent: 'Najmniej podejść',
  easy: 'Najłatwiejsza',
};

export interface Piece {
  edge: number;
  p0: number;
  p1: number;
}

export interface Route {
  id: string;
  kind: 'trail' | 'terrain' | 'gpx';
  labels: string[];
  pieces: Piece[];
  track: Track;
  stats: TrackStats;
  /** indeksy punktów śladu odpowiadające kolejnym punktom trasy */
  legIdx: number[];
  /** trasy PTTK przebyte po drodze (indeksy w routes) */
  routeRefs: number[];
}

interface Partial {
  to: number;
  edge: number;
  p0: number;
  p1: number;
}

class Heap {
  private k: number[] = [];
  private v: number[] = [];
  get size() { return this.k.length; }
  push(key: number, val: number) {
    const k = this.k, v = this.v;
    let i = k.length;
    k.push(key); v.push(val);
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (k[p] <= key) break;
      k[i] = k[p]; v[i] = v[p];
      i = p;
    }
    k[i] = key; v[i] = val;
  }
  pop(): [number, number] {
    const k = this.k, v = this.v;
    const rk = k[0], rv = v[0];
    const lk = k.pop()!, lv = v.pop()!;
    const n = k.length;
    if (n) {
      let i = 0;
      for (;;) {
        let c = 2 * i + 1;
        if (c >= n) break;
        if (c + 1 < n && k[c + 1] < k[c]) c++;
        if (k[c] >= lk) break;
        k[i] = k[c]; v[i] = v[c];
        i = c;
      }
      k[i] = lk; v[i] = lv;
    }
    return [rk, rv];
  }
}

export class TrailGraph {
  nodes: GNode[] = [];
  edges: Edge[] = [];
  routes: RawTrails['routes'];
  private cell = 250;
  private index = new Map<number, number[]>(); // klucz komórki → [edge, seg, edge, seg...]
  private ox: number;
  private oz: number;
  readonly totalKm: number;

  constructor(raw: RawTrails, dem: Dem) {
    this.routes = raw.routes;
    this.ox = -dem.widthM / 2 - 1000;
    this.oz = -dem.heightM / 2 - 1000;
    for (const [lon, lat] of raw.nodes) {
      const [x, z] = dem.lonLatToWorld(lon, lat);
      this.nodes.push({ lon, lat, e: dem.sampleLonLat(lon, lat), x, z, edges: [] });
    }
    let total = 0;
    raw.edges.forEach((re, id) => {
      const n = re.g.length / 3;
      const lon = new Float64Array(n), lat = new Float64Array(n);
      const x = new Float32Array(n), z = new Float32Array(n), e = new Float32Array(n), cum = new Float32Array(n);
      let len3 = 0;
      for (let i = 0; i < n; i++) {
        lon[i] = re.g[i * 3];
        lat[i] = re.g[i * 3 + 1];
        e[i] = re.g[i * 3 + 2];
        const w = dem.lonLatToWorld(lon[i], lat[i]);
        x[i] = w[0];
        z[i] = w[1];
        if (i) {
          const h = haversine(lon[i - 1], lat[i - 1], lon[i], lat[i]);
          cum[i] = cum[i - 1] + h;
          len3 += Math.hypot(h, e[i] - e[i - 1]);
        }
      }
      const { up, down } = climb(e);
      const len2 = cum[n - 1];
      total += len2;
      const colored = re.c.filter((c) => c !== 'other');
      const edge: Edge = {
        id, a: re.a, b: re.b, n, lon, lat, x, z, e, cum, len2, len3, up, down,
        colors: colored.length ? colored : ['other'],
        routes: re.r,
        sac: re.sac,
        oneway: re.ow,
        name: re.name,
        way: re.way,
      };
      this.edges.push(edge);
      this.nodes[re.a].edges.push(id);
      if (re.b !== re.a) this.nodes[re.b].edges.push(id);
      for (let i = 0; i < n - 1; i++) {
        const k0 = this.key(x[i], z[i]), k1 = this.key(x[i + 1], z[i + 1]);
        this.addIdx(k0, id, i);
        if (k1 !== k0) this.addIdx(k1, id, i);
      }
    });
    this.totalKm = total / 1000;
  }

  private key(x: number, z: number) {
    return Math.floor((x - this.ox) / this.cell) * 100000 + Math.floor((z - this.oz) / this.cell);
  }
  private addIdx(k: number, e: number, s: number) {
    let a = this.index.get(k);
    if (!a) this.index.set(k, (a = []));
    a.push(e, s);
  }

  /** Najbliższy punkt na sieci szlaków (w metrach świata). */
  snap(x: number, z: number, maxDist = 600): Snap | null {
    let best: Snap | null = null;
    let bestD = maxDist;
    const r = Math.ceil(maxDist / this.cell);
    const cx = Math.floor((x - this.ox) / this.cell), cz = Math.floor((z - this.oz) / this.cell);
    const seen = new Set<number>();
    for (let i = -r; i <= r; i++) {
      for (let j = -r; j <= r; j++) {
        const list = this.index.get((cx + i) * 100000 + (cz + j));
        if (!list) continue;
        for (let k = 0; k < list.length; k += 2) {
          const eid = list[k], s = list[k + 1];
          const tag = eid * 100000 + s;
          if (seen.has(tag)) continue;
          seen.add(tag);
          const ed = this.edges[eid];
          const ax = ed.x[s], az = ed.z[s], bx = ed.x[s + 1], bz = ed.z[s + 1];
          const dx = bx - ax, dz = bz - az;
          const l2 = dx * dx + dz * dz;
          let t = l2 > 0 ? ((x - ax) * dx + (z - az) * dz) / l2 : 0;
          t = t < 0 ? 0 : t > 1 ? 1 : t;
          const px = ax + dx * t, pz = az + dz * t;
          const d = Math.hypot(px - x, pz - z);
          if (d < bestD) {
            bestD = d;
            best = {
              edge: eid,
              p: s + t,
              x: px,
              z: pz,
              e: ed.e[s] + (ed.e[s + 1] - ed.e[s]) * t,
              lon: ed.lon[s] + (ed.lon[s + 1] - ed.lon[s]) * t,
              lat: ed.lat[s] + (ed.lat[s + 1] - ed.lat[s]) * t,
              dist: d,
            };
          }
        }
      }
    }
    return best;
  }

  /** Metryki fragmentu krawędzi przebytego od p0 do p1. */
  private partMetrics(ed: Edge, p0: number, p1: number) {
    const pts = this.slice(ed, p0, p1);
    let len2 = 0;
    const es: number[] = [];
    for (let i = 0; i < pts.length; i++) {
      es.push(pts[i][4]);
      if (i) len2 += haversine(pts[i - 1][0], pts[i - 1][1], pts[i][0], pts[i][1]);
    }
    const { up, down } = climb(es);
    let len3 = 0;
    for (let i = 1; i < pts.length; i++) {
      len3 += Math.hypot(haversine(pts[i - 1][0], pts[i - 1][1], pts[i][0], pts[i][1]), pts[i][4] - pts[i - 1][4]);
    }
    return { len2, len3, up, down };
  }

  /** Punkty fragmentu krawędzi: [lon, lat, x, z, e]. */
  slice(ed: Edge, p0: number, p1: number): [number, number, number, number, number][] {
    const at = (p: number): [number, number, number, number, number] => {
      const i = Math.min(ed.n - 2, Math.max(0, Math.floor(p)));
      const t = Math.min(1, Math.max(0, p - i));
      const L = (arr: ArrayLike<number>) => arr[i] + (arr[i + 1] - arr[i]) * t;
      return [L(ed.lon), L(ed.lat), L(ed.x), L(ed.z), L(ed.e)];
    };
    const out: [number, number, number, number, number][] = [at(p0)];
    if (p1 >= p0) {
      for (let i = Math.floor(p0) + 1; i < p1; i++) out.push(at(i));
    } else {
      for (let i = Math.ceil(p0) - 1; i > p1; i--) out.push(at(i));
    }
    out.push(at(p1));
    return out;
  }

  private cost(mode: CostMode, m: { len2: number; len3: number; up: number; down: number }, sac: number, pen = 1) {
    let c: number;
    switch (mode) {
      case 'distance':
        c = m.len3;
        break;
      case 'ascent':
        c = m.up * 25 + m.len3 * 0.6;
        break;
      case 'easy': {
        const hard = sac >= 5 ? 6 : sac >= 4 ? 3 : sac >= 3 ? 1.5 : 1;
        c = walkTime(m.len2, m.up, m.down) * hard * 3600;
        break;
      }
      default:
        c = walkTime(m.len2, m.up, m.down) * sacFactor(sac) * 3600;
    }
    return c * pen;
  }

  private edgeCost(ed: Edge, forward: boolean, mode: CostMode, pen: number) {
    if (ed.oneway === 1 && !forward) return Infinity;
    if (ed.oneway === -1 && forward) return Infinity;
    return this.cost(mode, forward ? ed : { len2: ed.len2, len3: ed.len3, up: ed.down, down: ed.up }, ed.sac, pen);
  }

  /** Najlepsza ścieżka między dwoma punktami przyczepionymi do sieci. */
  private leg(s: Snap, t: Snap, mode: CostMode, penalty?: Map<number, number>): Piece[] | null {
    const N = this.nodes.length;
    const VS = N, VT = N + 1;
    const extra = new Map<number, Partial[]>();
    const addP = (from: number, p: Partial) => {
      if (!extra.has(from)) extra.set(from, []);
      extra.get(from)!.push(p);
    };
    const es = this.edges[s.edge], et = this.edges[t.edge];
    // ze startu do końców krawędzi startowej
    addP(VS, { to: es.a, edge: es.id, p0: s.p, p1: 0 });
    addP(VS, { to: es.b, edge: es.id, p0: s.p, p1: es.n - 1 });
    // z końców krawędzi docelowej do celu
    addP(et.a, { to: VT, edge: et.id, p0: 0, p1: t.p });
    addP(et.b, { to: VT, edge: et.id, p0: et.n - 1, p1: t.p });
    if (s.edge === t.edge) addP(VS, { to: VT, edge: es.id, p0: s.p, p1: t.p });

    const pc = new Map<string, number>();
    const partialCost = (p: Partial) => {
      const key = `${p.edge}:${p.p0}:${p.p1}`;
      let c = pc.get(key);
      if (c === undefined) {
        const ed = this.edges[p.edge];
        const forward = p.p1 >= p.p0;
        if ((ed.oneway === 1 && !forward) || (ed.oneway === -1 && forward)) c = Infinity;
        else c = this.cost(mode, this.partMetrics(ed, p.p0, p.p1), ed.sac, penalty?.get(ed.id) ?? 1);
        pc.set(key, c);
      }
      return c;
    };

    const dist = new Float64Array(N + 2).fill(Infinity);
    const prevNode = new Int32Array(N + 2).fill(-1);
    const prevPiece: (Piece | null)[] = new Array(N + 2).fill(null);
    const heap = new Heap();
    dist[VS] = 0;
    heap.push(0, VS);
    while (heap.size) {
      const [d, u] = heap.pop();
      if (d > dist[u]) continue;
      if (u === VT) break;
      const relax = (v: number, c: number, piece: Piece) => {
        const nd = d + c;
        if (nd < dist[v]) {
          dist[v] = nd;
          prevNode[v] = u;
          prevPiece[v] = piece;
          heap.push(nd, v);
        }
      };
      if (u < N) {
        for (const eid of this.nodes[u].edges) {
          const ed = this.edges[eid];
          const pen = penalty?.get(eid) ?? 1;
          if (ed.a === u) relax(ed.b, this.edgeCost(ed, true, mode, pen), { edge: eid, p0: 0, p1: ed.n - 1 });
          if (ed.b === u) relax(ed.a, this.edgeCost(ed, false, mode, pen), { edge: eid, p0: ed.n - 1, p1: 0 });
        }
      }
      const ex = extra.get(u);
      if (ex) for (const p of ex) relax(p.to, partialCost(p), { edge: p.edge, p0: p.p0, p1: p.p1 });
    }
    if (!Number.isFinite(dist[VT])) return null;
    const pieces: Piece[] = [];
    let v = VT;
    while (v !== VS && v >= 0) {
      const p = prevPiece[v];
      if (p && Math.abs(p.p1 - p.p0) > 1e-6) pieces.push(p);
      v = prevNode[v];
    }
    return pieces.reverse();
  }

  /** Trasa przez kolejne punkty (co najmniej dwa). */
  route(snaps: Snap[], mode: CostMode, penalty?: Map<number, number>): Route | null {
    const all: Piece[] = [];
    const legPieceIdx: number[] = [0];
    for (let i = 0; i < snaps.length - 1; i++) {
      const leg = this.leg(snaps[i], snaps[i + 1], mode, penalty);
      if (!leg) return null;
      all.push(...leg);
      legPieceIdx.push(all.length);
    }
    return this.buildRoute(all, legPieceIdx, [MODE_LABEL[mode]]);
  }

  private buildRoute(pieces: Piece[], legPieceIdx: number[], labels: string[]): Route {
    const pts: [number, number, number, number, number][] = [];
    const col: number[] = [];
    const sac: number[] = [];
    const legIdx: number[] = [0];
    const refs = new Set<number>();
    pieces.forEach((pc, k) => {
      const ed = this.edges[pc.edge];
      ed.routes.forEach((r) => refs.add(r));
      const sl = this.slice(ed, pc.p0, pc.p1);
      const start = pts.length ? 1 : 0;
      for (let i = start; i < sl.length; i++) {
        pts.push(sl[i]);
        col.push(colorIndex(ed.colors[0]));
        sac.push(ed.sac);
      }
      if (legPieceIdx.includes(k + 1)) legIdx.push(pts.length - 1);
    });
    const tr = newTrack(pts.length);
    pts.forEach((p, i) => {
      tr.lon[i] = p[0]; tr.lat[i] = p[1]; tr.x[i] = p[2]; tr.z[i] = p[3]; tr.e[i] = p[4];
      tr.color[i] = col[i]; tr.sac[i] = sac[i];
    });
    finalizeTrack(tr);
    return {
      id: Math.random().toString(36).slice(2, 9),
      kind: 'trail',
      labels,
      pieces,
      track: tr,
      stats: trackStats(tr),
      legIdx,
      routeRefs: [...refs],
    };
  }

  /** Długość wspólna dwóch tras (po krawędziach) / długość krótszej. */
  private overlap(a: Route, b: Route) {
    const la = new Map<number, number>();
    for (const p of a.pieces) la.set(p.edge, (la.get(p.edge) ?? 0) + Math.abs(p.p1 - p.p0) / (this.edges[p.edge].n - 1) * this.edges[p.edge].len2);
    let shared = 0;
    for (const p of b.pieces) {
      const l = Math.abs(p.p1 - p.p0) / (this.edges[p.edge].n - 1) * this.edges[p.edge].len2;
      shared += Math.min(l, la.get(p.edge) ?? 0);
    }
    return shared / Math.max(1, Math.min(a.stats.len2, b.stats.len2));
  }

  /** Zestaw różnych wariantów: najszybszy, najkrótszy, najmniej podejść, najłatwiejszy + objazdy. */
  alternatives(snaps: Snap[]): Route[] {
    const out: Route[] = [];
    const push = (r: Route | null) => {
      if (!r) return;
      for (const o of out) {
        if (this.overlap(o, r) > 0.93 && Math.abs(o.stats.len2 - r.stats.len2) < 0.03 * o.stats.len2) {
          for (const l of r.labels) if (!o.labels.includes(l)) o.labels.push(l);
          return;
        }
      }
      out.push(r);
    };
    const modes: CostMode[] = ['time', 'distance', 'ascent', 'easy'];
    for (const m of modes) push(this.route(snaps, m));
    if (!out.length) return out;
    // objazdy: karamy krawędzie poprzednich wariantów
    const pen = new Map<number, number>();
    for (let k = 0; k < 3 && out.length < 5; k++) {
      for (const r of out) for (const p of r.pieces) pen.set(p.edge, (pen.get(p.edge) ?? 1) * 1.7);
      const alt = this.route(snaps, 'time', pen);
      if (!alt) break;
      alt.labels = ['Wariant'];
      // odrzuć absurdalnie długie objazdy
      if (alt.stats.time > out[0].stats.time * 1.8) break;
      const before = out.length;
      push(alt);
      if (out.length === before) continue;
    }
    let w = 1;
    for (const r of out) if (r.labels[0] === 'Wariant') r.labels = [`Wariant ${++w - 1}`];
    return out;
  }

  /** Nazwy tras (szlaków) przypisanych do krawędzi. */
  edgeRouteNames(eid: number) {
    return this.edges[eid].routes.map((r) => this.routes[r]).filter(Boolean);
  }
}

/** Buduje trasę z dowolnego śladu (wyznaczonego w terenie lub z pliku GPX). */
export function routeFromPolyline(
  dem: Dem,
  pts: { lon: number; lat: number; e?: number }[],
  kind: Route['kind'],
  label: string,
  timeFactor = 1,
  height?: (lon: number, lat: number) => number
): Route {
  const tr = newTrack(pts.length);
  pts.forEach((p, i) => {
    tr.lon[i] = p.lon;
    tr.lat[i] = p.lat;
    const [x, z] = dem.lonLatToWorld(p.lon, p.lat);
    tr.x[i] = x;
    tr.z[i] = z;
    tr.e[i] = p.e ?? (height ? height(p.lon, p.lat) : dem.sampleLonLat(p.lon, p.lat));
    tr.color[i] = colorIndex(kind === 'terrain' ? 'offtrail' : 'purple');
    tr.sac[i] = kind === 'terrain' ? 2 : 1;
  });
  finalizeTrack(tr, timeFactor);
  return {
    id: Math.random().toString(36).slice(2, 9),
    kind,
    labels: [label],
    pieces: [],
    track: tr,
    stats: trackStats(tr),
    legIdx: [0, pts.length - 1],
    routeRefs: [],
  };
}
