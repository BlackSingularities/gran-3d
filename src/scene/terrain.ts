import * as THREE from 'three';
import type { Dem } from '../core/dem';
import type { Biome } from '../core/region';
import { aoPassFragment, passVertex, shadowPassFragment, terrainFragment, terrainVertex, wallFragment, wallVertex } from './shaders';

export const STYLE_ID = { terrain: 0, hypso: 1, slope: 2, aspect: 3, satellite: 4, paper: 5 } as const;

export class TerrainLayer {
  readonly mesh: THREE.Mesh;
  readonly walls: THREE.Mesh;
  readonly material: THREE.ShaderMaterial;
  readonly wallMaterial: THREE.ShaderMaterial;
  readonly demTex: THREE.DataTexture;
  private shadowRT: THREE.WebGLRenderTarget;
  private aoRT: THREE.WebGLRenderTarget;
  private passScene = new THREE.Scene();
  private passCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private passMesh: THREE.Mesh;
  private shadowMat: THREE.ShaderMaterial;
  private vsTex: THREE.DataTexture | null = null;
  private satCanvas: HTMLCanvasElement | null = null;
  private satTex: THREE.CanvasTexture | null = null;
  private satAbort: AbortController | null = null;
  satProgress = 0;

  constructor(private renderer: THREE.WebGLRenderer, readonly dem: Dem, biome: Biome, buildMesh = true) {
    const { w, h } = dem;
    this.demTex = new THREE.DataTexture(dem.data, w, h, THREE.RedFormat, THREE.FloatType);
    this.demTex.minFilter = this.demTex.magFilter = THREE.NearestFilter;
    this.demTex.needsUpdate = true;

    const rtOpts = {
      minFilter: THREE.LinearFilter,
      magFilter: THREE.LinearFilter,
      depthBuffer: false,
      type: THREE.UnsignedByteType,
    };
    this.shadowRT = new THREE.WebGLRenderTarget(w, h, rtOpts);
    this.aoRT = new THREE.WebGLRenderTarget(w, h, rtOpts);

    const common = {
      uDem: { value: this.demTex },
      uSize: { value: new THREE.Vector2(w, h) },
      uMpp: { value: dem.mpp },
    };
    this.shadowMat = new THREE.ShaderMaterial({
      vertexShader: passVertex,
      fragmentShader: shadowPassFragment,
      uniforms: { ...common, uSun: { value: new THREE.Vector3(0, 1, 0) } },
    });
    const aoMat = new THREE.ShaderMaterial({ vertexShader: passVertex, fragmentShader: aoPassFragment, uniforms: { ...common } });
    const tri = new THREE.BufferGeometry();
    tri.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
    this.passMesh = new THREE.Mesh(tri, aoMat);
    this.passMesh.frustumCulled = false;
    this.passScene.add(this.passMesh);
    this.runPass(aoMat, this.aoRT);

    const blank = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
    blank.needsUpdate = true;

    this.material = new THREE.ShaderMaterial({
      vertexShader: terrainVertex,
      fragmentShader: terrainFragment,
      uniforms: {
        ...common,
        uExag: { value: 1 },
        uMinMax: { value: new THREE.Vector2(dem.min, dem.max) },
        uStyle: { value: 0 },
        uShadowTex: { value: this.shadowRT.texture },
        uAoTex: { value: this.aoRT.texture },
        uSatTex: { value: blank },
        uSatReady: { value: 0 },
        uVsTex: { value: blank },
        uVsOn: { value: 0 },
        uContourOn: { value: 1 },
        uContourInt: { value: 50 },
        uShadowOn: { value: 1 },
        uSunDir: { value: new THREE.Vector3(0, 1, 0) },
        uSunColor: { value: new THREE.Color(1, 1, 1) },
        uSkyColor: { value: new THREE.Color(0.4, 0.5, 0.6) },
        uFogColor: { value: new THREE.Color(0.6, 0.7, 0.8) },
        uFogDensity: { value: 1 / 60000 },
        uRelOn: { value: 0 },
        uRef: { value: new THREE.Vector3() },
        uRelRange: { value: 500 },
        uCursor: { value: new THREE.Vector4() },
        uLens: { value: 0 },
        uLensOn: { value: 0 },
        uBandTol: { value: 10 },
        uGridOn: { value: 0 },
        uSnowLine: { value: 2500 },
        uSnowOn: { value: 1 },
        uAutumn: { value: 0 },
        uBiome: { value: new THREE.Vector3(biome.forest, biome.shrub, biome.rock) },
        uLand: { value: blank },
        uLandOn: { value: 0 },
        uNoiseAmt: { value: 1 },
      },
    });

    this.mesh = new THREE.Mesh(buildMesh ? this.buildGeometry() : new THREE.BufferGeometry(), this.material);
    this.mesh.visible = buildMesh;
    this.mesh.frustumCulled = false;

    this.wallMaterial = new THREE.ShaderMaterial({
      vertexShader: wallVertex,
      fragmentShader: wallFragment,
      uniforms: {
        uFogColor: this.material.uniforms.uFogColor,
        uFogDensity: this.material.uniforms.uFogDensity,
        uBase: { value: dem.min - 600 },
        uLight: { value: new THREE.Color(1, 1, 1) },
      },
      side: THREE.DoubleSide,
    });
    this.walls = new THREE.Mesh(this.buildWalls(), this.wallMaterial);
    this.walls.frustumCulled = false;
  }

  private runPass(mat: THREE.ShaderMaterial, rt: THREE.WebGLRenderTarget) {
    this.passMesh.material = mat;
    const prev = this.renderer.getRenderTarget();
    this.renderer.setRenderTarget(rt);
    this.renderer.render(this.passScene, this.passCam);
    this.renderer.setRenderTarget(prev);
  }

  private buildGeometry() {
    const { w, h } = this.dem;
    const pos = new Float32Array(w * h * 3);
    const grid = new Float32Array(w * h * 2);
    for (let y = 0; y < h; y++) {
      const z = this.dem.gyToZ(y);
      for (let x = 0; x < w; x++) {
        const i = y * w + x;
        pos[i * 3] = this.dem.gxToX(x);
        pos[i * 3 + 1] = this.dem.data[i];
        pos[i * 3 + 2] = z;
        grid[i * 2] = x;
        grid[i * 2 + 1] = y;
      }
    }
    const idx = new Uint32Array((w - 1) * (h - 1) * 6);
    let k = 0;
    for (let y = 0; y < h - 1; y++) {
      for (let x = 0; x < w - 1; x++) {
        const a = y * w + x, b = a + 1, c = a + w, d = c + 1;
        // przekątna zgodna z lokalnym kształtem terenu (mniej „schodków” na graniach)
        const dem = this.dem.data;
        if (Math.abs(dem[a] - dem[d]) < Math.abs(dem[b] - dem[c])) {
          idx[k++] = a; idx[k++] = c; idx[k++] = d;
          idx[k++] = a; idx[k++] = d; idx[k++] = b;
        } else {
          idx[k++] = a; idx[k++] = c; idx[k++] = b;
          idx[k++] = b; idx[k++] = c; idx[k++] = d;
        }
      }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('grid', new THREE.BufferAttribute(grid, 2));
    g.setIndex(new THREE.BufferAttribute(idx, 1));
    return g;
  }

  private buildWalls() {
    const { w, h, data } = this.dem;
    const base = this.dem.min - 600;
    const pos: number[] = [];
    const top: number[] = [];
    const idx: number[] = [];
    const strip = (pts: [number, number][], flip: boolean) => {
      const start = pos.length / 3;
      for (const [gx, gy] of pts) {
        const e = data[gy * w + gx];
        const x = this.dem.gxToX(gx), z = this.dem.gyToZ(gy);
        pos.push(x, e, z, x, base, z);
        top.push(e, e);
      }
      for (let i = 0; i < pts.length - 1; i++) {
        const a = start + i * 2, b = a + 1, c = a + 2, d = a + 3;
        if (flip) idx.push(a, c, b, b, c, d);
        else idx.push(a, b, c, b, d, c);
      }
    };
    const north: [number, number][] = [], south: [number, number][] = [], west: [number, number][] = [], east: [number, number][] = [];
    for (let x = 0; x < w; x++) { north.push([x, 0]); south.push([x, h - 1]); }
    for (let y = 0; y < h; y++) { west.push([0, y]); east.push([w - 1, y]); }
    strip(north, true);
    strip(south, false);
    strip(west, false);
    strip(east, true);
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('top', new THREE.Float32BufferAttribute(top, 1));
    g.setIndex(idx);
    return g;
  }

  /** Przelicza mapę cieni rzucanych dla nowego położenia Słońca. */
  updateShadows(sun: THREE.Vector3) {
    this.shadowMat.uniforms.uSun.value.copy(sun);
    this.runPass(this.shadowMat, this.shadowRT);
  }

  setViewshed(mask: Uint8Array | null) {
    const u = this.material.uniforms;
    if (!mask) {
      u.uVsOn.value = 0;
      return;
    }
    if (!this.vsTex) {
      this.vsTex = new THREE.DataTexture(mask, this.dem.w, this.dem.h, THREE.RedFormat, THREE.UnsignedByteType);
      this.vsTex.minFilter = this.vsTex.magFilter = THREE.LinearFilter;
    } else {
      this.vsTex.image.data = mask;
    }
    this.vsTex.needsUpdate = true;
    u.uVsTex.value = this.vsTex;
    u.uVsOn.value = 1;
  }

  /** Wczytuje mozaikę Sentinel-2 cloudless (EOX) dopasowaną do siatki DEM. */
  loadSatellite(onProgress: (f: number) => void) {
    if (this.satCanvas) return;
    const { dem } = this;
    // mozaika Sentinel-2 ma rozdzielczość ~10 m – powyżej z13 nic nie zyskujemy
    const z = Math.min(dem.zoom + 1, 13);
    const k = 2 ** (z - dem.zoom);
    const W = Math.round(dem.w * k), H = Math.round(dem.h * k);
    const canvas = document.createElement('canvas');
    canvas.width = W;
    canvas.height = H;
    const ctx = canvas.getContext('2d')!;
    ctx.fillStyle = '#3b4a3a';
    ctx.fillRect(0, 0, W, H);
    this.satCanvas = canvas;
    this.satTex = new THREE.CanvasTexture(canvas);
    this.satTex.flipY = false;
    this.satTex.colorSpace = THREE.NoColorSpace;
    this.satTex.minFilter = THREE.LinearMipmapLinearFilter;
    this.satTex.anisotropy = this.renderer.capabilities.getMaxAnisotropy();
    this.material.uniforms.uSatTex.value = this.satTex;
    this.material.uniforms.uSatReady.value = 1;

    const ox = Math.round(dem.px0 * k), oy = Math.round(dem.py0 * k);
    const tx0 = Math.floor(ox / 256), tx1 = Math.floor((ox + W - 1) / 256);
    const ty0 = Math.floor(oy / 256), ty1 = Math.floor((oy + H - 1) / 256);
    const jobs: [number, number][] = [];
    const cx = (tx0 + tx1) / 2, cy = (ty0 + ty1) / 2;
    for (let ty = ty0; ty <= ty1; ty++) for (let tx = tx0; tx <= tx1; tx++) jobs.push([tx, ty]);
    jobs.sort((a, b) => Math.hypot(a[0] - cx, a[1] - cy) - Math.hypot(b[0] - cx, b[1] - cy));
    const total = jobs.length;
    let done = 0;
    let dirty = false;
    this.satAbort = new AbortController();
    const signal = this.satAbort.signal;
    const flush = setInterval(() => {
      if (dirty && this.satTex) { this.satTex.needsUpdate = true; dirty = false; }
      if (done >= total || signal.aborted) clearInterval(flush);
    }, 400);
    const worker = async () => {
      while (jobs.length && !signal.aborted) {
        const [tx, ty] = jobs.shift()!;
        try {
          const res = await fetch(`https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2020_3857/default/g/${z}/${ty}/${tx}.jpg`, { signal });
          if (res.ok) {
            const bmp = await createImageBitmap(await res.blob());
            ctx.drawImage(bmp, tx * 256 - ox, ty * 256 - oy);
            bmp.close();
            dirty = true;
          }
        } catch {
          /* pojedynczy kafel może się nie wczytać */
        }
        done++;
        this.satProgress = done / total;
        onProgress(done / total);
      }
    };
    for (let i = 0; i < 6; i++) void worker();
  }

  /** Maska pokrycia terenu (woda/las/skały) wypieczona z poligonów OSM. */
  loadLandcover(url: string) {
    new THREE.TextureLoader().load(url, (tex) => {
      tex.flipY = false;
      tex.colorSpace = THREE.NoColorSpace;
      tex.minFilter = THREE.LinearMipmapLinearFilter;
      tex.magFilter = THREE.LinearFilter;
      tex.anisotropy = 4;
      this.landTex = tex;
      this.material.uniforms.uLand.value = tex;
      this.material.uniforms.uLandOn.value = 1;
      this.onChange();
    });
  }
  private landTex: THREE.Texture | null = null;
  onChange: () => void = () => {};

  setStyle(id: number) {
    this.material.uniforms.uStyle.value = id;
  }

  dispose() {
    this.satAbort?.abort();
    this.mesh.geometry.dispose();
    this.walls.geometry.dispose();
    this.material.dispose();
    this.wallMaterial.dispose();
    this.shadowMat.dispose();
    (this.passMesh.material as THREE.Material).dispose();
    this.demTex.dispose();
    this.shadowRT.dispose();
    this.aoRT.dispose();
    this.vsTex?.dispose();
    this.satTex?.dispose();
    this.landTex?.dispose();
  }
}
