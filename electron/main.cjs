// GRAŃ – aplikacja desktopowa (Electron).
//
// Powłoka uruchamia lokalny serwer aplikacji (server/index.mjs) jako osobny proces Node
// wbudowany w Electron, nasłuchujący wyłącznie na 127.0.0.1, i otwiera go w oknie aplikacji.
// Przy pierwszym uruchomieniu użytkownik wybiera folder na dane map i ich maksymalny rozmiar.
// Wszystko, co program pobiera (kafle terenu, zdjęcia lotnicze, szlaki, dane okolic, zaznaczone obszary),
// trafia na stałe do tego jednego folderu; po przekroczeniu limitu najdawniej używane kafle są usuwane.
// Aktualizacje: wydania z redzikowski.com (kopia GitHub Releases), instalator sprawdzany sumą SHA-512.
'use strict';

const { app, BrowserWindow, dialog, ipcMain, Menu, net, session, shell } = require('electron');
const { spawn, spawnSync } = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const netNode = require('node:net');

const APP_NAME = 'GRAŃ';
const IS_WIN = process.platform === 'win32';
const ROOT = app.getAppPath();
const PREFERRED_PORT = 47190; // stały port = stałe pochodzenie strony, więc ustawienia interfejsu przetrwają restart
const BG = '#070a0c';

// katalog ustawień z nazwą ASCII (nazwa „GRAŃ” bywa gubiona przy wyznaczaniu ścieżki)
app.setPath('userData', path.join(app.getPath('appData'), 'GRAN'));

// ------------------------------------------------------------------ jedna instancja
if (!app.requestSingleInstanceLock()) {
  app.quit();
  process.exit(0);
}

// ------------------------------------------------------------------ ustawienia
const SETTINGS_FILE = path.join(app.getPath('userData'), 'settings.json');
const LOCAL_ROOT = path.join(process.env.LOCALAPPDATA || app.getPath('userData'), 'GRAN');
const DEFAULT_DATA_DIR = path.join(LOCAL_ROOT, 'data');
const DATA_SUBDIR = 'GRAN-dane';
// dawny tryb „usuwaj przy zamknięciu” (do wersji 1.0.3) – pozostałości są sprzątane przy starcie
const LEGACY_SESSION_DIR = path.join(LOCAL_ROOT, 'session');
// ścieżka folderu danych dla deinstalatora (UTF-16 LE, czytana przez build/installer.nsh)
const DATADIR_FILE = path.join(app.getPath('userData'), 'datadir.txt');
const LIMITS_GB = { min: 1, max: 500, def: 10 };

function readSettings() {
  try {
    const s = JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8'));
    if (!s || typeof s !== 'object' || !(Number(s.limitGB) > 0)) return null;
    return {
      limitGB: clamp(Math.round(Number(s.limitGB)), LIMITS_GB.min, LIMITS_GB.max),
      dataDir: typeof s.dataDir === 'string' && s.dataDir ? s.dataDir : DEFAULT_DATA_DIR,
      autoUpdate: s.autoUpdate !== false,
      window: s.window ?? null,
    };
  } catch {
    return null;
  }
}

function writeSettings(s) {
  fs.mkdirSync(path.dirname(SETTINGS_FILE), { recursive: true });
  fs.writeFileSync(SETTINGS_FILE, JSON.stringify(s, null, 2));
  try {
    fs.writeFileSync(DATADIR_FILE, Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(s.dataDir, 'utf16le')]));
  } catch {
    /* tylko dla deinstalatora */
  }
}

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

let settings = readSettings();

/** Folder danych, bufor surowych pobrań i bufor przeglądarki (zdjęcia lotnicze) – wszystko w jednym miejscu. */
function storageDirs(s = settings) {
  const cache = path.join(s.dataDir, '.cache');
  return { data: s.dataDir, cache, browser: path.join(cache, 'browser') };
}

/** Część limitu na bufor przeglądarki (kafle zdjęć lotniczych i satelitarnych): 5%, 64–512 MB. */
const browserCacheMB = (s) => Math.round(clamp(s.limitGB * 1024 * 0.05, 64, 512));

// bufor przeglądarki w folderze danych, z własnym limitem – reszta limitu należy do serwera danych
if (settings) {
  app.commandLine.appendSwitch('disk-cache-dir', storageDirs().browser);
  app.commandLine.appendSwitch('disk-cache-size', String(browserCacheMB(settings) * 1024 * 1024));
}

// ------------------------------------------------------------------ pomocnicze
function rmrf(dir) {
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  } catch (e) {
    log(`nie udało się usunąć ${dir}: ${e.message}`);
  }
}

function dirSize(dir) {
  let n = 0;
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) n += dirSize(p);
    else {
      try {
        n += fs.statSync(p).size;
      } catch {
        /* plik zniknął */
      }
    }
  }
  return n;
}

function freeSpace(dir) {
  // pierwszy istniejący katalog nadrzędny
  let d = dir;
  while (d && !fs.existsSync(d)) {
    const up = path.dirname(d);
    if (up === d) break;
    d = up;
  }
  try {
    const st = fs.statfsSync(d);
    return st.bavail * st.bsize;
  } catch {
    return null;
  }
}

const LOG_FILE = path.join(app.getPath('userData'), 'logs', 'gran.log');
let logStream = null;
function log(line) {
  try {
    if (!logStream) {
      fs.mkdirSync(path.dirname(LOG_FILE), { recursive: true });
      // dziennik bieżącej sesji (poprzedni zostaje jako .old)
      if (fs.existsSync(LOG_FILE)) fs.renameSync(LOG_FILE, LOG_FILE + '.old');
      logStream = fs.createWriteStream(LOG_FILE, { flags: 'a' });
    }
    logStream.write(`[${new Date().toISOString()}] ${line}\n`);
  } catch {
    /* brak dziennika nie blokuje programu */
  }
}

function portFree(port) {
  return new Promise((resolve) => {
    const srv = netNode.createServer();
    srv.once('error', () => resolve(false));
    srv.listen(port, '127.0.0.1', () => srv.close(() => resolve(true)));
  });
}

async function pickPort() {
  for (let p = PREFERRED_PORT; p < PREFERRED_PORT + 10; p++) if (await portFree(p)) return p;
  return 0; // system wybierze wolny port
}

// ------------------------------------------------------------------ lokalny serwer
let server = null;
let serverPort = 0;

function startServer() {
  return new Promise(async (resolve, reject) => {
    const dirs = storageDirs();
    fs.mkdirSync(dirs.data, { recursive: true });
    fs.mkdirSync(dirs.cache, { recursive: true });
    const port = await pickPort();
    const child = spawn(process.execPath, [path.join(ROOT, 'server', 'index.mjs')], {
      cwd: ROOT,
      env: {
        ...process.env,
        ELECTRON_RUN_AS_NODE: '1',
        NODE_ENV: 'production',
        PORT: String(port),
        HOST: '127.0.0.1',
        GRAN_DATA_DIR: dirs.data,
        GRAN_CACHE_DIR: dirs.cache,
        GRAN_LIMIT_MB: String(Math.round(settings.limitGB * 1024) - browserCacheMB(settings)),
        GRAN_ADMIN_TOKEN: '',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
      detached: !IS_WIN, // grupa procesów – zamknięcie zabiera też zadania wypiekania
    });
    server = child;
    let ready = false;
    let buf = '';
    const onData = (d) => {
      buf += d.toString();
      const lines = buf.split(/\r?\n/);
      buf = lines.pop() ?? '';
      for (const line of lines) {
        const m = line.match(/^@@ready (\d+)/);
        if (m && !ready) {
          ready = true;
          serverPort = Number(m[1]);
          resolve(serverPort);
        } else if (line.trim()) log(line);
      }
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    child.on('exit', (code) => {
      log(`serwer zakończył pracę (kod ${code})`);
      if (server === child) server = null;
      if (!ready) reject(new Error(`Lokalny serwer nie wystartował (kod ${code}). Szczegóły: ${LOG_FILE}`));
      else if (!quitting && !restarting) onServerCrash();
    });
    setTimeout(() => !ready && reject(new Error('Lokalny serwer nie odpowiada.')), 30000);
  });
}

function stopServer() {
  const child = server;
  server = null;
  if (!child || child.exitCode !== null) return Promise.resolve();
  return new Promise((resolve) => {
    const done = setTimeout(resolve, 5000);
    child.once('exit', () => {
      clearTimeout(done);
      resolve();
    });
    try {
      if (IS_WIN) spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true });
      else process.kill(-child.pid, 'SIGTERM');
    } catch {
      child.kill();
    }
  });
}

async function onServerCrash() {
  const r = await dialog.showMessageBox(mainWin ?? undefined, {
    type: 'error',
    title: APP_NAME,
    message: 'Lokalny serwer danych przestał działać.',
    detail: `Szczegóły w dzienniku: ${LOG_FILE}`,
    buttons: ['Uruchom ponownie', 'Zamknij program'],
    defaultId: 0,
    noLink: true,
  });
  if (r.response === 0) await restart();
  else app.quit();
}

// ------------------------------------------------------------------ okna
let mainWin = null;
let setupWin = null;

const PRELOAD = path.join(__dirname, 'preload.cjs');
const ICON = path.join(ROOT, 'build', IS_WIN ? 'icon.ico' : 'icon.png');

function appUrl() {
  return `http://127.0.0.1:${serverPort}/`;
}

function createMainWindow() {
  const st = settings.window ?? {};
  mainWin = new BrowserWindow({
    width: st.width ?? 1440,
    height: st.height ?? 900,
    x: st.x,
    y: st.y,
    minWidth: 960,
    minHeight: 600,
    show: false,
    title: APP_NAME,
    backgroundColor: BG,
    icon: fs.existsSync(ICON) ? ICON : undefined,
    autoHideMenuBar: true,
    webPreferences: {
      preload: PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      backgroundThrottling: false,
      spellcheck: false,
    },
  });
  if (st.maximized) mainWin.maximize();
  mainWin.once('ready-to-show', () => mainWin.show());
  hardenContents(mainWin.webContents);
  mainWin.webContents.on('before-input-event', (e, input) => onShortcut(e, input, mainWin));
  mainWin.webContents.on('render-process-gone', (_e, d) => {
    log(`proces okna zakończony: ${d.reason}`);
    if (d.reason !== 'clean-exit' && !quitting) mainWin?.reload();
  });
  const saveBounds = () => {
    if (!mainWin || mainWin.isDestroyed()) return;
    const maximized = mainWin.isMaximized();
    const b = maximized ? (settings.window ?? {}) : mainWin.getBounds();
    settings.window = { x: b.x, y: b.y, width: b.width, height: b.height, maximized };
  };
  mainWin.on('resize', saveBounds);
  mainWin.on('move', saveBounds);
  mainWin.on('close', () => {
    saveBounds();
    writeSettings(settings);
  });
  mainWin.on('closed', () => (mainWin = null));
  mainWin.loadURL(appUrl());
}

/** Linki zewnętrzne otwierane w przeglądarce systemowej, nawigacja ograniczona do lokalnej aplikacji. */
function hardenContents(wc) {
  wc.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//.test(url)) void shell.openExternal(url);
    return { action: 'deny' };
  });
  wc.on('will-navigate', (e, url) => {
    if (url.startsWith(appUrl()) || url.startsWith('file:')) return;
    e.preventDefault();
    if (/^https?:\/\//.test(url)) void shell.openExternal(url);
  });
}

function onShortcut(e, input, win) {
  if (input.type !== 'keyDown') return;
  const ctrl = input.control || input.meta;
  if (input.key === 'F11') {
    win.setFullScreen(!win.isFullScreen());
    e.preventDefault();
  } else if (ctrl && input.key === ',') {
    openSetup('settings');
    e.preventDefault();
  } else if ((ctrl && input.key.toLowerCase() === 'r' && !input.shift) || input.key === 'F5') {
    win.webContents.reload();
    e.preventDefault();
  } else if (ctrl && input.shift && input.key.toLowerCase() === 'i') {
    win.webContents.toggleDevTools();
    e.preventDefault();
  } else if (ctrl && (input.key === '=' || input.key === '+')) {
    win.webContents.setZoomLevel(Math.min(3, win.webContents.getZoomLevel() + 0.5));
    e.preventDefault();
  } else if (ctrl && input.key === '-') {
    win.webContents.setZoomLevel(Math.max(-3, win.webContents.getZoomLevel() - 0.5));
    e.preventDefault();
  } else if (ctrl && input.key === '0') {
    win.webContents.setZoomLevel(0);
    e.preventDefault();
  }
}

let setupResolve = null;
/** Okno wyboru przechowywania danych: przy pierwszym starcie („first”) albo z ustawień („settings”). */
function openSetup(kind) {
  if (setupWin) {
    setupWin.focus();
    return Promise.resolve(null);
  }
  setupWin = new BrowserWindow({
    width: 780,
    height: 780,
    minWidth: 640,
    minHeight: 620,
    show: false,
    resizable: true,
    title: kind === 'first' ? `${APP_NAME} – pierwsze uruchomienie` : `${APP_NAME} – dane map`,
    backgroundColor: BG,
    icon: fs.existsSync(ICON) ? ICON : undefined,
    parent: kind === 'settings' && mainWin ? mainWin : undefined,
    modal: kind === 'settings' && !!mainWin,
    autoHideMenuBar: true,
    webPreferences: { preload: PRELOAD, contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  setupWin.once('ready-to-show', () => setupWin.show());
  hardenContents(setupWin.webContents);
  setupWin.loadFile(path.join(__dirname, 'setup.html'), { query: { kind } });
  return new Promise((resolve) => {
    setupResolve = resolve;
    setupWin.on('closed', () => {
      setupWin = null;
      const r = setupResolve;
      setupResolve = null;
      r?.(null);
    });
  });
}

// ------------------------------------------------------------------ IPC
ipcMain.handle('gran:setup:get', () => {
  const s = settings ?? { limitGB: LIMITS_GB.def, dataDir: DEFAULT_DATA_DIR, autoUpdate: true };
  return {
    first: !settings,
    version: app.getVersion(),
    limitGB: s.limitGB,
    dataDir: s.dataDir,
    autoUpdate: s.autoUpdate,
    defaultDataDir: DEFAULT_DATA_DIR,
    limits: LIMITS_GB,
    free: freeSpace(s.dataDir),
    used: settings ? dirSize(s.dataDir) : 0,
  };
});

ipcMain.handle('gran:setup:free', (_e, dir) => freeSpace(String(dir || DEFAULT_DATA_DIR)));

ipcMain.handle('gran:setup:pick', async () => {
  const r = await dialog.showOpenDialog(setupWin ?? undefined, {
    title: 'Folder na dane map',
    defaultPath: settings?.dataDir ?? DEFAULT_DATA_DIR,
    properties: ['openDirectory', 'createDirectory'],
  });
  if (r.canceled || !r.filePaths[0]) return null;
  // zawsze własny podfolder – „Wyczyść dane” nigdy nie dotknie innych plików użytkownika
  const picked = r.filePaths[0];
  const dir = path.basename(picked) === DATA_SUBDIR ? picked : path.join(picked, DATA_SUBDIR);
  try {
    fs.mkdirSync(dir, { recursive: true });
    const probe = path.join(dir, '.gran-write-test');
    fs.writeFileSync(probe, '');
    fs.rmSync(probe);
  } catch {
    await dialog.showMessageBox(setupWin ?? undefined, { type: 'warning', title: APP_NAME, message: 'Brak uprawnień do zapisu w tym folderze.', detail: dir });
    return null;
  }
  return { dir, free: freeSpace(dir) };
});

ipcMain.handle('gran:setup:save', async (_e, cfg) => {
  const next = {
    limitGB: clamp(Math.round(Number(cfg?.limitGB) || LIMITS_GB.def), LIMITS_GB.min, LIMITS_GB.max),
    dataDir: typeof cfg?.dataDir === 'string' && cfg.dataDir ? path.resolve(cfg.dataDir) : DEFAULT_DATA_DIR,
    autoUpdate: cfg?.autoUpdate !== false,
    window: settings?.window ?? null,
  };
  const prev = settings;
  const moved = !!prev && path.resolve(prev.dataDir) !== next.dataDir;
  let move = false;
  if (moved && dirSize(prev.dataDir) > 0) {
    const nested = isInside(next.dataDir, prev.dataDir) || isInside(prev.dataDir, next.dataDir);
    const r = await dialog.showMessageBox(setupWin ?? undefined, {
      type: 'question',
      title: APP_NAME,
      message: 'Przenieść pobrane dane do nowego folderu?',
      detail: `Z: ${prev.dataDir}\nDo: ${next.dataDir}\n\n„Przenieś dane” – zapisane obszary i kafle trafią do nowego folderu, a stary zostanie usunięty.\n„Zacznij od zera” – nowy folder zacznie się wypełniać od początku, stare dane zostaną w poprzednim miejscu.`,
      buttons: nested ? ['Zacznij od zera', 'Anuluj'] : ['Przenieś dane', 'Zacznij od zera', 'Anuluj'],
      defaultId: 0,
      cancelId: nested ? 1 : 2,
      noLink: true,
    });
    if (r.response === (nested ? 1 : 2)) return false;
    move = !nested && r.response === 0;
  }
  if (move) {
    restarting = true;
    await stopServer();
    setupWin?.setProgressBar(2); // nieokreślony postęp na pasku zadań
    try {
      await moveData(prev.dataDir, next.dataDir);
    } catch (e) {
      log(`przenoszenie danych: ${e.message}`);
      setupWin?.setProgressBar(-1);
      restarting = false;
      await dialog.showMessageBox(setupWin ?? undefined, { type: 'error', title: APP_NAME, message: 'Nie udało się przenieść danych.', detail: `${e.message}\n\nDane, których nie przeniesiono, zostały w dotychczasowym folderze.` });
      await restart();
      return false;
    }
    restarting = false;
  }
  settings = next;
  writeSettings(next);
  const resolve = setupResolve;
  setupResolve = null;
  setupWin?.close();
  if (prev && (moved || prev.limitGB !== next.limitGB)) {
    // folder i rozmiar bufora przeglądarki ustawia się przy starcie procesu – pełny restart programu
    quitting = true;
    await stopServer();
    cleaned = true;
    app.relaunch();
    app.exit(0);
    return true;
  }
  resolve?.(next);
  return true;
});

/** Czy `child` leży wewnątrz `parent` (albo jest nim). */
function isInside(child, parent) {
  const rel = path.relative(path.resolve(parent), path.resolve(child));
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

/** Przeniesienie zawartości folderu danych: zmiana nazwy na tym samym dysku, kopia + usunięcie między dyskami. */
async function moveData(from, to) {
  await fs.promises.mkdir(to, { recursive: true });
  for (const e of await fs.promises.readdir(from)) {
    const a = path.join(from, e);
    const b = path.join(to, e);
    if (fs.existsSync(b)) await fs.promises.rm(b, { recursive: true, force: true });
    try {
      await fs.promises.rename(a, b);
    } catch (err) {
      if (err.code !== 'EXDEV' && err.code !== 'EPERM') throw err;
      await fs.promises.cp(a, b, { recursive: true, force: true });
      await fs.promises.rm(a, { recursive: true, force: true, maxRetries: 3 });
    }
  }
  rmrf(from);
}

ipcMain.handle('gran:setup:clear', async () => {
  if (!settings) return 0;
  const dirs = storageDirs();
  restarting = true;
  await stopServer();
  // bufor przeglądarki jest w użyciu – czyści go Chromium, resztę usuwamy z dysku
  for (const e of fs.existsSync(dirs.data) ? fs.readdirSync(dirs.data) : []) {
    if (e !== '.cache') rmrf(path.join(dirs.data, e));
  }
  for (const e of fs.existsSync(dirs.cache) ? fs.readdirSync(dirs.cache) : []) {
    if (e !== 'browser') rmrf(path.join(dirs.cache, e));
  }
  await session.defaultSession.clearCache();
  restarting = false;
  await restart();
  return dirSize(dirs.data);
});

ipcMain.handle('gran:setup:cancel', () => {
  setupWin?.close();
});

ipcMain.handle('gran:open-settings', () => {
  void openSetup('settings');
});

ipcMain.handle('gran:info', () => ({
  version: app.getVersion(),
  limitGB: settings?.limitGB ?? LIMITS_GB.def,
  dataDir: settings?.dataDir ?? '',
}));

// ------------------------------------------------------------------ cykl życia
let quitting = false;
let restarting = false;
let booting = true; // do otwarcia głównego okna zamknięcie okna konfiguracji nie kończy programu

async function restart() {
  restarting = true;
  await stopServer();
  try {
    await startServer();
  } catch (e) {
    restarting = false;
    dialog.showErrorBox(APP_NAME, e.message);
    app.quit();
    return;
  }
  restarting = false;
  if (mainWin) mainWin.loadURL(appUrl());
  else createMainWindow();
}

async function main() {
  Menu.setApplicationMenu(null);
  if (!settings) {
    const s = await openSetup('first');
    if (!s) {
      app.quit();
      return;
    }
    // bufor przeglądarki w wybranym folderze działa od następnego startu procesu
    cleaned = true;
    app.relaunch();
    app.exit(0);
    return;
  }
  if (fs.existsSync(LEGACY_SESSION_DIR)) rmrf(LEGACY_SESSION_DIR);
  writeSettings(settings);
  try {
    await startServer();
  } catch (e) {
    dialog.showErrorBox(APP_NAME, e.message);
    app.quit();
    return;
  }
  createMainWindow();
  booting = false;
  setupUpdater();
}

app.on('second-instance', () => {
  const w = mainWin ?? setupWin;
  if (!w) return;
  if (w.isMinimized()) w.restore();
  w.focus();
});

app.on('window-all-closed', () => {
  if (setupWin || restarting || booting) return;
  app.quit();
});

let cleaned = false;
app.on('before-quit', (e) => {
  quitting = true;
  if (cleaned) return;
  e.preventDefault();
  void (async () => {
    try {
      if (settings) writeSettings(settings);
      await stopServer();
    } catch (err) {
      log(`sprzątanie przy wyjściu: ${err.message}`);
    }
    cleaned = true;
    app.quit();
  })();
});

// ------------------------------------------------------------------ aktualizacje (redzikowski.com)
// Program sprawdza wydania przy starcie i co 6 godzin na redzikowski.com – serwer strony udostępnia kopię
// GitHub Releases w tym samym formacie, a pliki pobierane są przez niego. Nowa wersja = propozycja
// w oknie dialogowym; pobieranie po zgodzie (postęp na pasku zadań), sprawdzenie sumy SHA-512 z latest.yml,
// potem instalator z uprawnieniami administratora (okno UAC) aktualizuje program w tym samym folderze.
const UPDATE_HOST = 'https://redzikowski.com';
const UPDATE_FEED = `${UPDATE_HOST}/api/github/repos/BlackSingularities/gran-3d/releases/latest`;
const UPDATE_DIR = path.join(app.getPath('temp'), 'GRAN-aktualizacja');

let updateOffered = null; // wersja, o której użytkownik już wie w tej sesji
let updateBusy = false;

/** Porównanie wersji x.y.z: dodatnie, gdy a > b. */
function cmpVersion(a, b) {
  const pa = String(a).replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
  const pb = String(b).replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < 3; i++) if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  return 0;
}

async function fetchOk(url, type) {
  const r = await net.fetch(url, { headers: { 'User-Agent': `GRAN/${app.getVersion()}` }, cache: 'no-store' });
  if (!r.ok) throw new Error(`redzikowski.com odpowiedział kodem ${r.status}`);
  return type === 'json' ? r.json() : type === 'text' ? r.text() : r;
}

/** Najnowsze wydanie: wersja, instalator i jego suma SHA-512 (z latest.yml tego samego wydania). */
async function latestRelease() {
  const rel = await fetchOk(UPDATE_FEED, 'json');
  const version = String(rel.tag_name || '').replace(/^v/, '');
  const exe = (rel.assets || []).find((a) => /^GRAN-Setup-.*\.exe$/i.test(a.name));
  const yml = (rel.assets || []).find((a) => a.name === 'latest.yml');
  return { version, exe, yml, notes: String(rel.body || '') };
}

async function expectedSha512(yml, name) {
  if (!yml) throw new Error('Wydanie nie zawiera pliku latest.yml z sumą kontrolną.');
  const text = await fetchOk(yml.browser_download_url, 'text');
  // sekcja „files” z adresem instalatora, a w razie czego główne pole sha512
  const block = text.split(/\n\s*-\s+url:\s*/).find((b) => b.trim().startsWith(name));
  const m = (block ?? text).match(/sha512:\s*([A-Za-z0-9+/=]+)/);
  if (!m) throw new Error('Brak sumy SHA-512 w latest.yml.');
  return m[1];
}

/** Pobranie instalatora do katalogu tymczasowego z postępem i weryfikacją sumy. */
async function downloadInstaller(rel) {
  const sha = await expectedSha512(rel.yml, rel.exe.name);
  fs.rmSync(UPDATE_DIR, { recursive: true, force: true });
  fs.mkdirSync(UPDATE_DIR, { recursive: true });
  const file = path.join(UPDATE_DIR, rel.exe.name);
  const r = await fetchOk(rel.exe.browser_download_url);
  const total = Number(r.headers.get('content-length')) || rel.exe.size || 0;
  const hash = crypto.createHash('sha512');
  const out = fs.createWriteStream(file);
  let got = 0;
  try {
    const reader = r.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      hash.update(value);
      got += value.length;
      if (!out.write(value)) await new Promise((res) => out.once('drain', res));
      if (total) mainWin?.setProgressBar(Math.max(0.01, got / total));
    }
  } finally {
    await new Promise((res) => out.end(res));
  }
  if (hash.digest('base64') !== sha) {
    fs.rmSync(file, { force: true });
    throw new Error('Pobrany instalator ma niezgodną sumę kontrolną – spróbuj ponownie później.');
  }
  return file;
}

/** Instalator NSIS w trybie cichym z podniesieniem uprawnień (UAC); po instalacji uruchamia nową wersję. */
function runInstaller(file) {
  const ps = `Start-Process -FilePath '${file.replace(/'/g, "''")}' -ArgumentList '--updated','/S','--force-run' -Verb RunAs`;
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', ps], {
    detached: true,
    stdio: 'ignore',
    windowsHide: true,
  });
  return new Promise((resolve) => {
    child.once('exit', (code) => resolve(code === 0));
    child.once('error', () => resolve(false));
  });
}

async function offerUpdate(rel, manual) {
  if (updateBusy || (updateOffered === rel.version && !manual)) return;
  updateOffered = rel.version;
  const r = await dialog.showMessageBox(mainWin ?? undefined, {
    type: 'info',
    title: APP_NAME,
    message: `Dostępna jest nowa wersja ${rel.version}.`,
    detail: `Masz wersję ${app.getVersion()}. Pobrać i zainstalować aktualizację${rel.exe.size ? ` (${Math.round(rel.exe.size / 2 ** 20)} MB)` : ''}? Dane map i ustawienia zostaną zachowane. Instalacja wymaga potwierdzenia uprawnień administratora.`,
    buttons: ['Aktualizuj teraz', 'Później'],
    defaultId: 0,
    cancelId: 1,
    noLink: true,
  });
  if (r.response !== 0) return;
  updateBusy = true;
  mainWin?.setProgressBar(0.01);
  let file;
  try {
    file = await downloadInstaller(rel);
  } catch (err) {
    updateBusy = false;
    mainWin?.setProgressBar(-1);
    log(`aktualizacje: ${err?.message ?? err}`);
    dialog.showErrorBox(APP_NAME, `Nie udało się pobrać aktualizacji: ${err?.message ?? err}`);
    return;
  }
  mainWin?.setProgressBar(-1);
  const go = await dialog.showMessageBox(mainWin ?? undefined, {
    type: 'info',
    title: APP_NAME,
    message: `Wersja ${rel.version} jest gotowa do instalacji.`,
    detail: 'Program zamknie się, Windows poprosi o zgodę administratora, a po instalacji GRAŃ uruchomi się ponownie w nowej wersji.',
    buttons: ['Zainstaluj teraz', 'Później'],
    defaultId: 0,
    cancelId: 1,
    noLink: true,
  });
  updateBusy = false;
  if (go.response !== 0) return;
  quitting = true;
  if (settings) writeSettings(settings);
  await stopServer();
  if (!(await runInstaller(file))) {
    quitting = false;
    await restart();
    dialog.showErrorBox(APP_NAME, 'Instalacja aktualizacji została przerwana (brak zgody administratora?). Spróbuj ponownie przyciskiem „Sprawdź aktualizacje”.');
    return;
  }
  cleaned = true;
  app.exit(0);
}

/** Sprawdzenie wydań. Wynik dla okna ustawień: aktualna / dostępna wersja X / błąd. */
async function checkUpdates(manual = false) {
  const current = app.getVersion();
  if (!app.isPackaged) return { state: 'dev', current };
  if (!net.isOnline()) return { state: 'offline', current };
  try {
    const rel = await latestRelease();
    if (!rel.version || !rel.exe || cmpVersion(rel.version, current) <= 0) return { state: 'latest', current, latest: rel.version || current };
    void offerUpdate(rel, manual);
    return { state: 'available', current, latest: rel.version };
  } catch (err) {
    log(`aktualizacje: ${err?.message ?? err}`);
    return { state: 'error', current, error: String(err?.message ?? err) };
  }
}

function setupUpdater() {
  // pozostałość po poprzedniej aktualizacji
  fs.rm(UPDATE_DIR, { recursive: true, force: true }, () => {});
  if (!app.isPackaged || settings?.autoUpdate === false) return;
  setTimeout(() => void checkUpdates(), 15_000);
  setInterval(() => void checkUpdates(), 6 * 60 * 60 * 1000).unref();
}

ipcMain.handle('gran:update:check', () => checkUpdates(true));

app.whenReady().then(main);

// porządek w nieobsłużonych błędach – zamiast cichego zawieszenia
process.on('uncaughtException', (e) => log(`uncaughtException: ${e.stack ?? e}`));
