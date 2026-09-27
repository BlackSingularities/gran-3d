import * as THREE from 'three';
import { routeFromPolyline, TrailGraph, type Route } from './core/graph';
import { bearing, clamp, curvatureDrop, DEG, fmtDist, fmtEle, fmtSigned, haversine } from './core/geo';
import { download, parseGpx, routeToGpx } from './core/gpx';
import { timeModel } from './core/metrics';
import { loadRegion, REGIONS, regionForPoint, type LoadedRegion, type Poi } from './core/region';
import { Store, todayWarsaw, type MeasurePoint, type State, type Tool, type Waypoint } from './core/store';
import { seasonalSnowline, sunPosition, sunVector, warsawDate } from './core/sun';
import { Engine } from './scene/engine';
import { Overlay, type Marker } from './scene/overlay';
import { ALT_COLORS, RouteLayer } from './scene/routes';
import { STYLE_ID, TerrainLayer } from './scene/terrain';
import { TrailsLayer } from './scene/trails';
import { Profile } from './ui/profile';

export interface CursorInfo {
  x: number;
  z: number;
  e: number;
  lon: number;
  lat: number;
  slope: number;
  aspect: number;
}

export interface Selection {
  kind: 'poi' | 'point' | 'trail';
  poi?: Poi;
  point?: MeasurePoint;
  edges?: number[];
  routeIdx?: number;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

/** Model oświetlenia nieba zależny od wysokości Słońca. */
function skyModel(alt: number) {
  const s = (a: number, b: number, x: number) => {
    const t = clamp((x - a) / (b - a), 0, 1);
    return t * t * (3 - 2 * t);
  };
  const day = s(-6, 12, alt);
  const golden = 1 - s(2, 22, alt);
  const up = s(-4, 4, alt);
  const sun = new THREE.Color().setRGB(1.0, 0.94 - golden * 0.38, 0.86 - golden * 0.58).multiplyScalar(3.1 * up * (0.45 + 0.55 * s(0, 30, alt)));
  const zenith = new THREE.Color('#0b1522').lerp(new THREE.Color('#3f6f9f'), day);
  const horizon = new THREE.Color('#1a2433').lerp(new THREE.Color('#e8a878'), golden * up * 0.9).lerp(new THREE.Color('#bcd2e2'), s(8, 30, alt));
  const ambient = new THREE.Color('#1c2a42').multiplyScalar(0.35).lerp(new THREE.Color('#9cb8d6').multiplyScalar(0.95), day);
  return { sun, zenith, horizon, ambient, day, golden: golden * up };
}

export class App {
  store: Store;
  engine: Engine;
  overlay: Overlay;
  profile: Profile;
  region: LoadedRegion | null = null;
  graph: TrailGraph | null = null;
  terrain: TerrainLayer | null = null;
  trails: TrailsLayer | null = null;
  routesLayer = new RouteLayer();
  worker: Worker;
  private reqId = 0;
  private pending = new Map<number, (d: any) => void>();
  cursor: CursorInfo | null = null;
  selection: Selection | null = null;
  hoverEdge: number | null = null;
  private wpMarkers = new Map<string, Marker>();
  private refMarker: Marker | null = null;
  private vsMarker: Marker | null = null;
  private mMarkers: Marker[] = [];
  private hoverMarker: Marker | null = null;
  private selMarker: Marker | null = null;
  private shadowQueued = false;
  private routeTimer = 0;
  private hashTimer = 0;
  measureInfo: { surface: number; horiz: number; straight: number; dh: number; bearing: number; visible: boolean | null; angle: number } | null = null;
  fly: { i: number; playing: boolean; speed: number } | null = null;
  satProgress = 0;
  vsMask: Uint8Array | null = null;
  listeners: (() => void)[] = [];
  loading = false;

  constructor() {
    const params = this.readHash();
    this.store = new Store({
      regionId: params.region ?? 'tatry',
      tool: 'explore',
      style: 'terrain',
      contours: true,
      trails: true,
      labels: true,
      shadows: true,
      grid: false,
      snow: true,
      exag: 1.4,
      day: todayWarsaw(),
      hour: 11.5,
      routeMode: 'trails',
      maxSlope: 32,
      timeKind: 'pttk',
      pace: 1,
      waypoints: [],
      routes: [],
      activeRoute: 0,
      routing: false,
      ref: null,
      relRange: 600,
      viewshed: null,
      vsEye: 1.7,
      measure: [],
      gpx: null,
      profileOpen: true,
      panelOpen: window.innerWidth > 760,
      panorama: false,
      flying: false,
    } satisfies State);

    this.engine = new Engine($('gl') as HTMLCanvasElement);
    this.engine.world.scale.y = this.engine.exag = this.store.state.exag;
    this.overlay = new Overlay($('overlay'), this.engine);
    this.profile = new Profile($('profile-canvas') as HTMLCanvasElement, $('profile-tip'));
    this.worker = new Worker(new URL('./workers/terrain.worker.ts', import.meta.url), { type: 'module' });
    this.worker.onmessage = (e) => {
      const cb = this.pending.get(e.data.id);
      if (cb) {
        this.pending.delete(e.data.id);
        cb(e.data);
      }
    };

    this.engine.world.add(this.routesLayer.group);
    this.engine.onFrame((dt) => this.frame(dt));
    this.store.on((s, ch) => this.onState(s, ch));
    this.overlay.onPoiClick = (p) => this.selectPoi(p, true);
    this.bindCanvas();
    this.pendingHash = params;
  }

  private pendingHash: ReturnType<App['readHash']> | null = null;

  changed() {
    for (const l of this.listeners) l();
  }

  // ---------------------------------------------------------------- region
  async loadRegion(id: string, onProgress: (f: number, label: string) => void) {
    const def = REGIONS.find((r) => r.id === id) ?? REGIONS[0];
    this.loading = true;
    this.stopFly();
    this.engine.exitPanorama();
    const data = await loadRegion(def, onProgress);
    onProgress(0.86, 'Budowa bryły terenu');
    await new Promise((r) => setTimeout(r, 20));
    // sprzątanie poprzedniego regionu
    if (this.terrain) {
      this.engine.world.remove(this.terrain.mesh, this.terrain.walls);
      this.terrain.dispose();
    }
    if (this.trails) {
      this.engine.world.remove(this.trails.group);
      this.trails.dispose();
    }
    this.overlay.clear();
    this.wpMarkers.clear();
    this.refMarker = this.vsMarker = this.hoverMarker = this.selMarker = null;
    this.mMarkers = [];
    this.selection = null;
    this.vsMask = null;

    this.region = data;
    this.engine.setDem(data.dem);
    this.terrain = new TerrainLayer(this.engine.renderer, data.dem, data.biome);
    this.engine.world.add(this.terrain.mesh, this.terrain.walls);
    this.terrain.onChange = () => (this.engine.dirty = true);
    this.terrain.loadLandcover(`${import.meta.env.BASE_URL}data/${def.id}/landcover.png`);
    onProgress(0.93, 'Sieć szlaków i punkty');
    this.graph = new TrailGraph(data.trails, data.dem);
    this.trails = new TrailsLayer(this.graph);
    this.engine.world.add(this.trails.group);
    this.overlay.setPois(data.pois);
    this.worker.postMessage({ type: 'init', w: data.dem.w, h: data.dem.h, mpp: data.dem.mpp, data: data.dem.data });
    this.routesLayer.setRoutes([], 0);
    this.routesLayer.setGpx(null);
    this.routesLayer.setMeasure(null, null, false);

    this.store.set({ regionId: def.id, waypoints: [], routes: [], ref: null, viewshed: null, measure: [], gpx: null, panorama: false });
    this.satProgress = 0;
    this.applyStyle();
    this.applyLayers();
    this.updateSun();

    const h = def.home;
    const [x, z] = data.dem.lonLatToWorld(h.lon, h.lat);
    const ph = this.pendingHash;
    if (ph?.view && ph.region === def.id) {
      const [vx, vz] = data.dem.lonLatToWorld(ph.view.lon, ph.view.lat);
      this.engine.setView({ x: vx, z: vz, distance: ph.view.distance, heading: ph.view.heading, pitch: ph.view.pitch });
    } else {
      this.engine.setView({ x, z, distance: h.distance * 1.9, heading: h.heading - 25, pitch: 30 });
      this.engine.flyTo({ x, z, distance: h.distance, heading: h.heading, pitch: h.pitch }, 3200);
    }
    if (ph?.region === def.id) {
      if (ph.mode) this.store.set({ routeMode: ph.mode });
      if (ph.wps?.length) {
        for (const [lon, lat] of ph.wps) {
          const [wx, wz] = data.dem.lonLatToWorld(lon, lat);
          this.addWaypoint(wx, wz, undefined, true);
        }
        this.store.set({ tool: 'route' });
      }
    }
    this.pendingHash = null;
    this.loading = false;
    onProgress(1, 'Gotowe');
    this.changed();
  }

  // ---------------------------------------------------------------- stan → scena
  private onState(s: State, ch: Set<keyof State>) {
    if (!this.terrain) return;
    if (ch.has('style')) this.applyStyle();
    if (['contours', 'trails', 'labels', 'shadows', 'grid', 'snow'].some((k) => ch.has(k as keyof State))) this.applyLayers();
    if (ch.has('exag')) {
      this.engine.setExag(s.exag);
      this.terrain.material.uniforms.uExag.value = s.exag;
      this.refreshMarkers();
    }
    if (ch.has('day') || ch.has('hour')) this.updateSun();
    if (ch.has('timeKind') || ch.has('pace')) {
      timeModel.kind = s.timeKind;
      timeModel.pace = s.pace;
      this.scheduleRoute(0);
      if (s.gpx) this.store.set({ gpx: routeFromPolyline(this.region!.dem, Array.from({ length: s.gpx.track.n }, (_, i) => ({ lon: s.gpx!.track.lon[i], lat: s.gpx!.track.lat[i] })), 'gpx', s.gpx.labels[0]) });
    }
    if (ch.has('routes') || ch.has('activeRoute')) {
      this.routesLayer.setRoutes(s.routes, s.activeRoute);
      this.updateProfile();
    }
    if (ch.has('gpx')) {
      this.routesLayer.setGpx(s.gpx);
      this.updateProfile();
    }
    if (ch.has('ref') || ch.has('relRange')) this.applyRef();
    if (ch.has('tool')) this.onTool(s.tool);
    if (ch.has('measure')) this.applyMeasure();
    if (ch.has('profileOpen')) document.getElementById('app')!.classList.toggle('has-profile', s.profileOpen && !!this.profileTrack);
    if (ch.has('panelOpen')) document.getElementById('app')!.classList.toggle('panel-closed', !s.panelOpen);
    if (ch.has('waypoints') || ch.has('routes') || ch.has('activeRoute') || ch.has('routeMode')) this.scheduleHash();
    this.engine.dirty = true;
    this.changed();
  }

  applyStyle() {
    const s = this.store.state;
    this.terrain!.setStyle(STYLE_ID[s.style]);
    if (s.style === 'satellite') {
      this.terrain!.loadSatellite((f) => {
        this.satProgress = f;
        this.engine.dirty = true;
        if (f === 1 || Math.round(f * 100) % 10 === 0) this.changed();
      });
    }
    this.engine.dirty = true;
  }

  applyLayers() {
    const s = this.store.state;
    const u = this.terrain!.material.uniforms;
    u.uContourOn.value = s.contours && !s.panorama ? 1 : 0;
    u.uShadowOn.value = s.shadows ? 1 : 0;
    u.uGridOn.value = s.grid ? 1 : 0;
    u.uSnowOn.value = s.snow ? 1 : 0;
    this.trails!.setVisible(s.trails);
    this.overlay.enabled = s.labels;
    this.overlay.update(true);
    this.engine.dirty = true;
  }

  sunInfo() {
    const s = this.store.state;
    const dem = this.region!.dem;
    const date = warsawDate(s.day, s.hour);
    return { date, ...sunPosition(date, dem.centerLat, dem.centerLon) };
  }

  updateSun() {
    if (!this.terrain) return;
    const { date, azimuth, altitude } = this.sunInfo();
    const v = sunVector(azimuth, Math.max(altitude, -2));
    const u = this.terrain.material.uniforms;
    const sky = skyModel(altitude);
    u.uSunDir.value.set(v[0], Math.max(v[1], 0.02), v[2]).normalize();
    u.uSunColor.value.copy(sky.sun);
    u.uSkyColor.value.copy(sky.ambient);
    u.uFogColor.value.copy(sky.horizon).multiplyScalar(0.9 + sky.day * 0.25);
    u.uFogDensity.value = 1 / (70000 + sky.day * 20000);
    // mgła sceny dla linii (szlaki, trasy) – zgodna z perspektywą powietrzną terenu
    const fog = (this.engine.scene.fog as THREE.FogExp2 | null) ?? (this.engine.scene.fog = new THREE.FogExp2(0xffffff, 1));
    fog.color.copy(u.uFogColor.value);
    fog.density = u.uFogDensity.value * 1.05;
    this.terrain.wallMaterial.uniforms.uLight.value.setScalar(0.25 + sky.day * 0.85);
    u.uSnowLine.value = seasonalSnowline(date);
    const m = date.getUTCMonth();
    const doy = (date.getTime() - Date.UTC(date.getUTCFullYear(), 0, 1)) / 86400000;
    u.uAutumn.value = m >= 8 && m <= 10 ? clamp(1 - Math.abs(doy - 290) / 28, 0, 1) * 0.75 : 0;
    const zen = sky.zenith.clone().convertLinearToSRGB().getStyle();
    const hor = sky.horizon.clone().convertLinearToSRGB().getStyle();
    const mid = sky.zenith.clone().lerp(sky.horizon, 0.55).convertLinearToSRGB().getStyle();
    $('sky').style.background = `linear-gradient(180deg, ${zen} 0%, ${mid} 45%, ${hor} 72%, ${hor} 100%)`;
    if (!this.shadowQueued) {
      this.shadowQueued = true;
      requestAnimationFrame(() => {
        this.shadowQueued = false;
        const { azimuth: az, altitude: al } = this.sunInfo();
        const sv = sunVector(az, al);
        this.terrain?.updateShadows(new THREE.Vector3(sv[0], sv[1], sv[2]));
        this.engine.dirty = true;
      });
    }
    this.engine.dirty = true;
  }

  // ---------------------------------------------------------------- pętla
  private frame(dt: number) {
    if (!this.terrain) return;
    const view = this.engine.view;
    const d = view.distance;
    const u = this.terrain.material.uniforms;
    u.uContourInt.value = d < 2200 ? 10 : d < 6000 ? 20 : d < 16000 ? 50 : 100;
    const s = this.store.state;
    this.trails?.update(d, s.routes.length > 0 || !!s.gpx);
    if (this.routesLayer.update(dt, d)) this.engine.dirty = true;
    this.overlay.update();
    if (this.fly?.playing) this.stepFly(dt);
  }

  // ---------------------------------------------------------------- wejście
  private bindCanvas() {
    const canvas = this.engine.canvas;
    let down: { x: number; y: number; t: number; button: number } | null = null;
    let moveQueued: PointerEvent | null = null;
    canvas.addEventListener('pointerdown', (e) => {
      down = { x: e.clientX, y: e.clientY, t: performance.now(), button: e.button };
      this.closeCtx();
    });
    canvas.addEventListener('pointerup', (e) => {
      if (!down) return;
      const moved = Math.hypot(e.clientX - down.x, e.clientY - down.y);
      if (moved < 5 && down.button === 0 && performance.now() - down.t < 600) this.onClick(e);
      down = null;
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!moveQueued) requestAnimationFrame(() => {
        if (moveQueued) this.onMove(moveQueued);
        moveQueued = null;
      });
      moveQueued = e;
    });
    canvas.addEventListener('pointerleave', () => {
      this.cursor = null;
      if (this.terrain) this.terrain.material.uniforms.uCursor.value.w = 0;
      this.setHoverEdge(null, 0, 0);
      this.engine.dirty = true;
      this.changed();
    });
    canvas.addEventListener('dblclick', (e) => {
      const p = this.engine.pick(e.clientX, e.clientY);
      if (!p) return;
      const v = this.engine.view;
      this.engine.flyTo({ x: p.x, z: p.z, distance: Math.max(700, v.distance * 0.45) }, 1100);
    });
    canvas.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      if (this.engine.panorama) return;
      const p = this.engine.pick(e.clientX, e.clientY);
      if (p) this.openCtx?.(e.clientX, e.clientY, this.pointFromWorld(p.x, p.z));
    });
    canvas.addEventListener(
      'wheel',
      (e) => {
        if (this.engine.panorama) {
          e.preventDefault();
          this.engine.zoomFov(e.deltaY);
        }
      },
      { passive: false }
    );
  }

  openCtx: ((x: number, y: number, p: MeasurePoint) => void) | null = null;
  closeCtx() {
    const el = $('ctx');
    el.hidden = true;
  }

  pointFromWorld(x: number, z: number): MeasurePoint {
    const dem = this.region!.dem;
    const [lon, lat] = dem.worldToLonLat(x, z);
    return { x, z, e: dem.sampleWorld(x, z), lon, lat };
  }

  private onMove(e: PointerEvent) {
    if (!this.region || this.loading) return;
    const p = this.engine.pick(e.clientX, e.clientY);
    const u = this.terrain!.material.uniforms;
    if (!p) {
      this.cursor = null;
      u.uCursor.value.w = 0;
      this.setHoverEdge(null, 0, 0);
      this.changed();
      return;
    }
    const dem = this.region.dem;
    const gx = dem.xToGx(p.x), gy = dem.zToGy(p.z);
    const [lon, lat] = dem.worldToLonLat(p.x, p.z);
    this.cursor = { x: p.x, z: p.z, e: p.y, lon, lat, slope: dem.slope(gx, gy), aspect: dem.aspect(gx, gy) };
    u.uCursor.value.set(p.x, p.z, p.y, this.engine.panorama ? 0 : 1);
    // podświetlenie szlaku pod kursorem
    const tool = this.store.state.tool;
    if (this.graph && this.store.state.trails && (tool === 'explore' || tool === 'route') && !this.engine.panorama) {
      const mpp = this.engine.metersPerPixel() ?? 20;
      const snap = this.graph.snap(p.x, p.z, clamp(mpp * 9, 15, 400));
      this.setHoverEdge(snap ? snap.edge : null, e.clientX, e.clientY);
    }
    if (tool === 'measure' && this.store.state.measure.length) this.applyMeasure();
    this.engine.dirty = true;
    this.changed();
  }

  private tipEl: HTMLElement | null = null;
  private setHoverEdge(eid: number | null, cx: number, cy: number) {
    if (eid === this.hoverEdge && eid == null) return;
    const sel = this.selection?.kind === 'trail' ? this.selection.edges ?? [] : [];
    if (eid !== this.hoverEdge) {
      this.hoverEdge = eid;
      this.trails?.highlight(eid != null ? [eid, ...sel] : sel.length ? sel : null);
      this.engine.dirty = true;
    }
    if (!this.tipEl) {
      this.tipEl = document.createElement('div');
      this.tipEl.className = 'trail-tip';
      document.getElementById('app')!.appendChild(this.tipEl);
    }
    if (eid == null || !this.graph) {
      this.tipEl.style.display = 'none';
      return;
    }
    this.tipEl.style.display = '';
    this.tipEl.style.left = `${cx}px`;
    this.tipEl.style.top = `${cy}px`;
    const ed = this.graph.edges[eid];
    const names = this.graph.edgeRouteNames(eid);
    const rows = names.slice(0, 3).map((r) => `<div class="row"><span class="trail-chip"><i style="background:${trailHex(r.color)}"></i></span>${escapeHtml(r.name)}</div>`).join('');
    this.tipEl.innerHTML = `${rows || '<div>Odcinek szlaku</div>'}<small>${fmtDist(ed.len2)} · ↗ ${Math.round(ed.up)} m ↘ ${Math.round(ed.down)} m${ed.sac ? ` · T${ed.sac}` : ''}</small>`;
  }

  private onClick(e: PointerEvent) {
    if (!this.region || this.engine.panorama) return;
    const p = this.engine.pick(e.clientX, e.clientY);
    if (!p) return;
    const s = this.store.state;
    switch (s.tool) {
      case 'route':
        this.addWaypoint(p.x, p.z);
        break;
      case 'relative':
        this.setRef(p.x, p.z);
        break;
      case 'viewshed':
        this.runViewshed(p.x, p.z);
        break;
      case 'measure':
        this.store.set({ measure: [...s.measure, this.pointFromWorld(p.x, p.z)] });
        break;
      default:
        if (this.hoverEdge != null && this.graph) this.selectTrail(this.hoverEdge);
        else this.selectPoint(p.x, p.z);
    }
  }

  // ---------------------------------------------------------------- zaznaczenia
  selectPoi(p: Poi, fly = false) {
    this.selection = { kind: 'poi', poi: p };
    this.setSelMarker(p.x, p.z, p.d);
    if (this.store.state.tool !== 'explore') this.store.set({ tool: 'explore' });
    if (fly) {
      const v = this.engine.view;
      this.engine.flyTo({ x: p.x, z: p.z, distance: Math.min(v.distance, 6000) }, 1400);
    }
    if (!this.store.state.panelOpen) this.store.set({ panelOpen: true });
    this.changed();
  }

  selectPoint(x: number, z: number) {
    this.selection = { kind: 'point', point: this.pointFromWorld(x, z) };
    this.setSelMarker(x, z, this.region!.dem.sampleWorld(x, z));
    this.changed();
  }

  selectTrail(eid: number) {
    if (!this.graph) return;
    const ed = this.graph.edges[eid];
    // cały szlak (relacja) o najwyższym priorytecie przechodzący przez odcinek
    const rIdx = ed.routes[0];
    const edges = rIdx != null ? this.graph.edges.filter((e) => e.routes.includes(rIdx)).map((e) => e.id) : [eid];
    this.selection = { kind: 'trail', edges, routeIdx: rIdx };
    this.trails?.highlight(edges);
    this.setSelMarker(0, 0, 0, true);
    this.changed();
  }

  selectRouteByIndex(rIdx: number, fly = true) {
    if (!this.graph) return;
    const edges = this.graph.edges.filter((e) => e.routes.includes(rIdx)).map((e) => e.id);
    if (!edges.length) return;
    this.selection = { kind: 'trail', edges, routeIdx: rIdx };
    this.trails?.highlight(edges);
    if (this.store.state.tool !== 'explore') this.store.set({ tool: 'explore' });
    if (fly) this.fitEdges(edges);
    this.changed();
  }

  fitEdges(edges: number[]) {
    let x0 = Infinity, x1 = -Infinity, z0 = Infinity, z1 = -Infinity;
    for (const id of edges) {
      const ed = this.graph!.edges[id];
      for (let i = 0; i < ed.n; i++) {
        x0 = Math.min(x0, ed.x[i]); x1 = Math.max(x1, ed.x[i]);
        z0 = Math.min(z0, ed.z[i]); z1 = Math.max(z1, ed.z[i]);
      }
    }
    this.fitBox(x0, x1, z0, z1);
  }

  fitBox(x0: number, x1: number, z0: number, z1: number) {
    const size = Math.max(x1 - x0, z1 - z0, 600);
    this.engine.flyTo({ x: (x0 + x1) / 2, z: (z0 + z1) / 2, distance: size * 1.5, pitch: 52 }, 1500);
  }

  clearSelection() {
    this.selection = null;
    this.trails?.highlight(null);
    this.setSelMarker(0, 0, 0, true);
    this.changed();
  }

  private setSelMarker(x: number, z: number, e: number, remove = false) {
    this.overlay.remove(this.selMarker);
    this.selMarker = null;
    if (remove) return;
    const el = document.createElement('div');
    el.className = 'marker';
    el.innerHTML = '<div class="marker-foot"></div>';
    this.selMarker = this.overlay.add({ el, x, z, e });
  }

  // ---------------------------------------------------------------- narzędzia
  setTool(t: Tool) {
    if (this.engine.panorama) this.exitPanorama();
    this.store.set({ tool: this.store.state.tool === t && t !== 'explore' ? 'explore' : t });
  }

  private onTool(t: Tool) {
    const u = this.terrain!.material.uniforms;
    u.uRelOn.value = t === 'relative' && this.store.state.ref ? 1 : 0;
    u.uVsOn.value = t === 'viewshed' && this.vsMask ? 1 : 0;
    if (t !== 'explore') this.clearSelection();
    this.refreshMarkers();
    this.updateProfile();
  }

  // --- punkty trasy
  addWaypoint(x: number, z: number, index?: number, silent = false) {
    const s = this.store.state;
    const dem = this.region!.dem;
    const snap = s.routeMode === 'trails' ? this.graph!.snap(x, z, 450) : null;
    if (s.routeMode === 'trails' && !snap) {
      if (!silent) this.toast('W pobliżu nie ma szlaku. Kliknij bliżej szlaku albo przełącz tryb na „przez teren”.', true);
      return;
    }
    const px = snap ? snap.x : x, pz = snap ? snap.z : z;
    const [lon, lat] = dem.worldToLonLat(px, pz);
    const wp: Waypoint = { id: Math.random().toString(36).slice(2, 8), x: px, z: pz, lon, lat, e: dem.sampleWorld(px, pz), snap, label: this.nearbyName(px, pz) };
    const list = [...s.waypoints];
    if (index == null) list.push(wp); else list.splice(index, 0, wp);
    this.store.set({ waypoints: list, tool: 'route' });
    this.refreshMarkers();
    this.scheduleRoute(0);
  }

  moveWaypoint(id: string, x: number, z: number, final: boolean) {
    const s = this.store.state;
    const dem = this.region!.dem;
    const snap = s.routeMode === 'trails' ? this.graph!.snap(x, z, 450) : null;
    if (s.routeMode === 'trails' && !snap) return;
    const px = snap ? snap.x : x, pz = snap ? snap.z : z;
    const [lon, lat] = dem.worldToLonLat(px, pz);
    const list = s.waypoints.map((w) => (w.id === id ? { ...w, x: px, z: pz, lon, lat, e: dem.sampleWorld(px, pz), snap, label: final ? this.nearbyName(px, pz) : w.label } : w));
    this.store.set({ waypoints: list });
    const m = this.wpMarkers.get(id);
    if (m) { m.x = px; m.z = pz; m.e = dem.sampleWorld(px, pz); }
    if (final || s.routeMode === 'trails') this.scheduleRoute(final ? 0 : 60);
  }

  removeWaypoint(id: string) {
    this.store.set({ waypoints: this.store.state.waypoints.filter((w) => w.id !== id) });
    this.refreshMarkers();
    this.scheduleRoute(0);
  }

  reverseRoute() {
    this.store.set({ waypoints: [...this.store.state.waypoints].reverse() });
    this.refreshMarkers();
    this.scheduleRoute(0);
  }

  clearRoute() {
    this.stopFly();
    this.store.set({ waypoints: [], routes: [], activeRoute: 0 });
    this.refreshMarkers();
  }

  setRouteMode(m: 'trails' | 'terrain') {
    const s = this.store.state;
    // przepięcie istniejących punktów
    const wps = s.waypoints
      .map((w) => {
        const snap = m === 'trails' ? this.graph!.snap(w.x, w.z, 450) : null;
        if (m === 'trails' && !snap) return null;
        return { ...w, snap, x: snap?.x ?? w.x, z: snap?.z ?? w.z };
      })
      .filter(Boolean) as Waypoint[];
    this.store.set({ routeMode: m, waypoints: wps });
    this.refreshMarkers();
    this.scheduleRoute(0);
  }

  private nearbyName(x: number, z: number) {
    let best: Poi | null = null;
    let bd = 350;
    for (const p of this.region!.pois) {
      if (p.t === 'lake' || p.t === 'viewpoint') continue;
      const d = Math.hypot(p.x - x, p.z - z);
      if (d < bd) { bd = d; best = p; }
    }
    return best?.n;
  }

  scheduleRoute(delay = 0) {
    clearTimeout(this.routeTimer);
    this.routeTimer = window.setTimeout(() => void this.computeRoutes(), delay);
  }

  async computeRoutes() {
    const s = this.store.state;
    if (s.waypoints.length < 2 || !this.graph) {
      this.store.set({ routes: [], activeRoute: 0 });
      return;
    }
    const prevLabel = s.routes[s.activeRoute]?.labels[0];
    if (s.routeMode === 'trails') {
      const snaps = s.waypoints.map((w) => w.snap!).filter(Boolean);
      const routes = this.graph.alternatives(snaps);
      if (!routes.length) this.toast('Nie znaleziono połączenia szlakami między tymi punktami.', true);
      let active = routes.findIndex((r) => r.labels.includes(prevLabel ?? ''));
      if (active < 0) active = 0;
      this.store.set({ routes, activeRoute: active });
    } else {
      this.store.set({ routing: true });
      const dem = this.region!.dem;
      const pts: { lon: number; lat: number }[] = [];
      for (let i = 0; i < s.waypoints.length - 1; i++) {
        const a = s.waypoints[i], b = s.waypoints[i + 1];
        const res = await this.request({
          type: 'path',
          a: [dem.xToGx(a.x), dem.zToGy(a.z)],
          b: [dem.xToGx(b.x), dem.zToGy(b.z)],
          maxSlope: s.maxSlope,
        });
        if (!res.path) {
          this.store.set({ routing: false, routes: [] });
          this.toast(`Brak przejścia przy nachyleniu ≤ ${s.maxSlope}°. Zwiększ dopuszczalne nachylenie.`, true);
          return;
        }
        for (const [gx, gy] of res.path as [number, number][]) {
          if (pts.length && i > 0 && gx === res.path[0][0] && gy === res.path[0][1]) continue;
          pts.push({ lon: dem.gxToLon(gx), lat: dem.gyToLat(gy) });
        }
      }
      const terrain = routeFromPolyline(dem, densify(pts, dem), 'terrain', 'Przez teren', 1.3);
      const routes: Route[] = [terrain];
      // porównanie ze szlakiem, jeśli punkty leżą blisko sieci
      const snaps = s.waypoints.map((w) => this.graph!.snap(w.x, w.z, 450));
      if (snaps.every(Boolean)) {
        const tr = this.graph.route(snaps as NonNullable<(typeof snaps)[number]>[], 'time');
        if (tr) { tr.labels = ['Szlakiem (porównanie)']; routes.push(tr); }
      }
      this.store.set({ routes, activeRoute: 0, routing: false });
    }
  }

  setActiveRoute(i: number) {
    this.stopFly();
    this.store.set({ activeRoute: i });
  }

  get activeRoute(): Route | null {
    const s = this.store.state;
    return s.routes[s.activeRoute] ?? null;
  }

  fitRoute(r: Route | null = this.activeRoute) {
    if (!r) return;
    const t = r.track;
    let x0 = Infinity, x1 = -Infinity, z0 = Infinity, z1 = -Infinity;
    for (let i = 0; i < t.n; i++) {
      x0 = Math.min(x0, t.x[i]); x1 = Math.max(x1, t.x[i]);
      z0 = Math.min(z0, t.z[i]); z1 = Math.max(z1, t.z[i]);
    }
    this.fitBox(x0, x1, z0, z1);
  }

  // --- wysokość względna
  setRef(x: number, z: number) {
    const p = this.pointFromWorld(x, z);
    this.store.set({ ref: p, tool: 'relative' });
  }

  private applyRef() {
    const s = this.store.state;
    const u = this.terrain!.material.uniforms;
    if (s.ref) {
      u.uRef.value.set(s.ref.x, s.ref.z, s.ref.e);
      u.uRelRange.value = s.relRange;
    }
    u.uRelOn.value = s.ref && s.tool === 'relative' ? 1 : 0;
    this.refreshMarkers();
  }

  /** Statystyki wysokości względnej: jaki odsetek regionu jest wyżej/niżej, najwyższe punkty ponad odniesieniem. */
  relStats() {
    const s = this.store.state;
    if (!s.ref || !this.region) return null;
    const dem = this.region.dem;
    let above = 0;
    const d = dem.data;
    for (let i = 0; i < d.length; i += 3) if (d[i] > s.ref.e) above++;
    const frac = above / Math.ceil(d.length / 3);
    const peaks = this.region.pois
      .filter((p) => p.t === 'peak')
      .map((p) => ({ p, dh: p.ele - s.ref!.e, dist: haversine(s.ref!.lon, s.ref!.lat, p.lon, p.lat) }))
      .filter((q) => q.dist > 150)
      .sort((a, b) => b.p.prom / (1 + b.dist / 8000) - a.p.prom / (1 + a.dist / 8000))
      .slice(0, 8);
    return { frac, peaks };
  }

  // --- widoczność
  async runViewshed(x: number, z: number) {
    const s = this.store.state;
    const dem = this.region!.dem;
    const point = this.pointFromWorld(x, z);
    this.store.set({ tool: 'viewshed', viewshed: { point, eye: s.vsEye, areaKm2: NaN, count: 0, peaks: [] } });
    this.refreshMarkers();
    const res = await this.request({ type: 'viewshed', gx: dem.xToGx(x), gy: dem.zToGy(z), eye: s.vsEye, target: 0 });
    const mask = res.mask as Uint8Array;
    this.vsMask = mask;
    this.terrain!.setViewshed(mask);
    if (this.store.state.tool !== 'viewshed') this.terrain!.material.uniforms.uVsOn.value = 0;
    // widoczne szczyty
    const peaks: { id: number; dist: number }[] = [];
    for (const p of this.region!.pois) {
      if (p.t !== 'peak' && p.t !== 'hut') continue;
      const gx = Math.round(dem.lonToGx(p.lon)), gy = Math.round(dem.latToGy(p.lat));
      let vis = false;
      for (let dy = -1; dy <= 1 && !vis; dy++) for (let dx = -1; dx <= 1 && !vis; dx++) {
        const X = gx + dx, Y = gy + dy;
        if (X >= 0 && Y >= 0 && X < dem.w && Y < dem.h && mask[Y * dem.w + X]) vis = true;
      }
      if (vis) peaks.push({ id: p.id, dist: haversine(point.lon, point.lat, p.lon, p.lat) });
    }
    peaks.sort((a, b) => this.region!.pois[b.id].prom / (1 + b.dist / 15000) - this.region!.pois[a.id].prom / (1 + a.dist / 15000));
    this.store.set({ viewshed: { point, eye: s.vsEye, areaKm2: res.areaKm2, count: res.count, peaks } });
    this.engine.dirty = true;
  }

  clearViewshed() {
    this.vsMask = null;
    this.terrain?.setViewshed(null);
    this.store.set({ viewshed: null });
    this.refreshMarkers();
  }

  private request(msg: Record<string, unknown>): Promise<any> {
    const id = ++this.reqId;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ ...msg, id });
    });
  }

  // --- pomiar i linia wzroku
  private applyMeasure() {
    const s = this.store.state;
    const pts = [...s.measure];
    if (!this.region) return;
    const dem = this.region.dem;
    this.mMarkers.forEach((m) => this.overlay.remove(m));
    this.mMarkers = [];
    if (!pts.length) {
      this.routesLayer.setMeasure(null, null, false);
      this.measureInfo = null;
      this.updateProfile();
      return;
    }
    // podgląd następnego punktu pod kursorem
    const preview = s.tool === 'measure' && this.cursor ? [...pts, { x: this.cursor.x, z: this.cursor.z, e: this.cursor.e, lon: this.cursor.lon, lat: this.cursor.lat }] : pts;
    const surface: number[] = [];
    let surf = 0, horiz = 0;
    for (let i = 0; i < preview.length - 1; i++) {
      const a = preview[i], b = preview[i + 1];
      const pr = dem.profile(a.x, a.z, b.x, b.z);
      for (let k = 0; k < pr.d.length; k++) {
        if (i > 0 && k === 0) continue;
        const f = pr.len > 0 ? pr.d[k] / pr.len : 0;
        surface.push(a.x + (b.x - a.x) * f, pr.e[k] + 4, a.z + (b.z - a.z) * f);
        if (k) surf += Math.hypot(pr.d[k] - pr.d[k - 1], pr.e[k] - pr.e[k - 1]);
      }
      horiz += haversine(a.lon, a.lat, b.lon, b.lat);
    }
    const A = preview[0], B = preview[preview.length - 1];
    const straight = Math.hypot(haversine(A.lon, A.lat, B.lon, B.lat), B.e - A.e);
    let sight: number[] | null = null;
    let visible: boolean | null = null;
    if (preview.length >= 2) {
      const eye = 1.7;
      const pr = dem.profile(A.x, A.z, B.x, B.z);
      visible = true;
      const h0 = A.e + eye, h1 = B.e + eye;
      for (let k = 1; k < pr.d.length - 1; k++) {
        const f = pr.d[k] / pr.len;
        const line = h0 + (h1 - h0) * f;
        if (pr.e[k] + bulge(pr.d[k], pr.len) > line + 0.5) { visible = false; break; }
      }
      sight = [A.x, A.e + eye + 2, A.z, B.x, B.e + eye + 2, B.z];
    }
    this.routesLayer.setMeasure(surface, sight, visible === false);
    const hb = haversine(A.lon, A.lat, B.lon, B.lat);
    this.measureInfo = {
      surface: surf,
      horiz,
      straight,
      dh: B.e - A.e,
      bearing: bearing(A.lon, A.lat, B.lon, B.lat),
      visible,
      angle: hb > 0 ? Math.atan2(B.e - A.e, hb) / DEG : 0,
    };
    pts.forEach((p, i) => {
      const el = document.createElement('div');
      el.className = 'marker';
      el.innerHTML = `<div class="mpt" title="Punkt ${i + 1}"></div>`;
      this.mMarkers.push(this.overlay.add({ el, x: p.x, z: p.z, e: p.e }));
    });
    if (preview.length >= 2) {
      const el = document.createElement('div');
      el.className = 'marker';
      el.innerHTML = `<div class="mlabel">${fmtDist(surf)} · ${fmtSigned(B.e - A.e)}</div>`;
      this.mMarkers.push(this.overlay.add({ el, x: B.x, z: B.z, e: B.e }));
    }
    if (s.measure.length >= 2 && (!this.cursor || s.tool !== 'measure')) this.updateProfile();
    else if (s.measure.length >= 2) this.updateProfileThrottled();
    this.changed();
  }

  private profTimer = 0;
  private updateProfileThrottled() {
    if (this.profTimer) return;
    this.profTimer = window.setTimeout(() => {
      this.profTimer = 0;
      this.updateProfile();
    }, 120);
  }

  clearMeasure() {
    this.store.set({ measure: [] });
  }

  // ---------------------------------------------------------------- profil
  profileTrack: Route | null = null;
  private lastProfileId = '';
  updateProfile() {
    const s = this.store.state;
    const app = document.getElementById('app')!;
    let data: Parameters<Profile['set']>[0] = null;
    this.profileTrack = null;
    const poisAlong = (r: Route) => {
      const out: { d: number; name: string; ele: number }[] = [];
      const t = r.track;
      for (const p of this.region!.pois) {
        if (p.t !== 'peak' && p.t !== 'saddle' && p.t !== 'hut') continue;
        for (let i = 0; i < t.n; i += 2) {
          if (Math.abs(t.x[i] - p.x) < 90 && Math.abs(t.z[i] - p.z) < 90 && Math.hypot(t.x[i] - p.x, t.z[i] - p.z) < 90) {
            out.push({ d: t.d2[i], name: p.n, ele: t.e[i] });
            break;
          }
        }
      }
      return out;
    };
    if ((s.tool === 'route' || s.tool === 'explore') && s.routes.length) {
      const r = s.routes[s.activeRoute];
      this.profileTrack = r;
      data = {
        track: r.track,
        color: ALT_COLORS[s.activeRoute % ALT_COLORS.length],
        ghosts: s.routes.map((g, i) => ({ track: g.track, color: ALT_COLORS[i % ALT_COLORS.length] })).filter((_, i) => i !== s.activeRoute),
        legIdx: r.legIdx,
        pois: poisAlong(r),
      };
    } else if (s.tool === 'measure' && s.measure.length >= 2) {
      const pts = s.measure;
      const dense: { lon: number; lat: number }[] = [];
      const dem = this.region!.dem;
      for (let i = 0; i < pts.length - 1; i++) {
        const pr = dem.profile(pts[i].x, pts[i].z, pts[i + 1].x, pts[i + 1].z);
        for (let k = i ? 1 : 0; k < pr.d.length; k++) {
          const f = pr.len > 0 ? pr.d[k] / pr.len : 0;
          const [lon, lat] = dem.worldToLonLat(pts[i].x + (pts[i + 1].x - pts[i].x) * f, pts[i].z + (pts[i + 1].z - pts[i].z) * f);
          dense.push({ lon, lat });
        }
      }
      const r = routeFromPolyline(dem, dense, 'terrain', 'Pomiar');
      this.profileTrack = r;
      let blockedAt: number | null = null;
      if (pts.length === 2) {
        const t = r.track;
        const h0 = t.e[0] + 1.7, h1 = t.e[t.n - 1] + 1.7;
        for (let i = 1; i < t.n - 1; i++) {
          const f = t.d2[i] / t.d2[t.n - 1];
          if (t.e[i] + bulge(t.d2[i], t.d2[t.n - 1]) > h0 + (h1 - h0) * f + 0.5) { blockedAt = t.d2[i]; break; }
        }
      }
      data = {
        track: r.track,
        color: '#e9c46a',
        legIdx: [],
        sight: pts.length === 2 ? { eyeA: 1.7, eyeB: 1.7, blockedAt } : undefined,
        pois: poisAlong(r),
        showTime: false,
      };
    } else if (s.gpx && (s.tool === 'explore' || s.tool === 'route')) {
      this.profileTrack = s.gpx;
      data = { track: s.gpx.track, color: '#c98bff', legIdx: [], pois: poisAlong(s.gpx) };
    }
    this.profile.set(data);
    // nowa trasa / pomiar → profil otwiera się ponownie
    const pid = this.profileTrack ? `${s.tool}:${this.profileTrack.kind}:${s.routes.map((r) => r.id).join()}` : '';
    if (pid && pid !== this.lastProfileId && !s.profileOpen) this.store.set({ profileOpen: true });
    this.lastProfileId = pid;
    app.classList.toggle('has-profile', !!data && this.store.state.profileOpen);
    this.changed();
  }

  onProfileHover(i: number | null) {
    const r = this.profileTrack;
    if (i == null || !r) {
      this.overlay.remove(this.hoverMarker);
      this.hoverMarker = null;
      return;
    }
    const t = r.track;
    if (!this.hoverMarker) {
      const el = document.createElement('div');
      el.className = 'marker';
      el.innerHTML = '<div class="hovermark"></div>';
      this.hoverMarker = this.overlay.add({ el, x: t.x[i], z: t.z[i], e: t.e[i] });
    }
    this.hoverMarker.x = t.x[i];
    this.hoverMarker.z = t.z[i];
    this.hoverMarker.e = t.e[i] + 6;
    this.engine.dirty = true;
  }

  // ---------------------------------------------------------------- znaczniki
  refreshMarkers() {
    if (!this.region) return;
    const s = this.store.state;
    const dem = this.region.dem;
    // punkty trasy
    const alive = new Set(s.waypoints.map((w) => w.id));
    for (const [id, m] of this.wpMarkers) if (!alive.has(id)) { this.overlay.remove(m); this.wpMarkers.delete(id); }
    s.waypoints.forEach((w, i) => {
      let m = this.wpMarkers.get(w.id);
      if (!m) {
        const el = document.createElement('div');
        el.className = 'marker';
        el.innerHTML = '<div class="wpm"><span></span></div>';
        m = this.overlay.add({ el, x: w.x, z: w.z, e: w.e, dimOccluded: true });
        this.wpMarkers.set(w.id, m);
        this.bindDrag(el, w.id);
      }
      const last = i === s.waypoints.length - 1 && s.waypoints.length > 1;
      const pin = m.el.querySelector('.wpm')!;
      pin.className = `wpm ${i === 0 ? 'wpm--start' : last ? 'wpm--end' : ''}`;
      pin.querySelector('span')!.textContent = i === 0 ? 'A' : last ? 'B' : String(i);
      m.x = w.x; m.z = w.z; m.e = dem.sampleWorld(w.x, w.z);
      m.hidden = s.tool !== 'route' && s.tool !== 'explore';
    });
    // punkt odniesienia
    this.overlay.remove(this.refMarker);
    this.refMarker = null;
    if (s.ref && s.tool === 'relative') {
      const el = document.createElement('div');
      el.className = 'marker';
      el.innerHTML = `<div class="pin"><div class="pin__label"><b>${fmtEle(s.ref.e)}</b> poziom 0</div><div class="pin__stem"></div><div class="pin__dot"></div></div>`;
      this.refMarker = this.overlay.add({ el, x: s.ref.x, z: s.ref.z, e: s.ref.e });
    }
    this.overlay.remove(this.vsMarker);
    this.vsMarker = null;
    if (s.viewshed && s.tool === 'viewshed') {
      const el = document.createElement('div');
      el.className = 'marker';
      el.innerHTML = `<div class="pin pin--eye"><div class="pin__label"><b>${fmtEle(s.viewshed.point.e)}</b> + ${s.viewshed.eye.toLocaleString('pl-PL')} m</div><div class="pin__stem"></div><div class="pin__dot"></div></div>`;
      this.vsMarker = this.overlay.add({ el, x: s.viewshed.point.x, z: s.viewshed.point.z, e: s.viewshed.point.e });
    }
    this.overlay.update(true);
  }

  private bindDrag(el: HTMLElement, id: string) {
    el.addEventListener('pointerdown', (e) => {
      e.stopPropagation();
      e.preventDefault();
      this.engine.controls.enabled = false;
      el.setPointerCapture(e.pointerId);
      let moved = false;
      const move = (ev: PointerEvent) => {
        const p = this.engine.pick(ev.clientX, ev.clientY);
        if (p) { moved = true; this.moveWaypoint(id, p.x, p.z, false); }
      };
      const up = (ev: PointerEvent) => {
        el.removeEventListener('pointermove', move);
        el.removeEventListener('pointerup', up);
        this.engine.controls.enabled = true;
        if (moved) {
          const p = this.engine.pick(ev.clientX, ev.clientY);
          if (p) this.moveWaypoint(id, p.x, p.z, true);
        }
      };
      el.addEventListener('pointermove', move);
      el.addEventListener('pointerup', up);
    });
    el.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      this.removeWaypoint(id);
    });
  }

  // ---------------------------------------------------------------- panorama
  private exagBeforePano: number | null = null;
  enterPanorama(x: number, z: number) {
    this.stopFly();
    // panorama zawsze w skali 1:1 – bez przewyższenia
    if (this.exagBeforePano == null) this.exagBeforePano = this.store.state.exag;
    this.engine.setExag(1);
    this.terrain!.material.uniforms.uExag.value = 1;
    this.store.state.exag = 1;
    this.engine.enterPanorama(x, z);
    this.store.set({ panorama: true });
    this.terrain!.material.uniforms.uContourOn.value = 0;
    document.getElementById('app')!.classList.add('is-panorama');
    this.terrain!.material.uniforms.uCursor.value.w = 0;
    this.changed();
  }

  exitPanorama() {
    this.engine.exitPanorama();
    if (this.exagBeforePano != null) {
      const e = this.exagBeforePano;
      this.exagBeforePano = null;
      this.store.set({ exag: e });
    }
    this.store.set({ panorama: false });
    this.terrain!.material.uniforms.uContourOn.value = this.store.state.contours ? 1 : 0;
    document.getElementById('app')!.classList.remove('is-panorama');
    this.changed();
  }

  // ---------------------------------------------------------------- przelot
  toggleFly() {
    const r = this.activeRoute ?? this.store.state.gpx;
    if (!r) return;
    if (this.fly) {
      this.fly.playing = !this.fly.playing;
      if (this.fly.playing && this.fly.i >= r.track.n - 1) this.fly.i = 0;
    } else {
      this.fly = { i: 0, playing: true, speed: Math.max(160, r.stats.len2 / 40) };
    }
    this.store.set({ flying: this.fly.playing });
    this.changed();
  }

  stopFly() {
    if (!this.fly) return;
    this.fly = null;
    this.engine.stopAnimation();
    this.profile.setCursor(null);
    this.onProfileHover(null);
    this.store.set({ flying: false });
  }

  private flyDist = 0;
  private stepFly(dt: number) {
    const r = this.activeRoute ?? this.store.state.gpx;
    if (!r || !this.fly) return;
    const t = r.track;
    if (this.fly.i === 0) this.flyDist = 0;
    this.flyDist += dt * this.fly.speed;
    while (this.fly.i < t.n - 1 && t.d2[this.fly.i + 1] < this.flyDist) this.fly.i++;
    const i = this.fly.i;
    if (i >= t.n - 1) {
      this.fly.playing = false;
      this.store.set({ flying: false });
      return;
    }
    const f = (this.flyDist - t.d2[i]) / Math.max(1e-3, t.d2[i + 1] - t.d2[i]);
    const x = t.x[i] + (t.x[i + 1] - t.x[i]) * f;
    const z = t.z[i] + (t.z[i + 1] - t.z[i]) * f;
    // kierunek: uśredniony na 400 m do przodu
    let j = i;
    while (j < t.n - 1 && t.d2[j] - t.d2[i] < 400) j++;
    const hx = t.x[j] - x, hz = t.z[j] - z;
    const heading = Math.atan2(hx, -hz) / DEG;
    const v = this.engine.view;
    const dh = ((heading - v.heading + 540) % 360) - 180;
    const ex = this.engine.exag;
    const target = new THREE.Vector3(x, this.engine.groundY(x, z) + 5 * ex, z);
    const dist = 1400;
    const pitch = 26 * DEG;
    const hh = (v.heading + dh * Math.min(1, dt * 1.6)) * DEG;
    const pos = new THREE.Vector3(
      x - Math.sin(hh) * Math.cos(pitch) * dist,
      target.y + Math.sin(pitch) * dist,
      z + Math.cos(hh) * Math.cos(pitch) * dist
    );
    const g = this.engine.groundY(pos.x, pos.z) + 120 * ex;
    if (pos.y < g) pos.y = g;
    this.engine.lookFrom(pos, target);
    this.profile.setCursor(i);
    this.onProfileHover(i);
  }

  // ---------------------------------------------------------------- GPX / link / zrzut
  exportGpx() {
    const r = this.activeRoute;
    if (!r) return;
    const s = this.store.state;
    const name = `${this.region!.def.name}: ${s.waypoints[0]?.label ?? 'start'} → ${s.waypoints[s.waypoints.length - 1]?.label ?? 'cel'}`;
    const gpx = routeToGpx(
      r,
      name,
      s.waypoints.map((w, i) => ({ name: w.label ?? `Punkt ${i + 1}`, lon: w.lon, lat: w.lat, e: w.e }))
    );
    download(`gran-${this.region!.def.id}-${new Date().toISOString().slice(0, 10)}.gpx`, gpx);
    this.toast('Zapisano ślad GPX.');
  }

  async importGpx(file: File) {
    try {
      const gpx = parseGpx(await file.text());
      const mid = gpx.points[Math.floor(gpx.points.length / 2)];
      const reg = regionForPoint(mid.lon, mid.lat);
      if (!reg) {
        this.toast('Ślad leży poza dostępnymi regionami.', true);
        return;
      }
      if (reg.id !== this.region?.def.id) {
        this.toast(`Przełączam na region: ${reg.name}`);
        await this.switchRegion?.(reg.id);
      }
      const dem = this.region!.dem;
      const pts = gpx.points.filter((p) => dem.inside(...dem.lonLatToWorld(p.lon, p.lat), 1));
      if (pts.length < 2) throw new Error('Ślad nie mieści się w obszarze regionu.');
      // wysokości z modelu terenu – spójne z profilem; oryginalne GPS bywają zaszumione
      const route = routeFromPolyline(dem, pts.map((p) => ({ lon: p.lon, lat: p.lat })), 'gpx', gpx.name);
      this.store.set({ gpx: route, tool: 'explore' });
      this.fitRoute(route);
      this.toast(`Wczytano „${gpx.name}” — ${fmtDist(route.stats.len2)}.`);
    } catch (e) {
      this.toast((e as Error).message, true);
    }
  }

  switchRegion: ((id: string) => Promise<void>) | null = null;

  shareLink() {
    this.writeHash(true);
    navigator.clipboard?.writeText(location.href).then(
      () => this.toast('Skopiowano link do widoku i trasy.'),
      () => this.toast('Nie udało się skopiować linku.', true)
    );
  }

  screenshot() {
    const url = this.engine.screenshot();
    const a = document.createElement('a');
    a.href = url;
    a.download = `gran-${this.region?.def.id}-${Date.now()}.png`;
    a.click();
    this.toast('Zapisano zrzut widoku 3D.');
  }

  // ---------------------------------------------------------------- adres URL
  scheduleHash() {
    clearTimeout(this.hashTimer);
    this.hashTimer = window.setTimeout(() => this.writeHash(false), 700);
  }

  writeHash(_force: boolean) {
    if (!this.region) return;
    const v = this.engine.view;
    const [lon, lat] = this.region.dem.worldToLonLat(v.x, v.z);
    const s = this.store.state;
    let h = `#${this.region.def.id}/${lat.toFixed(5)},${lon.toFixed(5)},${Math.round(v.distance)},${Math.round(v.heading)},${Math.round(v.pitch)}`;
    if (s.waypoints.length) h += `&w=${s.waypoints.map((w) => `${w.lon.toFixed(5)},${w.lat.toFixed(5)}`).join(';')}&m=${s.routeMode === 'trails' ? 't' : 'x'}`;
    history.replaceState(null, '', h);
  }

  readHash() {
    const h = location.hash.slice(1);
    const out: { region?: string; view?: { lat: number; lon: number; distance: number; heading: number; pitch: number }; wps?: [number, number][]; mode?: 'trails' | 'terrain' } = {};
    if (!h) return out;
    const [main, ...rest] = h.split('&');
    const [region, cam] = main.split('/');
    if (REGIONS.some((r) => r.id === region)) out.region = region;
    if (cam) {
      const [lat, lon, distance, heading, pitch] = cam.split(',').map(Number);
      if ([lat, lon, distance, heading, pitch].every(Number.isFinite)) out.view = { lat, lon, distance, heading, pitch };
    }
    for (const r of rest) {
      const [k, v] = r.split('=');
      if (k === 'w' && v) out.wps = v.split(';').map((p) => p.split(',').map(Number) as [number, number]).filter((p) => p.every(Number.isFinite));
      if (k === 'm') out.mode = v === 'x' ? 'terrain' : 'trails';
    }
    return out;
  }

  // ---------------------------------------------------------------- komunikaty
  private toastTimer = 0;
  toast(msg: string, err = false) {
    const el = $('toast');
    el.innerHTML = `<i></i>${escapeHtml(msg)}`;
    el.classList.toggle('is-err', err);
    el.classList.add('is-on');
    clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => el.classList.remove('is-on'), 3600);
  }
}

export function escapeHtml(s: string) {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

export function trailHex(c: string) {
  return (
    { red: '#e2382d', blue: '#2c6fe4', green: '#1ea25a', yellow: '#f6c515', black: '#1b1b1b', other: '#cdbfe6', orange: '#ff8a1c', purple: '#bb74ff', brown: '#8b5a2b' } as Record<string, string>
  )[c] ?? '#cdbfe6';
}

/** Wybrzuszenie krzywizny Ziemi (z refrakcją) w punkcie d odcinka o długości L. */
function bulge(d: number, L: number) {
  return curvatureDrop(Math.sqrt(Math.max(0, d * (L - d))));
}

/** Zagęszczenie śladu do ~rozdzielczości DEM (dla profilu i metryk). */
function densify(pts: { lon: number; lat: number }[], dem: { mpp: number }) {
  const out: { lon: number; lat: number }[] = [];
  for (let i = 0; i < pts.length; i++) {
    if (i) {
      const a = pts[i - 1], b = pts[i];
      const d = haversine(a.lon, a.lat, b.lon, b.lat);
      const n = Math.floor(d / (dem.mpp * 0.6));
      for (let k = 1; k <= n; k++) {
        const f = k / (n + 1);
        out.push({ lon: a.lon + (b.lon - a.lon) * f, lat: a.lat + (b.lat - a.lat) * f });
      }
    }
    out.push(pts[i]);
  }
  return out;
}
