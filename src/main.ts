// fonty dołączone do aplikacji (bez zależności od Google Fonts – działa też offline)
import '@fontsource-variable/bricolage-grotesque/opsz.css';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import '@fontsource/instrument-serif/400.css';
import '@fontsource/instrument-serif/400-italic.css';
import './style.css';
import { App } from './app';
import { initCatalog } from './core/region';
import { loadCoverage } from './core/area';
import { Chrome, drawLoaderTopo } from './ui/chrome';
import { MapsManager } from './ui/maps';
import { Panel } from './ui/panel';
import { android, setStorageHost } from './core/desktop';
import { fetchMobileSettings, StorageDialog } from './ui/storage';
import { UpdateBanner } from './ui/update';

const loader = document.getElementById('loader')!;
const fill = document.getElementById('loader-fill')!;
const step = document.getElementById('loader-step')!;
drawLoaderTopo();

// zasięg danych krajowych (LiDAR, ortofoto) i stan serwera
step.textContent = 'Łączenie z serwerem';
const [, , mobile] = await Promise.all([initCatalog(), loadCoverage(), fetchMobileSettings()]);

// Android: ustawienia przechowywania danych w oknie aplikacji; przy pierwszym starcie wybór jest wymagany
const updates = new UpdateBanner((m, err) => app.toast(m, err));
if (mobile) {
  const dialog = new StorageDialog(mobile);
  setStorageHost({
    info: async () => (await dialog.refresh(), dialog.info()),
    openSettings: () => dialog.open(),
    checkUpdates: async () => {
      const r = await updates.check(true);
      return { state: !r || r.error ? 'error' : r.newer ? 'available' : 'latest', current: r?.current ?? '', latest: r?.latest ?? undefined };
    },
  });
  if (!mobile.configured) {
    loader.classList.add('is-done');
    await dialog.open();
    loader.classList.remove('is-done');
  }
}
// Android: linki zewnętrzne w przeglądarce systemowej
if (android) {
  const bridge = android;
  document.addEventListener('click', (e) => {
    const a = (e.target as HTMLElement).closest?.('a[target="_blank"]') as HTMLAnchorElement | null;
    if (!a) return;
    e.preventDefault();
    bridge.openExternal(a.href);
  });
}

const app = new App();
if (import.meta.env.DEV) (window as unknown as { __gran: App }).__gran = app;
const panel = new Panel(app);
// MapsManager służy już tylko do śledzenia zadań serwera (pasek postępu danych okolicy)
const maps = new MapsManager();

const chrome = new Chrome(app, () => {}, () => {});
maps.onJob = (job, last, extra) => chrome.setJob(job, last, extra);
void maps.poll();

let scaleTick = 0;
let statTick = 0;
app.listeners.push(() => {
  panel.render();
  chrome.update();
});
app.engine.onFrame(() => {
  if (!app.region) return;
  if (++statTick % 20 === 0) {
    chrome.updateTileStat();
    panel.syncPerf();
  }
  if (app.engine.isMoving) {
    chrome.update();
    if (++scaleTick % 6 === 0) chrome.updateScale();
    app.scheduleHash();
  }
});
app.store.on((s, ch) => {
  if (ch.has('hour')) panel.syncHour();
  if (ch.has('panelOpen')) document.getElementById('app')!.classList.toggle('panel-closed', !s.panelOpen);
});
document.getElementById('app')!.classList.toggle('panel-closed', !app.store.state.panelOpen);

// start od razu nad mapą – teren strumieniowany, dane okolicy dociągną się same
try {
  await app.start(app.startView(), (f, label) => {
    fill.style.width = `${Math.round(f * 100)}%`;
    step.textContent = label;
  });
} catch (e) {
  step.textContent = `Błąd: ${(e as Error).message}`;
  throw e;
}
setTimeout(() => loader.classList.add('is-done'), 200);
document.getElementById('app')!.classList.add('is-ready');
panel.render(true);
chrome.updateScale();

// dostęp diagnostyczny z konsoli
(window as unknown as { gran: App }).gran = app;

// Android: propozycja aktualizacji, gdy w GitHub Releases pojawi się nowsza wersja
updates.start();
