import './style.css';
import { App } from './app';
import { initCatalog, REGIONS } from './core/region';
import { Chrome, drawLoaderTopo } from './ui/chrome';
import { MapsManager } from './ui/maps';
import { Panel } from './ui/panel';

const loader = document.getElementById('loader')!;
const fill = document.getElementById('loader-fill')!;
const step = document.getElementById('loader-step')!;
const regionLabel = document.getElementById('loader-region')!;
drawLoaderTopo();

// katalog pasm i lista pobranych (z serwera albo z plików)
step.textContent = 'Katalog pasm';
await initCatalog();

const app = new App();
const panel = new Panel(app);
const maps = new MapsManager();

async function switchRegion(id: string) {
  const def = REGIONS.find((r) => r.id === id) ?? REGIONS[0];
  if (!def) return;
  regionLabel.textContent = def.name;
  loader.classList.remove('is-done');
  fill.style.width = '0%';
  try {
    await app.loadRegion(def.id, (f, label) => {
      fill.style.width = `${Math.round(f * 100)}%`;
      step.textContent = label;
    });
  } catch (e) {
    step.textContent = `Błąd: ${(e as Error).message}`;
    throw e;
  }
  setTimeout(() => loader.classList.add('is-done'), 250);
  document.getElementById('app')!.classList.add('is-ready');
  panel.render(true);
}

const chrome = new Chrome(app, (id) => void switchRegion(id), () => maps.open(false));
app.switchRegion = switchRegion;

// menedżer map: nowe pasmo gotowe → otwórz (pierwsze) albo zaproponuj przejście
maps.onChange = () => chrome.refreshRegions();
maps.onInstalled = (id, first) => {
  if (first || !app.region) {
    maps.close();
    void switchRegion(id).then(() => chrome.updateScale());
    return;
  }
  const r = REGIONS.find((x) => x.id === id);
  if (r) app.toast(`Pobrano pasmo: ${r.name} – jest już w menu regionów.`);
};
maps.onOpen = (id) => void switchRegion(id).then(() => chrome.updateScale());

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

if (REGIONS.length) {
  void switchRegion(app.store.state.regionId).then(() => chrome.updateScale());
} else {
  // brak pobranych pasm – ekran wyboru zamiast mapy
  loader.classList.add('is-done');
  maps.open(true);
}

// dostęp diagnostyczny z konsoli
(window as unknown as { gran: App }).gran = app;
