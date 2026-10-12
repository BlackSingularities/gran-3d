// Android: sprawdzanie nowych wydań (lokalny serwer pyta GitHub Releases) i propozycja aktualizacji.
// „Aktualizuj” pobiera APK natywnie (MainActivity.installUpdate) i otwiera systemowy instalator.
// Na Windows to samo robi powłoka Electron (electron-updater) – tu nic się nie dzieje.
import { android } from '../core/desktop';

interface Release {
  current: string;
  latest: string | null;
  newer: boolean;
  apk?: string | null;
  apkSize?: number | null;
  page?: string;
  error?: string;
}

const SKIP_KEY = 'gran.update.skip';
const API = `${import.meta.env.BASE_URL}api/update`;

export class UpdateBanner {
  private el: HTMLElement;
  private rel: Release | null = null;

  constructor(private toast: (msg: string, err?: boolean) => void) {
    this.el = document.createElement('div');
    this.el.className = 'upd';
    this.el.hidden = true;
    document.getElementById('app')!.appendChild(this.el);
    (window as unknown as { __granUpdateProgress: (p: number) => void }).__granUpdateProgress = (p) => this.progress(p);
  }

  /** Automatyczne sprawdzenie po starcie (z pominięciem wersji odłożonej „na później” przez 3 dni). */
  start() {
    if (!android) return;
    setTimeout(() => void this.check(false), 8000);
    setInterval(() => void this.check(false), 6 * 3600 * 1000);
  }

  async check(manual: boolean): Promise<Release | null> {
    try {
      this.rel = (await (await fetch(`${API}${manual ? '?force' : ''}`, { cache: 'no-store' })).json()) as Release;
    } catch {
      if (manual) this.toast('Nie udało się sprawdzić aktualizacji.', true);
      return null;
    }
    const r = this.rel;
    if (r.error) {
      if (manual) this.toast('Nie udało się sprawdzić aktualizacji – brak połączenia z GitHub.', true);
    } else if (!r.newer) {
      if (manual) this.toast(`Masz najnowszą wersję (${r.current}).`);
    } else if (manual || !this.skipped(r.latest!)) this.show(r);
    return r;
  }

  private skipped(v: string) {
    try {
      const s = JSON.parse(localStorage.getItem(SKIP_KEY) ?? 'null') as { v: string; until: number } | null;
      return !!s && s.v === v && Date.now() < s.until;
    } catch {
      return false;
    }
  }

  private show(r: Release) {
    const size = r.apkSize ? ` · ${Math.round(r.apkSize / 2 ** 20)} MB` : '';
    this.el.innerHTML = `
      <div class="kicker">Aktualizacja</div>
      <div class="upd__title">Dostępna jest wersja ${r.latest}</div>
      <div class="upd__sub">Masz ${r.current}${size}. Dane map i ustawienia zostaną zachowane.</div>
      <div class="upd__bar" hidden><i></i></div>
      <div class="upd__btns">
        <button class="btn" data-u="later">Później</button>
        <button class="btn btn--primary" data-u="go">Aktualizuj</button>
      </div>`;
    this.el.hidden = false;
    this.el.querySelector('[data-u="later"]')!.addEventListener('click', () => {
      try {
        localStorage.setItem(SKIP_KEY, JSON.stringify({ v: r.latest, until: Date.now() + 3 * 86400 * 1000 }));
      } catch {
        /* brak localStorage */
      }
      this.el.hidden = true;
    });
    this.el.querySelector('[data-u="go"]')!.addEventListener('click', () => {
      if (!android) return;
      if (r.apk && android.installUpdate) {
        android.installUpdate(r.apk, r.latest!);
        this.progress(0);
      } else if (r.page) android.openExternal(r.page);
    });
  }

  /** Postęp pobierania z warstwy natywnej: 0–1, 2 = instalator otwarty, −1 = błąd. */
  private progress(p: number) {
    const bar = this.el.querySelector<HTMLElement>('.upd__bar');
    const go = this.el.querySelector<HTMLButtonElement>('[data-u="go"]');
    if (!bar || !go) return;
    if (p < 0) {
      bar.hidden = true;
      go.disabled = false;
      go.textContent = 'Spróbuj ponownie';
      this.toast('Nie udało się pobrać aktualizacji.', true);
      return;
    }
    if (p >= 2) {
      bar.hidden = true;
      go.disabled = false;
      go.textContent = 'Zainstaluj';
      return;
    }
    bar.hidden = false;
    go.disabled = true;
    go.textContent = `Pobieranie ${Math.round(p * 100)}%`;
    bar.querySelector('i')!.style.width = `${Math.max(2, p * 100)}%`;
  }
}
