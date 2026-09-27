import { escapeHtml } from '../app';
import { estimate, getCoverage, loadCoverage, validate, type BBox, type Quality, lidarResFromZoom } from '../core/area';
import { fmtInt } from '../core/geo';
import { CATALOG, initCatalog, REGIONS, SERVER_MODE, STATUS, type RegionDef } from '../core/region';
import { ICON } from './icons';

interface Job {
  id: string;
  level: 'base' | 'hd';
  state: 'queued' | 'running' | 'done' | 'error';
  progress: number;
  label: string;
  error: string | null;
  log: string[];
}

interface AreaStatus {
  id: string;
  def: RegionDef;
  installed: boolean;
  level: 'base' | 'hd' | null;
  lidarZoom?: number;
  date: string | null;
  bytes: number;
}

interface ApiStatus {
  admin: boolean;
  limits: Record<Quality, number>;
  regions: AreaStatus[];
  job: Job | null;
  queue: { id: string; level: string }[];
  last: Job | null;
}

const TOKEN_KEY = 'gran.adminToken';
const COUNTRY: Record<string, string> = {
  PL: 'Polska', CZ: 'Czechy', SK: 'Słowacja', AT: 'Austria', CH: 'Szwajcaria', IT: 'Włochy',
  FR: 'Francja', DE: 'Niemcy', SI: 'Słowenia', ES: 'Hiszpania', NO: 'Norwegia', SE: 'Szwecja',
};
const FOCUS: Record<string, { label: string; view: BBox }> = {
  europe: { label: 'Europa', view: [-11, 35, 32, 61] },
  alps: { label: 'Alpy', view: [4.3, 43.1, 17.2, 49.6] },
  carpathians: { label: 'Karpaty', view: [16.2, 43.5, 27.2, 50.8] },
  sudetes: { label: 'Sudety', view: [13.5, 49.5, 17.8, 51.4] },
  scandinavia: { label: 'Skandynawia', view: [3, 54, 31, 71] },
};
const LABELS = new Set(['PL', 'CZ', 'SK', 'AT', 'CH', 'IT', 'FR', 'DE', 'SI']);
const mb = (v: number) => (v < 10 ? v.toFixed(1).replace('.', ',') : fmtInt(v)) + ' MB';

const tileLon = (x: number, z: number) => x / 2 ** z * 360 - 180;
const tileLat = (y: number, z: number) => 180 / Math.PI * Math.atan(Math.sinh(Math.PI * (1 - 2 * y / 2 ** z)));
const lonTile = (lon: number, z: number) => Math.floor((lon + 180) / 360 * 2 ** z);
const latTile = (lat: number, z: number) => Math.floor((1 - Math.asinh(Math.tan(Math.max(-85, Math.min(85, lat)) * Math.PI / 180)) / Math.PI) / 2 * 2 ** z);

/** Mapa wyboru obszaru i zarządzanie pobranymi wycinkami. */
export class MapsManager {
  private el: HTMLElement;
  private timer = 0;
  private api: ApiStatus | null = null;
  private onboarding = false;
  private message = '';
  private lastDone = '';
  private built = false;
  private view: BBox = [...FOCUS.europe.view];
  private selection: BBox | null = null;
  private quality: Quality = 'normal';
  private drag: { lon: number; lat: number; x: number; y: number } | null = null;
  onInstalled: (id: string, first: boolean) => void = () => {};
  onChange: () => void = () => {};
  onOpen: (id: string) => void = () => {};

  constructor() {
    this.el = document.createElement('div');
    this.el.id = 'maps';
    this.el.className = 'maps';
    this.el.hidden = true;
    document.getElementById('app')!.appendChild(this.el);
    this.el.addEventListener('click', (e) => void this.onClick(e));
  }

  get isOpen() { return !this.el.hidden; }

  open(onboarding = false) {
    this.onboarding = onboarding;
    this.el.hidden = false;
    this.el.classList.toggle('is-onboarding', onboarding);
    void this.prepare();
    clearInterval(this.timer);
    this.timer = window.setInterval(() => void this.refresh(), 1200);
  }

  private async prepare() {
    try {
      await loadCoverage();
      if (!this.built) this.build();
      await this.refresh();
    } catch (e) {
      this.el.innerHTML = `<div class="maps__box"><h1 class="h1">Nie udało się otworzyć mapy</h1><p class="lede">${escapeHtml((e as Error).message)}</p></div>`;
    }
  }

  close() {
    if (this.onboarding && !REGIONS.length) return;
    this.el.hidden = true;
    clearInterval(this.timer);
  }

  private token() {
    try { return localStorage.getItem(TOKEN_KEY) ?? ''; } catch { return ''; }
  }

  private async call(method: string, url: string, body?: unknown): Promise<Record<string, unknown> | false> {
    const res = await fetch(`${import.meta.env.BASE_URL}${url}`, {
      method,
      headers: { 'Content-Type': 'application/json', ...(this.token() ? { Authorization: `Bearer ${this.token()}` } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    const json = await res.json().catch(() => ({})) as Record<string, unknown>;
    if (res.status === 401) this.message = 'Ta instancja jest chroniona – podaj hasło administratora.';
    else if (!res.ok) this.message = String(json.error ?? `Błąd ${res.status}`);
    else this.message = '';
    this.renderStatus();
    return res.ok ? json : false;
  }

  private async refresh() {
    const before = new Set(REGIONS.map((r) => r.id));
    await initCatalog();
    if (SERVER_MODE) {
      try { this.api = await (await fetch(`${import.meta.env.BASE_URL}api/status`, { cache: 'no-store' })).json(); }
      catch { this.api = null; }
    }
    const last = this.api?.last;
    if (last && last.state === 'done' && `${last.id}:${last.level}` !== this.lastDone) {
      const fresh = !this.lastDone;
      this.lastDone = `${last.id}:${last.level}`;
      if (!fresh || !before.has(last.id)) this.onInstalled(last.id, before.size === 0);
    }
    const now = new Set(REGIONS.map((r) => r.id));
    if (before.size !== now.size || [...now].some((id) => !before.has(id))) this.onChange();
    if (!this.el.hidden && this.built) this.renderStatus();
  }

  private build() {
    this.el.innerHTML = `<div class="maps__box maps__box--picker grain">
      <button class="btn btn--icon maps__close" data-close>${ICON.x}</button>
      <div class="kicker" data-kicker></div>
      <h1 class="h1" data-title></h1>
      <p class="lede">Przybliż mapę i zaznacz kwadrat dokładnie nad terenem, który chcesz mieć offline. Teren i szlaki działają w całej Europie; LiDAR może pokrywać tylko część kwadratu w Polsce lub Czechach.</p>
      <div class="maps__token" data-tokenbox hidden></div>
      <div class="maps__pickergrid">
        <section class="areamap">
          <div class="areamap__toolbar">
            <div class="areamap__focus">${Object.entries(FOCUS).map(([id, f]) => `<button class="chip" data-focus="${id}">${f.label}</button>`).join('')}</div>
            <span>przeciągnij = kwadrat · kółko = zoom</span>
          </div>
          <div class="areamap__canvas" data-map>
            <svg data-map-svg role="img" aria-label="Mapa wyboru obszaru Europy"></svg>
            <div class="areamap__zoom"><button data-zoom="1" title="Przybliż">+</button><button data-zoom="-1" title="Oddal">−</button></div>
            <div class="areamap__hint" data-maphint>Przeciągnij po mapie, aby zaznaczyć kwadrat</div>
          </div>
        </section>
        <aside class="areaform">
          <div class="kicker">Wybrany kwadrat</div>
          <label class="areaform__label">Nazwa<input data-name maxlength="60" placeholder="np. Dolomity — Cortina" /></label>
          <div class="areaform__label">Jakość</div>
          <div class="qualitypick">
            <button data-quality="normal" class="is-on"><b>Standard</b><small>teren ~10–25 m · szlaki OSM</small></button>
            <button data-quality="high"><b>LiDAR</b><small>3 m · tylko Polska i Czechy</small></button>
          </div>
          <div class="areaform__stats" data-selection></div>
          <button class="btn btn--signal areaform__download" data-create disabled>Pobierz zaznaczony kwadrat</button>
          <p class="areaform__note">Kwadrat zostanie zapamiętany na serwerze. Po pobraniu pojawi się w przełączniku terenu.</p>
        </aside>
      </div>
      <div class="maps__owned">
        <div class="maps__ownedhead"><div><span class="kicker">Pobrane kwadraty</span><b data-count></b></div><span data-disk></span></div>
        <div class="maps__list" data-areas></div>
      </div>
      <p class="note">Mapa wyboru i szlaki © OpenStreetMap contributors · model terenu Terrarium/AWS · granice Natural Earth · LiDAR © GUGiK i © ČÚZK.</p>
    </div>`;
    const map = this.el.querySelector<HTMLElement>('[data-map]')!;
    map.addEventListener('pointerdown', (e) => this.mapDown(e));
    map.addEventListener('pointermove', (e) => this.mapMove(e));
    map.addEventListener('pointerup', (e) => this.mapUp(e));
    map.addEventListener('pointercancel', () => { this.drag = null; });
    map.addEventListener('wheel', (e) => this.mapWheel(e), { passive: false });
    this.el.querySelectorAll<HTMLElement>('[data-focus]').forEach((button) => button.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      const focus = button.dataset.focus!;
      this.view = [...FOCUS[focus].view];
      this.selection = null;
      this.renderMap();
      this.renderSelection();
    }));
    this.el.querySelectorAll<HTMLElement>('[data-zoom]').forEach((button) => button.addEventListener('click', (e) => {
      e.stopPropagation();
      const [w, s, east, n] = this.view;
      this.zoomAt((w + east) / 2, (s + n) / 2, button.dataset.zoom === '1' ? 0.65 : 1.45);
    }));
    this.el.querySelectorAll<HTMLButtonElement>('[data-quality]').forEach((button) => button.addEventListener('click', (e) => {
      e.stopPropagation();
      if (button.disabled) return;
      this.quality = button.dataset.quality as Quality;
      this.renderSelection();
    }));
    this.built = true;
    this.renderMap();
    this.renderSelection();
  }

  private point(e: PointerEvent | WheelEvent) {
    const r = this.el.querySelector<HTMLElement>('[data-map]')!.getBoundingClientRect();
    const x = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    const y = Math.max(0, Math.min(1, (e.clientY - r.top) / r.height));
    return { lon: this.view[0] + x * (this.view[2] - this.view[0]), lat: this.view[3] - y * (this.view[3] - this.view[1]), x: e.clientX, y: e.clientY };
  }

  private mapDown(e: PointerEvent) {
    if (e.button !== 0 || (e.target as Element).closest('button')) return;
    const map = this.el.querySelector<HTMLElement>('[data-map]')!;
    map.setPointerCapture(e.pointerId);
    this.drag = this.point(e);
    this.selection = [this.drag.lon, this.drag.lat, this.drag.lon, this.drag.lat];
    this.renderMap();
  }

  private mapMove(e: PointerEvent) {
    if (!this.drag) return;
    const p = this.point(e);
    // Równe boki w kilometrach: stopień długości geograficznej skraca się
    // wraz z szerokością, więc korygujemy go przez cosinus szerokości.
    const cos = Math.max(.2, Math.cos(((this.drag.lat + p.lat) / 2) * Math.PI / 180));
    const side = Math.max(Math.abs(p.lat - this.drag.lat), Math.abs(p.lon - this.drag.lon) * cos);
    const lon2 = this.drag.lon + Math.sign(p.lon - this.drag.lon || 1) * side / cos;
    const lat2 = this.drag.lat + Math.sign(p.lat - this.drag.lat || 1) * side;
    this.selection = [Math.min(this.drag.lon, lon2), Math.min(this.drag.lat, lat2), Math.max(this.drag.lon, lon2), Math.max(this.drag.lat, lat2)];
    this.renderMap();
    this.renderSelection();
  }

  private mapUp(e: PointerEvent) {
    if (!this.drag) return;
    const start = this.drag, p = this.point(e);
    this.drag = null;
    if (Math.hypot(p.x - start.x, p.y - start.y) < 7) {
      this.selection = null;
      this.zoomAt(p.lon, p.lat, 0.45);
    }
    this.renderMap();
    this.renderSelection();
  }

  private mapWheel(e: WheelEvent) {
    e.preventDefault();
    const p = this.point(e);
    this.zoomAt(p.lon, p.lat, e.deltaY > 0 ? 1.25 : 0.8);
  }

  private zoomAt(lon: number, lat: number, factor: number) {
    const [w, s, e, n] = this.view;
    const width = Math.max(1.2, Math.min(50, (e - w) * factor));
    const height = Math.max(0.8, Math.min(36, (n - s) * factor));
    const fx = (lon - w) / (e - w), fy = (lat - s) / (n - s);
    this.view = [lon - width * fx, lat - height * fy, lon + width * (1 - fx), lat + height * (1 - fy)];
    this.renderMap();
  }

  private renderMap() {
    const svg = this.el.querySelector<SVGElement>('[data-map-svg]');
    if (!svg) return;
    const [w, s, e, n] = this.view;
    svg.setAttribute('viewBox', `${w} ${-n} ${e - w} ${n - s}`);
    // Szczegółowy podkład OSM. Zoom rośnie wraz ze zbliżeniem, a kafle
    // pozostają w tym samym geograficznym układzie co granice i zaznaczenie.
    const z = Math.max(4, Math.min(12, Math.ceil(Math.log2(360 / (e - w) * 2.3)) + 1));
    const max = 2 ** z - 1;
    const x0 = Math.max(0, lonTile(w, z)), x1 = Math.min(max, lonTile(e, z));
    const y0 = Math.max(0, latTile(n, z)), y1 = Math.min(max, latTile(s, z));
    const tiles: string[] = [];
    for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) {
      const tw = tileLon(x, z), te = tileLon(x + 1, z);
      const tn = tileLat(y, z), ts = tileLat(y + 1, z);
      tiles.push(`<image class="areamap__tile" href="https://tile.openstreetmap.org/${z}/${x}/${y}.png" x="${tw}" y="${-tn}" width="${te - tw}" height="${tn - ts}" preserveAspectRatio="none"/>`);
    }
    const countries = getCoverage();
    const paths = countries.map((c) => {
      const d = c.ring.map(([x, y], i) => `${i ? 'L' : 'M'}${x},${-y}`).join('') + 'Z';
      return `<path class="areamap__country${c.lidar ? ' has-lidar' : ''}" d="${d}"><title>${escapeHtml(c.name)}${c.lidar ? ' · LiDAR' : ''}</title></path>`;
    }).join('');
    const labels = e - w > 7 ? countries.filter((c) => LABELS.has(c.code)).map((c) => {
      const xs = c.ring.map((p) => p[0]), ys = c.ring.map((p) => p[1]);
      const x = (Math.min(...xs) + Math.max(...xs)) / 2, y = (Math.min(...ys) + Math.max(...ys)) / 2;
      return `<text x="${x}" y="${-y}" class="areamap__label">${c.code}</text>`;
    }).join('') : '';
    let selected = '';
    if (this.selection) {
      const [sw, ss, se, sn] = this.selection;
      selected = `<rect class="areamap__selection" x="${sw}" y="${-sn}" width="${se - sw}" height="${sn - ss}"/>`;
    }
    svg.innerHTML = `<g class="areamap__tiles">${tiles.join('')}</g><g>${paths}</g><g>${labels}</g>${selected}`;
  }

  private renderSelection() {
    const box = this.el.querySelector<HTMLElement>('[data-selection]');
    const create = this.el.querySelector<HTMLButtonElement>('[data-create]');
    const hint = this.el.querySelector<HTMLElement>('[data-maphint]');
    if (!box || !create || !hint) return;
    const high = this.el.querySelector<HTMLButtonElement>('[data-quality="high"]')!;
    if (!this.selection || this.selection[2] - this.selection[0] < 1e-4 || this.selection[3] - this.selection[1] < 1e-4) {
      box.innerHTML = '<div class="areaform__empty">Nie zaznaczono kwadratu</div>';
      create.disabled = true;
      high.disabled = true;
      hint.hidden = false;
      return;
    }
    hint.hidden = true;
    const normalError = validate(this.selection, 'normal');
    const highError = validate(this.selection, 'high');
    high.disabled = !!highError;
    if (this.quality === 'high' && highError) this.quality = 'normal';
    this.el.querySelectorAll<HTMLElement>('[data-quality]').forEach((b) => b.classList.toggle('is-on', b.dataset.quality === this.quality));
    const info = estimate(this.selection, this.quality);
    const hs = high.querySelector('small');
    if (hs) hs.textContent = `${info.res} m · tylko Polska i Czechy`;
    const names = Object.entries(this.countryFractions(this.selection)).filter(([, f]) => f > 0.01).map(([c]) => COUNTRY[c] ?? c);
    const error = this.quality === 'high' ? highError : normalError;
    box.innerHTML = `<div class="areaform__numbers">
      <div><b>${fmtInt(info.km2)}</b><small>km²</small></div>
      <div><b>~${mb(info.mb)}</b><small>na dysku</small></div>
      <div><b>~${info.min < 2 ? info.min.toFixed(1).replace('.', ',') : fmtInt(info.min)}</b><small>min</small></div>
    </div>
    <div class="areaform__where">${names.length ? names.join(' · ') : 'poza obsługiwanym zasięgiem'}</div>
    ${error ? `<div class="areaform__error">${escapeHtml(error)}</div>` : '<div class="areaform__ok">Gotowe do pobrania</div>'}`;
    create.disabled = !!error || !SERVER_MODE;
  }

  private countryFractions(bbox: BBox) {
    const result: Record<string, number> = {};
    const [w, s, e, n] = bbox, N = 10;
    const coverage = getCoverage();
    for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) {
      const lon = w + (i + .5) / N * (e - w), lat = s + (j + .5) / N * (n - s);
      const c = coverage.find((k) => this.inRing(lon, lat, k.ring));
      if (c) result[c.code] = (result[c.code] ?? 0) + 1 / (N * N);
    }
    return result;
  }

  private inRing(lon: number, lat: number, ring: [number, number][]) {
    let inside = false;
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const [xi, yi] = ring[i], [xj, yj] = ring[j];
      if (yi > lat !== yj > lat && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi) inside = !inside;
    }
    return inside;
  }

  private fallbackAreas(): AreaStatus[] {
    return CATALOG.filter((r) => STATUS.has(r.id)).map((def) => {
      const s = STATUS.get(def.id)!;
      return { id: def.id, def, installed: s.installed, level: s.level ?? null, date: s.date ?? null, bytes: s.bytes ?? 0 };
    });
  }

  private card(a: AreaStatus) {
    const job = this.api?.job?.id === a.id ? this.api.job : null;
    const queued = this.api?.queue.some((q) => q.id === a.id);
    const err = this.api?.last?.id === a.id && this.api.last.state === 'error' ? this.api.last.error : null;
    let status = '', actions = '';
    if (job) {
      status = '<div class="mcard__bar"><i></i></div><div class="mcard__job"><span class="spinner"></span><span data-joblabel></span></div>';
      actions = `<button class="btn btn--ghost" data-cancel="${a.id}">Anuluj</button>`;
    } else if (queued) {
      status = '<div class="mcard__job">W kolejce…</div>';
      actions = `<button class="btn btn--ghost" data-cancel="${a.id}">Anuluj</button>`;
    } else if (a.installed) {
      status = `<div class="mcard__ok">${ICON.pin}<b>${a.level === 'hd' ? `LiDAR ${lidarResFromZoom(a.lidarZoom)} m` : 'Standard'}</b>${a.bytes ? ` · ${mb(a.bytes / 1048576)}` : ''}</div>`;
      actions = `<button class="btn btn--primary" data-open="${a.id}">Otwórz</button><button class="btn btn--ghost btn--icon" title="Usuń mapę" data-remove="${a.id}">${ICON.trash}</button>`;
    }
    const countries = (a.def.countries ?? []).map((c) => COUNTRY[c] ?? c).join(' · ');
    return `<div class="mcard ${a.installed ? 'is-installed' : ''} ${job ? 'is-busy' : ''}" data-area="${a.id}">
      <div class="mcard__main"><div class="mcard__name">${escapeHtml(a.def.name || 'Nowy obszar')}</div>
      <div class="mcard__sub">${escapeHtml(a.def.subtitle || countries || 'własny kwadrat terenu')}</div>
      <div class="mcard__meta"><span>${fmtInt(this.areaKm2(a.def.bbox))} km²</span>${countries ? `<span>${escapeHtml(countries)}</span>` : ''}<span>${a.def.bbox.map((v) => v.toFixed(2)).join(' · ')}</span></div>
      ${status}${err ? `<div class="mcard__err">Błąd: ${escapeHtml(err)}</div>` : ''}</div>
      <div class="mcard__actions">${actions}</div></div>`;
  }

  private areaKm2([w, s, e, n]: BBox) { return (e - w) * 111.32 * Math.cos((s + n) / 2 * Math.PI / 180) * (n - s) * 110.57; }

  private renderStatus() {
    if (!this.built) return;
    this.el.querySelector<HTMLElement>('[data-kicker]')!.textContent = this.onboarding ? 'Pierwsze uruchomienie' : 'Mapy offline';
    this.el.querySelector<HTMLElement>('[data-title]')!.textContent = this.onboarding ? 'Zaznacz pierwszy kwadrat' : 'Pobierz tylko to, czego potrzebujesz';
    this.el.querySelector<HTMLElement>('[data-close]')!.hidden = this.onboarding && !REGIONS.length;
    const areas = this.api?.regions ?? this.fallbackAreas();
    const total = areas.reduce((s, a) => s + (a.bytes ?? 0), 0);
    this.el.querySelector<HTMLElement>('[data-count]')!.textContent = `${areas.filter((a) => a.installed).length} pobranych`;
    this.el.querySelector<HTMLElement>('[data-disk]')!.textContent = total ? `${mb(total / 1048576)} na dysku` : '';
    this.el.querySelector<HTMLElement>('[data-areas]')!.innerHTML = areas.length
      ? areas.map((a) => this.card(a)).join('')
      : '<div class="maps__empty">Nie masz jeszcze żadnego kwadratu. Zaznacz pierwszy powyżej — na przykład fragment Alp.</div>';
    const needToken = !!this.api?.admin && !this.token();
    const token = this.el.querySelector<HTMLElement>('[data-tokenbox]')!;
    token.hidden = !(needToken || this.message);
    if (!token.hidden) token.innerHTML = `${this.message ? `<span>${escapeHtml(this.message)}</span>` : ''}${needToken || this.message.includes('hasło') ? '<input type="password" placeholder="Hasło administratora" data-token><button class="btn" data-savetoken>Zapisz</button>' : ''}`;
    const job = this.api?.job;
    if (job) {
      const card = this.el.querySelector<HTMLElement>(`[data-area="${job.id}"]`);
      const bar = card?.querySelector<HTMLElement>('.mcard__bar i');
      if (bar) bar.style.width = `${(job.progress * 100).toFixed(1)}%`;
      const label = card?.querySelector<HTMLElement>('[data-joblabel]');
      if (label) label.textContent = `${job.label} · ${fmtInt(job.progress * 100)}%`;
    }
    this.renderSelection();
  }

  private async onClick(e: MouseEvent) {
    const t = e.target as HTMLElement;
    if (t === this.el || t.closest('[data-close]')) return this.close();
    const focus = t.closest<HTMLElement>('[data-focus]')?.dataset.focus;
    if (focus && FOCUS[focus]) { this.view = [...FOCUS[focus].view]; this.selection = null; this.renderMap(); this.renderSelection(); return; }
    const zoom = t.closest<HTMLElement>('[data-zoom]')?.dataset.zoom;
    if (zoom) {
      const [w, s, east, n] = this.view;
      this.zoomAt((w + east) / 2, (s + n) / 2, zoom === '1' ? 0.65 : 1.45);
      return;
    }
    const q = t.closest<HTMLElement>('[data-quality]')?.dataset.quality as Quality | undefined;
    if (q && !t.closest<HTMLButtonElement>('button')?.disabled) { this.quality = q; this.renderSelection(); return; }
    if (t.closest('[data-savetoken]')) {
      const value = this.el.querySelector<HTMLInputElement>('[data-token]')?.value ?? '';
      try { localStorage.setItem(TOKEN_KEY, value); } catch { /* brak localStorage */ }
      this.message = ''; await this.refresh(); return;
    }
    if (t.closest('[data-create]') && this.selection) {
      const name = this.el.querySelector<HTMLInputElement>('[data-name]')?.value.trim() ?? '';
      const result = await this.call('POST', 'api/areas', { name, bbox: this.selection, quality: this.quality });
      if (result) { this.selection = null; const input = this.el.querySelector<HTMLInputElement>('[data-name]'); if (input) input.value = ''; await this.refresh(); this.renderMap(); }
      return;
    }
    const open = t.closest<HTMLElement>('[data-open]')?.dataset.open;
    if (open) { this.el.hidden = true; clearInterval(this.timer); this.onOpen(open); return; }
    const cancel = t.closest<HTMLElement>('[data-cancel]')?.dataset.cancel;
    if (cancel && await this.call('POST', 'api/cancel', { id: cancel })) { await this.refresh(); return; }
    const remove = t.closest<HTMLElement>('[data-remove]')?.dataset.remove;
    if (remove) {
      const area = (this.api?.regions ?? []).find((a) => a.id === remove);
      if (!confirm(`Usunąć kwadrat „${area?.def.name ?? remove}” z serwera?`)) return;
      if (await this.call('DELETE', `api/regions/${remove}`)) await this.refresh();
    }
  }
}
