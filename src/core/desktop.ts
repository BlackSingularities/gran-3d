// Integracja z aplikacją desktopową (Electron, electron/preload.cjs). W przeglądarce `desktop` jest null.

export interface DesktopInfo {
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

export interface StorageStatus {
  permanent: number;
  cache: number;
  limit: number;
  at: number;
}

export const fmtBytes = (b: number) =>
  b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1).replace('.', ',')} GB` : `${Math.round(b / 1024 ** 2)} MB`;
