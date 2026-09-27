import type { Poi } from '../core/region';
import { fmtDist, fmtInt } from '../core/geo';
import type { Engine } from './engine';

export interface Marker {
  el: HTMLElement;
  x: number;
  z: number;
  e: number;
  /** czy przygaszać, gdy zasłonięty przez teren */
  dimOccluded?: boolean;
  hidden?: boolean;
}

interface Label {
  poi: Poi;
  el: HTMLElement | null;
  shown: boolean;
}

const GLYPH: Record<string, string> = {
  peak: '<svg class="poi__glyph" viewBox="0 0 10 9"><path d="M5 0.5 L9.5 8.5 H0.5 Z" fill="#fffaf0" stroke="#111" stroke-width="0.8"/></svg>',
  saddle: '<svg class="poi__glyph" viewBox="0 0 10 9"><path d="M0.5 3 C3 8, 7 8, 9.5 3" fill="none" stroke="#fffaf0" stroke-width="1.6"/></svg>',
  hut: '<svg class="poi__glyph" viewBox="0 0 10 9" style="width:12px;height:11px"><path d="M1 8.5 V4 L5 0.8 L9 4 V8.5 Z" fill="#ff5a36" stroke="#fff" stroke-width="0.9"/></svg>',
  shelter: '<svg class="poi__glyph" viewBox="0 0 10 9"><path d="M1 8.5 V4 L5 0.8 L9 4 V8.5 Z" fill="#d9b56f" stroke="#111" stroke-width="0.8"/></svg>',
  lake: '<svg class="poi__glyph" viewBox="0 0 10 9"><ellipse cx="5" cy="5" rx="4.2" ry="2.6" fill="#7cc8ff" stroke="#fff" stroke-width="0.8"/></svg>',
  waterfall: '<svg class="poi__glyph" viewBox="0 0 10 9"><path d="M3 1 V8 M5 1 V8 M7 1 V8" stroke="#7cc8ff" stroke-width="1.2"/></svg>',
  cave: '<svg class="poi__glyph" viewBox="0 0 10 9"><path d="M1 8.5 C1 2, 9 2, 9 8.5 Z" fill="#111" stroke="#fffaf0" stroke-width="0.9"/></svg>',
  viewpoint: '<svg class="poi__glyph" viewBox="0 0 10 9"><circle cx="5" cy="5" r="3" fill="none" stroke="#fffaf0" stroke-width="1.2"/><circle cx="5" cy="5" r="1" fill="#fffaf0"/></svg>',
};

export class Overlay {
  private labels: Label[] = [];
  private markers = new Set<Marker>();
  enabled = true;
  onPoiClick: (p: Poi) => void = () => {};
  onPoiHover: (p: Poi | null) => void = () => {};
  private frame = 0;

  constructor(readonly root: HTMLElement, private engine: Engine) {}

  setPois(pois: Poi[]) {
    for (const l of this.labels) l.el?.remove();
    // kandydaci na etykiety: wszystkie nazwane punkty, posortowane wg ważności
    this.labels = pois
      .filter((p) => p.n && p.t !== 'viewpoint')
      .sort((a, b) => b.prom - a.prom)
      .slice(0, 900)
      .map((poi) => ({ poi, el: null, shown: false }));
  }

  private makeEl(p: Poi) {
    const el = document.createElement('div');
    el.className = `poi poi--${p.t}`;
    const ele = p.t === 'lake' || p.t === 'cave' || p.t === 'waterfall' ? '' : `<span class="poi__ele">${fmtInt(p.ele)}</span>`;
    el.innerHTML = `<span class="poi__name">${p.n}</span>${ele}<span class="poi__stem"></span>${GLYPH[p.t] ?? ''}`;
    el.addEventListener('click', (e) => {
      e.stopPropagation();
      this.onPoiClick(p);
    });
    el.addEventListener('pointerenter', () => this.onPoiHover(p));
    el.addEventListener('pointerleave', () => this.onPoiHover(null));
    this.root.appendChild(el);
    return el;
  }

  add(m: Marker) {
    this.markers.add(m);
    if (!m.el.parentElement) this.root.appendChild(m.el);
    this.place(m);
    return m;
  }

  remove(m: Marker | null | undefined) {
    if (!m) return;
    this.markers.delete(m);
    m.el.remove();
  }

  private place(m: Marker) {
    const p = m.hidden ? null : this.engine.project(m.x, m.e, m.z);
    if (!p) {
      m.el.style.display = 'none';
      return;
    }
    m.el.style.display = '';
    m.el.style.transform = `translate(${p.x.toFixed(1)}px, ${p.y.toFixed(1)}px)`;
    if (m.dimOccluded) m.el.classList.toggle('is-behind', this.engine.occluded(m.x, m.e, m.z, 20));
  }

  update(force = false) {
    for (const m of this.markers) this.place(m);
    this.frame++;
    if (!force && !this.engine.isMoving && this.frame % 20 !== 0) return;
    this.layoutLabels();
  }

  private layoutLabels() {
    const W = this.root.clientWidth, H = this.root.clientHeight;
    const cam = this.engine.camera.position;
    const pano = this.engine.panorama;
    const maxLabels = Math.round(Math.min(70, (W * H) / (pano ? 16000 : 26000)));
    const placed: [number, number, number, number][] = [];
    const exag = this.engine.exag;
    // punktacja: ważność ↓ z odległością
    const scored = this.labels.map((l) => {
      const d = Math.hypot(l.poi.x - cam.x, l.poi.z - cam.z, l.poi.ele * exag - cam.y);
      return { l, d, s: l.poi.prom / (1 + d / (pano ? 9000 : 22000)) };
    });
    scored.sort((a, b) => b.s - a.s);
    let count = 0;
    for (const { l, d } of scored) {
      let show = false;
      let px = 0, py = 0;
      if (this.enabled && count < maxLabels) {
        const p = this.engine.project(l.poi.x, l.poi.ele, l.poi.z);
        if (p && p.x > -40 && p.x < W + 40 && p.y > 50 && p.y < H - 10) {
          const w = l.poi.n.length * (l.poi.t === 'hut' ? 6.6 : 8.2) + 12;
          const h = 38;
          const r: [number, number, number, number] = [p.x - w / 2, p.y - h - 6, p.x + w / 2, p.y];
          const hit = placed.some((q) => r[0] < q[2] && r[2] > q[0] && r[1] < q[3] && r[3] > q[1]);
          if (!hit && !this.engine.occluded(l.poi.x, l.poi.ele, l.poi.z, 28)) {
            show = true;
            placed.push([r[0] - 6, r[1] - 3, r[2] + 6, r[3] + 3]);
            px = p.x;
            py = p.y;
            count++;
          }
        }
      }
      if (show) {
        if (!l.el) l.el = this.makeEl(l.poi);
        l.el.classList.remove('is-hidden');
        l.el.classList.toggle('poi--major', l.poi.t === 'peak' && l.poi.prom > 5 && d < 30000);
        l.el.classList.toggle('is-far', d > 26000);
        l.el.style.transform = `translate(${px.toFixed(1)}px, ${py.toFixed(1)}px) translate(-50%, -100%)`;
        if (pano) {
          const ele = l.el.querySelector('.poi__ele');
          if (ele) ele.textContent = `${fmtInt(l.poi.ele)} · ${fmtDist(d)}`;
        }
        l.shown = true;
      } else if (l.el && l.shown) {
        l.el.classList.add('is-hidden');
        l.shown = false;
      }
    }
  }

  clear() {
    for (const l of this.labels) l.el?.remove();
    this.labels = [];
    for (const m of this.markers) m.el.remove();
    this.markers.clear();
  }
}
