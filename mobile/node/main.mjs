// GRAŃ (Android) – start lokalnego serwera w silniku Node.js osadzonym w aplikacji (nodejs-mobile).
// Katalogi: DATADIR (ustawia wtyczka) = <pliki aplikacji>/nodejs/data
//   settings.json – tryb przechowywania i limit, maps/ – kafle i dane okolic.
import fs from 'node:fs';
import path from 'node:path';

const root = process.env.DATADIR || path.join(process.cwd(), 'gran-data');
const settingsFile = path.join(root, 'settings.json');
const dataDir = path.join(root, 'maps');

let settings = null;
try {
  settings = JSON.parse(fs.readFileSync(settingsFile, 'utf8'));
} catch {
  /* pierwsze uruchomienie – wybór w oknie aplikacji */
}

// tryb „usuwaj za każdym razem”: dane poprzedniej sesji znikają przy starcie (Android nie gwarantuje zdarzenia zamknięcia)
if (settings?.mode === 'session') fs.rmSync(dataDir, { recursive: true, force: true, maxRetries: 3 });
fs.mkdirSync(path.join(dataDir, '.cache'), { recursive: true });

const pkg = JSON.parse(fs.readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
Object.assign(process.env, {
  NODE_ENV: 'production',
  HOST: '127.0.0.1',
  PORT: '47190',
  GRAN_DATA_DIR: dataDir,
  GRAN_CACHE_DIR: path.join(dataDir, '.cache'),
  GRAN_LIMIT_MB: String(Math.round((settings?.limitGB ?? 8) * 1024)),
  GRAN_INPROC: '1',
  GRAN_SETTINGS_FILE: settingsFile,
  GRAN_VERSION: pkg.version,
  GRAN_ADMIN_TOKEN: '',
});

await import('./app/server/index.mjs');
