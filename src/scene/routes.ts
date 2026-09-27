import * as THREE from 'three';
import { Line2 } from 'three/examples/jsm/lines/Line2.js';
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import type { Route } from '../core/graph';

export const ALT_COLORS = ['#ff5a36', '#3fd6c6', '#f7c948', '#b98cff', '#86e07f', '#ff8fc7'];
const LIFT = 7;

interface Lines {
  objs: Line2[];
  mats: LineMaterial[];
}

function positions(r: Route, lift = LIFT) {
  const t = r.track;
  const p: number[] = [];
  for (let i = 0; i < t.n; i++) p.push(t.x[i], t.e[i] + lift, t.z[i]);
  return p;
}

export class RouteLayer {
  readonly group = new THREE.Group();
  private routeLines: Lines = { objs: [], mats: [] };
  private gpxLines: Lines = { objs: [], mats: [] };
  private measureLines: Lines = { objs: [], mats: [] };
  private flowMat: LineMaterial | null = null;

  private add(set: Lines, pos: number[], params: ConstructorParameters<typeof LineMaterial>[0], order: number, dashed = false) {
    const g = new LineGeometry();
    g.setPositions(pos);
    const m = new LineMaterial({ fog: true, worldUnits: false, transparent: true, ...params });
    const l = new Line2(g, m);
    if (dashed) l.computeLineDistances();
    l.renderOrder = order;
    l.frustumCulled = false;
    set.objs.push(l);
    set.mats.push(m);
    this.group.add(l);
    return m;
  }

  private clear(set: Lines) {
    for (const o of set.objs) {
      this.group.remove(o);
      o.geometry.dispose();
    }
    for (const m of set.mats) m.dispose();
    set.objs = [];
    set.mats = [];
  }

  setRoutes(routes: Route[], active: number) {
    this.clear(this.routeLines);
    this.flowMat = null;
    routes.forEach((r, i) => {
      if (i === active) return;
      const p = positions(r);
      const c = ALT_COLORS[i % ALT_COLORS.length];
      this.add(this.routeLines, p, { color: c, linewidth: 3, opacity: 0.2, depthTest: false }, 6);
      this.add(this.routeLines, p, { color: 0x0b0f12, linewidth: 5.5, opacity: 0.55, depthWrite: false }, 7);
      this.add(this.routeLines, p, { color: c, linewidth: 3, opacity: 0.85 }, 8);
    });
    const r = routes[active];
    if (!r) return;
    const p = positions(r);
    const c = ALT_COLORS[active % ALT_COLORS.length];
    this.add(this.routeLines, p, { color: c, linewidth: 4, opacity: 0.32, depthTest: false }, 9);
    this.add(this.routeLines, p, { color: 0x0b0f12, linewidth: 9, opacity: 0.8, depthWrite: false }, 10);
    this.add(this.routeLines, p, { color: c, linewidth: 5 }, 11);
    this.flowMat = this.add(
      this.routeLines,
      positions(r, LIFT + 0.5),
      { color: 0xfff8ec, linewidth: 1.8, dashed: true, dashSize: 40, gapSize: 110, opacity: 0.95 },
      12,
      true
    );
  }

  setGpx(r: Route | null) {
    this.clear(this.gpxLines);
    if (!r) return;
    const p = positions(r, LIFT - 1);
    this.add(this.gpxLines, p, { color: 0xbb74ff, linewidth: 3, opacity: 0.3, depthTest: false }, 6);
    this.add(this.gpxLines, p, { color: 0x1a0c29, linewidth: 7, opacity: 0.7, depthWrite: false }, 7);
    this.add(this.gpxLines, p, { color: 0xc98bff, linewidth: 3.5 }, 8);
  }

  /** Linia pomiaru: odcinki po powierzchni terenu + linia wzroku (prosta) między końcami. */
  setMeasure(surface: number[] | null, sight: number[] | null, sightBlocked: boolean) {
    this.clear(this.measureLines);
    if (surface && surface.length >= 6) {
      this.add(this.measureLines, surface, { color: 0x0b0f12, linewidth: 6, opacity: 0.7, depthWrite: false }, 13);
      this.add(this.measureLines, surface, { color: 0xe9c46a, linewidth: 3 }, 14);
    }
    if (sight) {
      this.add(
        this.measureLines,
        sight,
        { color: sightBlocked ? 0xff5a4a : 0x9cf0c8, linewidth: 2, dashed: true, dashSize: 30, gapSize: 25, opacity: 0.9, depthTest: false },
        15,
        true
      );
    }
  }

  update(dt: number, camDist: number) {
    if (this.flowMat) {
      const k = Math.max(0.4, camDist / 6000);
      this.flowMat.dashSize = 40 * k;
      this.flowMat.gapSize = 110 * k;
      this.flowMat.dashOffset -= dt * 90 * k;
      return true;
    }
    return false;
  }
}
