import * as THREE from 'three';
import type { Dem } from '../core/dem';
import { terrainFragment, tileVertex } from './shaders';

/**
 * Teren kaflowy z poziomami szczegółowości (drzewo czwórkowe w siatce Web Mercator).
 * Kafle wysokości 259×259 (256 + ramka) z modelu LiDAR, doczytywane wokół kamery;
 * do każdego kafla – ortofotomapa złożona z usług krajowych (PL/SK/CZ).
 */

export interface TileIndex {
  minZ: number;
  maxZ: number;
  tiles: Record<string, [number, number]>;
}

const S = 259;
const SEG = 128;
const WM = 20037508.342789244;

const ORTHO_URL: Record<string, (bb: string, n: number) => string> = {
  pl: (bb, n) =>
    `https://mapy.geoportal.gov.pl/wss/service/PZGIK/ORTO/WMS/HighResolution?SERVICE=WMS&REQUEST=GetMap&VERSION=1.3.0&LAYERS=Raster&STYLES=&CRS=EPSG:3857&BBOX=${bb}&WIDTH=${n}&HEIGHT=${n}&FORMAT=image/png&TRANSPARENT=TRUE`,
  sk: (bb, n) =>
    `https://zbgis.skgeodesy.sk/zbgis/rest/services/Ortofoto/MapServer/export?bbox=${bb}&bboxSR=3857&imageSR=3857&size=${n},${n}&format=png32&transparent=true&f=image`,
  cz: (bb, n) =>
    `https://ags.cuzk.gov.cz/arcgis1/rest/services/ORTOFOTO_WM/MapServer/export?bbox=${bb}&bboxSR=3857&imageSR=3857&size=${n},${n}&format=png32&transparent=true&f=image`,
};

const key = (z: number, x: number, y: number) => (z * 65536 + x) * 65536 + y;

class TNode {
  state = 0; // 0 brak, 1 wczytywanie, 2 gotowy, 3 błąd
  data: Float32Array | null = null;
  tex: THREE.DataTexture | null = null;
  mesh: THREE.Mesh | null = null;
  mat: THREE.ShaderMaterial | null = null;
  ortho: THREE.Texture | null = null;
  orthoState = 0;
  used = 0;
  prio = 0;
  readonly box = new THREE.Box3();
  constructor(
    readonly z: number,
    readonly x: number,
    readonly y: number,
    readonly min: number,
    readonly max: number,
    readonly ox: number,
    readonly oz: number,
    readonly sm: number
  ) {}
  get size() { return 256 * this.sm; }
}

export class TileTerrain {
  readonly group = new THREE.Group();
  private nodes = new Map<number, TNode>();
  private roots: TNode[] = [];
  private geo: THREE.BufferGeometry;
  private workers: Worker[] = [];
  private jobs = new Map<number, TNode>();
  private reqId = 0;
  private queue = new Set<TNode>();
  private orthoQueue = new Set<TNode>();
  private inflight = 0;
  private orthoInflight = 0;
  private frame = 0;
  private frustum = new THREE.Frustum();
  private m4 = new THREE.Matrix4();
  private tmpBox = new THREE.Box3();
  private rendered = new Set<TNode>();
  private blank: THREE.DataTexture;
  private canvas = document.createElement('canvas');
  private readyCount = 0;
  orthoOn = false;
  onChange: () => void = () => {};
  /** jakość: mnożnik odległości podziału (większy = więcej szczegółów) */
  quality = 1;

  constructor(
    private dem: Dem,
    readonly index: TileIndex,
    private baseUrl: string,
    private shared: Record<string, THREE.IUniform>,
    private orthoSources: string[],
    private anisotropy: number
  ) {
    this.geo = this.buildGeometry();
    this.blank = new THREE.DataTexture(new Uint8Array([0, 0, 0, 0]), 1, 1);
    this.blank.needsUpdate = true;
    const n = Math.min(4, Math.max(2, (navigator.hardwareConcurrency || 4) >> 1));
    for (let i = 0; i < n; i++) {
      const w = new Worker(new URL('../workers/tiles.worker.ts', import.meta.url), { type: 'module' });
      w.onmessage = (e) => this.onTile(e.data);
      this.workers.push(w);
    }
    for (const k of Object.keys(index.tiles)) {
      const [z, x, y] = k.split('/').map(Number);
      if (z === index.minZ) this.roots.push(this.node(z, x, y)!);
    }
    for (const r of this.roots) this.request(r, 0);
    this.pump();
  }

  get maxZ() { return this.index.maxZ; }
  get loading() { return this.inflight + this.queue.size; }
  get orthoLoading() { return this.orthoInflight + this.orthoQueue.size; }

  private node(z: number, x: number, y: number) {
    const k = key(z, x, y);
    let n = this.nodes.get(k);
    if (n) return n;
    const mm = this.index.tiles[`${z}/${x}/${y}`];
    if (!mm) return null;
    const d = this.dem;
    const s = 2 ** (z - d.zoom);
    const ox = ((x * 256) / s - d.px0 - 0.5 - (d.w - 1) / 2) * d.mpp;
    const oz = ((y * 256) / s - d.py0 - 0.5 - (d.h - 1) / 2) * d.mpp;
    n = new TNode(z, x, y, mm[0], mm[1], ox, oz, d.mpp / s);
    this.nodes.set(k, n);
    return n;
  }

  private children(n: TNode) {
    if (n.z >= this.index.maxZ) return null;
    const a = this.node(n.z + 1, n.x * 2, n.y * 2);
    if (!a) return null;
    return [a, this.node(n.z + 1, n.x * 2 + 1, n.y * 2)!, this.node(n.z + 1, n.x * 2, n.y * 2 + 1)!, this.node(n.z + 1, n.x * 2 + 1, n.y * 2 + 1)!].filter(Boolean);
  }

  /** Wspólna siatka 129×129 z fartuchem maskującym szczeliny między poziomami. */
  private buildGeometry() {
    const N = SEG + 1;
    const tpos: number[] = [];
    const skirt: number[] = [];
    const idx: number[] = [];
    for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) {
      tpos.push((i * 256) / SEG, (j * 256) / SEG);
      skirt.push(0);
    }
    for (let j = 0; j < SEG; j++) for (let i = 0; i < SEG; i++) {
      const a = j * N + i, b = a + 1, c = a + N, d = c + 1;
      idx.push(a, c, b, b, c, d);
    }
    const border: number[] = [];
    for (let i = 0; i < N; i++) border.push(i);
    for (let j = 1; j < N; j++) border.push(j * N + SEG);
    for (let i = SEG - 1; i >= 0; i--) border.push(SEG * N + i);
    for (let j = SEG - 1; j >= 0; j--) border.push(j * N);
    const base = tpos.length / 2;
    for (const v of border) {
      tpos.push(tpos[v * 2], tpos[v * 2 + 1]);
      skirt.push(1);
    }
    for (let k = 0; k < border.length - 1; k++) {
      const a = border[k], b = border[k + 1], c = base + k, d = base + k + 1;
      // obie orientacje – fartuch widoczny z każdej strony
      idx.push(a, c, b, b, c, d, a, b, c, b, d, c);
    }
    const g = new THREE.BufferGeometry();
    const pos = new Float32Array((tpos.length / 2) * 3);
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('tpos', new THREE.Float32BufferAttribute(tpos, 2));
    g.setAttribute('skirt', new THREE.Float32BufferAttribute(skirt, 1));
    g.setIndex(idx);
    return g;
  }

  private worldBox(n: TNode, exag: number, out: THREE.Box3) {
    out.min.set(n.ox, (n.min - 30) * exag, n.oz);
    out.max.set(n.ox + n.size, (n.max + 10) * exag, n.oz + n.size);
    return out;
  }

  private request(n: TNode, prio: number) {
    if (n.state !== 0) return;
    n.prio = prio;
    this.queue.add(n);
  }

  private pump() {
    while (this.inflight < this.workers.length * 3 && this.queue.size) {
      let best: TNode | null = null;
      for (const n of this.queue) if (!best || n.prio < best.prio) best = n;
      this.queue.delete(best!);
      if (best!.state !== 0) continue;
      best!.state = 1;
      const id = ++this.reqId;
      this.jobs.set(id, best!);
      this.inflight++;
      this.workers[id % this.workers.length].postMessage({ id, url: `${this.baseUrl}${best!.z}/${best!.x}/${best!.y}.png` });
    }
  }

  private onTile(msg: { id: number; data?: Float32Array; error?: string }) {
    const n = this.jobs.get(msg.id);
    this.jobs.delete(msg.id);
    this.inflight--;
    if (!n) return;
    if (!msg.data) {
      n.state = 3;
    } else {
      n.data = msg.data;
      n.tex = new THREE.DataTexture(n.data, S, S, THREE.RedFormat, THREE.FloatType);
      n.tex.minFilter = n.tex.magFilter = THREE.NearestFilter;
      n.tex.needsUpdate = true;
      n.state = 2;
      this.readyCount++;
    }
    this.pump();
    this.onChange();
  }

  /** Czy wczytano wszystkie kafle najniższego poziomu. */
  get rootsReady() {
    return this.roots.every((r) => r.state >= 2);
  }

  whenReady() {
    return new Promise<void>((resolve) => {
      const check = () => (this.rootsReady ? resolve() : setTimeout(check, 50));
      check();
    });
  }

  /** Wybór kafli do narysowania; zwraca true, jeśli obraz się zmienił. */
  update(camera: THREE.PerspectiveCamera, exag: number, viewH: number) {
    this.frame++;
    this.m4.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.m4);
    const cam = camera.position;
    const K = 2.1 * this.quality * Math.max(0.6, viewH / 900) * (38 / camera.fov);
    const render = new Set<TNode>();
    const visit = (n: TNode) => {
      const box = this.worldBox(n, exag, this.tmpBox);
      if (!this.frustum.intersectsBox(box)) return;
      n.used = this.frame;
      const d = box.distanceToPoint(cam);
      const kids = d < K * n.size ? this.children(n) : null;
      if (kids) {
        let ok = true;
        for (const k of kids) {
          k.used = this.frame;
          if (k.state === 0) this.request(k, d / n.size + (this.index.maxZ - k.z) * 0.1);
          if (k.state !== 2) ok = false;
        }
        if (ok) {
          for (const k of kids) visit(k);
          return;
        }
      }
      render.add(n);
    };
    for (const r of this.roots) {
      if (r.state === 2) visit(r);
      else if (r.state === 0) this.request(r, 0);
    }
    // aktualizacja siatek
    let changed = false;
    for (const n of this.rendered) {
      if (!render.has(n) && n.mesh) { n.mesh.visible = false; changed = true; }
    }
    for (const n of render) {
      if (!n.mesh) this.makeMesh(n);
      if (!n.mesh!.visible || !this.rendered.has(n)) changed = true;
      n.mesh!.visible = true;
      this.applyOrtho(n, cam);
    }
    this.rendered = render;
    // kolejka: porzuć żądania kafli, których już nie potrzebujemy
    for (const n of this.queue) if (n.used < this.frame - 2 && n.z > this.index.minZ) this.queue.delete(n);
    for (const n of this.orthoQueue) if (!render.has(n)) this.orthoQueue.delete(n);
    this.pump();
    this.pumpOrtho();
    if (this.frame % 60 === 0) this.evict();
    return changed;
  }

  private makeMesh(n: TNode) {
    const fine = n.z - (this.index.maxZ - 2);
    n.mat = new THREE.ShaderMaterial({
      vertexShader: tileVertex,
      fragmentShader: terrainFragment,
      defines: { TILE: '' },
      uniforms: {
        ...this.shared,
        uTile: { value: n.tex },
        uTileInfo: { value: new THREE.Vector4(n.ox, n.oz, n.sm, n.size * 0.03 + 15) },
        uOrtho: { value: this.blank },
        uOrthoRect: { value: new THREE.Vector4(0, 0, 1, 1) },
        uOrthoOn: { value: 0 },
        uNoiseAmt: { value: THREE.MathUtils.clamp(1 - fine * 0.35, 0.3, 1) },
      },
    });
    n.mesh = new THREE.Mesh(this.geo, n.mat);
    n.mesh.frustumCulled = false;
    n.mesh.matrixAutoUpdate = false;
    this.group.add(n.mesh);
  }

  // ------------------------------------------------------------ ortofotomapa
  private applyOrtho(n: TNode, cam: THREE.Vector3) {
    const u = n.mat!.uniforms;
    if (!this.orthoOn) {
      u.uOrthoOn.value = 0;
      return;
    }
    if (n.orthoState === 0) {
      n.prio = Math.hypot(n.ox + n.size / 2 - cam.x, n.oz + n.size / 2 - cam.z) / n.size;
      this.orthoQueue.add(n);
    }
    // własna tekstura albo fragment tekstury przodka
    let a: TNode | null = n;
    let dz = 0;
    while (a && a.orthoState !== 2) {
      dz++;
      a = a.z > this.index.minZ ? this.nodes.get(key(a.z - 1, a.x >> 1, a.y >> 1)) ?? null : null;
    }
    if (!a) {
      u.uOrthoOn.value = 0;
      return;
    }
    const f = 1 / 2 ** dz;
    u.uOrtho.value = a.ortho;
    u.uOrthoRect.value.set((n.x - a.x * 2 ** dz) * f, (n.y - a.y * 2 ** dz) * f, f, f);
    u.uOrthoOn.value = 1;
    a.used = this.frame;
  }

  private pumpOrtho() {
    while (this.orthoInflight < 6 && this.orthoQueue.size) {
      let best: TNode | null = null;
      for (const n of this.orthoQueue) if (!best || n.z > best.z || (n.z === best.z && n.prio < best.prio)) best = n;
      // najpierw grube poziomy – szybko wypełniają ekran
      for (const n of this.orthoQueue) if (n.z < best!.z) best = n;
      this.orthoQueue.delete(best!);
      if (best!.orthoState !== 0) continue;
      void this.loadOrtho(best!);
    }
  }

  private async loadOrtho(n: TNode) {
    n.orthoState = 1;
    this.orthoInflight++;
    const size = n.z >= this.index.maxZ ? 1024 : 512;
    const tiles = 256 * 2 ** n.z;
    const mx0 = ((n.x * 256) / tiles) * 2 * WM - WM, mx1 = (((n.x + 1) * 256) / tiles) * 2 * WM - WM;
    const my0 = WM - (((n.y + 1) * 256) / tiles) * 2 * WM, my1 = WM - ((n.y * 256) / tiles) * 2 * WM;
    const bb = `${mx0.toFixed(2)},${my0.toFixed(2)},${mx1.toFixed(2)},${my1.toFixed(2)}`;
    try {
      const imgs = await Promise.all(
        this.orthoSources.map(async (src) => {
          try {
            const r = await fetch(ORTHO_URL[src](bb, size));
            if (!r.ok) return null;
            const blob = await r.blob();
            if (!blob.type.startsWith('image')) return null;
            return await createImageBitmap(blob);
          } catch {
            return null;
          }
        })
      );
      if (!imgs.some(Boolean)) throw new Error('brak obrazu');
      const c = this.canvas;
      c.width = c.height = size;
      const ctx = c.getContext('2d')!;
      ctx.clearRect(0, 0, size, size);
      for (const im of imgs) if (im) { ctx.drawImage(im, 0, 0, size, size); im.close(); }
      const bmp = await createImageBitmap(c);
      const t = new THREE.Texture(bmp);
      t.flipY = false;
      t.colorSpace = THREE.NoColorSpace;
      t.minFilter = THREE.LinearMipmapLinearFilter;
      t.anisotropy = this.anisotropy;
      t.needsUpdate = true;
      n.ortho = t;
      n.orthoState = 2;
    } catch {
      n.orthoState = 3;
    }
    this.orthoInflight--;
    this.pumpOrtho();
    this.onChange();
  }

  setOrtho(on: boolean) {
    this.orthoOn = on;
    this.onChange();
  }

  // ------------------------------------------------------------ pamięć
  private evict() {
    // ortofoto: osobny limit (tekstury 512–1024 px są największe)
    const withOrtho = [...this.nodes.values()].filter((n) => n.orthoState === 2);
    if (withOrtho.length > 160) {
      withOrtho.sort((a, b) => a.used - b.used);
      for (const n of withOrtho.slice(0, withOrtho.length - 130)) {
        if (n.used > this.frame - 30) break;
        (n.ortho!.image as ImageBitmap)?.close?.();
        n.ortho!.dispose();
        n.ortho = null;
        n.orthoState = 0;
      }
    }
    const ready = [...this.nodes.values()].filter((n) => n.state === 2 && n.z > this.index.minZ);
    if (ready.length < 420) return;
    ready.sort((a, b) => a.used - b.used);
    for (const n of ready.slice(0, ready.length - 360)) {
      if (n.used > this.frame - 30) break;
      this.drop(n);
    }
  }

  private drop(n: TNode) {
    if (n.mesh) { this.group.remove(n.mesh); n.mesh = null; }
    n.mat?.dispose();
    n.mat = null;
    n.tex?.dispose();
    n.tex = null;
    n.data = null;
    if (n.ortho) {
      (n.ortho.image as ImageBitmap)?.close?.();
      n.ortho.dispose();
    }
    n.ortho = null;
    n.orthoState = 0;
    n.state = 0;
    this.readyCount--;
  }

  /** Wysokość terenu z najdokładniejszego wczytanego kafla (m n.p.m.) lub null. */
  heightAt(x: number, z: number): number | null {
    const d = this.dem;
    const cgx = d.px0 + x / d.mpp + (d.w - 1) / 2 + 0.5;
    const cgy = d.py0 + z / d.mpp + (d.h - 1) / 2 + 0.5;
    for (let lz = this.index.maxZ; lz >= this.index.minZ; lz--) {
      const s = 2 ** (lz - d.zoom);
      const cx = cgx * s, cy = cgy * s;
      const tx = Math.floor(cx / 256), ty = Math.floor(cy / 256);
      const n = this.nodes.get(key(lz, tx, ty));
      if (!n || !n.data) continue;
      const u = cx - tx * 256 + 1, v = cy - ty * 256 + 1;
      const i0 = Math.floor(u), j0 = Math.floor(v);
      const fx = u - i0, fy = v - j0;
      const D = n.data, o = j0 * S + i0;
      return (D[o] * (1 - fx) + D[o + 1] * fx) * (1 - fy) + (D[o + S] * (1 - fx) + D[o + S + 1] * fx) * fy;
    }
    return null;
  }

  dispose() {
    for (const n of this.nodes.values()) if (n.state === 2) this.drop(n);
    this.geo.dispose();
    this.blank.dispose();
    for (const w of this.workers) w.terminate();
  }
}
