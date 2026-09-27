import { frameWorldToLonLat } from '../core/frame';
import { lat2px, lon2px } from '../core/geo';
import { tileNearLidar } from '../core/area';
import type { App } from '../app';
import { escapeHtml, trailHex } from '../app';
import { bearing, compassDir, DEG, fmt1, fmtCoords, fmtDecimal, fmtDist, fmtEle, fmtInt, fmtSigned, fmtTime, haversine } from '../core/geo';
import type { Lens, MeasurePoint, Tool } from '../core/store';
import { ALT_COLORS } from '../scene/routes';
import { TRAIL_NAME_PL } from '../scene/trails';
import { ICON, POI_ICON, POI_TYPE_PL } from './icons';
import { fmtDuration, type BakeJob } from './maps';

const $ = (id: string) => document.getElementById(id)!;

const TOOLS: { id: Tool; name: string; key: string; icon: string }[] = [
  { id: 'explore', name: 'Eksploracja', key: '1', icon: ICON.explore },
  { id: 'route', name: 'Planowanie trasy', key: '2', icon: ICON.route },
  { id: 'measure', name: 'Pomiar i linia wzroku', key: '3', icon: ICON.measure },
];

/** Soczewki kursora – co teren pokazuje wokół wskazanego punktu. */
export const LENSES: { id: Exclude<Lens, 'none'>; name: string; short: string; key: string; icon: string; hint: string }[] = [
  { id: 'band', name: 'Ta sama wysokość', short: 'Ta sama wys.', key: 'H', icon: ICON.band, hint: 'poziomica albo pas terenu na wysokości kursora' },
  { id: 'rel', name: 'Wysokość względna', short: 'Względna', key: 'W', icon: ICON.relative, hint: 'teren barwiony różnicą wysokości względem kursora' },
  { id: 'vis', name: 'Widoczność', short: 'Widoczność', key: 'O', icon: ICON.viewshed, hint: 'rozświetlone jest to, co widać z miejsca pod kursorem' },
];

const HINTS: Partial<Record<Tool, string>> = {
  route: '<b>Kliknij</b> start i cel · kolejne kliknięcia dodają punkty · przeciągaj znaczniki',
  measure: '<b>Klikaj</b> kolejne punkty · <kbd>Esc</kbd> kończy',
};


/** Stylizowana sylwetka pasma (deterministyczna). */
function silhouette(seed: string, max: number) {
  let h = 0;
  for (const c of seed) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const rnd = () => ((h = (h * 1664525 + 1013904223) >>> 0) / 2 ** 32);
  const k = Math.min(1, max / 2655);
  const pts: string[] = [];
  let y = 30;
  for (let i = 0; i <= 12; i++) {
    const x = (i / 12) * 76;
    const peak = Math.sin((i / 12) * Math.PI) * 22 * k;
    y = 32 - peak - rnd() * 9 * k - 2;
    pts.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  }
  return `<svg class="menu__spark" viewBox="0 0 76 34"><polyline points="${pts.join(' ')} 76,34 0,34" fill="rgba(239,231,214,0.08)" stroke="rgba(239,231,214,0.7)" stroke-width="1.2" stroke-linejoin="round"/></svg>`;
}

export class Chrome {
  private hintEl: HTMLElement | null = null;
  private panoEl: HTMLElement | null = null;
  private paletteIdx = 0;
  private paletteItems: { kind: 'poi' | 'route' | 'geo'; id: number }[] = [];
  private geo: { q: string; items: { name: string; label: string; type: string; lon: number; lat: number; bbox: [number, number, number, number] | null }[] } = { q: '', items: [] };
  private geoTimer = 0;

  constructor(private app: App, _switchRegion: (id: string) => void, private openMaps: () => void = () => {}) {
    this.buildRail();
    this.buildLensDock();
    this.buildRegionMenu();
    this.buildHelp();
    this.bindKeys();
    this.bindDrop();
    this.bindPalette();
    app.openCtx = (x, y, p) => this.openCtx(x, y, p);
    $('compass').addEventListener('click', () => app.engine.flyTo({ heading: 0 }, 800));
    $('attrib').innerHTML =
      'LiDAR i ortofoto: © <a href="https://www.geoportal.gov.pl" target="_blank" rel="noopener">GUGiK</a>, © ČÚZK, © ÚGKK SR · Teren: Terrarium/AWS · Szlaki: © <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> · Sentinel‑2 © EOX';
    const open = document.createElement('button');
    open.id = 'panel-open';
    open.innerHTML = `${ICON.layers} Panel`;
    open.addEventListener('click', () => app.store.set({ panelOpen: true }));
    $('app').appendChild(open);
    app.profile.onHover = (i) => app.onProfileHover(i);
    app.profile.onClick = (i) => {
      const r = app.profileTrack;
      if (!r) return;
      app.engine.flyTo({ x: r.track.x[i], z: r.track.z[i], distance: Math.min(app.engine.view.distance, 3000) }, 1000);
    };
  }

  // ------------------------------------------------------------- szyna
  private buildRail() {
    const rail = $('rail');
    rail.innerHTML =
      TOOLS.map((t) => `<button class="tool" data-tool="${t.id}" aria-label="${t.name}">${t.icon}<span class="tool__tip">${t.name}<kbd>${t.key}</kbd></span></button>`).join('') +
      `<div class="rail__sep"></div>
       <button class="tool" data-act="layers" aria-label="Mapa i światło">${ICON.layers}<span class="tool__tip">Mapa i światło<kbd>M</kbd></span></button>
       <button class="tool" data-act="search" aria-label="Szukaj">${ICON.target}<span class="tool__tip">Szukaj<kbd>/</kbd></span></button>
       <button class="tool" data-act="help" aria-label="Pomoc">${ICON.help}<span class="tool__tip">Skróty i pomoc<kbd>?</kbd></span></button>`;
    rail.querySelectorAll<HTMLElement>('[data-tool]').forEach((b) => b.addEventListener('click', () => this.app.setTool(b.dataset.tool as Tool)));
    rail.querySelector('[data-act="layers"]')!.addEventListener('click', () => this.openMapTab());
    rail.querySelector('[data-act="search"]')!.addEventListener('click', () => this.openPalette());
    rail.querySelector('[data-act="help"]')!.addEventListener('click', () => ($('help').hidden = false));

    $('search-btn').addEventListener('click', () => this.openPalette());
  }

  openMapTab() {
    this.app.store.set({ panelOpen: true });
    (document.querySelector('.panel__tabs [data-tab="map"]') as HTMLButtonElement).click();
  }

  // ------------------------------------------------------------- regiony
  private buildRegionMenu() {
    const btn = $('region-btn'), menu = $('region-menu');
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      menu.hidden = !menu.hidden;
      this.renderRegionMenu();
    });
    document.addEventListener('click', (e) => {
      if (!menu.contains(e.target as Node)) menu.hidden = true;
    });
  }

  private renderRegionMenu() {
    const menu = $('region-menu');
    const a = this.app;
    const recent = a.recentPlaces();
    const cur = a.region?.def;
    menu.innerHTML =
      `<button class="menu__more" data-search>${ICON.target}<span><b>Szukaj miejsca na świecie…</b><small>szczyt, miejscowość, pasmo górskie · klawisz /</small></span></button>` +
      (recent.length ? `<div class="menu__label">Ostatnio odwiedzone</div>` : '') +
      recent
        .map((r, i) => `<button class="menu__item ${cur && r.name === cur.name ? 'is-on' : ''}" data-recent="${i}">${silhouette(r.name, 1800)}<span><span class="menu__name">${escapeHtml(r.name)}</span><br><span class="menu__sub">${r.lat.toFixed(3)}°N · ${r.lon.toFixed(3)}°E</span></span><span class="menu__max">${new Date(r.when).toLocaleDateString('pl-PL', { day: 'numeric', month: 'short' })}</span></button>`)
        .join('');
    menu.querySelector('[data-search]')!.addEventListener('click', () => {
      menu.hidden = true;
      this.openPalette();
    });
    menu.querySelectorAll<HTMLElement>('[data-recent]').forEach((b) =>
      b.addEventListener('click', () => {
        menu.hidden = true;
        const r = recent[Number(b.dataset.recent)];
        a.flyToLonLat(r.lon, r.lat, 22000);
      })
    );
  }

  /** Pasek postępu wypiekania obszaru – widoczny w całej aplikacji. */
  setJob(job: BakeJob | null, last: BakeJob | null) {
    let el = document.getElementById('jobpill');
    if (!el) {
      el = document.createElement('button');
      el.id = 'jobpill';
      el.className = 'jobpill';
      el.addEventListener('click', () => this.openMaps());
      $('app').appendChild(el);
    }
    const recent = last && last.finished && Date.now() - last.finished < 8000 ? last : null;
    const j = job ?? recent;
    if (!j) {
      el.classList.remove('is-on');
      return;
    }
    el.classList.add('is-on');
    el.classList.toggle('is-done', !job && j.state === 'done');
    el.classList.toggle('is-err', !job && j.state === 'error');
    const pct = Math.round(j.progress * 100);
    const title = escapeHtml(j.name || 'Nowy obszar');
    const res = j.res ? `LiDAR ${j.res} m` : 'Standard';
    const html = job
      ? `<span class="jobpill__ring" style="--p:${pct}"><i>${pct}%</i></span>
         <span class="jobpill__main"><b>Wypiekanie: ${title}</b><small>${escapeHtml(j.label)} · ${res}${j.km2 ? ` · ${fmtInt(j.km2)} km²` : ''}</small></span>
         <span class="jobpill__time"><b>${j.eta != null ? `~${fmtDuration(j.eta)}` : '…'}</b><small>${fmtDuration(j.elapsed ?? 0)} od startu</small></span>
         <i class="jobpill__bar" style="width:${pct}%"></i>`
      : j.state === 'done'
        ? `<span class="jobpill__ring is-ok">✓</span><span class="jobpill__main"><b>Gotowe: ${title}</b><small>${res} · otwórz z listy obszarów</small></span>`
        : `<span class="jobpill__ring is-bad">!</span><span class="jobpill__main"><b>Nie udało się: ${title}</b><small>${escapeHtml(j.error ?? '')}</small></span>`;
    if (el.dataset.h !== html) {
      el.dataset.h = html;
      el.innerHTML = html;
    }
  }

  /** Lista pobranych kwadratów się zmieniła (menedżer map). */
  refreshRegions() {
    if (!$('region-menu').hidden) this.renderRegionMenu();
  }

  // ------------------------------------------------------------- aktualizacje
  updateTileStat() {
    const a = this.app;
    const el = $('tilestat');
    const t = a.tiles;
    if (!t) { el.innerHTML = ''; return; }
    const n = t.loading, o = a.store.state.style === 'satellite' ? t.orthoLoading : 0;
    // źródło terenu pod kamerą: LiDAR (PL/CZ) albo model globalny (Terrarium, ~30 m)
    let src = 'Teren <b>~30 m</b>';
    if (a.wf) {
      const v = a.engine.view;
      const [lon, lat] = frameWorldToLonLat(a.wf, v.x, v.z);
      const x = Math.floor(lon2px(lon, 12) / 256), y = Math.floor(lat2px(lat, 12) / 256);
      if (tileNearLidar(12, x, y)) src = `LiDAR <b>${v.distance < 12000 ? '3 m' : v.distance < 30000 ? '6 m' : '12 m'}</b>`;
    }
    const html = n + o > 0
      ? `<span class="spinner"></span>${src} · doczytywanie ${n ? `${n} kafli` : ''}${n && o ? ', ' : ''}${o ? `${o} zdjęć` : ''}`
      : src;
    if (el.dataset.h !== html) { el.dataset.h = html; el.innerHTML = html; }
  }

  // ------------------------------------------------------------- soczewka kursora
  private buildLensDock() {
    // lewa kolumna: szyna narzędzi + osobny panel soczewki pod nią
    const col = document.createElement('div');
    col.id = 'leftcol';
    $('app').appendChild(col);
    col.appendChild($('rail'));
    const dock = document.createElement('div');
    dock.id = 'lensdock';
    dock.className = 'lensrail';
    col.appendChild(dock);
    dock.addEventListener('click', (e) => {
      const t = (e.target as HTMLElement).closest<HTMLElement>('[data-lens],[data-lp],[data-unpin]');
      if (!t) return;
      const a = this.app;
      if (t.dataset.lens === 'none') { if (a.store.state.lens !== 'none') a.setLens(a.store.state.lens); }
      else if (t.dataset.lens) a.setLens(t.dataset.lens as Lens);
      else if (t.dataset.unpin != null) a.unpinLens();
      else if (t.dataset.lp) {
        const [k, v] = t.dataset.lp.split(':');
        const n = Number(v);
        const lens = ({ band: 'band', rel: 'rel', eye: 'vis' } as Record<string, Lens>)[k];
        if (k === 'band') a.store.set({ bandTol: n });
        if (k === 'rel') a.store.set({ relRange: n });
        if (k === 'eye') {
          a.store.set({ vsEye: n });
          const vs = a.store.state.viewshed;
          if (vs) void a.runViewshed(vs.point.x, vs.point.z);
        }
        // wybór parametru włącza też odpowiednią soczewkę
        if (a.store.state.lens !== lens) a.setLens(lens);
        else a.applyLens();
      }
    });
  }

  private updateLensDock() {
    const a = this.app, s = a.store.state;
    const dock = $('lensdock');
    const pinned = a.lensPinned;
    const key = [s.lens, s.bandTol, s.relRange, s.vsEye, pinned, s.panorama].join('|');
    if (dock.dataset.k === key) return;
    dock.dataset.k = key;
    const chip = (k: string, v: number, label: string, cur: number) => `<button class="lens__chip ${cur === v ? 'is-on' : ''}" data-lp="${k}:${v}">${label}</button>`;
    const params: Record<string, string> = {
      iso: '',
      band: [0, 5, 10, 25, 50].map((v) => chip('band', v, v === 0 ? 'linia' : `±${v} m`, s.bandTol)).join(''),
      rel: [200, 500, 1000].map((v) => chip('rel', v, `±${v} m`, s.relRange)).join(''),
      vis: [1.7, 10, 30, 100].map((v) => chip('eye', v, v === 1.7 ? 'oczy 1,7 m' : `+${v} m`, s.vsEye)).join(''),
    };
    const paramLabel: Record<string, string> = { band: 'poziomica lub szerokość pasa', rel: 'zakres barw', vis: 'wysokość obserwatora' };
    const none = s.lens === 'none';
    dock.innerHTML =
      `<div class="lensitem">
        <button class="tool lens-btn lens-btn--cursor ${none ? 'is-on' : ''}" data-lens="none" aria-label="Kursor">${ICON.cursor}</button>
        <div class="flyout"><div class="flyout__head"><b>Kursor</b><kbd>Esc</kbd></div><div class="flyout__hint">zwykła mysz: kliknięcie zaznacza szczyty, szlaki i punkty terenu; soczewki wyłączone</div></div>
      </div><div class="lensrail__sep"></div>` +
      LENSES.map((l) => {
        const on = s.lens === l.id;
        const status = on
          ? pinned
            ? `<button class="lens__chip lens__pin" data-unpin title="Odepnij (Esc)">${ICON.pin} przypięta · odepnij</button>`
            : '<span class="lens__live">na żywo · kliknij mapę, by przypiąć</span>'
          : '';
        return `<div class="lensitem">
          <button class="tool lens-btn ${on ? 'is-on' : ''} ${on && pinned ? 'is-pinned' : ''}" data-lens="${l.id}" aria-label="${l.name}">${l.icon}</button>
          <div class="flyout">
            <div class="flyout__head"><b>${l.name}</b><kbd>${l.key}</kbd></div>
            <div class="flyout__hint">${l.hint}</div>
            ${params[l.id] ? `<div class="flyout__k">${paramLabel[l.id]}</div><div class="flyout__chips">${params[l.id]}</div>` : ''}
            ${status ? `<div class="flyout__status">${status}</div>` : ''}
          </div>
        </div>`;
      }).join('');
  }

  update() {
    const a = this.app, s = a.store.state;
    if (!a.region) return;
    this.updateLensDock();
    this.updateTileStat();
    if (a.sectorReady) {
      $('region-name').textContent = a.region.def.name || 'Okolica';
      $('region-sub').textContent = `${fmtInt(a.region.dem.min)}–${fmtInt(a.region.dem.max)} m · ${fmtInt(a.graph?.totalKm ?? 0)} km szlaków`;
    } else {
      $('region-name').textContent = a.pendingSector ? 'Przygotowuję okolicę…' : 'Widok świata';
      $('region-sub').textContent = a.pendingSector ? 'szlaki, szczyty i miejscowości' : 'przybliż, aby wczytać szlaki';
    }
    document.querySelectorAll<HTMLElement>('#rail [data-tool]').forEach((b) => b.classList.toggle('is-on', b.dataset.tool === s.tool));
    this.updateReadout();
    this.updateCompass();
    this.updateHint();
    this.updatePano();
    this.updateProfileHead();
  }

  private updateHint() {
    const s = this.app.store.state;
    const lens = LENSES.find((l) => l.id === s.lens);
    let txt = HINTS[s.tool];
    let show = !!txt && !s.panorama && !(s.tool === 'route' && s.waypoints.length >= 2) && !(s.tool === 'measure' && s.measure.length >= 2);
    const key = lens ? `lens:${lens.id}:${this.app.lensPinned}` : s.tool;
    if (lens && s.tool === 'explore' && !s.panorama) {
      txt = this.app.lensPinned
        ? `Soczewka <b>${lens.name}</b> przypięta · <kbd>Esc</kbd> odepnij · kliknij gdzie indziej, by przenieść`
        : `Soczewka <b>${lens.name}</b>: ${lens.hint} · <b>kliknij</b>, by przypiąć`;
      show = true;
    }
    if (!show) {
      this.hintEl?.remove();
      this.hintEl = null;
      return;
    }
    if (!this.hintEl) {
      this.hintEl = document.createElement('div');
      this.hintEl.className = 'hint-tool';
      $('app').appendChild(this.hintEl);
    }
    if (this.hintEl.dataset.t !== key) {
      this.hintEl.dataset.t = key;
      this.hintEl.innerHTML = txt!;
    }
  }

  private updatePano() {
    const a = this.app;
    if (!a.store.state.panorama) {
      this.panoEl?.remove();
      this.panoEl = null;
      return;
    }
    if (!this.panoEl) {
      this.panoEl = document.createElement('div');
      this.panoEl.className = 'pano-bar';
      $('app').appendChild(this.panoEl);
    }
    const cam = a.engine.camera.position;
    const e = cam.y / a.engine.exag - 2;
    const v = a.engine.view;
    this.panoEl.innerHTML = `<b>Panorama</b><span>${fmtEle(e)} · patrzysz na ${compassDir(v.heading)} (${fmtInt(v.heading)}°) · kółko = zoom</span><button class="btn btn--primary" id="pano-exit">Wyjdź <kbd>Esc</kbd></button>`;
    this.panoEl.querySelector('#pano-exit')!.addEventListener('click', () => a.exitPanorama());
  }

  private updateReadout() {
    const a = this.app, s = a.store.state;
    const c = a.cursor;
    const el = $('readout');
    if (!c) {
      el.innerHTML = '';
      return;
    }
    let html = `<div><em>Położenie</em><b>${fmtCoords(c.lon, c.lat)}</b></div><div><em>Wysokość</em><b>${fmtEle(c.e)}</b></div><div><em>Nachylenie</em><b>${fmtInt(c.slope)}°</b> ${compassDir(c.aspect)}</div>`;
    if (s.lens === 'band' || s.lens === 'iso') {
      const lvl = s.ref?.e ?? c.e;
      html += `<div><em>${s.bandTol ? 'Pas wysokości' : 'Poziomica'}</em><b>${fmtEle(lvl)}</b>${s.bandTol ? ` ± ${s.bandTol} m` : ''}</div>`;
      if (s.ref) html += `<div><em>Δh od poziomu</em><b class="${c.e - lvl >= 0 ? 'up' : 'down'}">${fmtSigned(c.e - lvl)}</b></div>`;
    } else if (s.lens === 'vis' && !a.lensPinned && Number.isFinite(a.liveArea)) {
      html += `<div><em>Widać stąd</em><b>${fmt1(a.liveArea)} km²</b></div>`;
    }
    if (s.ref && s.lens === 'rel') {
      const dh = c.e - s.ref.e;
      const d = haversine(s.ref.lon, s.ref.lat, c.lon, c.lat);
      const ang = d > 0 ? Math.atan2(dh, d) / DEG : 0;
      html += `<div><em>Δh od punktu</em><b class="${dh >= 0 ? 'up' : 'down'}">${fmtSigned(dh)}</b></div><div><em>Odległość</em><b>${fmtDist(d)}</b> · ${fmt1(ang)}°</div><div><em>Azymut</em><b>${fmtInt(bearing(s.ref.lon, s.ref.lat, c.lon, c.lat))}°</b></div>`;
    }
    if (s.tool === 'measure' && s.measure.length) {
      const last = s.measure[s.measure.length - 1];
      html += `<div><em>Od ostatniego</em><b>${fmtDist(haversine(last.lon, last.lat, c.lon, c.lat))}</b> · ${fmtSigned(c.e - last.e)}</div>`;
    }
    el.innerHTML = html;
  }

  private updateCompass() {
    const a = this.app;
    const v = a.engine.view;
    const el = $('compass');
    const rot = -v.heading;
    if (el.dataset.r === rot.toFixed(1)) return;
    el.dataset.r = rot.toFixed(1);
    el.innerHTML = `<svg viewBox="0 0 64 64"><g transform="rotate(${rot} 32 32)">
      <circle cx="32" cy="32" r="24" fill="none" stroke="rgba(239,231,214,.18)"/>
      ${Array.from({ length: 24 }, (_, i) => `<line x1="32" y1="${i % 6 === 0 ? 7 : 9}" x2="32" y2="11" stroke="rgba(239,231,214,${i % 6 === 0 ? 0.7 : 0.25})" transform="rotate(${i * 15} 32 32)"/>`).join('')}
      <path d="M32 12 L36.5 32 L32 29 L27.5 32 Z" fill="#ff5a36"/><path d="M32 52 L36.5 32 L32 35 L27.5 32 Z" fill="rgba(239,231,214,.55)"/>
      <text x="32" y="6.5" text-anchor="middle" font-family="Geist Mono" font-size="7" fill="#efe7d6">N</text></g>
      <text x="32" y="61" text-anchor="middle" font-family="Geist Mono" font-size="6" fill="rgba(239,231,214,.5)">${fmtInt(v.pitch)}°</text></svg>`;
  }

  updateScale() {
    const mpp = this.app.engine.metersPerPixel();
    const el = $('scalebar');
    if (!mpp) {
      el.style.opacity = '0';
      return;
    }
    el.style.opacity = '1';
    const target = mpp * 110;
    const steps = [10, 20, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000, 10000, 20000];
    const m = steps.reduce((p, s) => (s <= target ? s : p), steps[0]);
    (el.querySelector('.scalebar__bar') as HTMLElement).style.width = `${m / mpp}px`;
    el.querySelector('span')!.textContent = m >= 1000 ? `${m / 1000} km` : `${m} m`;
  }

  private updateProfileHead() {
    const a = this.app, s = a.store.state;
    const head = $('profile-head');
    const r = a.profileTrack;
    if (!r) {
      head.innerHTML = '';
      return;
    }
    const st = r.stats;
    const isMeasure = s.tool === 'measure';
    const title = isMeasure ? 'Przekrój terenu' : r.kind === 'gpx' && s.gpx === r ? r.labels[0] : r.labels.join(' · ');
    const key = [title, r.id, s.flying, isMeasure].join('|');
    if (head.dataset.k === key) return;
    head.dataset.k = key;
    const color = isMeasure ? '#e9c46a' : r.kind === 'gpx' ? '#c98bff' : ALT_COLORS[s.activeRoute % ALT_COLORS.length];
    head.innerHTML = `<span class="profile__title" style="color:${color}">${escapeHtml(title)}</span>
      <div class="profile__stats">
        <span><em>dystans</em><b>${fmtDist(isMeasure ? st.len3 : st.len3)}</b></span>
        <span><em>↗</em><b>${fmtInt(st.up)} m</b></span>
        <span><em>↘</em><b>${fmtInt(st.down)} m</b></span>
        ${isMeasure ? '' : `<span><em>czas</em><b>${fmtTime(st.time)}</b></span>`}
        <span><em>max</em><b>${fmtInt(st.maxEle)} m</b></span>
      </div>
      <div class="profile__actions">
        ${isMeasure ? '' : `<button class="btn btn--icon" data-pa="fly" title="Przelot kamery wzdłuż trasy (F)">${s.flying ? ICON.pause : ICON.fly}</button>`}
        <button class="btn btn--icon" data-pa="close" title="Ukryj profil">${ICON.chevronDown}</button>
      </div>`;
    head.querySelector('[data-pa="fly"]')?.addEventListener('click', () => a.toggleFly());
    head.querySelector('[data-pa="close"]')!.addEventListener('click', () => {
      a.store.set({ profileOpen: false });
      $('app').classList.remove('has-profile');
    });
  }

  // ------------------------------------------------------------- menu kontekstowe
  private openCtx(x: number, y: number, p: MeasurePoint) {
    const a = this.app;
    const el = $('ctx');
    const s = a.store.state;
    const snap = a.graph?.snap(p.x, p.z, 450);
    const names = snap ? a.graph!.edgeRouteNames(snap.edge) : [];
    el.innerHTML = `
      <div class="ctx__head"><b>${fmtEle(p.e)}</b><span>${fmtDecimal(p.lon, p.lat)}${snap ? ` · szlak ${fmtDist(snap.dist)}` : ''}</span>
      ${names.length ? `<div style="margin-top:6px;display:flex;gap:6px;flex-wrap:wrap">${names.slice(0, 3).map((r) => `<span class="trail-chip"><i style="background:${trailHex(r.color)}"></i>${TRAIL_NAME_PL[r.color] ?? ''}</span>`).join('')}</div>` : ''}</div>
      <button data-c="start">${ICON.flag}Trasa stąd</button>
      <button data-c="end">${ICON.pin}Trasa dotąd</button>
      ${s.waypoints.length >= 2 ? `<button data-c="via">${ICON.plus}Dodaj punkt pośredni</button>` : ''}
      <hr>
      <button data-c="band">${ICON.band}Teren na tej samej wysokości<kbd>H</kbd></button>
      <button data-c="ref">${ICON.relative}Wysokość względna od tego punktu<kbd>W</kbd></button>
      <button data-c="vs">${ICON.viewshed}Co widać z tego miejsca?<kbd>O</kbd></button>
      <button data-c="pano">${ICON.person}Stań tutaj – panorama</button>
      <button data-c="measure">${ICON.measure}Pomiar stąd<kbd>5</kbd></button>
      <hr>
      <button data-c="info">${ICON.target}Informacje o punkcie</button>
      <button data-c="copy">${ICON.copy}Kopiuj współrzędne</button>`;
    el.hidden = false;
    const r = el.getBoundingClientRect();
    el.style.left = `${Math.min(x, innerWidth - r.width - 10)}px`;
    el.style.top = `${Math.min(y, innerHeight - r.height - 10)}px`;
    el.querySelectorAll<HTMLElement>('[data-c]').forEach((b) =>
      b.addEventListener('click', () => {
        el.hidden = true;
        switch (b.dataset.c) {
          case 'start':
            a.store.set({ tool: 'route' });
            if (s.waypoints.length >= 2) a.removeWaypoint(s.waypoints[0].id);
            a.addWaypoint(p.x, p.z, 0);
            break;
          case 'end':
            a.store.set({ tool: 'route' });
            if (s.waypoints.length >= 2) a.removeWaypoint(s.waypoints[s.waypoints.length - 1].id);
            a.addWaypoint(p.x, p.z);
            break;
          case 'via': a.addWaypoint(p.x, p.z, s.waypoints.length - 1); break;
          case 'band': a.pinLens(p.x, p.z, 'band'); break;
          case 'ref': a.pinLens(p.x, p.z, 'rel'); break;
          case 'vs': a.pinLens(p.x, p.z, 'vis'); break;
          case 'pano': a.enterPanorama(p.x, p.z); break;
          case 'measure': a.store.set({ tool: 'measure', measure: [p] }); break;
          case 'info': a.store.set({ tool: 'explore' }); a.selectPoint(p.x, p.z); a.store.set({ panelOpen: true }); break;
          case 'copy': navigator.clipboard?.writeText(fmtDecimal(p.lon, p.lat)); a.toast('Skopiowano współrzędne.'); break;
        }
      })
    );
  }

  // ------------------------------------------------------------- wyszukiwarka
  private bindPalette() {
    const pal = $('palette');
    pal.innerHTML = `<div class="palette__box">
      <div class="palette__input">${ICON.target}<input id="pal-q" placeholder="Rysy, Morskie Oko, czerwony szlak…" autocomplete="off" spellcheck="false"></div>
      <div class="palette__list" id="pal-list"></div>
      <div class="palette__foot"><span><kbd>↑</kbd> <kbd>↓</kbd> wybór</span><span><kbd>Enter</kbd> leć</span><span><kbd>Shift</kbd>+<kbd>Enter</kbd> dodaj do trasy</span><span><kbd>Esc</kbd> zamknij</span></div></div>`;
    const q = pal.querySelector<HTMLInputElement>('#pal-q')!;
    q.addEventListener('input', () => {
      this.paletteIdx = 0;
      this.renderPalette(q.value);
    });
    q.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') { this.paletteIdx = Math.min(this.paletteItems.length - 1, this.paletteIdx + 1); this.renderPalette(q.value); e.preventDefault(); }
      if (e.key === 'ArrowUp') { this.paletteIdx = Math.max(0, this.paletteIdx - 1); this.renderPalette(q.value); e.preventDefault(); }
      if (e.key === 'Enter') this.choosePalette(this.paletteIdx, e.shiftKey);
      if (e.key === 'Escape') this.closePalette();
    });
    pal.addEventListener('click', (e) => {
      if (e.target === pal) this.closePalette();
    });
  }

  openPalette() {
    const pal = $('palette');
    pal.hidden = false;
    const q = pal.querySelector<HTMLInputElement>('#pal-q')!;
    q.value = '';
    this.paletteIdx = 0;
    this.renderPalette('');
    setTimeout(() => q.focus(), 10);
  }

  closePalette() {
    $('palette').hidden = true;
  }

  private renderPalette(query: string) {
    const a = this.app;
    const norm = (t: string) => t.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/ł/g, 'l');
    const nq = norm(query.trim());
    const pois = a.region!.pois;
    let items: { kind: 'poi' | 'route' | 'geo'; id: number; score: number }[] = [];
    // wyniki z całego świata (Nominatim przez serwer) – z opóźnieniem, żeby nie zasypywać usługi
    if (nq.length >= 2 && this.geo.q !== query.trim()) {
      clearTimeout(this.geoTimer);
      const q = query.trim();
      this.geoTimer = window.setTimeout(async () => {
        try {
          const r = await fetch(`${import.meta.env.BASE_URL}api/geocode?q=${encodeURIComponent(q)}`);
          const items = r.ok ? await r.json() : [];
          this.geo = { q, items: Array.isArray(items) ? items : [] };
          const input = document.querySelector<HTMLInputElement>('#pal-q');
          if (input && input.value.trim() === q && !$('palette').hidden) this.renderPalette(input.value);
        } catch {
          /* bez wyników globalnych */
        }
      }, 350);
    }
    if (!nq) {
      items = [...pois].filter((p) => p.t === 'peak' || p.t === 'hut' || p.t === 'lake' || (p.t === 'place' && (p.k === 'town' || p.k === 'city'))).sort((x, y) => y.prom - x.prom).slice(0, 12).map((p) => ({ kind: 'poi' as const, id: p.id, score: 0 }));
    } else {
      for (const p of pois) {
        const n = norm(p.n);
        const i = n.indexOf(nq);
        if (i >= 0) items.push({ kind: 'poi', id: p.id, score: (i === 0 ? 100 : 50) + p.prom });
      }
      const words: Record<string, string> = { czerwony: 'red', niebieski: 'blue', zielony: 'green', zolty: 'yellow', czarny: 'black' };
      a.graph!.routes.forEach((r, i) => {
        const n = norm(r.name + ' ' + r.ref);
        const colorHit = Object.entries(words).some(([w, c]) => nq.includes(w) && r.color === c);
        if (n.includes(nq) || colorHit) items.push({ kind: 'route', id: i, score: 20 + (colorHit ? 10 : 0) });
      });
      items.sort((x, y) => y.score - x.score);
      items = items.slice(0, 20);
      if (this.geo.q === query.trim()) this.geo.items.forEach((_, i) => items.push({ kind: 'geo', id: i, score: 0 }));
    }
    this.paletteItems = items;
    const hl = (t: string) => {
      if (!nq) return escapeHtml(t);
      const i = norm(t).indexOf(nq);
      return i < 0 ? escapeHtml(t) : `${escapeHtml(t.slice(0, i))}<mark>${escapeHtml(t.slice(i, i + nq.length))}</mark>${escapeHtml(t.slice(i + nq.length))}`;
    };
    const list = $('pal-list');
    list.innerHTML = items.length
      ? items.map((it, k) => {
          if (it.kind === 'geo') {
            const g = this.geo.items[it.id];
            const head = k === 0 || items[k - 1].kind !== 'geo' ? '<div class="palette__head">Na świecie</div>' : '';
            return `${head}<button class="palette__item ${k === this.paletteIdx ? 'is-on' : ''}" data-k="${k}"><span class="ico">${ICON.pin}</span><span class="nm">${hl(g.name)}<small>${escapeHtml(g.label.split(',').slice(1, 4).join(',').trim())}</small></span><span class="el">${escapeHtml(g.type)}</span></button>`;
          }
          if (it.kind === 'poi') {
            const p = pois[it.id];
            return `<button class="palette__item ${k === this.paletteIdx ? 'is-on' : ''}" data-k="${k}"><span class="ico">${POI_ICON[p.t] ?? ICON.pin}</span><span class="nm">${hl(p.n)}<small>${POI_TYPE_PL[p.t] ?? ''}</small></span><span class="el">${p.t === 'lake' ? '' : fmtEle(p.ele)}</span></button>`;
          }
          const r = a.graph!.routes[it.id];
          return `<button class="palette__item ${k === this.paletteIdx ? 'is-on' : ''}" data-k="${k}"><span class="ico" style="color:${trailHex(r.color)}">${ICON.trail}</span><span class="nm">${hl(r.name)}<small>szlak ${TRAIL_NAME_PL[r.color] ?? ''}</small></span><span class="el">${escapeHtml(r.ref)}</span></button>`;
        }).join('')
      : `<div class="empty" style="margin:10px">${nq.length >= 2 ? 'Szukam na świecie…' : 'Wpisz nazwę szczytu, miejscowości albo pasma.'}</div>`;
    list.querySelectorAll<HTMLElement>('[data-k]').forEach((b) => b.addEventListener('click', (e) => this.choosePalette(Number(b.dataset.k), e.shiftKey)));
    list.querySelector('.is-on')?.scrollIntoView({ block: 'nearest' });
  }

  private choosePalette(k: number, addToRoute: boolean) {
    const it = this.paletteItems[k];
    if (!it) return;
    this.closePalette();
    const a = this.app;
    if (it.kind === 'geo') {
      const g = this.geo.items[it.id];
      // odległość kamery z rozmiaru obiektu (szczyt – blisko, kraj – wysoko)
      let dist = 18000;
      if (g.bbox) {
        const km = Math.max((g.bbox[2] - g.bbox[0]) * 111 * Math.cos((g.lat * Math.PI) / 180), (g.bbox[3] - g.bbox[1]) * 111);
        dist = Math.min(1_200_000, Math.max(6000, km * 1000 * 1.6));
      }
      a.flyToLonLat(g.lon, g.lat, dist);
      return;
    }
    if (it.kind === 'poi') {
      const p = a.region!.pois[it.id];
      if (addToRoute) {
        a.store.set({ tool: 'route' });
        a.addWaypoint(p.x, p.z);
      } else a.selectPoi(p, true);
    } else a.selectRouteByIndex(it.id);
  }

  // ------------------------------------------------------------- klawiatura
  private bindKeys() {
    const a = this.app;
    window.addEventListener('keydown', (e) => {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA') return;
      const s = a.store.state;
      if (e.key === 'Escape') {
        $('help').hidden = true;
        $('ctx').hidden = true;
        if (!$('palette').hidden) return this.closePalette();
        if (s.panorama) return a.exitPanorama();
        if (s.flying || a.fly) return a.stopFly();
        if (s.tool === 'measure' && s.measure.length) return a.store.set({ tool: 'explore' });
        if (a.lensPinned) return a.unpinLens();
        if (s.lens !== 'none') return a.setLens(s.lens);
        if (a.selection) return a.clearSelection();
        if (s.tool !== 'explore') return a.setTool('explore');
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const t = TOOLS.find((x) => x.key === e.key);
      if (t) return a.setTool(t.id);
      switch (e.key.toLowerCase()) {
        case '/': e.preventDefault(); this.openPalette(); break;
        case '?': $('help').hidden = !$('help').hidden; break;
        case 'm': this.openMapTab(); break;
        case 'p': a.store.set({ panelOpen: !s.panelOpen }); break;
        case 't': a.store.set({ trails: !s.trails }); break;
        case 'l': a.store.set({ labels: !s.labels }); break;
        case 'c': a.store.set({ contours: !s.contours }); break;
        case 'i': case 'h': a.setLens('band'); break;
        case 'w': a.setLens('rel'); break;
        case 'o': a.setLens('vis'); break;
        case 'f': a.toggleFly(); break;
        case 'n': a.engine.flyTo({ heading: 0 }, 800); break;
        case 'r': {
          const h = a.region!.def.home;
          const [x, z] = a.region!.dem.lonLatToWorld(h.lon, h.lat);
          a.engine.flyTo({ x, z, distance: h.distance, heading: h.heading, pitch: h.pitch }, 1600);
          break;
        }
        case 'v': {
          const v = a.engine.view;
          a.engine.flyTo({ pitch: v.pitch > 70 ? 35 : 85 }, 900);
          break;
        }
        case 'g': a.exportGpx(); break;
        case 's': a.screenshot(); break;
        case '[': a.store.set({ hour: Math.max(0, s.hour - 0.5) }); break;
        case ']': a.store.set({ hour: Math.min(23.9, s.hour + 0.5) }); break;
        case '+': case '=': a.store.set({ exag: Math.min(3, Math.round((s.exag + 0.1) * 10) / 10) }); break;
        case '-': a.store.set({ exag: Math.max(1, Math.round((s.exag - 0.1) * 10) / 10) }); break;
        case 'backspace': case 'delete':
          if (s.tool === 'route' && s.waypoints.length) a.removeWaypoint(s.waypoints[s.waypoints.length - 1].id);
          if (s.tool === 'measure' && s.measure.length) a.store.set({ measure: s.measure.slice(0, -1) });
          break;
      }
    });
  }

  private buildHelp() {
    const rows: [string, string][] = [
      ['<kbd>1</kbd>–<kbd>3</kbd>', 'eksploracja · trasa · pomiar'], ['<kbd>H</kbd> <kbd>W</kbd> <kbd>O</kbd>', 'soczewka: ta sama wysokość · względna · widoczność'], ['<kbd>/</kbd>', 'wyszukiwarka'], ['<kbd>M</kbd>', 'mapa i światło'], ['<kbd>P</kbd>', 'pokaż / ukryj panel'],
      ['<kbd>T</kbd> <kbd>L</kbd> <kbd>C</kbd>', 'szlaki · nazwy · poziomice'], ['klik z soczewką / <kbd>Esc</kbd>', 'przypnij / odepnij soczewkę'], ['<kbd>+</kbd> <kbd>−</kbd>', 'przewyższenie terenu'],
      ['<kbd>[</kbd> <kbd>]</kbd>', 'pora dnia −/+ 30 min'], ['<kbd>F</kbd>', 'przelot nad trasą'], ['<kbd>N</kbd>', 'północ u góry'],
      ['<kbd>V</kbd>', 'widok z góry / ukośny'], ['<kbd>R</kbd>', 'widok początkowy regionu'], ['<kbd>G</kbd>', 'eksport GPX'],
      ['<kbd>S</kbd>', 'zrzut ekranu PNG'], ['<kbd>⌫</kbd>', 'usuń ostatni punkt'], ['<kbd>Esc</kbd>', 'anuluj / wyjdź'],
      ['lewy przycisk', 'przesuwanie'], ['prawy przycisk', 'obrót / nachylenie · menu analiz (klik)'], ['dwuklik', 'przybliż do punktu'],
    ];
    $('help').innerHTML = `<div class="help__box grain">
      <button class="btn btn--icon help__close" id="help-x">${ICON.x}</button>
      <div class="kicker">Skróty i sterowanie</div><h1 class="h1">Jak korzystać z <em>Grani</em></h1>
      <p class="lede">Atlas łączy rzeczywisty model terenu z siecią szlaków OSM. Wszystkie obliczenia – trasy, widoczność, cienie – odbywają się lokalnie w przeglądarce.</p>
      <div class="help__grid">${rows.map(([k, v]) => `<div class="help__row"><span>${v}</span><span>${k}</span></div>`).join('')}</div>
      <p class="note" style="margin-top:18px">Pliki <b>.gpx</b> możesz upuścić bezpośrednio na mapę. Link z przycisku „Link” zapisuje widok kamery i punkty trasy.</p></div>`;
    $('help').addEventListener('click', (e) => {
      if (e.target === $('help') || (e.target as HTMLElement).closest('#help-x')) $('help').hidden = true;
    });
  }

  private bindDrop() {
    const app = $('app');
    app.addEventListener('dragover', (e) => e.preventDefault());
    app.addEventListener('drop', (e) => {
      e.preventDefault();
      const f = e.dataTransfer?.files?.[0];
      if (f) void this.app.importGpx(f);
    });
  }
}

/** Tło ekranu ładowania: poziomice generowane z szumu. */
export function drawLoaderTopo() {
  const svg = $('loader-topo');
  const paths: string[] = [];
  const centers = [[260, 330, 1], [560, 470, 0.8], [420, 180, 0.6]];
  let k = 0;
  for (const [cx, cy, s] of centers) {
    for (let r = 20; r < 380 * s; r += 16) {
      const pts: string[] = [];
      for (let i = 0; i <= 72; i++) {
        const a = (i / 72) * Math.PI * 2;
        const w = 1 + 0.18 * Math.sin(a * 3 + r * 0.03 + cx) + 0.1 * Math.sin(a * 5 - r * 0.05) + 0.06 * Math.sin(a * 9 + cy);
        pts.push(`${(cx + Math.cos(a) * r * w).toFixed(1)},${(cy + Math.sin(a) * r * w * 0.8).toFixed(1)}`);
      }
      paths.push(`<path class="${k % 5 === 0 ? 'idx' : ''}" style="animation-delay:${(k * 0.025).toFixed(2)}s" d="M${pts.join(' L')}Z"/>`);
      k++;
    }
  }
  svg.innerHTML = paths.join('');
}
