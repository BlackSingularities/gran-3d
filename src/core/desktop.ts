// Integracja z aplikacją Windows (Electron, electron/preload.cjs). W przeglądarce (tryb deweloperski) `desktop` jest null.

export interface DesktopInfo {
  version: string;
  limitGB: number;
  dataDir: string;
}

export interface UpdateCheck {
  state: 'latest' | 'available' | 'error' | 'offline' | 'dev';
  current: string;
  latest?: string;
}

interface DesktopBridge {
  info(): Promise<DesktopInfo>;
  openSettings(): Promise<void>;
  /** Ręczne sprawdzenie aktualizacji na redzikowski.com (propozycję instalacji pokazuje sam program). */
  checkUpdates(): Promise<UpdateCheck>;
}

export const desktop: DesktopBridge | null = (globalThis as { granDesktop?: DesktopBridge }).granDesktop ?? null;

export interface StorageStatus {
  permanent: number;
  cache: number;
  limit: number;
  at: number;
}

export const fmtBytes = (b: number) =>
  b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1).replace('.', ',')} GB` : `${Math.round(b / 1024 ** 2)} MB`;

/** Zapis pliku (GPX, zrzut ekranu) – okno „Zapisz jako” programu albo zwykłe pobranie przeglądarki. */
export async function saveFile(name: string, data: Blob | string): Promise<void> {
  const url = typeof data === 'string' ? data : URL.createObjectURL(data);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  if (typeof data !== 'string') setTimeout(() => URL.revokeObjectURL(url), 2000);
}
