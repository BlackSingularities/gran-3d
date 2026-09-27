// Wspólne ustawienia skryptów wypiekających: katalog pasm, katalogi danych i bufora, raport postępu.
import fs from 'node:fs';
import path from 'node:path';

export const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/(\w:)/, '$1')), '..', '..');
export const DATA_DIR = path.resolve(process.env.GRAN_DATA_DIR || path.join(ROOT, 'data'));
export const CACHE_DIR = path.resolve(process.env.GRAN_CACHE_DIR || path.join(ROOT, '.cache'));
export const CATALOG = JSON.parse(fs.readFileSync(path.join(ROOT, 'public', 'catalog.json'), 'utf8')).regions;

/** Linia postępu czytana przez serwer: @@progress <0..1> <opis> */
export function progress(f, label) {
  console.log(`@@progress ${Math.max(0, Math.min(1, f)).toFixed(3)} ${label}`);
}

/** Wybrane regiony z argumentów wiersza poleceń (brak = wszystkie). */
export function selectRegions(argv) {
  const ids = argv.slice(2).filter((a) => !a.startsWith('-'));
  // obszar użytkownika (data/<id>/region.json) albo pasmo z katalogu propozycji
  const byId = (id) => {
    const f = path.join(DATA_DIR, id, 'region.json');
    if (fs.existsSync(f)) return JSON.parse(fs.readFileSync(f, 'utf8'));
    return CATALOG.find((r) => r.id === id);
  };
  const list = ids.length ? ids.map(byId) : CATALOG;
  const missing = ids.filter((_, i) => !list[i]);
  if (missing.length) throw new Error(`Nieznane obszary: ${missing.join(', ')}`);
  return list;
}
