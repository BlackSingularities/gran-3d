import regionsJson from '../../regions.json';
import { Dem, type DemMeta } from './dem';
import type { TileIndex } from '../scene/tiles';

export interface RegionDef {
  id: string;
  name: string;
  subtitle: string;
  bbox: [number, number, number, number];
  zoom: number;
  home: { lon: number; lat: number; heading: number; pitch: number; distance: number };
  hd?: { zoom: number; lidar: string[]; ortho: string[] };
}

/** Piętra roślinności (m n.p.m.) – sterują realistycznym cieniowaniem terenu. */
export interface Biome {
  forest: number; // górna granica lasu
  shrub: number; // górna granica kosodrzewiny / zarośli
  rock: number; // od tej wysokości dominują skały i piargi
}

export const BIOMES: Record<string, Biome> = {
  tatry: { forest: 1500, shrub: 1820, rock: 2150 },
  karkonosze: { forest: 1250, shrub: 1450, rock: 1560 },
  pieniny: { forest: 1400, shrub: 1500, rock: 1600 },
  'babia-gora': { forest: 1330, shrub: 1620, rock: 1690 },
  bieszczady: { forest: 1120, shrub: 1180, rock: 1500 },
};

export const REGIONS = regionsJson as unknown as RegionDef[];

export interface RawTrails {
  nodes: [number, number, number][];
  edges: {
    a: number;
    b: number;
    g: number[];
    r: number[];
    c: string[];
    sac: number;
    ow: number;
    name: string;
    hw: string;
    way: number;
  }[];
  routes: { id: number; name: string; ref: string; color: string; network: string; operator: string }[];
}

export type PoiType = 'peak' | 'saddle' | 'hut' | 'shelter' | 'lake' | 'waterfall' | 'cave' | 'viewpoint';
export interface Poi {
  t: PoiType;
  n: string;
  lon: number;
  lat: number;
  e: number | null;
  d: number;
  w: string;
  o: string;
  // wyliczane po wczytaniu
  x: number;
  z: number;
  ele: number;
  prom: number;
  id: number;
}

export interface LoadedRegion {
  def: RegionDef;
  biome: Biome;
  dem: Dem;
  trails: RawTrails;
  pois: Poi[];
  tiles: TileIndex | null;
}

async function fetchWithProgress(url: string, onProgress: (f: number) => void) {
  const res = await fetch(url);
  if (!res.ok || !res.body) throw new Error(`Nie można wczytać ${url} (${res.status})`);
  const total = Number(res.headers.get('content-length')) || 0;
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let got = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    got += value.length;
    if (total) onProgress(got / total);
  }
  const out = new Uint8Array(got);
  let o = 0;
  for (const c of chunks) { out.set(c, o); o += c.length; }
  onProgress(1);
  return out.buffer;
}

export async function loadRegion(def: RegionDef, onProgress: (f: number, label: string) => void): Promise<LoadedRegion> {
  const base = `${import.meta.env.BASE_URL}data/${def.id}/`;
  onProgress(0.02, 'Metadane regionu');
  const meta: DemMeta = await (await fetch(base + 'meta.json')).json();
  const demBuf = await fetchWithProgress(base + 'dem.bin', (f) => onProgress(0.05 + f * 0.6, 'Numeryczny model terenu'));
  onProgress(0.68, 'Sieć szlaków');
  const [trails, poisRaw] = await Promise.all([
    fetch(base + 'trails.json').then((r) => r.json() as Promise<RawTrails>),
    fetch(base + 'pois.json').then((r) => r.json() as Promise<Poi[]>),
  ]);
  onProgress(0.8, 'Przygotowanie siatki');
  const dem = new Dem(meta, new Uint16Array(demBuf));
  const pois = poisRaw.map((p, id) => {
    const [x, z] = dem.lonLatToWorld(p.lon, p.lat);
    return { ...p, x, z, ele: p.e ?? p.d, prom: 0, id };
  });
  computeProminence(pois, dem);
  // piramida kafli LiDAR (opcjonalna – powstaje skryptem bake-hd)
  let tiles: TileIndex | null = null;
  if (def.hd) {
    try {
      const r = await fetch(base + 'tiles/index.json');
      if (r.ok && (r.headers.get('content-type') ?? '').includes('json')) tiles = await r.json();
    } catch {
      tiles = null;
    }
  }
  return { def, biome: BIOMES[def.id] ?? BIOMES.tatry, dem, trails, pois, tiles };
}

/**
 * Przybliżona „ważność” szczytu: odległość do najbliższego wyższego szczytu (izolacja)
 * połączona z wysokością względną nad otoczeniem. Służy do rozrzedzania etykiet.
 */
function computeProminence(pois: Poi[], dem: Dem) {
  const peaks = pois.filter((p) => p.t === 'peak');
  for (const p of peaks) {
    let iso = 30000;
    for (const q of peaks) {
      if (q.ele > p.ele) {
        const d = Math.hypot(q.x - p.x, q.z - p.z);
        if (d < iso) iso = d;
      }
    }
    // najniższy punkt w promieniu 600 m – miara wybitności lokalnej
    let low = p.ele;
    const gx = dem.xToGx(p.x), gy = dem.zToGy(p.z);
    const r = Math.ceil(600 / dem.mpp);
    for (let a = 0; a < 16; a++) {
      const ang = (a / 16) * Math.PI * 2;
      const e = dem.sample(gx + Math.cos(ang) * r, gy + Math.sin(ang) * r);
      if (e < low) low = e;
    }
    p.prom = Math.min(iso, 30000) / 1000 + (p.ele - low) / 120;
  }
  for (const p of pois) {
    if (p.t === 'hut') p.prom = 6;
    else if (p.t === 'lake') p.prom = 3;
    else if (p.t === 'saddle') p.prom = 1.2;
    else if (p.t !== 'peak') p.prom = 0.8;
  }
}

export function regionForPoint(lon: number, lat: number) {
  return REGIONS.find((r) => lon > r.bbox[0] && lon < r.bbox[2] && lat > r.bbox[1] && lat < r.bbox[3]);
}
