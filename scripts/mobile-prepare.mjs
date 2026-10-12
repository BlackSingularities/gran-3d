// Przygotowanie zasobów aplikacji Android (mobile/www): ekran startowy + projekt Node.js
// (serwer, skrypty wypiekania, zbudowany interfejs i zależności produkcyjne) dla wtyczki capacitor-nodejs.
// Wymaga wcześniejszego `npm run build` w katalogu głównym.
import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WWW = path.join(ROOT, 'mobile', 'www');
const NODE = path.join(WWW, 'nodejs');
const APP = path.join(NODE, 'app');
const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));

if (!fs.existsSync(path.join(ROOT, 'dist', 'index.html'))) {
  console.error('Brak zbudowanego interfejsu – uruchom najpierw `npm run build` w katalogu głównym.');
  process.exit(1);
}

const copy = (from, to, filter) => fs.cpSync(path.join(ROOT, from), to, { recursive: true, filter });
const noMaps = (src) => !src.endsWith('.map');

fs.rmSync(NODE, { recursive: true, force: true });
fs.mkdirSync(path.join(APP, 'public'), { recursive: true });

copy('server', path.join(APP, 'server'));
copy('scripts', path.join(APP, 'scripts'), (src) => !src.endsWith('mobile-prepare.mjs'));
copy('dist', path.join(APP, 'dist'), noMaps);
for (const f of ['catalog.json', 'coverage.json']) copy(path.join('public', f), path.join(APP, 'public', f));
fs.copyFileSync(path.join(ROOT, 'mobile', 'node', 'main.mjs'), path.join(NODE, 'main.mjs'));

fs.writeFileSync(
  path.join(NODE, 'package.json'),
  JSON.stringify(
    {
      name: 'gran-node',
      private: true,
      version: pkg.version,
      type: 'module',
      main: 'main.mjs',
      dependencies: { geotiff: pkg.dependencies.geotiff, pngjs: pkg.dependencies.pngjs },
    },
    null,
    2
  )
);
execSync('npm install --omit=dev --no-package-lock --no-audit --no-fund --ignore-scripts', { cwd: NODE, stdio: 'inherit' });

// mniej plików do rozpakowania przy pierwszym starcie: bez map źródeł, dokumentacji i testów
function prune(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (/^(test|tests|docs?|example|examples|\.github)$/i.test(e.name)) fs.rmSync(p, { recursive: true, force: true });
      else prune(p);
    } else if (/\.(map|md|markdown|ts)$/i.test(e.name) && !e.name.endsWith('.d.ts')) fs.rmSync(p, { force: true });
  }
}
prune(path.join(NODE, 'node_modules'));

// zasoby ekranu startowego
fs.mkdirSync(path.join(WWW, 'brand'), { recursive: true });
fs.copyFileSync(path.join(ROOT, 'public', 'brand', 'logo-mark.png'), path.join(WWW, 'brand', 'logo-mark.png'));
fs.cpSync(path.join(ROOT, 'electron', 'fonts'), path.join(WWW, 'fonts'), { recursive: true, filter: (s) => !s.endsWith('.txt') });

let files = 0, bytes = 0;
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else { files++; bytes += fs.statSync(p).size; }
  }
})(NODE);
console.log(`mobile/www/nodejs: ${files} plików, ${(bytes / 2 ** 20).toFixed(1)} MB`);
