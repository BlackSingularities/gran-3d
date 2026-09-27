import type { App } from '../app';
import { escapeHtml, trailHex } from '../app';
import { compassDir, fmt1, fmtCoords, fmtDecimal, fmtDist, fmtEle, fmtInt, fmtSigned, fmtTime, haversine } from '../core/geo';
import { DIFFICULTY, SAC_LABEL, TIME_MODEL_LABEL } from '../core/metrics';
import { GFX_PRESETS, GFX_PRESET_LABEL, type Gfx, type GfxPreset } from '../core/gfx';
import type { Poi } from '../core/region';
import type { Style } from '../core/store';
import { fmtHour, sunPosition, sunTimes, warsawDate } from '../core/sun';
import { ALT_COLORS } from '../scene/routes';
import { TRAIL_NAME_PL } from '../scene/trails';
import { ICON, POI_TYPE_PL } from './icons';
import { GRADE_COLORS, GRADE_LABELS } from './profile';

const STYLES: { id: Style; name: string; sw: string }[] = [
  { id: 'terrain', name: 'Teren', sw: 'linear-gradient(135deg,#1f3322 0%,#44512c 40%,#8a8577 70%,#e9edf2 100%)' },
  { id: 'satellite', name: 'Ortofoto', sw: 'linear-gradient(135deg,#27361f,#4b5433 45%,#7c7a6c 75%,#c9ccd0)' },
  { id: 'paper', name: 'Mapa', sw: 'linear-gradient(135deg,#cfe2bb 0%,#dfe8cf 45%,#f5f0e3 70%,#cfccc7 100%)' },
  { id: 'hypso', name: 'Hipsometria', sw: 'linear-gradient(135deg,#6a9f68,#c2d184 30%,#efc985 55%,#b67a4f 80%,#f4f4f6)' },
  { id: 'slope', name: 'Nachylenie', sw: 'linear-gradient(135deg,#efeee8 20%,#fcdc4d 40%,#f98f1f 55%,#e0302a 70%,#a13389 85%,#542e76)' },
  { id: 'aspect', name: 'Ekspozycja', sw: 'conic-gradient(from 0deg,#e87f7f,#e8d27f,#8fe87f,#7fe8d7,#7f9fe8,#c77fe8,#e87f7f)' },
];

const DIFF_HTML = (d: number) =>
  `<span class="diff d${d}">${[0, 1, 2, 3].map((i) => `<i class="${i <= d ? 'on' : ''}"></i>`).join('')}</span>`;

export class Panel {
  tab: 'tool' | 'map' | 'gfx' = 'tool';
  private key = '';
  private body: HTMLElement;
  private playTimer = 0;

  constructor(private app: App) {
    this.body = document.getElementById('panel-body')!;
    document.querySelectorAll<HTMLButtonElement>('.panel__tabs [data-tab]').forEach((b) =>
      b.addEventListener('click', () => {
        this.tab = b.dataset.tab as 'tool' | 'map' | 'gfx';
        document.querySelectorAll('.panel__tabs [data-tab]').forEach((x) => x.classList.toggle('is-on', x === b));
        this.render(true);
      })
    );
    document.getElementById('panel-toggle')!.addEventListener('click', () => app.store.set({ panelOpen: false }));
  }

  private computeKey() {
    const a = this.app, s = a.store.state;
    const sel = a.selection;
    return JSON.stringify([
      this.tab, s.regionId, s.tool, s.routeMode, s.routing, s.activeRoute, s.style, s.contours, s.trails, s.labels, s.shadows, s.grid, s.snow, s.lens, s.bandTol, a.lensPinned,
      s.day, s.flying, s.panorama, s.vsEye, s.timeKind, s.pace, s.profileOpen, s.gfx.preset, s.gfx.maxLevel, s.gfx.mesh, s.gfx.ortho, s.gfx.detail, s.gfx.fps, s.gfx.adaptive,
      sel ? [sel.kind, sel.poi?.id, sel.point?.x, sel.routeIdx] : null,
      s.waypoints.map((w) => w.id + (w.label ?? '')),
      s.routes.map((r) => r.id + r.labels.join()),
      s.ref ? [s.ref.x, s.ref.z] : null,
      s.viewshed ? [s.viewshed.point.x, s.viewshed.areaKm2] : null,
      s.measure.length, a.measureInfo && s.measure.length ? [Math.round(a.measureInfo.surface), a.measureInfo.visible] : null,
      s.gpx?.id, Math.round(a.satProgress * 10), s.lens === 'rel' ? s.relRange : 0,
    ]);
  }

  render(force = false) {
    if (!this.app.region) return;
    const k = this.computeKey();
    if (!force && k === this.key) return;
    this.key = k;
    const scroll = this.body.scrollTop;
    this.body.innerHTML = this.tab === 'map' ? this.mapTab() : this.tab === 'gfx' ? this.gfxTab() : this.toolTab();
    this.body.scrollTop = scroll;
    this.bind();
  }

  // ======================================================================= NARZĘDZIE
  private toolTab() {
    const s = this.app.store.state;
    switch (s.tool) {
      case 'route':
        return this.routeView();
      case 'measure':
        return this.measureView();
      default: {
        // przypięta soczewka kursora ma pierwszeństwo przed kartą regionu
        if (s.lens === 'rel' && s.ref) return this.relativeView();
        if (s.lens === 'vis' && s.viewshed) return this.viewshedView();
        if ((s.lens === 'iso' || s.lens === 'band') && s.ref) return this.levelView();
        return this.exploreView();
      }
    }
  }

  private exploreView() {
    const a = this.app;
    const sel = a.selection;
    if (sel?.kind === 'poi' && sel.poi) return this.poiCard(sel.poi);
    if (sel?.kind === 'point' && sel.point) return this.pointCard();
    if (sel?.kind === 'trail') return this.trailCard();
    const r = a.region!;
    const peaks = r.pois.filter((p) => p.t === 'peak').sort((x, y) => y.ele - x.ele);
    const top = peaks[0];
    const huts = r.pois.filter((p) => p.t === 'hut').length;
    const area = (r.dem.widthM * r.dem.heightM) / 1e6;
    const colors = new Map<string, number>();
    for (const e of a.graph!.edges) for (const c of e.colors) colors.set(c, (colors.get(c) ?? 0) + e.len2);
    const notable = [...peaks].sort((x, y) => y.prom - x.prom).slice(0, 9);
    const gpx = a.store.state.gpx;
    return `
      <div class="kicker">Region · ${r.def.bbox[1].toFixed(2)}°–${r.def.bbox[3].toFixed(2)}° N</div>
      <h1 class="h1">${r.def.name}</h1>
      <p class="lede">${r.def.subtitle}. Przeciągnij, aby przesunąć, prawy przycisk – obrót, kółko – przybliżenie. <b>Prawy klik na terenie</b> otwiera menu analiz.</p>
      <div class="stats">
        <div class="stat"><div class="stat__v">${fmtInt(top?.ele ?? r.dem.max)}<small>m</small></div><div class="stat__k">${top ? escapeHtml(top.n) : 'najwyżej'}</div></div>
        <div class="stat"><div class="stat__v">${fmtInt(r.dem.max - r.dem.min)}<small>m</small></div><div class="stat__k">rozpiętość wysokości</div></div>
        <div class="stat"><div class="stat__v">${fmtInt(a.graph!.totalKm)}<small>km</small></div><div class="stat__k">znakowanych szlaków</div></div>
        <div class="stat"><div class="stat__v">${fmtInt(peaks.length)}<small>/ ${huts} schr.</small></div><div class="stat__k">nazwanych szczytów</div></div>
        <div class="stat"><div class="stat__v">${fmtInt(area)}<small>km²</small></div><div class="stat__k">obszar modelu</div></div>
        <div class="stat"><div class="stat__v">${fmtInt(r.dem.mpp)}<small>m</small></div><div class="stat__k">rozdzielczość siatki</div></div>
      </div>
      ${gpx ? this.gpxCard() : ''}
      ${a.store.state.routes.length ? `<div class="section"><button class="btn btn--primary" data-act="tool-route">${ICON.route} Wróć do zaplanowanej trasy</button></div>` : ''}
      <div class="section">
        <div class="section__title"><span class="kicker">Szlaki w regionie</span></div>
        <div class="legend-rows">
          ${[...colors.entries()].filter(([, l]) => l > 500).sort((x, y) => y[1] - x[1]).map(([c, l]) => `<div class="legend-row"><i style="background:${trailHex(c)};box-shadow:0 0 0 1.5px rgba(255,246,226,.7) inset"></i><span>${TRAIL_NAME_PL[c] ?? c}</span><span>${fmtInt(l / 1000)} km</span></div>`).join('')}
        </div>
      </div>
      <div class="section">
        <div class="section__title"><span class="kicker">Najbardziej wybitne szczyty</span></div>
        <div class="peaklist">
          ${notable.map((p) => `<button class="peakrow" data-poi="${p.id}"><span class="peakrow__n">${escapeHtml(p.n)}</span><span class="peakrow__e">${fmtInt(p.ele)} m</span><span class="peakrow__d"></span></button>`).join('')}
        </div>
      </div>
      <div class="section">
        <div class="btns">
          <label class="btn">${ICON.upload} Wczytaj GPX<input type="file" accept=".gpx,application/gpx+xml" hidden data-act="gpx-file"></label>
          <button class="btn" data-act="shot">${ICON.camera} Zrzut widoku</button>
          <button class="btn" data-act="link">${ICON.link} Link</button>
        </div>
      </div>`;
  }

  private gpxCard() {
    const g = this.app.store.state.gpx!;
    const st = g.stats;
    return `
      <div class="section">
        <div class="section__title"><span class="kicker">Wczytany ślad GPX</span><button class="btn btn--ghost btn--icon" data-act="gpx-clear" title="Usuń ślad">${ICON.x}</button></div>
        <div style="font-family:var(--display);font-size:19px;font-weight:650;letter-spacing:-0.02em;line-height:1.15;margin-bottom:10px">${escapeHtml(g.labels[0])}</div>
        <div class="metrics">
          <div><div class="metric__k">Dystans</div><div class="metric__v">${fmtDist(st.len2)}</div></div>
          <div><div class="metric__k">Czas przejścia</div><div class="metric__v">${fmtTime(st.time)}</div></div>
          <div><div class="metric__k">Podejścia</div><div class="metric__v">↗ ${fmtInt(st.up)} m</div></div>
          <div><div class="metric__k">Zejścia</div><div class="metric__v">↘ ${fmtInt(st.down)} m</div></div>
        </div>
        <div class="btns" style="margin-top:12px"><button class="btn" data-act="fly">${this.app.store.state.flying ? ICON.pause : ICON.play} Przelot</button><button class="btn" data-act="fit-gpx">${ICON.expand} Pokaż</button></div>
      </div>`;
  }

  private actionsFor(x: number, z: number) {
    return `
      <div class="btns" style="margin-top:14px">
        <button class="btn btn--primary" data-act="wp" data-x="${x}" data-z="${z}">${ICON.flag} Dodaj do trasy</button>
        <button class="btn" data-act="ref" data-x="${x}" data-z="${z}">${ICON.relative} Poziom 0</button>
        <button class="btn" data-act="vs" data-x="${x}" data-z="${z}">${ICON.viewshed} Widoczność</button>
        <button class="btn" data-act="pano" data-x="${x}" data-z="${z}">${ICON.person} Panorama</button>
      </div>`;
  }

  private poiCard(p: Poi) {
    const a = this.app;
    const dem = a.region!.dem;
    const gx = dem.xToGx(p.x), gy = dem.zToGy(p.z);
    const snap = a.graph!.snap(p.x, p.z, 2000);
    const wiki = p.w ? `https://${p.w.split(':')[0]}.wikipedia.org/wiki/${encodeURIComponent(p.w.split(':').slice(1).join(':'))}` : '';
    const nearest = a.region!.pois
      .filter((q) => q.t === 'peak' && q.id !== p.id)
      .map((q) => ({ q, d: haversine(p.lon, p.lat, q.lon, q.lat) }))
      .sort((x, y) => x.d - y.d)
      .slice(0, 4);
    return `
      <div class="row row--between"><span class="kicker">${POI_TYPE_PL[p.t] ?? p.t}</span><button class="btn btn--ghost btn--icon" data-act="unselect" title="Zamknij">${ICON.x}</button></div>
      <h1 class="h1">${escapeHtml(p.n)}</h1>
      <div class="big">${fmtInt(p.ele)}<small>m n.p.m.</small></div>
      <div class="metrics" style="margin-top:14px">
        <div><div class="metric__k">Model terenu</div><div class="metric__v">${fmtEle(p.d)} <small>(${fmtSigned(p.d - p.ele)})</small></div></div>
        <div><div class="metric__k">Nachylenie</div><div class="metric__v">${fmtInt(dem.slope(gx, gy))}° <small>${compassDir(dem.aspect(gx, gy))}</small></div></div>
        <div class="metric--wide"><div class="metric__k">Współrzędne</div><div class="metric__v" style="font-family:var(--mono);font-size:12px">${fmtCoords(p.lon, p.lat)}</div></div>
        <div><div class="metric__k">Najbliższy szlak</div><div class="metric__v">${snap ? fmtDist(snap.dist) : '> 2 km'}</div></div>
        <div><div class="metric__k">Źródło</div><div class="metric__v"><a href="https://www.openstreetmap.org/${p.o}" target="_blank" rel="noopener" style="color:var(--brass)">OSM</a>${wiki ? ` · <a href="${wiki}" target="_blank" rel="noopener" style="color:var(--brass)">Wikipedia</a>` : ''}</div></div>
      </div>
      ${this.actionsFor(p.x, p.z)}
      <div class="section">
        <div class="section__title"><span class="kicker">Sąsiednie szczyty</span></div>
        <div class="peaklist">${nearest.map(({ q, d }) => `<button class="peakrow" data-poi="${q.id}"><span class="peakrow__n">${escapeHtml(q.n)}</span><span class="peakrow__e">${fmtSigned(q.ele - p.ele)}</span><span class="peakrow__d">${fmtDist(d)}</span></button>`).join('')}</div>
      </div>`;
  }

  private pointCard() {
    const a = this.app;
    const p = a.selection!.point!;
    const dem = a.region!.dem;
    const gx = dem.xToGx(p.x), gy = dem.zToGy(p.z);
    const snap = a.graph!.snap(p.x, p.z, 3000);
    const peak = a.region!.pois.filter((q) => q.t === 'peak').map((q) => ({ q, d: haversine(p.lon, p.lat, q.lon, q.lat) })).sort((x, y) => x.d - y.d)[0];
    return `
      <div class="row row--between"><span class="kicker">Punkt terenu</span><button class="btn btn--ghost btn--icon" data-act="unselect" title="Zamknij">${ICON.x}</button></div>
      <div class="big" style="margin-top:8px">${fmtInt(p.e)}<small>m n.p.m.</small></div>
      <div class="metrics" style="margin-top:14px">
        <div><div class="metric__k">Nachylenie</div><div class="metric__v">${fmt1(dem.slope(gx, gy))}°</div></div>
        <div><div class="metric__k">Ekspozycja</div><div class="metric__v">${compassDir(dem.aspect(gx, gy))} <small>${fmtInt(dem.aspect(gx, gy))}°</small></div></div>
        <div class="metric--wide"><div class="metric__k">Współrzędne</div><div class="metric__v" style="font-family:var(--mono);font-size:12px">${fmtCoords(p.lon, p.lat)}<br><span style="color:var(--paper-3)">${fmtDecimal(p.lon, p.lat)}</span></div></div>
        <div><div class="metric__k">Do szlaku</div><div class="metric__v">${snap ? fmtDist(snap.dist) : '> 3 km'}</div></div>
        <div><div class="metric__k">Najbliższy szczyt</div><div class="metric__v">${peak ? `${escapeHtml(peak.q.n)} <small>${fmtDist(peak.d)}</small>` : '—'}</div></div>
      </div>
      ${this.actionsFor(p.x, p.z)}
      <div class="btns" style="margin-top:6px"><button class="btn btn--ghost" data-act="copy" data-v="${fmtDecimal(p.lon, p.lat)}">${ICON.copy} Kopiuj współrzędne</button></div>`;
  }

  private trailCard() {
    const a = this.app;
    const sel = a.selection!;
    const g = a.graph!;
    const route = sel.routeIdx != null ? g.routes[sel.routeIdx] : null;
    let len = 0, up = 0, down = 0, maxE = -Infinity, minE = Infinity, sac = 0;
    for (const id of sel.edges ?? []) {
      const e = g.edges[id];
      len += e.len2; up += e.up; down += e.down; sac = Math.max(sac, e.sac);
      for (let i = 0; i < e.n; i++) { maxE = Math.max(maxE, e.e[i]); minE = Math.min(minE, e.e[i]); }
    }
    const huts = a.region!.pois.filter((p) => p.t === 'hut' || p.t === 'peak' || p.t === 'saddle').filter((p) =>
      (sel.edges ?? []).some((id) => { const e = g.edges[id]; for (let i = 0; i < e.n; i += 3) if (Math.hypot(e.x[i] - p.x, e.z[i] - p.z) < 80) return true; return false; })
    ).sort((x, y) => y.ele - x.ele).slice(0, 10);
    return `
      <div class="row row--between"><span class="kicker">Szlak turystyczny</span><button class="btn btn--ghost btn--icon" data-act="unselect" title="Zamknij">${ICON.x}</button></div>
      <div class="row" style="margin:10px 0 4px"><span class="trail-chip"><i style="background:${trailHex(route?.color ?? 'other')};width:30px;height:7px"></i></span><span class="kicker">${TRAIL_NAME_PL[route?.color ?? 'other'] ?? ''}${route?.ref ? ` · ${escapeHtml(route.ref)}` : ''}</span></div>
      <h1 class="h1" style="font-size:27px">${escapeHtml(route?.name ?? 'Odcinek szlaku')}</h1>
      <div class="metrics" style="margin-top:12px">
        <div><div class="metric__k">Długość w regionie</div><div class="metric__v">${fmtDist(len)}</div></div>
        <div><div class="metric__k">Rozpiętość</div><div class="metric__v">${fmtInt(minE)}–${fmtInt(maxE)} m</div></div>
        <div><div class="metric__k">Suma podejść</div><div class="metric__v">↗ ${fmtInt(up)} m</div></div>
        <div><div class="metric__k">Trudność (OSM)</div><div class="metric__v">${SAC_LABEL[sac]}</div></div>
      </div>
      ${route?.operator ? `<p class="note">Zarządca: <b>${escapeHtml(route.operator)}</b></p>` : ''}
      ${huts.length ? `<div class="section"><div class="section__title"><span class="kicker">Na trasie szlaku</span></div><div class="peaklist">${huts.map((p) => `<button class="peakrow" data-poi="${p.id}"><span class="peakrow__n">${escapeHtml(p.n)}</span><span class="peakrow__e">${POI_TYPE_PL[p.t]}</span><span class="peakrow__d">${fmtInt(p.ele)} m</span></button>`).join('')}</div></div>` : ''}
      <div class="btns" style="margin-top:14px"><button class="btn" data-act="fit-trail">${ICON.expand} Pokaż cały</button>${route ? `<a class="btn" href="https://www.openstreetmap.org/relation/${route.id}" target="_blank" rel="noopener">${ICON.link} OSM</a>` : ''}</div>`;
  }

  private routeView() {
    const a = this.app, s = a.store.state;
    const wps = s.waypoints;
    const r = a.activeRoute;
    const list = wps.length
      ? `<ol class="wps">${wps.map((w, i) => {
          const last = i === wps.length - 1 && wps.length > 1;
          return `<li class="wp ${i === 0 ? 'wp--start' : last ? 'wp--end' : ''}"><span class="wp__dot">${i === 0 ? 'A' : last ? 'B' : i}</span><div><div class="wp__name">${escapeHtml(w.label ?? (i === 0 ? 'Start' : last ? 'Cel' : `Punkt ${i}`))}</div><div class="wp__meta">${fmtEle(w.e)} · ${fmtDecimal(w.lon, w.lat)}</div></div><button class="wp__x" data-rm="${w.id}" title="Usuń punkt">${ICON.x}</button></li>`;
        }).join('')}</ol>`
      : '';
    const empty = wps.length < 2
      ? `<div class="empty" style="margin-top:12px">${wps.length ? 'Teraz kliknij <b>cel</b>' : 'Kliknij na mapie <b>punkt startowy</b>, potem <b>cel</b>'}.<br>Kolejne kliknięcia dodają punkty pośrednie; znaczniki można przeciągać, prawy klik na znaczniku usuwa go.</div>`
      : '';
    const alts = s.routes.length
      ? `<div class="section"><div class="section__title"><span class="kicker">Warianty (${s.routes.length})</span><span class="kicker" style="color:var(--paper-4)">porównanie z najszybszym</span></div><div class="alts">${s.routes.map((rt, i) => {
          const c = ALT_COLORS[i % ALT_COLORS.length];
          const dt = rt.stats.time - s.routes[0].stats.time;
          return `<button class="alt ${i === s.activeRoute ? 'is-on' : ''}" style="--c:${c}" data-alt="${i}"><span class="alt__bar"></span><div>
            <div class="alt__labels">${rt.labels.map((l) => `<span class="alt__label">${escapeHtml(l)}</span>`).join('')}</div>
            <div class="alt__main"><span class="alt__time">${fmtTime(rt.stats.time)}</span><span class="alt__dist">${fmtDist(rt.stats.len3)}</span></div>
            <div class="alt__sub"><span>↗ <b>${fmtInt(rt.stats.up)}</b> m</span><span>↘ <b>${fmtInt(rt.stats.down)}</b> m</span><span>max <b>${fmtInt(rt.stats.maxEle)}</b></span><span>${DIFF_HTML(rt.stats.difficulty)}</span></div>
          </div>${i ? `<span class="alt__delta">${dt >= 0 ? '+' : '−'}${fmtTime(Math.abs(dt))}</span>` : ''}</button>`;
        }).join('')}</div></div>`
      : '';
    let detail = '';
    if (r) {
      const st = r.stats;
      const names = [...new Set(r.routeRefs.map((i) => a.graph!.routes[i]).filter(Boolean).map((x) => JSON.stringify([x.color, x.name])))].slice(0, 8).map((j) => JSON.parse(j) as [string, string]);
      detail = `
        <div class="section">
          <div class="section__title"><span class="kicker">Szczegóły wariantu</span><span>${DIFF_HTML(st.difficulty)} <span class="kicker" style="margin-left:4px">${DIFFICULTY[st.difficulty]}</span></span></div>
          <div class="metrics">
            <div><div class="metric__k">Dystans rzeczywisty</div><div class="metric__v">${fmtDist(st.len3)}</div></div>
            <div><div class="metric__k">W rzucie poziomym</div><div class="metric__v">${fmtDist(st.len2)} <small>+${fmt1(((st.len3 - st.len2) / Math.max(1, st.len2)) * 100)}%</small></div></div>
            <div class="metric--wide"><div class="metric__k">Linia prosta A–B: ${fmtDist(st.straight)} · krętość ×${fmt1(st.len2 / Math.max(1, st.straight))}</div><div class="bar3"><i style="width:${Math.min(100, (st.straight / Math.max(1, st.len2)) * 100)}%"></i></div></div>
            <div><div class="metric__k">Podejścia</div><div class="metric__v">↗ ${fmtInt(st.up)} m</div></div>
            <div><div class="metric__k">Zejścia</div><div class="metric__v">↘ ${fmtInt(st.down)} m</div></div>
            <div><div class="metric__k">Czas (${TIME_MODEL_LABEL[s.timeKind]})</div><div class="metric__v">${fmtTime(st.time)}</div></div>
            <div><div class="metric__k">Punkty GOT PTTK</div><div class="metric__v">${st.got} <small>pkt</small></div></div>
            <div><div class="metric__k">Najwyżej / najniżej</div><div class="metric__v">${fmtInt(st.maxEle)} / ${fmtInt(st.minEle)} m</div></div>
            <div><div class="metric__k">Maks. nachylenie</div><div class="metric__v">${fmtInt(st.maxGrade * 100)}% <small>(${fmtInt(st.maxSlope)}°)</small></div></div>
          </div>
          ${names.length ? `<div class="chips" style="margin-top:14px">${names.map(([c, n]) => `<span class="chip"><i style="background:${trailHex(c)}"></i>${escapeHtml(n.length > 38 ? n.slice(0, 36) + '…' : n)}</span>`).join('')}</div>` : ''}
          <div class="chips" style="margin-top:12px;gap:4px">${GRADE_COLORS.map((c, i) => `<span class="chip" style="font-size:10px;padding:2px 7px"><i style="background:${c};border-radius:2px"></i>${GRADE_LABELS[i]}</span>`).join('')}</div>
          ${st.sacMax >= 4 ? `<p class="note">⚠ Na trasie są odcinki <b>${SAC_LABEL[st.sacMax]}</b> (łańcuchy, ekspozycja). Czas wydłużono odpowiednio do trudności.</p>` : ''}
          ${r.kind === 'terrain' ? `<p class="note">Trasa wyznaczona <b>po terenie</b> (A* po siatce ${fmtInt(a.region!.dem.mpp)} m, funkcja Toblera, nachylenie ≤ ${s.maxSlope}°). To analiza rzeźby — nie zachęta do schodzenia ze szlaku w parkach narodowych.</p>` : ''}
          <div class="btns" style="margin-top:14px">
            <button class="btn btn--primary" data-act="fly">${s.flying ? ICON.pause : ICON.play} ${s.flying ? 'Pauza' : 'Przelot'}</button>
            <button class="btn" data-act="fit">${ICON.expand} Pokaż</button>
            ${s.profileOpen ? '' : `<button class="btn" data-act="profile">${ICON.trail} Profil</button>`}
            <button class="btn" data-act="gpx">${ICON.gpx} GPX</button>
            <button class="btn" data-act="link">${ICON.link} Link</button>
          </div>
        </div>`;
    }
    return `
      <div class="kicker">Planowanie trasy</div>
      <h1 class="h1">Trasa <em>${s.routeMode === 'trails' ? 'szlakami' : 'przez teren'}</em></h1>
      <div class="seg" style="margin:12px 0 6px"><button data-mode="trails" class="${s.routeMode === 'trails' ? 'is-on' : ''}">Szlakami</button><button data-mode="terrain" class="${s.routeMode === 'terrain' ? 'is-on' : ''}">Przez teren</button></div>
      ${s.routeMode === 'terrain' ? `<div class="slider"><label>Maks. nachylenie terenu</label><output id="o-slope">${s.maxSlope}°</output><input type="range" min="15" max="50" step="1" value="${s.maxSlope}" data-range="maxSlope"></div>` : ''}
      <div class="section">
        <div class="section__title"><span class="kicker">Punkty</span>${wps.length ? `<span class="btns"><button class="btn btn--ghost btn--icon" data-act="reverse" title="Odwróć kierunek">${ICON.swap}</button><button class="btn btn--ghost btn--icon" data-act="clear-route" title="Wyczyść trasę">${ICON.trash}</button></span>` : ''}</div>
        ${list}${empty}
      </div>
      <div class="section">
        <div class="section__title"><span class="kicker">Czas przejścia</span></div>
        <div class="seg"><button data-tk="pttk" class="${s.timeKind === 'pttk' ? 'is-on' : ''}">Reguła PTTK</button><button data-tk="din" class="${s.timeKind === 'din' ? 'is-on' : ''}">DIN 33466</button></div>
        <div class="seg" style="margin-top:6px">${[[1.25, 'spokojne'], [1, 'normalne'], [0.8, 'szybkie']].map(([v, l]) => `<button data-pace="${v}" class="${s.pace === v ? 'is-on' : ''}">${l}</button>`).join('')}</div>
      </div>
      ${s.routing ? '<div class="section"><span class="spinner"></span> <span class="kicker" style="margin-left:6px">wyznaczam trasę w terenie…</span></div>' : ''}
      ${alts}
      ${detail}`;
  }

  private relativeView() {
    const a = this.app, s = a.store.state;
    const legend = `
      <div class="legend">${['#0a3a73', '#3a6fa0', '#8fb9d3', '#d6ecf2', '#fbf1cf', '#f7b066', '#e0602c', '#9e1519'].map((c) => `<i style="background:${c}"></i>`).join('')}</div>
      <div class="legend-labels"><span>${fmtSigned(-s.relRange)}</span><span>poziom 0</span><span>${fmtSigned(s.relRange)}</span></div>`;
    if (!s.ref) {
      return `
        <div class="kicker">Wysokość względna</div>
        <h1 class="h1">Co jest <em>wyżej</em>?</h1>
        <p class="lede">Kliknij dowolny punkt – stanie się <b>poziomem zero</b>. Teren zostanie przekolorowany według różnicy wysokości: cieplej – wyżej, chłodniej – niżej. Biała linia łączy miejsca dokładnie na tej samej wysokości, a pierścienie wyznaczają kolejne kilometry.</p>
        ${legend}
        <div class="empty" style="margin-top:18px">Kliknij na teren, aby ustawić punkt odniesienia.</div>`;
    }
    const st = a.relStats();
    return `
      <div class="row row--between"><span class="kicker">Punkt odniesienia</span><button class="btn btn--ghost btn--icon" data-act="clear-ref" title="Wyczyść">${ICON.x}</button></div>
      <div class="big" style="margin-top:6px">${fmtInt(s.ref.e)}<small>m n.p.m. = 0</small></div>
      <div class="note" style="font-family:var(--mono)">${fmtCoords(s.ref.lon, s.ref.lat)}</div>
      <div class="slider"><label>Zakres skali barw</label><output id="o-range">±${fmtInt(s.relRange)} m</output><input type="range" min="50" max="1500" step="50" value="${s.relRange}" data-range="relRange"></div>
      ${legend}
      ${st ? `<div class="stats" style="margin-top:16px"><div class="stat"><div class="stat__v">${fmtInt(st.frac * 100)}<small>%</small></div><div class="stat__k">terenu wyżej</div></div><div class="stat"><div class="stat__v">${fmtInt((1 - st.frac) * 100)}<small>%</small></div><div class="stat__k">terenu niżej</div></div></div>
      <div class="section"><div class="section__title"><span class="kicker">Szczyty względem punktu</span></div><div class="peaklist">${st.peaks.map(({ p, dh, dist }) => `<button class="peakrow" data-poi="${p.id}"><span class="peakrow__n">${escapeHtml(p.n)}</span><span class="peakrow__e" style="color:${dh > 0 ? '#ffb08c' : '#8fd3ff'}">${fmtSigned(dh)}</span><span class="peakrow__d">${fmtDist(dist)}</span></button>`).join('')}</div></div>` : ''}
      <p class="note">Najedź kursorem na teren – w pasku odczytu zobaczysz <b>Δh</b>, odległość i kąt wzniesienia względem punktu. Linia przerywana wyznacza poziomicę przechodzącą przez kursor (włącz/wyłącz klawiszem <b>I</b> lub w zakładce „Mapa i światło”).</p>
      <div class="btns" style="margin-top:12px"><button class="btn" data-act="vs" data-x="${s.ref.x}" data-z="${s.ref.z}">${ICON.viewshed} Widoczność stąd</button><button class="btn" data-act="pano" data-x="${s.ref.x}" data-z="${s.ref.z}">${ICON.person} Panorama</button></div>`;
  }

  /** Przypięta poziomica / pas tej samej wysokości. */
  private levelView() {
    const a = this.app, s = a.store.state;
    const ref = s.ref!;
    const dem = a.region!.dem;
    const tol = s.lens === 'band' && s.bandTol ? s.bandTol : 15;
    let inBand = 0, above = 0;
    const d = dem.data;
    for (let i = 0; i < d.length; i += 2) {
      if (Math.abs(d[i] - ref.e) <= tol) inBand++;
      if (d[i] > ref.e) above++;
    }
    const n = Math.ceil(d.length / 2);
    const same = a.region!.pois
      .filter((p) => p.t !== 'lake' && p.t !== 'viewpoint' && Math.abs(p.ele - ref.e) <= Math.max(tol, 15))
      .map((p) => ({ p, dist: haversine(ref.lon, ref.lat, p.lon, p.lat) }))
      .filter((q) => q.dist > 100)
      .sort((x, y) => x.dist - y.dist)
      .slice(0, 14);
    return `
      <div class="row row--between"><span class="kicker">${s.lens === 'band' && s.bandTol ? 'Ta sama wysokość' : 'Poziomica przez punkt'}</span><button class="btn btn--ghost btn--icon" data-act="clear-ref" title="Odepnij (Esc)">${ICON.x}</button></div>
      <div class="big" style="margin-top:6px">${fmtInt(ref.e)}<small>m n.p.m.${s.lens === 'band' && s.bandTol ? ` ± ${s.bandTol} m` : ''}</small></div>
      <div class="note" style="font-family:var(--mono)">${fmtCoords(ref.lon, ref.lat)}</div>
      <div class="stats" style="margin-top:14px">
        <div class="stat"><div class="stat__v">${fmt1((inBand / n) * 100)}<small>%</small></div><div class="stat__k">terenu w pasie ±${tol} m</div></div>
        <div class="stat"><div class="stat__v">${fmtInt((above / n) * 100)}<small>%</small></div><div class="stat__k">terenu wyżej</div></div>
      </div>
      <div class="section"><div class="section__title"><span class="kicker">Na tej samej wysokości (±${Math.max(tol, 15)} m)</span></div>
      ${same.length ? `<div class="peaklist">${same.map(({ p, dist }) => `<button class="peakrow" data-poi="${p.id}"><span class="peakrow__n">${escapeHtml(p.n)}</span><span class="peakrow__e">${fmtSigned(p.ele - ref.e)}</span><span class="peakrow__d">${fmtDist(dist)}</span></button>`).join('')}</div>` : '<div class="empty">Brak nazwanych punktów na tej wysokości.</div>'}</div>
      <p class="note">Przesuwaj kursor – w pasku odczytu zobaczysz <b>Δh</b> względem tego poziomu. Kliknięcie w inne miejsce przenosi poziom, <kbd>Esc</kbd> odpina soczewkę.</p>
      <div class="btns" style="margin-top:12px"><button class="btn" data-act="ref" data-x="${ref.x}" data-z="${ref.z}">${ICON.relative} Względna stąd</button><button class="btn" data-act="vs" data-x="${ref.x}" data-z="${ref.z}">${ICON.viewshed} Widoczność stąd</button></div>`;
  }

  private viewshedView() {
    const a = this.app, s = a.store.state;
    const eyes = [1.7, 10, 30, 100];
    const seg = `<div class="seg" style="margin:10px 0">${eyes.map((e) => `<button data-eye="${e}" class="${s.vsEye === e ? 'is-on' : ''}">${e === 1.7 ? 'oczy 1,7 m' : `${e} m`}</button>`).join('')}</div>`;
    if (!s.viewshed) {
      return `
        <div class="kicker">Analiza widoczności</div>
        <h1 class="h1">Co <em>widać</em> stąd?</h1>
        <p class="lede">Kliknij punkt obserwacji. Obliczę pole widzenia po całym modelu terenu z uwzględnieniem <b>krzywizny Ziemi i refrakcji</b>. Miejsca niewidoczne zostaną przyciemnione i zakreskowane.</p>
        <div class="kicker">Wysokość obserwatora nad terenem</div>${seg}
        <div class="empty" style="margin-top:12px">Kliknij na teren, aby wskazać obserwatora.</div>`;
    }
    const v = s.viewshed;
    const dem = a.region!.dem;
    const total = (dem.w * dem.h * dem.mpp * dem.mpp) / 1e6;
    const pois = a.region!.pois;
    const loading = !Number.isFinite(v.areaKm2);
    return `
      <div class="row row--between"><span class="kicker">Pole widzenia</span><button class="btn btn--ghost btn--icon" data-act="clear-vs" title="Wyczyść">${ICON.x}</button></div>
      ${loading ? '<div style="margin:14px 0"><span class="spinner"></span> <span class="kicker" style="margin-left:6px">liczę promienie widzenia…</span></div>' : `<div class="big big--ok" style="margin-top:6px">${fmt1(v.areaKm2)}<small>km² widocznych</small></div>
      <div class="note">${fmtInt((v.areaKm2 / total) * 100)}% modelu · obserwator na ${fmtInt(v.point.e)} m + ${v.eye.toLocaleString('pl-PL')} m</div>`}
      <div class="kicker" style="margin-top:14px">Wysokość obserwatora</div>${seg}
      ${!loading ? `<div class="section"><div class="section__title"><span class="kicker">Widoczne szczyty i schroniska (${v.peaks.length})</span></div>
      <div class="peaklist">${v.peaks.slice(0, 40).map(({ id, dist }) => { const p = pois[id]; return `<button class="peakrow" data-poi="${p.id}"><span class="peakrow__n">${escapeHtml(p.n)}</span><span class="peakrow__e">${fmtInt(p.ele)} m</span><span class="peakrow__d">${fmtDist(dist)}</span></button>`; }).join('')}</div></div>` : ''}
      <div class="btns" style="margin-top:14px"><button class="btn btn--primary" data-act="pano" data-x="${v.point.x}" data-z="${v.point.z}">${ICON.person} Stań tutaj – panorama</button></div>
      <p class="note">Model nie uwzględnia lasu ani zabudowy – to widoczność „gołej” rzeźby terenu (DEM ~${fmtInt(dem.mpp)} m).</p>`;
  }

  private measureView() {
    const a = this.app, s = a.store.state;
    const m = a.measureInfo;
    if (s.measure.length < 2 || !m) {
      return `
        <div class="kicker">Pomiar i linia wzroku</div>
        <h1 class="h1">Odległość <em>po terenie</em></h1>
        <p class="lede">Klikaj kolejne punkty. Zmierzę dystans <b>po powierzchni</b> (z uwzględnieniem wszystkich wzniesień), w rzucie poziomym i w linii prostej. Dla dwóch punktów sprawdzę też, czy <b>widać jeden z drugiego</b>.</p>
        <div class="empty">${s.measure.length ? 'Kliknij drugi punkt.' : 'Kliknij pierwszy punkt na terenie.'}</div>`;
    }
    const pts = s.measure;
    const vis = m.visible;
    return `
      <div class="row row--between"><span class="kicker">Pomiar · ${pts.length} pkt</span><span class="btns"><button class="btn btn--ghost btn--icon" data-act="undo-m" title="Cofnij punkt">${ICON.reset}</button><button class="btn btn--ghost btn--icon" data-act="clear-m" title="Wyczyść">${ICON.trash}</button></span></div>
      <div class="big" style="margin-top:6px">${fmtDist(m.surface)}<small>po terenie</small></div>
      <div class="metrics" style="margin-top:14px">
        <div><div class="metric__k">W rzucie poziomym</div><div class="metric__v">${fmtDist(m.horiz)}</div></div>
        <div><div class="metric__k">Różnica</div><div class="metric__v">+${fmt1(((m.surface - m.horiz) / Math.max(1, m.horiz)) * 100)}%</div></div>
        <div><div class="metric__k">Linia prosta A→B (3D)</div><div class="metric__v">${fmtDist(m.straight)}</div></div>
        <div><div class="metric__k">Przewyższenie A→B</div><div class="metric__v">${fmtSigned(m.dh)}</div></div>
        <div><div class="metric__k">Azymut A→B</div><div class="metric__v">${fmtInt(m.bearing)}° <small>${compassDir(m.bearing)}</small></div></div>
        <div><div class="metric__k">Kąt wzniesienia</div><div class="metric__v">${fmt1(m.angle)}°</div></div>
      </div>
      ${pts.length === 2 && vis != null ? `<div class="section"><div class="kicker">Linia wzroku (oczy 1,7 m, krzywizna Ziemi)</div><div class="big ${vis ? 'big--ok' : 'big--signal'}" style="font-size:34px;margin-top:6px">${vis ? 'Widać' : 'Nie widać'}</div><p class="note">${vis ? 'Z punktu A widać punkt B – teren nie przesłania linii wzroku.' : 'Teren przesłania linię wzroku – miejsce przeszkody zaznaczono na profilu.'}</p></div>` : ''}
      <p class="note">Prawy klik na mapie → „Pomiar stąd” rozpoczyna nowy pomiar.</p>`;
  }

  // ======================================================================= MAPA
  private mapTab() {
    const a = this.app, s = a.store.state;
    const dem = a.region!.dem;
    const times = sunTimes(s.day, dem.centerLat, dem.centerLon);
    const legend = this.styleLegend(s.style);
    const toggle = (k: keyof typeof s, label: string, key?: string) =>
      `<button class="toggle ${s[k] ? 'is-on' : ''}" data-toggle="${k}"><i></i>${label}${key ? `<kbd>${key}</kbd>` : ''}</button>`;
    return `
      <div class="kicker">Styl terenu</div>
      <div class="styles" style="margin-top:10px">${STYLES.map((st) => `<button class="style-card ${s.style === st.id ? 'is-on' : ''}" style="--sw:${st.sw}" data-style="${st.id}"><span>${st.name}</span></button>`).join('')}</div>
      ${s.style === 'satellite' ? `<p class="note">${a.tiles ? 'Zdjęcia lotnicze GUGiK / ZBGIS / ČÚZK doczytywane kaflami wokół kamery (do ~0,8 m/px); w tle mozaika Sentinel‑2.' : 'Mozaika satelitarna Sentinel‑2 (10 m).'}${a.satProgress < 1 ? ` <span class="spinner"></span> ${fmtInt(a.satProgress * 100)}%` : ''}</p>` : ''}
      ${legend}
      <div class="section">
        <div class="section__title"><span class="kicker">Warstwy</span></div>
        <div class="toggles">
          ${toggle('trails', 'Szlaki', 'T')}
          ${toggle('labels', 'Nazwy', 'L')}
          ${toggle('contours', 'Poziomice', 'C')}
          ${toggle('shadows', 'Cienie rzucane')}
          ${toggle('snow', 'Śnieg sezonowy')}
          ${toggle('grid', 'Siatka 1 km')}
        </div>
        <div class="slider"><label>Przewyższenie pionowe</label><output id="o-exag">×${fmt1(s.exag)}</output><input type="range" min="1" max="3" step="0.1" value="${s.exag}" data-range="exag"></div>
      </div>
      <div class="section">
        <div class="section__title"><span class="kicker">Słońce i pora roku</span><input type="date" value="${s.day}" data-date></div>
        <div class="sunbar" id="sunbar"><canvas></canvas></div>
        <div class="sunmeta"><span>wschód <b>${fmtHour(times.rise)}</b></span><span>południe <b>${fmtHour(times.noon)}</b> · ${fmtInt(times.maxAlt)}°</span><span>zachód <b>${fmtHour(times.set)}</b></span></div>
        <div class="row row--between" style="margin-top:10px">
          <div class="big" style="font-size:34px" id="o-hour">${fmtHour(s.hour)}</div>
          <div class="btns">
            <button class="btn btn--icon" data-act="daylapse" title="Animacja dnia">${this.playTimer ? ICON.pause : ICON.play}</button>
            <button class="btn" data-act="golden">Złota godzina</button>
            <button class="btn" data-act="now">Teraz</button>
          </div>
        </div>
        <p class="note" id="o-sun"></p>
      </div>`;
  }

  private gfxTab() {
    const a = this.app;
    const dem = a.region!.dem;
    return `${this.gfxSection()}
      <div class="section">
        <div class="section__title"><span class="kicker">Źródła danych</span></div>
        <p class="note" style="margin-top:0">${a.tiles ? `Model terenu: <b>LiDAR</b> – NMT <b>GUGiK</b> (PL)${a.region!.def.hd?.lidar.includes('cz') ? ' i DMR 5G <b>ČÚZK</b> (CZ)' : ''}, kafle do ~${a.tiles.maxZ >= 15 ? 3 : 6} m; poza zasięgiem – <b>Terrarium</b>. Ortofoto: <b>GUGiK</b>, <b>ZBGIS</b>, <b>ČÚZK</b>.` : `Model terenu: <b>Terrarium</b> (Mapzen / AWS Open Data; SRTM, EU‑DEM), siatka ~${fmtInt(dem.mpp)} m.`} Szlaki, szczyty, schroniska: <b>© OpenStreetMap</b> (ODbL). Obraz satelitarny: <b>Sentinel‑2 cloudless 2020 © EOX</b> (CC BY‑NC‑SA 4.0). Czas przejścia wg reguły <b>PTTK</b> (15 min/km + 1 min/10 m podejścia) lub normy <b>DIN 33466</b>; punkty <b>GOT PTTK</b> wg reguły 1 pkt/km + 1 pkt/100 m podejścia.</p>
      </div>`;
  }

  private gfxSection() {
    const a = this.app;
    const g = a.store.state.gfx;
    const presets = (Object.keys(GFX_PRESETS) as GfxPreset[])
      .map((p) => `<button data-gp="${p}" class="${g.preset === p ? 'is-on' : ''}">${GFX_PRESET_LABEL[p]}</button>`)
      .join('');
    const segs = (k: string, opts: [number, string][], cur: number) =>
      `<div class="seg seg--sm">${opts.map(([v, l]) => `<button data-gfxs="${k}:${v}" class="${cur === v ? 'is-on' : ''}">${l}</button>`).join('')}</div>`;
    const t = a.tiles;
    const full = t ? (t.maxZ >= 15 ? '3 m' : '6 m') : '';
    const tg = (k: string, label: string, on: boolean) => `<button class="toggle ${on ? 'is-on' : ''}" data-gfxt="${k}"><i></i>${label}</button>`;
    return `
      <div class="section">
        <div class="section__title"><span class="kicker">Jakość renderowania${g.preset === 'custom' ? ' · własna' : ''}</span><span class="kicker" id="o-perf" style="color:var(--teal)"></span></div>
        <div class="seg">${presets}</div>
        <div class="slider"><label>Rozdzielczość renderu</label><output>${fmtInt(g.scale * 100)}%</output><input type="range" min="0.25" max="1" step="0.05" value="${g.scale}" data-gfx="scale"></div>
        ${t ? `<div class="slider"><label>Zasięg szczegółów terenu</label><output>${fmt1(g.lod)}×</output><input type="range" min="0.3" max="2.5" step="0.1" value="${g.lod}" data-gfx="lod"></div>
        <div class="gfxrow"><span>Najdrobniejszy LiDAR</span>${segs('maxLevel', [[0, full], ...(t.maxZ >= 15 ? [[14, '6 m'] as [number, string]] : []), [13, '12 m'], [12, '25 m']], g.maxLevel >= t.maxZ ? 0 : g.maxLevel)}</div>` : ''}
        <div class="gfxrow"><span>Gęstość siatki kafla</span>${segs('mesh', [[32, '32'], [64, '64'], [128, '128']], g.mesh)}</div>
        <div class="gfxrow"><span>Zdjęcia lotnicze</span>${segs('ortho', [[256, '256'], [512, '512'], [1024, '1024 px']], g.ortho)}</div>
        <div class="slider"><label>Gęstość etykiet</label><output>${fmt1(g.labels)}×</output><input type="range" min="0.3" max="1.6" step="0.1" value="${g.labels}" data-gfx="labels"></div>
        <div class="toggles" style="margin-top:4px">
          ${tg('adaptive', 'Auto‑obniżanie', g.adaptive)}
          ${tg('fps', 'Limit 30 kl./s', g.fps === 30)}
          ${tg('detail', 'Mikrorzeźba skał', g.detail)}
        </div>
        <p class="note">Niższa rozdzielczość renderu i rzadsza siatka najbardziej odciążają kartę graficzną; limit poziomu LiDAR i mniejsze zdjęcia zmniejszają też transfer danych. Ustawienia są zapamiętywane w tej przeglądarce.</p>
      </div>`;
  }

  /** Odświeża licznik wydajności w sekcji jakości (bez przebudowy panelu). */
  syncPerf() {
    const el = this.body.querySelector('#o-perf');
    if (!el) return;
    const e = this.app.engine;
    const c = e.renderer.domElement;
    el.textContent = `${fmtInt(Math.min(240, 1 / Math.max(e.frameTime, 1 / 240)))} kl./s · ${c.width}×${c.height}`;
  }

  private styleLegend(st: Style) {
    if (st === 'slope') {
      const rows: [string, string, string][] = [
        ['#d6e6b8', 'łagodnie', '15–25°'], ['#fcdc4d', 'stromo', '25–30°'], ['#f98f1f', 'bardzo stromo', '30–35°'],
        ['#e0302a', 'lawiniasto', '35–40°'], ['#a13389', 'skrajnie', '40–45°'], ['#542e76', 'ściany', '> 45°'],
      ];
      return `<div class="legend-rows" style="margin-top:12px">${rows.map(([c, n, v]) => `<div class="legend-row"><i style="background:${c}"></i><span>${n}</span><span>${v}</span></div>`).join('')}</div><p class="note">Klasy nachylenia jak na mapach lawinowych – stoki 30–45° są najbardziej narażone na lawiny.</p>`;
    }
    if (st === 'aspect') {
      return `<div class="row" style="margin-top:12px;gap:14px"><div style="width:62px;height:62px;border-radius:50%;background:conic-gradient(from 0deg,#e87f7f,#e8d27f,#8fe87f,#7fe8d7,#7f9fe8,#c77fe8,#e87f7f);position:relative;box-shadow:inset 0 0 0 16px rgba(10,14,17,.5)"><span style="position:absolute;top:-2px;left:50%;transform:translateX(-50%);font-family:var(--mono);font-size:10px">N</span></div><p class="note" style="margin:0">Barwa – kierunek, w który opada stok; nasycenie – nachylenie. Stoki północne (czerwone) dłużej trzymają śnieg.</p></div>`;
    }
    if (st === 'hypso') {
      const cs = ['#669e66', '#8cba73', '#c2d185', '#edE39a', '#f0c985', '#dea369', '#bd7d52', '#99664f', '#a89999', '#f7f7fa'];
      return `<div class="legend">${cs.map((c) => `<i style="background:${c}"></i>`).join('')}</div><div class="legend-labels"><span>150</span><span>900</span><span>1700</span><span>2600 m</span></div>`;
    }
    if (st === 'paper') return `<p class="note">Styl mapy turystycznej: las na zielono, cieniowanie z północnego zachodu, poziomice co ${this.app.terrain?.material.uniforms.uContourInt.value ?? 50} m.</p>`;
    return `<p class="note">Piętra roślinności wyliczane z wysokości, nachylenia i ekspozycji: regiel, kosodrzewina, hale, turnie. Śnieg i barwy jesieni zależą od wybranej daty.</p>`;
  }

  // ======================================================================= ZDARZENIA
  private bind() {
    const a = this.app;
    const b = this.body;
    const num = (el: HTMLElement, k: string) => Number(el.dataset[k]);
    b.querySelectorAll<HTMLElement>('[data-poi]').forEach((el) =>
      el.addEventListener('click', () => a.selectPoi(a.region!.pois[num(el, 'poi')], true))
    );
    b.querySelectorAll<HTMLElement>('[data-alt]').forEach((el) =>
      el.addEventListener('click', () => a.setActiveRoute(num(el, 'alt')))
    );
    b.querySelectorAll<HTMLElement>('[data-rm]').forEach((el) => el.addEventListener('click', () => a.removeWaypoint(el.dataset.rm!)));
    b.querySelectorAll<HTMLElement>('[data-mode]').forEach((el) =>
      el.addEventListener('click', () => a.setRouteMode(el.dataset.mode as 'trails' | 'terrain'))
    );
    b.querySelectorAll<HTMLElement>('[data-tk]').forEach((el) => el.addEventListener('click', () => a.store.set({ timeKind: el.dataset.tk as 'pttk' | 'din' })));
    b.querySelectorAll<HTMLElement>('[data-pace]').forEach((el) => el.addEventListener('click', () => a.store.set({ pace: Number(el.dataset.pace) })));
    b.querySelectorAll<HTMLElement>('[data-eye]').forEach((el) =>
      el.addEventListener('click', () => {
        a.store.set({ vsEye: num(el, 'eye') });
        const v = a.store.state.viewshed;
        if (v) void a.runViewshed(v.point.x, v.point.z);
      })
    );
    b.querySelectorAll<HTMLElement>('[data-style]').forEach((el) =>
      el.addEventListener('click', () => a.store.set({ style: el.dataset.style as Style }))
    );
    b.querySelectorAll<HTMLElement>('[data-toggle]').forEach((el) =>
      el.addEventListener('click', () => {
        const k = el.dataset.toggle as 'trails';
        a.store.set({ [k]: !a.store.state[k] });
      })
    );
    b.querySelectorAll<HTMLInputElement>('[data-range]').forEach((el) => {
      const paint = () => el.style.setProperty('--p', `${((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100}%`);
      paint();
      el.addEventListener('input', () => {
        paint();
        const k = el.dataset.range!;
        const v = Number(el.value);
        if (k === 'exag') { a.store.set({ exag: v }); b.querySelector('#o-exag')!.textContent = `×${fmt1(v)}`; }
        if (k === 'relRange') { a.store.set({ relRange: v }); b.querySelector('#o-range')!.textContent = `±${fmtInt(v)} m`; }
        if (k === 'maxSlope') { a.store.set({ maxSlope: v }); b.querySelector('#o-slope')!.textContent = `${v}°`; }
      });
      el.addEventListener('change', () => {
        if (el.dataset.range === 'maxSlope') a.scheduleRoute(0);
        if (el.dataset.range === 'relRange') this.render(true);
      });
    });
    // jakość renderowania
    b.querySelectorAll<HTMLElement>('[data-gp]').forEach((el) =>
      el.addEventListener('click', () => {
        const p = el.dataset.gp as GfxPreset;
        a.store.set({ gfx: { preset: p, ...GFX_PRESETS[p] } });
      })
    );
    b.querySelectorAll<HTMLElement>('[data-gfxs]').forEach((el) =>
      el.addEventListener('click', () => {
        const [k, v] = el.dataset.gfxs!.split(':');
        a.store.set({ gfx: { ...a.store.state.gfx, [k]: Number(v), preset: 'custom' } as Gfx });
      })
    );
    b.querySelectorAll<HTMLElement>('[data-gfxt]').forEach((el) =>
      el.addEventListener('click', () => {
        const k = el.dataset.gfxt as 'adaptive' | 'detail' | 'fps';
        const g = a.store.state.gfx;
        const v = k === 'fps' ? (g.fps ? 0 : 30) : !g[k];
        a.store.set({ gfx: { ...g, [k]: v, preset: 'custom' } as Gfx });
      })
    );
    b.querySelectorAll<HTMLInputElement>('[data-gfx]').forEach((el) => {
      const paint = () => el.style.setProperty('--p', `${((Number(el.value) - Number(el.min)) / (Number(el.max) - Number(el.min))) * 100}%`);
      paint();
      const k = el.dataset.gfx as 'scale' | 'lod' | 'labels';
      const out = el.parentElement!.querySelector('output')!;
      el.addEventListener('input', () => {
        paint();
        const v = Number(el.value);
        out.textContent = k === 'scale' ? `${fmtInt(v * 100)}%` : `${fmt1(v)}×`;
        // w trakcie przeciągania bez zmiany presetu – panel się nie przebudowuje
        a.store.set({ gfx: { ...a.store.state.gfx, [k]: v } });
      });
      el.addEventListener('change', () => a.store.set({ gfx: { ...a.store.state.gfx, preset: 'custom' } }));
    });
    const date = b.querySelector<HTMLInputElement>('[data-date]');
    date?.addEventListener('change', () => date.value && a.store.set({ day: date.value }));
    const file = b.querySelector<HTMLInputElement>('[data-act="gpx-file"]');
    file?.addEventListener('change', () => file.files?.[0] && void a.importGpx(file.files[0]));

    b.querySelectorAll<HTMLElement>('[data-act]').forEach((el) => {
      if (el.tagName === 'INPUT') return;
      el.addEventListener('click', () => {
        const x = Number(el.dataset.x), z = Number(el.dataset.z);
        switch (el.dataset.act) {
          case 'unselect': a.clearSelection(); break;
          case 'wp': a.store.set({ tool: 'route' }); a.addWaypoint(x, z); break;
          case 'ref': a.pinLens(x, z, 'rel'); break;
          case 'vs': a.pinLens(x, z, 'vis'); break;
          case 'band': a.pinLens(x, z, 'band'); break;
          case 'pano': a.enterPanorama(x, z); break;
          case 'copy': navigator.clipboard?.writeText(el.dataset.v ?? ''); a.toast('Skopiowano współrzędne.'); break;
          case 'fit-trail': if (a.selection?.edges) a.fitEdges(a.selection.edges); break;
          case 'tool-route': a.store.set({ tool: 'route' }); break;
          case 'reverse': a.reverseRoute(); break;
          case 'clear-route': a.clearRoute(); break;
          case 'fly': a.toggleFly(); break;
          case 'fit': a.fitRoute(); break;
          case 'profile': a.store.set({ profileOpen: true }); break;
          case 'fit-gpx': a.fitRoute(a.store.state.gpx); break;
          case 'gpx': a.exportGpx(); break;
          case 'gpx-clear': a.store.set({ gpx: null }); break;
          case 'link': a.shareLink(); break;
          case 'shot': a.screenshot(); break;
          case 'clear-ref': a.unpinLens(); break;
          case 'clear-vs': a.unpinLens(); break;
          case 'clear-m': a.clearMeasure(); break;
          case 'undo-m': a.store.set({ measure: a.store.state.measure.slice(0, -1) }); break;
          case 'now': {
            const now = new Date();
            const f = new Intl.DateTimeFormat('sv-SE', { timeZone: 'Europe/Warsaw', hour: '2-digit', minute: '2-digit', year: 'numeric', month: '2-digit', day: '2-digit' });
            const parts = f.format(now).split(' ');
            const [hh, mm] = parts[1].split(':').map(Number);
            a.store.set({ day: parts[0], hour: hh + mm / 60 });
            this.syncHour();
            break;
          }
          case 'golden': {
            const dem = a.region!.dem;
            const t = sunTimes(a.store.state.day, dem.centerLat, dem.centerLon);
            a.store.set({ hour: Math.max(0, t.set - 0.6) });
            this.syncHour();
            break;
          }
          case 'daylapse': this.toggleDaylapse(); break;
        }
      });
    });
    const sunbar = b.querySelector<HTMLElement>('#sunbar');
    if (sunbar) this.bindSunbar(sunbar);
    this.syncHour();
  }

  private toggleDaylapse() {
    const a = this.app;
    if (this.playTimer) {
      clearInterval(this.playTimer);
      this.playTimer = 0;
    } else {
      const dem = a.region!.dem;
      const t = sunTimes(a.store.state.day, dem.centerLat, dem.centerLon);
      let h = a.store.state.hour;
      if (h < t.rise - 0.5 || h > t.set) h = t.rise - 0.4;
      this.playTimer = window.setInterval(() => {
        h += 0.05;
        if (h > t.set + 0.6) h = t.rise - 0.4;
        a.store.set({ hour: h });
        this.syncHour();
      }, 50);
    }
    this.render(true);
  }

  private bindSunbar(el: HTMLElement) {
    const set = (e: PointerEvent) => {
      const r = el.getBoundingClientRect();
      const h = Math.max(0, Math.min(23.99, ((e.clientX - r.left) / r.width) * 24));
      this.app.store.set({ hour: Math.round(h * 12) / 12 });
      this.syncHour();
    };
    el.addEventListener('pointerdown', (e) => {
      el.setPointerCapture(e.pointerId);
      set(e);
      const mv = (ev: PointerEvent) => set(ev);
      el.addEventListener('pointermove', mv);
      el.addEventListener('pointerup', () => el.removeEventListener('pointermove', mv), { once: true });
    });
  }

  /** Odświeża wykres dnia i opis Słońca bez przebudowy panelu. */
  syncHour() {
    const a = this.app;
    const s = a.store.state;
    const out = this.body.querySelector('#o-hour');
    if (out) out.textContent = fmtHour(s.hour);
    const bar = this.body.querySelector<HTMLElement>('#sunbar');
    if (!bar || !a.region) return;
    const cv = bar.querySelector('canvas')!;
    const W = bar.clientWidth, H = bar.clientHeight;
    const dpr = Math.min(2, devicePixelRatio);
    cv.width = W * dpr;
    cv.height = H * dpr;
    const c = cv.getContext('2d')!;
    c.scale(dpr, dpr);
    const dem = a.region.dem;
    const alts: number[] = [];
    for (let i = 0; i <= 96; i++) alts.push(sunPosition(warsawDate(s.day, i / 4), dem.centerLat, dem.centerLon).altitude);
    const maxA = Math.max(20, ...alts);
    const y0 = H * 0.62;
    const Y = (al: number) => y0 - (al / maxA) * (y0 - 8);
    // tło: noc / dzień
    const g = c.createLinearGradient(0, 0, W, 0);
    for (let i = 0; i <= 24; i++) {
      const al = alts[i * 4];
      const col = al > 6 ? 'rgba(120,160,200,0.22)' : al > -1 ? 'rgba(240,150,90,0.25)' : al > -8 ? 'rgba(60,70,120,0.2)' : 'rgba(10,14,30,0.3)';
      g.addColorStop(i / 24, col);
    }
    c.fillStyle = g;
    c.fillRect(0, 0, W, H);
    c.strokeStyle = 'rgba(239,231,214,0.18)';
    c.beginPath();
    c.moveTo(0, y0);
    c.lineTo(W, y0);
    c.stroke();
    c.fillStyle = 'rgba(239,231,214,0.35)';
    c.font = '9px "Geist Mono", monospace';
    for (let hh = 3; hh < 24; hh += 3) {
      c.fillText(String(hh).padStart(2, '0'), (hh / 24) * W - 6, H - 5);
    }
    c.strokeStyle = '#f7c948';
    c.lineWidth = 1.6;
    c.beginPath();
    alts.forEach((al, i) => {
      const x = (i / 96) * W;
      if (i) c.lineTo(x, Y(al)); else c.moveTo(x, Y(al));
    });
    c.stroke();
    const cur = sunPosition(warsawDate(s.day, s.hour), dem.centerLat, dem.centerLon);
    const x = (s.hour / 24) * W;
    c.strokeStyle = 'rgba(255,255,255,0.6)';
    c.lineWidth = 1;
    c.beginPath();
    c.moveTo(x, 0);
    c.lineTo(x, H);
    c.stroke();
    c.fillStyle = cur.altitude > 0 ? '#fff3c4' : '#8aa0c8';
    c.beginPath();
    c.arc(x, Y(cur.altitude), 5.5, 0, Math.PI * 2);
    c.fill();
    const note = this.body.querySelector('#o-sun');
    if (note) {
      note.innerHTML = cur.altitude > 0
        ? `Słońce: azymut <b>${fmtInt(cur.azimuth)}° ${compassDir(cur.azimuth)}</b>, wysokość <b>${fmt1(cur.altitude)}°</b>. Cienie liczone z modelu terenu w czasie rzeczywistym.`
        : `Słońce <b>${fmt1(-cur.altitude)}° pod horyzontem</b> (${cur.altitude > -6 ? 'zmierzch cywilny' : cur.altitude > -12 ? 'zmierzch żeglarski' : 'noc'}).`;
    }
  }
}

