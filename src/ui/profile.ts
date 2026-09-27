import { fmtDist, fmtEle, fmtInt, fmtTime } from '../core/geo';
import { TRAIL_COLORS, gradeClass, type Track } from '../core/metrics';
import { TRAIL_HEX } from '../scene/trails';

export const GRADE_COLORS = ['#5d8a67', '#a7bf6c', '#ecc95c', '#f28b3c', '#e0452f', '#8f2a61'];
export const GRADE_LABELS = ['< 5%', '5–12%', '12–20%', '20–30%', '30–45%', '> 45%'];

export interface ProfileData {
  track: Track;
  color: string;
  ghosts?: { track: Track; color: string }[];
  legIdx?: number[];
  pois?: { d: number; name: string; ele: number }[];
  sight?: { eyeA: number; eyeB: number; blockedAt: number | null };
  showTime?: boolean;
}

const PAD = { l: 46, r: 14, t: 16, b: 24 };

export class Profile {
  private ctx: CanvasRenderingContext2D;
  private data: ProfileData | null = null;
  private hoverD: number | null = null;
  private cursorD: number | null = null;
  onHover: (i: number | null) => void = () => {};
  onClick: (i: number) => void = () => {};
  private w = 0;
  private h = 0;
  private yMin = 0;
  private yMax = 1;
  private xMax = 1;

  constructor(private canvas: HTMLCanvasElement, private tip: HTMLElement) {
    this.ctx = canvas.getContext('2d')!;
    new ResizeObserver(() => this.resize()).observe(canvas.parentElement!);
    canvas.addEventListener('pointermove', (e) => this.move(e));
    canvas.addEventListener('pointerleave', () => {
      this.hoverD = null;
      this.tip.hidden = true;
      this.onHover(null);
      this.draw();
    });
    canvas.addEventListener('click', (e) => {
      const i = this.indexAtX(e.offsetX);
      if (i != null) this.onClick(i);
    });
  }

  private resize() {
    const r = this.canvas.parentElement!.getBoundingClientRect();
    const dpr = Math.min(2, window.devicePixelRatio);
    this.w = r.width;
    this.h = r.height;
    this.canvas.width = Math.max(1, Math.round(r.width * dpr));
    this.canvas.height = Math.max(1, Math.round(r.height * dpr));
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.draw();
  }

  set(data: ProfileData | null) {
    this.data = data;
    this.hoverD = null;
    this.tip.hidden = true;
    if (data) {
      const t = data.track;
      let lo = Infinity, hi = -Infinity;
      const scan = (tr: Track) => {
        for (let i = 0; i < tr.n; i++) {
          if (tr.e[i] < lo) lo = tr.e[i];
          if (tr.e[i] > hi) hi = tr.e[i];
        }
      };
      scan(t);
      data.ghosts?.forEach((g) => scan(g.track));
      if (data.sight) hi = Math.max(hi, t.e[0] + data.sight.eyeA, t.e[t.n - 1] + data.sight.eyeB);
      const span = Math.max(60, hi - lo);
      this.yMin = lo - span * 0.12;
      this.yMax = hi + span * 0.22;
      this.xMax = Math.max(t.d2[t.n - 1], ...(data.ghosts ?? []).map((g) => g.track.d2[g.track.n - 1]));
    }
    this.draw();
  }

  /** Zewnętrzny kursor (np. podczas przelotu kamery). */
  setCursor(i: number | null) {
    this.cursorD = i == null || !this.data ? null : this.data.track.d2[i];
    this.draw();
  }

  private X = (d: number) => PAD.l + (d / this.xMax) * (this.w - PAD.l - PAD.r);
  private Y = (e: number) => PAD.t + (1 - (e - this.yMin) / (this.yMax - this.yMin)) * (this.h - PAD.t - PAD.b);

  private indexAtX(px: number) {
    if (!this.data) return null;
    const t = this.data.track;
    const d = ((px - PAD.l) / (this.w - PAD.l - PAD.r)) * this.xMax;
    if (d < 0 || d > t.d2[t.n - 1]) return null;
    let lo = 0, hi = t.n - 1;
    while (hi - lo > 1) {
      const m = (lo + hi) >> 1;
      if (t.d2[m] < d) lo = m; else hi = m;
    }
    return d - t.d2[lo] < t.d2[hi] - d ? lo : hi;
  }

  private move(e: PointerEvent) {
    const i = this.indexAtX(e.offsetX);
    if (i == null || !this.data) {
      this.hoverD = null;
      this.tip.hidden = true;
      this.onHover(null);
      this.draw();
      return;
    }
    const t = this.data.track;
    this.hoverD = t.d2[i];
    // nachylenie lokalne w oknie ±30 m
    let a = i, b = i;
    while (a > 0 && t.d2[i] - t.d2[a] < 30) a--;
    while (b < t.n - 1 && t.d2[b] - t.d2[i] < 30) b++;
    const g = t.d2[b] - t.d2[a] > 5 ? (t.e[b] - t.e[a]) / (t.d2[b] - t.d2[a]) : 0;
    this.tip.hidden = false;
    this.tip.innerHTML = `<b>${fmtEle(t.e[i])}</b>${fmtDist(t.d2[i])} <span>·</span> ${g >= 0 ? '↗' : '↘'} ${fmtInt(Math.abs(g * 100))}%${
      this.data.showTime !== false ? ` <span>·</span> ${fmtTime(t.t[i])}` : ''
    }`;
    const x = this.X(t.d2[i]);
    this.tip.style.left = `${Math.min(this.w - 90, Math.max(90, x))}px`;
    this.onHover(i);
    this.draw();
  }

  draw() {
    const c = this.ctx, W = this.w, H = this.h;
    c.clearRect(0, 0, W, H);
    if (!this.data || W < 50) return;
    const { track: t } = this.data;
    const X = this.X, Y = this.Y;
    const base = H - PAD.b;

    // siatka wysokości
    const span = this.yMax - this.yMin;
    const step = [10, 20, 50, 100, 200, 250, 500, 1000].find((s) => span / s <= 5) ?? 1000;
    c.font = '10px "Geist Mono", monospace';
    c.textAlign = 'right';
    c.textBaseline = 'middle';
    for (let e = Math.ceil(this.yMin / step) * step; e < this.yMax; e += step) {
      const y = Y(e);
      c.strokeStyle = 'rgba(239,231,214,0.07)';
      c.beginPath();
      c.moveTo(PAD.l, y);
      c.lineTo(W - PAD.r, y);
      c.stroke();
      c.fillStyle = 'rgba(239,231,214,0.42)';
      c.fillText(fmtInt(e), PAD.l - 8, y);
    }
    // oś odległości
    const kmStep = [0.1, 0.2, 0.5, 1, 2, 5, 10].find((s) => this.xMax / 1000 / s <= 12) ?? 10;
    c.textAlign = 'center';
    c.textBaseline = 'top';
    for (let k = 0; k * kmStep * 1000 <= this.xMax; k++) {
      const x = X(k * kmStep * 1000);
      c.fillStyle = 'rgba(239,231,214,0.42)';
      c.fillText(k === 0 ? '0' : `${(k * kmStep).toLocaleString('pl-PL')} km`, x, base + 8);
      c.fillStyle = 'rgba(239,231,214,0.25)';
      c.fillRect(x, base, 1, 4);
    }

    // warianty (duchy)
    for (const g of this.data.ghosts ?? []) {
      c.strokeStyle = g.color;
      c.globalAlpha = 0.55;
      c.setLineDash([3, 3]);
      c.lineWidth = 1.2;
      c.beginPath();
      for (let i = 0; i < g.track.n; i++) {
        const x = X(g.track.d2[i]), y = Y(g.track.e[i]);
        if (i) c.lineTo(x, y); else c.moveTo(x, y);
      }
      c.stroke();
      c.setLineDash([]);
      c.globalAlpha = 1;
    }

    // wypełnienie wg nachylenia
    const grad = c.createLinearGradient(0, PAD.t, 0, base);
    grad.addColorStop(0, 'rgba(255,255,255,0.0)');
    grad.addColorStop(1, 'rgba(0,0,0,0.55)');
    let i0 = 0;
    const cls = (a: number, b: number) => {
      const dd = t.d2[b] - t.d2[a];
      return dd > 0 ? gradeClass((t.e[b] - t.e[a]) / dd) : 0;
    };
    // uśrednianie klas w oknach ~40 m
    const win: number[] = [];
    for (let i = 0; i < t.n - 1; i++) {
      let a = i, b = i + 1;
      while (a > 0 && t.d2[i] - t.d2[a] < 20) a--;
      while (b < t.n - 1 && t.d2[b] - t.d2[i + 1] < 20) b++;
      win.push(cls(a, b));
    }
    while (i0 < t.n - 1) {
      const k = win[i0];
      let i1 = i0 + 1;
      while (i1 < t.n - 1 && win[i1] === k) i1++;
      c.beginPath();
      c.moveTo(X(t.d2[i0]), base);
      for (let i = i0; i <= i1; i++) c.lineTo(X(t.d2[i]), Y(t.e[i]));
      c.lineTo(X(t.d2[i1]), base);
      c.closePath();
      c.fillStyle = GRADE_COLORS[k];
      c.globalAlpha = 0.78;
      c.fill();
      c.globalAlpha = 1;
      i0 = i1;
    }
    c.fillStyle = grad;
    c.beginPath();
    c.moveTo(X(0), base);
    for (let i = 0; i < t.n; i++) c.lineTo(X(t.d2[i]), Y(t.e[i]));
    c.lineTo(X(t.d2[t.n - 1]), base);
    c.fill();

    // linia profilu
    c.strokeStyle = '#fff6e6';
    c.lineWidth = 1.6;
    c.lineJoin = 'round';
    c.beginPath();
    for (let i = 0; i < t.n; i++) {
      const x = X(t.d2[i]), y = Y(t.e[i]);
      if (i) c.lineTo(x, y); else c.moveTo(x, y);
    }
    c.stroke();

    // pasek kolorów szlaków
    for (let i = 0; i < t.n - 1; i++) {
      const col = TRAIL_HEX[TRAIL_COLORS[t.color[i + 1]]] ?? '#888';
      c.fillStyle = col;
      c.fillRect(X(t.d2[i]), base - 4, Math.max(1, X(t.d2[i + 1]) - X(t.d2[i]) + 0.5), 4);
    }

    // linia wzroku (pomiar)
    if (this.data.sight) {
      const s = this.data.sight;
      const blocked = s.blockedAt != null;
      c.strokeStyle = blocked ? '#ff5a4a' : '#9cf0c8';
      c.setLineDash([6, 4]);
      c.lineWidth = 1.6;
      c.beginPath();
      c.moveTo(X(0), Y(t.e[0] + s.eyeA));
      c.lineTo(X(t.d2[t.n - 1]), Y(t.e[t.n - 1] + s.eyeB));
      c.stroke();
      c.setLineDash([]);
      if (blocked) {
        const x = X(s.blockedAt!);
        c.fillStyle = '#ff5a4a';
        c.beginPath();
        c.arc(x, Y(this.eleAt(s.blockedAt!)), 4, 0, Math.PI * 2);
        c.fill();
      }
    }

    // punkty trasy
    const legs = this.data.legIdx ?? [];
    c.textAlign = 'center';
    c.textBaseline = 'bottom';
    legs.forEach((li, k) => {
      if (li == null || li >= t.n) return;
      const x = X(t.d2[li]), y = Y(t.e[li]);
      c.strokeStyle = 'rgba(239,231,214,0.35)';
      c.setLineDash([2, 3]);
      c.beginPath();
      c.moveTo(x, y);
      c.lineTo(x, base);
      c.stroke();
      c.setLineDash([]);
      const last = k === legs.length - 1;
      c.fillStyle = k === 0 ? '#efe7d6' : last ? '#ff5a36' : '#26323a';
      c.strokeStyle = '#efe7d6';
      c.beginPath();
      c.arc(x, y, 7, 0, Math.PI * 2);
      c.fill();
      if (!last && k !== 0) c.stroke();
      c.fillStyle = k === 0 ? '#0c1114' : '#fff';
      c.font = '600 9px "Geist Mono", monospace';
      c.textBaseline = 'middle';
      c.fillText(k === 0 ? 'A' : last ? 'B' : String(k), x, y + 0.5);
    });

    // nazwy mijanych punktów
    const pois = (this.data.pois ?? []).slice().sort((a, b) => b.ele - a.ele);
    const used: [number, number][] = [];
    c.font = 'italic 13px "Instrument Serif", serif';
    c.textBaseline = 'bottom';
    for (const p of pois) {
      const x = X(p.d), y = Y(p.ele) - 8;
      const w = c.measureText(p.name).width;
      if (used.some(([a, b]) => x - w / 2 < b && x + w / 2 > a)) continue;
      if (x - w / 2 < PAD.l || x + w / 2 > W - PAD.r) continue;
      used.push([x - w / 2 - 6, x + w / 2 + 6]);
      c.fillStyle = 'rgba(12,17,20,0.6)';
      c.fillRect(x - 0.5, y + 2, 1, 5);
      c.fillStyle = '#f3ead8';
      c.fillText(p.name, x, y);
      if (used.length > 10) break;
    }

    // kursor
    const cd = this.hoverD ?? this.cursorD;
    if (cd != null) {
      const x = X(cd), e = this.eleAt(cd);
      c.strokeStyle = 'rgba(255,255,255,0.7)';
      c.lineWidth = 1;
      c.beginPath();
      c.moveTo(x, PAD.t - 6);
      c.lineTo(x, base);
      c.stroke();
      c.fillStyle = '#fff';
      c.strokeStyle = this.data.color;
      c.lineWidth = 3;
      c.beginPath();
      c.arc(x, Y(e), 5, 0, Math.PI * 2);
      c.fill();
      c.stroke();
    }
  }

  private eleAt(d: number) {
    const t = this.data!.track;
    let lo = 0, hi = t.n - 1;
    while (hi - lo > 1) {
      const m = (lo + hi) >> 1;
      if (t.d2[m] < d) lo = m; else hi = m;
    }
    const f = t.d2[hi] > t.d2[lo] ? (d - t.d2[lo]) / (t.d2[hi] - t.d2[lo]) : 0;
    return t.e[lo] + (t.e[hi] - t.e[lo]) * f;
  }
}
