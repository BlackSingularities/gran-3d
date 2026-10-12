// GRAŃ – aplikacja desktopowa (Electron).
//
// Powłoka uruchamia lokalny serwer aplikacji (server/index.mjs) jako osobny proces Node
// wbudowany w Electron, nasłuchujący wyłącznie na 127.0.0.1, i otwiera go w oknie aplikacji.
// Przy pierwszym uruchomieniu użytkownik wybiera sposób przechowywania danych:
//   • „keep”    – kafle i dane okolic zostają na dysku, z limitem pojemności (najdawniej używane są usuwane),
//   • „session” – wszystko jest usuwane przy każdym zamknięciu programu.
'use strict';

const { app, BrowserWindow, dialog, ipcMain, Menu, net, session, shell } = require('electron');
const { spawn, spawnSync } = require('node:child_process');
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
const SESSION_DIR = path.join(LOCAL_ROOT, 'session');
const DATA_SUBDIR = 'GRAN-dane';
const LIMITS_GB = { min: 1, max: 500, def: 10 };

function readSettings() {
  try {
    const s = JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8'));
    if (s.mode !== 'keep' && s.mode !== 'session') return null;
    return {
      mode: s.mode,
      limitGB: clamp(Number(s.limitGB) || LIMITS_GB.def, LIMITS_GB.min, LIMITS_GB.max),
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
}

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

let settings = readSettings();

/** Katalog danych i bufora dla bieżącego trybu. */
function storageDirs(s = settings) {
  const base = s.mode === 'session' ? SESSION_DIR : s.dataDir;
  return { data: base, cache: path.join(base, '.cache') };
}

// pamięć podręczna przeglądarki (kafle zdjęć, odpowiedzi HTTP) – mała część limitu, maks. 512 MB
if (settings) {
  const mb = Math.round(clamp(settings.limitGB * 1024 * 0.05, 64, 512));
  app.commandLine.appendSwitch('disk-cache-size', String(mb * 1024 * 1024));
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
        GRAN_LIMIT_MB: String(Math.round(settings.limitGB * 1024)),
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
    title: kind === 'first' ? `${APP_NAME} – pierwsze uruchomienie` : `${APP_NAME} – dane i pamięć`,
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
  const s = settings ?? { mode: 'keep', limitGB: LIMITS_GB.def, dataDir: DEFAULT_DATA_DIR, autoUpdate: true };
  const dirs = settings ? storageDirs(settings) : null;
  return {
    first: !settings,
    version: app.getVersion(),
    mode: s.mode,
    limitGB: s.limitGB,
    dataDir: s.dataDir,
    autoUpdate: s.autoUpdate,
    defaultDataDir: DEFAULT_DATA_DIR,
    limits: LIMITS_GB,
    free: freeSpace(s.mode === 'session' ? SESSION_DIR : s.dataDir),
    used: dirs ? dirSize(dirs.data) : 0,
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
    mode: cfg?.mode === 'session' ? 'session' : 'keep',
    limitGB: clamp(Math.round(Number(cfg?.limitGB) || LIMITS_GB.def), LIMITS_GB.min, LIMITS_GB.max),
    dataDir: typeof cfg?.dataDir === 'string' && cfg.dataDir ? cfg.dataDir : DEFAULT_DATA_DIR,
    autoUpdate: cfg?.autoUpdate !== false,
    window: settings?.window ?? null,
  };
  const prev = settings;
  settings = next;
  writeSettings(next);
  const resolve = setupResolve;
  setupResolve = null;
  setupWin?.close();
  if (prev && (prev.mode !== next.mode || prev.limitGB !== next.limitGB || prev.dataDir !== next.dataDir)) {
    await restart();
  }
  resolve?.(next);
  return true;
});

ipcMain.handle('gran:setup:clear', async () => {
  if (!settings) return 0;
  const dirs = storageDirs();
  restarting = true;
  await stopServer();
  rmrf(dirs.data);
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
  mode: settings?.mode ?? 'keep',
  limitGB: settings?.limitGB ?? LIMITS_GB.def,
  dataDir: settings ? storageDirs().data : '',
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
  }
  // tryb sesji: pozostałości po awarii lub zamknięciu systemu bez wyjścia z programu
  if (settings.mode === 'session') rmrf(SESSION_DIR);
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
      if (settings?.mode === 'session') {
        rmrf(SESSION_DIR);
        await session.defaultSession.clearCache();
        await session.defaultSession.clearStorageData({ storages: ['cachestorage', 'shadercache', 'serviceworkers'] });
      }
    } catch (err) {
      log(`sprzątanie przy wyjściu: ${err.message}`);
    }
    cleaned = true;
    app.quit();
  })();
});

// ------------------------------------------------------------------ aktualizacje (GitHub Releases)
function setupUpdater() {
  if (!app.isPackaged || settings?.autoUpdate === false) return;
  let autoUpdater;
  try {
    ({ autoUpdater } = require('electron-updater'));
  } catch {
    return;
  }
  autoUpdater.autoDownload = true;
  autoUpdater.autoInstallOnAppQuit = true;
  autoUpdater.logger = { info: log, warn: log, error: log, debug: () => {} };
  autoUpdater.on('update-downloaded', async (info) => {
    const r = await dialog.showMessageBox(mainWin ?? undefined, {
      type: 'info',
      title: APP_NAME,
      message: `Dostępna jest nowa wersja ${info.version}.`,
      detail: 'Aktualizacja została pobrana. Zainstalować ją teraz? Program uruchomi się ponownie.',
      buttons: ['Zainstaluj i uruchom ponownie', 'Przy następnym zamknięciu'],
      defaultId: 0,
      noLink: true,
    });
    if (r.response === 0) {
      quitting = true;
      await stopServer();
      if (settings?.mode === 'session') rmrf(SESSION_DIR);
      cleaned = true;
      autoUpdater.quitAndInstall(false, true);
    }
  });
  autoUpdater.on('error', (err) => log(`aktualizacje: ${err?.message ?? err}`));
  const check = () => net.isOnline() && autoUpdater.checkForUpdates().catch((err) => log(`aktualizacje: ${err.message}`));
  setTimeout(check, 15_000);
  setInterval(check, 6 * 60 * 60 * 1000).unref();
}

app.whenReady().then(main);

// porządek w nieobsłużonych błędach – zamiast cichego zawieszenia
process.on('uncaughtException', (e) => log(`uncaughtException: ${e.stack ?? e}`));
