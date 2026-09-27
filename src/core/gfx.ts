/** Ustawienia jakości renderowania – presety i zapis w przeglądarce. */
export interface Gfx {
  preset: GfxPreset | 'custom';
  /** skala rozdzielczości renderu względem gęstości pikseli ekranu (0,25–1) */
  scale: number;
  /** automatyczne obniżanie rozdzielczości przy spadku płynności */
  adaptive: boolean;
  /** mnożnik odległości podziału kafli LiDAR */
  lod: number;
  /** najdrobniejszy poziom kafli: 0 = pełny, inaczej limit zoomu (13 ≈ 12 m, 14 ≈ 6 m) */
  maxLevel: number;
  /** gęstość siatki kafla (podziałów na bok) */
  mesh: 32 | 64 | 128;
  /** rozdzielczość zdjęć lotniczych na najdrobniejszym poziomie */
  ortho: 256 | 512 | 1024;
  /** proceduralna mikrorzeźba skał */
  detail: boolean;
  /** limit klatek na sekundę (0 = bez limitu) */
  fps: 0 | 30;
  /** gęstość etykiet (mnożnik) */
  labels: number;
}

export type GfxPreset = 'min' | 'low' | 'mid' | 'high' | 'ultra';

export const GFX_PRESETS: Record<GfxPreset, Omit<Gfx, 'preset'>> = {
  min: { scale: 0.35, adaptive: true, lod: 0.4, maxLevel: 13, mesh: 32, ortho: 256, detail: false, fps: 30, labels: 0.6 },
  low: { scale: 0.5, adaptive: true, lod: 0.65, maxLevel: 14, mesh: 64, ortho: 512, detail: false, fps: 30, labels: 0.8 },
  mid: { scale: 0.75, adaptive: true, lod: 1, maxLevel: 0, mesh: 64, ortho: 512, detail: true, fps: 0, labels: 1 },
  high: { scale: 1, adaptive: true, lod: 1.2, maxLevel: 0, mesh: 128, ortho: 1024, detail: true, fps: 0, labels: 1 },
  ultra: { scale: 1, adaptive: false, lod: 1.8, maxLevel: 0, mesh: 128, ortho: 1024, detail: true, fps: 0, labels: 1.3 },
};

export const GFX_PRESET_LABEL: Record<GfxPreset, string> = { min: 'Minimalna', low: 'Niska', mid: 'Średnia', high: 'Wysoka', ultra: 'Ultra' };

const KEY = 'gran.gfx.v1';

/** Słabsze urządzenia startują od niższego presetu. */
function guessPreset(): GfxPreset {
  const mobile = /Android|iPhone|iPad/i.test(navigator.userAgent);
  const cores = navigator.hardwareConcurrency || 4;
  if (mobile) return cores >= 8 ? 'low' : 'min';
  return cores <= 4 ? 'mid' : 'high';
}

export function loadGfx(): Gfx {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const g = JSON.parse(raw) as Gfx;
      if (g && typeof g.scale === 'number') return { ...GFX_PRESETS.high, ...g };
    }
  } catch {
    /* brak dostępu do localStorage */
  }
  const p = guessPreset();
  return { preset: p, ...GFX_PRESETS[p] };
}

export function saveGfx(g: Gfx) {
  try {
    localStorage.setItem(KEY, JSON.stringify(g));
  } catch {
    /* ignoruj */
  }
}
