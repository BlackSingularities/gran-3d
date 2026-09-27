import './style.css';
import { App } from './app';
import { initCatalog } from './core/region';
import { loadCoverage } from './core/area';
import { Chrome, drawLoaderTopo } from './ui/chrome';
import { MapsManager } from './ui/maps';
import { Panel } from './ui/panel';

const loader = document.getElementById('loader')!;
const fill = document.getElementById('loader-fill')!;
const step = document.getElementById('loader-step')!;
drawLoaderTopo();

// zasięg danych krajowych (LiDAR, ortofoto) i stan serwera
step.textContent = 'Łączenie z serwerem';
await Promise.all([initCatalog(), loadCoverage()]);

const app = new App();
const panel = new Panel(app);
// MapsManager służy już tylko do śledzenia zadań serwera (pasek postępu danych okolicy)
const maps = new MapsManager();

const chrome = new Chrome(app, () => {}, () => {});
maps.onJob = (job, last) => chrome.setJob(job, last);
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
