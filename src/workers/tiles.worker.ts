/// <reference lib="webworker" />
// Pobieranie i dekodowanie kafli wysokości (PNG, wysokość zakodowana w RGB: (R·65536 + G·256 + B) / 10 − 1000 m).

const S = 259;
let canvas: OffscreenCanvas | null = null;
let ctx: OffscreenCanvasRenderingContext2D | null = null;

self.onmessage = async (ev: MessageEvent<{ id: number; url: string }>) => {
  const { id, url } = ev.data;
  try {
    const res = await fetch(url);
    // 202: serwer wypieka blok LiDAR – ponowić za chwilę
    if (res.status === 202) return (self as unknown as Worker).postMessage({ id, retry: true });
    if (!res.ok) throw new Error(String(res.status));
    const bmp = await createImageBitmap(await res.blob(), { premultiplyAlpha: 'none', colorSpaceConversion: 'none' });
    if (!canvas) {
      canvas = new OffscreenCanvas(S, S);
      ctx = canvas.getContext('2d', { willReadFrequently: true }) as OffscreenCanvasRenderingContext2D;
    }
    ctx!.clearRect(0, 0, S, S);
    ctx!.drawImage(bmp, 0, 0);
    bmp.close();
    const px = ctx!.getImageData(0, 0, S, S, { colorSpace: 'srgb' }).data;
    const out = new Float32Array(S * S);
    for (let i = 0, j = 0; i < out.length; i++, j += 4) {
      out[i] = (px[j] * 65536 + px[j + 1] * 256 + px[j + 2]) / 10 - 1000;
    }
    (self as unknown as Worker).postMessage({ id, data: out }, [out.buffer]);
  } catch (e) {
    (self as unknown as Worker).postMessage({ id, error: (e as Error).message });
  }
};
