import { escapeHtml } from '../app';
import { fmtInt } from '../core/geo';
import { CATALOG, initCatalog, regionArea, REGIONS, SERVER_MODE, STATUS, type RegionDef } from '../core/region';
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

interface ApiStatus {
  admin: boolean;
  job: Job | null;
  queue: { id: string; level: string }[];
  last: Job | null;
}

const TOKEN_KEY = 'gran.adminToken';
const COUNTRY: Record<string, string> = { PL: 'Polska', SK: 'Słowacja', CZ: 'Czechy', UA: 'Ukraina' };
const LIDAR: Record<string, string> = { pl: 'GUGiK', cz: 'ČÚZK' };

/** Szacunkowe rozmiary pakietów (MB) – z doświadczeń dla gotowych pasm. */
function estimate(r: RegionDef) {
  const { km2 } = regionArea(r);
  const base = 1 + km2 * 0.0065;
  const hd = r.hd ? km2 * (r.hd.zoom >= 15 ? 0.075 : 0.022) : 0;
  return { km2, base, hd: base + hd };
}

const mb = (v: number) => (v < 10 ? v.toFixed(1).replace('.', ',') : fmtInt(v)) + ' MB';

/**
 * Menedżer map: wybór pasm do pobrania, postęp wypiekania na serwerze, usuwanie.
 * Przy braku pobranych pasm działa jako ekran powitalny.
 */
export class MapsManager {
  private el: HTMLElement;
  private timer = 0;
  private api: ApiStatus | null = null;
  private onboarding = false;
  private message = '';
  private lastDone = '';
  onInstalled: (id: string, first: boolean) => void = () => {};
  onChange: () => void = () => {};
  onOpen: (id: string) => void = () => {};

  constructor() {
    this.el = document.createElement('div');
    this.el.id = 'maps';
    this.el.className = 'maps';
    this.el.hidden = true;
    document.getElementById('app')!.appendChild(this.el);
    this.el.addEventListener('click', (e) => this.onClick(e));
  }

  get isOpen() {
    return !this.el.hidden;
  }

  open(onboarding = false) {
    this.onboarding = onboarding;
    this.el.hidden = false;
    this.el.classList.toggle('is-onboarding', onboarding);
    void this.refresh();
    clearInterval(this.timer);
    this.timer = window.setInterval(() => void this.refresh(), 1200);
  }

  close() {
    if (this.onboarding && !REGIONS.length) return;
    this.el.hidden = true;
    clearInterval(this.timer);
  }

  private token() {
    try { return localStorage.getItem(TOKEN_KEY) ?? ''; } catch { return ''; }
  }

  private async call(method: string, url: string, body?: unknown) {
    const res = await fetch(`${import.meta.env.BASE_URL}${url}`, {
      method,
      headers: { 'Content-Type': 'application/json', ...(this.token() ? { Authorization: `Bearer ${this.token()}` } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    if (res.status === 401) {
      this.message = 'Ta instancja jest chroniona – podaj hasło administratora (GRAN_ADMIN_TOKEN).';
      this.render();
      return false;
    }
    if (!res.ok) {
      const j = await res.json().catch(() => ({}));
      this.message = (j as { error?: string }).error ?? `Błąd ${res.status}`;
      this.render();
      return false;
    }
    this.message = '';
    return true;
  }

  private async refresh() {
    const before = new Set(REGIONS.map((r) => r.id));
    await initCatalog();
    if (SERVER_MODE) {
      try {
        this.api = await (await fetch(`${import.meta.env.BASE_URL}api/status`, { cache: 'no-store' })).json();
      } catch {
        this.api = null;
      }
    }
    // świeżo zakończona instalacja
    const last = this.api?.last;
    if (last && last.state === 'done' && `${last.id}:${last.level}` !== this.lastDone) {
      const fresh = !this.lastDone;
      this.lastDone = `${last.id}:${last.level}`;
      if (!fresh || !before.has(last.id)) this.onInstalled(last.id, before.size === 0);
    }
    const now = new Set(REGIONS.map((r) => r.id));
    if (before.size !== now.size || [...now].some((id) => !before.has(id))) this.onChange();
    if (!this.el.hidden) this.render();
  }

  private card(r: RegionDef) {
    const st = STATUS.get(r.id);
    const est = estimate(r);
    const job = this.api?.job?.id === r.id ? this.api.job : null;
    const queued = this.api?.queue.some((q) => q.id === r.id);
    const err = this.api?.last?.id === r.id && this.api.last.state === 'error' ? this.api.last.error : null;
    const lidar = r.hd?.lidar.map((l) => LIDAR[l]).filter(Boolean).join(' + ');
    const res = r.hd ? (r.hd.zoom >= 15 ? '3 m' : '6 m') : '';
    let status = '';
    let actions = '';
    if (job) {
      status = `<div class="mcard__bar"><i style="width:${(job.progress * 100).toFixed(1)}%"></i></div><div class="mcard__job"><span class="spinner"></span>${escapeHtml(job.label)} · ${fmtInt(job.progress * 100)}%</div>`;
      actions = `<button class="btn btn--ghost" data-cancel="${r.id}">Anuluj</button>`;
    } else if (queued) {
      status = '<div class="mcard__job">W kolejce…</div>';
      actions = `<button class="btn btn--ghost" data-cancel="${r.id}">Anuluj</button>`;
    } else if (st?.installed) {
      status = `<div class="mcard__ok">${ICON.pin}<b>${st.level === 'hd' ? `LiDAR ${res}` : 'Podstawowy'}</b>${st.bytes ? ` · ${mb(st.bytes / 1048576)}` : ''}</div>`;
      actions = `${!this.onboarding ? `<button class="btn btn--primary" data-open="${r.id}">Otwórz</button>` : ''}
        ${SERVER_MODE && st.level !== 'hd' && r.hd ? `<button class="btn" data-install="${r.id}:hd">Ulepsz do LiDAR ${res}</button>` : ''}
        ${SERVER_MODE ? `<button class="btn btn--ghost btn--icon" title="Usuń pasmo" data-remove="${r.id}">${ICON.trash}</button>` : ''}`;
    } else if (SERVER_MODE) {
      actions = `<button class="btn" data-install="${r.id}:base">Podstawowy · ~${mb(est.base)}</button>
        ${r.hd ? `<button class="btn btn--signal" data-install="${r.id}:hd">LiDAR ${res} · ~${mb(est.hd)}</button>` : ''}`;
    } else {
      status = '<div class="mcard__job">niepobrane</div>';
    }
    return `<div class="mcard ${st?.installed ? 'is-installed' : ''} ${job ? 'is-busy' : ''}">
      <div class="mcard__main">
        <div class="mcard__name">${escapeHtml(r.name)}</div>
        <div class="mcard__sub">${escapeHtml(r.subtitle)}</div>
        <div class="mcard__meta">
          ${r.peak ? `<span>▲ ${escapeHtml(r.peak.name)} ${fmtInt(r.peak.ele)} m</span>` : ''}
          <span>${fmtInt(est.km2)} km²</span>
          <span>${(r.countries ?? []).map((c) => COUNTRY[c] ?? c).join(' · ')}</span>
          ${lidar ? `<span class="mcard__lidar">LiDAR: ${lidar}</span>` : ''}
        </div>
        ${status}
        ${err ? `<div class="mcard__err">Błąd: ${escapeHtml(err)}</div>` : ''}
      </div>
      <div class="mcard__actions">${actions}</div>
    </div>`;
  }

  render() {
    const groups = new Map<string, RegionDef[]>();
    for (const r of CATALOG) {
      const g = r.group ?? 'Inne';
      if (!groups.has(g)) groups.set(g, []);
      groups.get(g)!.push(r);
    }
    const installed = REGIONS.length;
    const total = [...STATUS.values()].reduce((s, v) => s + (v.bytes ?? 0), 0);
    const scroll = this.el.querySelector('.maps__list')?.scrollTop ?? 0;
    const needToken = this.api?.admin && !this.token();
    this.el.innerHTML = `<div class="maps__box grain">
      ${!this.onboarding || installed ? `<button class="btn btn--icon maps__close" data-close>${ICON.x}</button>` : ''}
      <div class="kicker">${this.onboarding ? 'Pierwsze uruchomienie' : 'Menedżer map'}</div>
      <h1 class="h1">${this.onboarding ? 'Wybierz góry do pobrania' : 'Pasma górskie'}</h1>
      <p class="lede">${
        SERVER_MODE
          ? `Dane każdego pasma pobierane są bezpośrednio ze źródeł (OpenStreetMap, model terenu Terrarium, LiDAR GUGiK/ČÚZK) i zapisywane na tym serwerze. <b>Podstawowy</b> pakiet (model ~25 m) wystarcza do tras i analiz; <b>LiDAR</b> dodaje teren w rozdzielczości 3–6 m po stronie polskiej (i czeskiej).`
          : `Ta instancja działa bez serwera menedżera. Pasma dodasz poleceniem <kbd>npm run bake:all</kbd> albo uruchamiając aplikację przez <kbd>npm start</kbd>.`
      }</p>
      <div class="maps__summary"><span><b>${installed}</b> z ${CATALOG.length} pasm pobranych</span>${total ? `<span>${mb(total / 1048576)} na dysku</span>` : ''}${this.api?.queue.length ? `<span>${this.api.queue.length} w kolejce</span>` : ''}</div>
      ${needToken || this.message ? `<div class="maps__token">${this.message ? `<span>${escapeHtml(this.message)}</span>` : ''}${needToken || this.message.includes('hasło') ? `<input type="password" placeholder="Hasło administratora" data-token><button class="btn" data-savetoken>Zapisz</button>` : ''}</div>` : ''}
      <div class="maps__list">
        ${[...groups.entries()].map(([g, list]) => `<div class="maps__group"><div class="kicker">${escapeHtml(g)}</div>${list.map((r) => this.card(r)).join('')}</div>`).join('')}
      </div>
      <p class="note">Rozmiary są orientacyjne; pobieranie LiDAR trwa kilka–kilkanaście minut (serwer GUGiK udostępnia dane fragmentami). Dane: © OpenStreetMap (ODbL), Terrarium/AWS, © GUGiK, © ČÚZK.</p>
    </div>`;
    const list = this.el.querySelector('.maps__list');
    if (list) list.scrollTop = scroll;
  }

  private async onClick(e: MouseEvent) {
    const t = e.target as HTMLElement;
    if (t === this.el || t.closest('[data-close]')) return this.close();
    const b = t.closest<HTMLElement>('[data-install],[data-remove],[data-cancel],[data-open],[data-savetoken]');
    if (!b) return;
    if (b.dataset.savetoken != null) {
      const v = this.el.querySelector<HTMLInputElement>('[data-token]')?.value ?? '';
      try { localStorage.setItem(TOKEN_KEY, v); } catch { /* ignoruj */ }
      this.message = '';
      this.render();
      return;
    }
    if (b.dataset.open) {
      this.el.hidden = true;
      clearInterval(this.timer);
      this.onOpen(b.dataset.open);
      return;
    }
    if (b.dataset.install) {
      const [id, level] = b.dataset.install.split(':');
      if (await this.call('POST', 'api/install', { id, level })) await this.refresh();
    }
    if (b.dataset.cancel && (await this.call('POST', 'api/cancel', { id: b.dataset.cancel }))) await this.refresh();
    if (b.dataset.remove) {
      const r = CATALOG.find((x) => x.id === b.dataset.remove);
      if (!confirm(`Usunąć dane pasma „${r?.name}” z serwera?`)) return;
      if (await this.call('DELETE', `api/regions/${b.dataset.remove}`)) await this.refresh();
    }
  }
}
