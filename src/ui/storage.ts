// Okno „Dane i pamięć” wewnątrz aplikacji – używane na Androidzie, gdzie ustawienia przechowuje
// lokalny serwer (/api/settings). Wygląd i treść jak okno pierwszego uruchomienia wersji Windows.
import { fmtBytes, type DesktopInfo } from '../core/desktop';

interface Settings {
  configured: boolean;
  mode: 'keep' | 'session';
  limitGB: number;
  free: number | null;
  used: number;
  version: string;
}

const STEPS = [1, 2, 3, 5, 8, 10, 15, 20, 30, 50, 75, 100, 150, 200, 300, 500];
const API = `${import.meta.env.BASE_URL}api/settings`;

export async function fetchMobileSettings(): Promise<Settings | null> {
  try {
    const r = await fetch(API, { cache: 'no-store' });
    return r.ok ? ((await r.json()) as Settings) : null;
  } catch {
    return null;
  }
}

export class StorageDialog {
  private el: HTMLElement;
  private s: Settings;
  private mode: 'keep' | 'session';
  private limitGB: number;
  private resolve: (() => void) | null = null;

  constructor(settings: Settings) {
    this.s = settings;
    this.mode = settings.mode;
    this.limitGB = settings.limitGB;
    this.el = document.createElement('div');
    this.el.className = 'stg';
    this.el.hidden = true;
    document.getElementById('app')!.appendChild(this.el);
  }

  info(): DesktopInfo {
    return { platform: 'android', version: this.s.version, mode: this.s.mode, limitGB: this.s.limitGB, dataDir: '' };
  }

  async refresh() {
    this.s = (await fetchMobileSettings()) ?? this.s;
  }

  /** Otwiera okno; przy pierwszym uruchomieniu nie da się go zamknąć bez wyboru. */
  async open(): Promise<void> {
    await this.refresh();
    this.mode = this.s.mode;
    this.limitGB = this.s.limitGB;
    this.render();
    this.el.hidden = false;
    return new Promise((r) => (this.resolve = r));
  }

  private close() {
    this.el.hidden = true;
    const r = this.resolve;
    this.resolve = null;
    r?.();
  }

  private render() {
    const first = !this.s.configured;
    const idx = STEPS.reduce((best, v, i) => (Math.abs(v - this.limitGB) < Math.abs(STEPS[best] - this.limitGB) ? i : best), 0);
    const free = this.s.free;
    this.el.innerHTML = `
      <div class="stg__box" role="dialog" aria-modal="true">
        <div class="kicker">${first ? 'Pierwsze uruchomienie' : 'Ustawienia'}${this.s.version ? ` · wersja ${this.s.version}` : ''}</div>
        <div class="h1">${first ? 'Gdzie trzymać dane map?' : 'Dane i pamięć'}</div>
        <p class="stg__lead">Grań pobiera kafle terenu LiDAR, zdjęcia lotnicze, szlaki i dane okolic w miarę oglądania mapy. Wybierz, czy mają zostawać w pamięci telefonu, czy znikać za każdym razem.${first ? ' Zmienisz to później w zakładce „Jakość”.' : ''}</p>
        <div class="stg__choices">
          <button class="stg__choice ${this.mode === 'keep' ? 'is-on' : ''}" data-mode="keep"><i></i><b>Zachowuj dane</b>
            <span>Odwiedzone okolice wczytują się od razu, zaznaczone obszary działają bez internetu. Po przekroczeniu limitu najstarsze kafle są usuwane automatycznie.</span></button>
          <button class="stg__choice ${this.mode === 'session' ? 'is-on' : ''}" data-mode="session"><i></i><b>Usuwaj za każdym razem</b>
            <span>Kafle i dane map są usuwane przy zamknięciu i przy każdym uruchomieniu aplikacji. Teren pobierany jest od nowa.</span></button>
        </div>
        <div class="stg__sec">
          <div class="stg__row"><label>Maksymalna pojemność danych</label><output>${this.limitGB}<small>GB</small></output></div>
          <input type="range" min="0" max="${STEPS.length - 1}" step="1" value="${idx}" style="--p:${(idx / (STEPS.length - 1)) * 100}%" data-stg="limit">
          <div class="stg__ticks"><span>1 GB</span><span>10 GB</span><span>50 GB</span><span>500 GB</span></div>
          <p class="note">${this.mode === 'session' ? 'Limit obowiązuje w trakcie jednej sesji.' : 'Kafle, zdjęcia i dane okolic nie przekroczą tej wartości.'}${free != null ? ` Wolne miejsce: <b>${fmtBytes(free)}</b>.` : ''}${free != null && this.limitGB * 1024 ** 3 > free ? ' <span style="color:var(--warn)">Limit jest większy niż wolne miejsce.</span>' : ''}</p>
        </div>
        ${first ? '' : `<div class="stg__sec stg__usage"><div><div class="stg__row" style="margin:0"><label>Zajęte teraz</label></div><output>${fmtBytes(this.s.used)}</output></div><button class="btn stg__danger" data-stg="clear">Wyczyść dane teraz</button></div>`}
        <div class="stg__foot">
          ${first ? '' : '<button class="btn" data-stg="cancel">Anuluj</button>'}
          <button class="btn btn--primary" data-stg="ok">${first ? 'Rozpocznij' : 'Zapisz'}</button>
        </div>
      </div>`;
    this.el.querySelectorAll<HTMLElement>('[data-mode]').forEach((b) =>
      b.addEventListener('click', () => {
        this.mode = b.dataset.mode as 'keep' | 'session';
        this.render();
      })
    );
    this.el.querySelector<HTMLInputElement>('[data-stg="limit"]')!.addEventListener('input', (e) => {
      this.limitGB = STEPS[Number((e.target as HTMLInputElement).value)];
      this.render();
    });
    this.el.querySelector('[data-stg="cancel"]')?.addEventListener('click', () => this.close());
    this.el.querySelector('[data-stg="clear"]')?.addEventListener('click', async (e) => {
      const b = e.currentTarget as HTMLButtonElement;
      b.disabled = true;
      b.textContent = 'Usuwanie…';
      await fetch(API, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ clear: true }) });
      await this.refresh();
      location.reload();
    });
    this.el.querySelector('[data-stg="ok"]')!.addEventListener('click', async () => {
      await fetch(API, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ mode: this.mode, limitGB: this.limitGB }) });
      await this.refresh();
      this.close();
    });
  }
}
