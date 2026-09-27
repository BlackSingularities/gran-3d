import './style.css';
import { App } from './app';
import { REGIONS } from './core/region';
import { Chrome, drawLoaderTopo } from './ui/chrome';
import { Panel } from './ui/panel';

const loader = document.getElementById('loader')!;
const fill = document.getElementById('loader-fill')!;
const step = document.getElementById('loader-step')!;
const regionLabel = document.getElementById('loader-region')!;
drawLoaderTopo();

const app = new App();
const panel = new Panel(app);

async function switchRegion(id: string) {
  const def = REGIONS.find((r) => r.id === id) ?? REGIONS[0];
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

const chrome = new Chrome(app, (id) => void switchRegion(id));
app.switchRegion = switchRegion;

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

void switchRegion(app.store.state.regionId).then(() => chrome.updateScale());

// dostęp diagnostyczny z konsoli
(window as unknown as { gran: App }).gran = app;
