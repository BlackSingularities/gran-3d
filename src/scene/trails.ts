import * as THREE from 'three';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';
import type { TrailGraph } from '../core/graph';

export const TRAIL_HEX: Record<string, string> = {
  red: '#e2382d',
  blue: '#2c6fe4',
  green: '#1ea25a',
  yellow: '#f6c515',
  black: '#1b1b1b',
  other: '#cdbfe6',
  offtrail: '#ff9d2e',
  purple: '#bb74ff',
  orange: '#ff8a1c',
  brown: '#8b5a2b',
};

export const TRAIL_NAME_PL: Record<string, string> = {
  red: 'czerwony',
  blue: 'niebieski',
  green: 'zielony',
  yellow: 'żółty',
  black: 'czarny',
  other: 'inny',
  orange: 'pomarańczowy',
  purple: 'fioletowy',
  brown: 'brązowy',
  offtrail: 'poza szlakiem',
};

const LIFT = 3;

export class TrailsLayer {
  readonly group = new THREE.Group();
  private core: LineSegments2;
  private casing: LineSegments2;
  private coreMat: LineMaterial;
  private casingMat: LineMaterial;
  private hl: LineSegments2 | null = null;
  private hlMat: LineMaterial;

  constructor(private graph: TrailGraph) {
    const pos: number[] = [];
    const col: number[] = [];
    const cpos: number[] = [];
    const c = new THREE.Color();
    for (const ed of graph.edges) {
      const k = ed.colors.length;
      for (let ci = 0; ci < k; ci++) {
        c.set(TRAIL_HEX[ed.colors[ci]] ?? TRAIL_HEX.other).convertSRGBToLinear();
        const off = (ci - (k - 1) / 2) * 7;
        let px = 0, pz = 0;
        for (let i = 0; i < ed.n; i++) {
          const i0 = Math.max(0, i - 1), i1 = Math.min(ed.n - 1, i + 1);
          let dx = ed.x[i1] - ed.x[i0], dz = ed.z[i1] - ed.z[i0];
          const l = Math.hypot(dx, dz) || 1;
          dx /= l; dz /= l;
          const x = ed.x[i] - dz * off, z = ed.z[i] + dx * off;
          if (i > 0) {
            pos.push(px, ed.e[i - 1] + LIFT, pz, x, ed.e[i] + LIFT, z);
            col.push(c.r, c.g, c.b, c.r, c.g, c.b);
            if (ci === 0) cpos.push(ed.x[i - 1], ed.e[i - 1] + LIFT - 0.5, ed.z[i - 1], ed.x[i], ed.e[i] + LIFT - 0.5, ed.z[i]);
          }
          px = x; pz = z;
        }
      }
    }
    const g = new LineSegmentsGeometry();
    g.setPositions(pos);
    g.setColors(col);
    this.coreMat = new LineMaterial({ fog: true, linewidth: 2.4, vertexColors: true, worldUnits: false, transparent: true, opacity: 1 });
    this.core = new LineSegments2(g, this.coreMat);
    this.core.renderOrder = 2;

    const cg = new LineSegmentsGeometry();
    cg.setPositions(cpos);
    this.casingMat = new LineMaterial({ fog: true, linewidth: 5, color: 0xfff6e2, worldUnits: false, transparent: true, opacity: 0.75, depthWrite: false });
    this.casing = new LineSegments2(cg, this.casingMat);
    this.casing.renderOrder = 1;

    this.hlMat = new LineMaterial({ fog: true, linewidth: 9, color: 0xffe9a8, worldUnits: false, transparent: true, opacity: 0.55, depthTest: false });

    this.group.add(this.casing, this.core);
  }

  setVisible(v: boolean) {
    this.group.visible = v;
  }

  /** Grubość linii zależna od odległości kamery; `dim` przygasza sieć, gdy wyświetlana jest trasa. */
  update(distance: number, dim: boolean) {
    const w = THREE.MathUtils.clamp(3.1 - distance / 16000, 1.1, 3.1);
    this.coreMat.linewidth = w;
    this.casingMat.linewidth = w + 2.2;
    const op = dim ? 0.45 : 1;
    this.coreMat.opacity = op;
    this.casingMat.opacity = (dim ? 0.3 : 0.75) * THREE.MathUtils.clamp(1.4 - distance / 40000, 0.35, 1);
  }

  highlight(edgeIds: number[] | null) {
    if (this.hl) {
      this.group.remove(this.hl);
      this.hl.geometry.dispose();
      this.hl = null;
    }
    if (!edgeIds || !edgeIds.length) return;
    const pos: number[] = [];
    for (const id of edgeIds) {
      const ed = this.graph.edges[id];
      for (let i = 1; i < ed.n; i++) {
        pos.push(ed.x[i - 1], ed.e[i - 1] + LIFT + 1, ed.z[i - 1], ed.x[i], ed.e[i] + LIFT + 1, ed.z[i]);
      }
    }
    const g = new LineSegmentsGeometry();
    g.setPositions(pos);
    this.hl = new LineSegments2(g, this.hlMat);
    this.hl.renderOrder = 5;
    this.group.add(this.hl);
  }

  dispose() {
    this.core.geometry.dispose();
    this.casing.geometry.dispose();
    this.coreMat.dispose();
    this.casingMat.dispose();
    this.hlMat.dispose();
    this.hl?.geometry.dispose();
  }
}
