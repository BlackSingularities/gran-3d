// Integracja z aplikacją desktopową (Electron, electron/preload.cjs). W przeglądarce `desktop` jest null.

export interface DesktopInfo {
  platform?: 'desktop' | 'android';
  version: string;
  mode: 'keep' | 'session';
  limitGB: number;
  dataDir: string;
}

interface DesktopBridge {
  info(): Promise<DesktopInfo>;
  openSettings(): Promise<void>;
}

export const desktop: DesktopBridge | null = (globalThis as { granDesktop?: DesktopBridge }).granDesktop ?? null;

/** Kto obsługuje ustawienia „Dane i pamięć”: powłoka Electron albo okno w aplikacji (Android). */
export let storageHost: DesktopBridge | null = desktop;
export function setStorageHost(h: DesktopBridge) {
  storageHost = h;
}

export interface StorageStatus {
  permanent: number;
  cache: number;
  limit: number;
  at: number;
}

export const fmtBytes = (b: number) =>
  b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1).replace('.', ',')} GB` : `${Math.round(b / 1024 ** 2)} MB`;

/** Most natywny aplikacji Android (MainActivity: addJavascriptInterface). W innych środowiskach null. */
interface AndroidBridge {
  saveFile(name: string, mime: string, base64: string): void;
  openExternal(url: string): void;
}

export const android: AndroidBridge | null = (globalThis as { GranAndroid?: AndroidBridge }).GranAndroid ?? null;

/** Zapis pliku: Android – do folderu „Pobrane” przez most natywny; gdzie indziej – zwykłe pobranie przeglądarki. */
export async function saveFile(name: string, data: Blob | string): Promise<void> {
  if (android) {
    const blob = typeof data === 'string' ? await (await fetch(data)).blob() : data;
    const buf = new Uint8Array(await blob.arrayBuffer());
    let bin = '';
    for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    android.saveFile(name, blob.type || 'application/octet-stream', btoa(bin));
    return;
  }
  const url = typeof data === 'string' ? data : URL.createObjectURL(data);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  if (typeof data !== 'string') setTimeout(() => URL.revokeObjectURL(url), 2000);
}
