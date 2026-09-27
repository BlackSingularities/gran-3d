import { DEG, EARTH_R, lat2px, lon2px, px2lat, px2lon } from './geo';

/**
 * Ramka świata: płaski układ Web Mercator z punktem odniesienia ustalonym na sesję.
 * x – wschód, z – południe, w metrach (skala z szerokości punktu odniesienia).
 * Teren globalny i dane kolejnych sektorów są w niej pozycjonowane bez przeliczania kamery.
 */
export interface WorldFrame {
  /** piksel Web Mercator (zoom 20) punktu (0, 0) */
  ox: number;
  oy: number;
  /** metry świata na piksel zoomu 20 */
  mpp20: number;
  lon0: number;
  lat0: number;
}

export const FRAME_Z = 20;

export function makeFrame(lon: number, lat: number): WorldFrame {
  return {
    ox: lon2px(lon, FRAME_Z),
    oy: lat2px(lat, FRAME_Z),
    mpp20: (2 * Math.PI * EARTH_R * Math.cos(lat * DEG)) / (256 * 2 ** FRAME_Z),
    lon0: lon,
    lat0: lat,
  };
}

export function frameLonLatToWorld(f: WorldFrame, lon: number, lat: number): [number, number] {
  return [(lon2px(lon, FRAME_Z) - f.ox) * f.mpp20, (lat2px(lat, FRAME_Z) - f.oy) * f.mpp20];
}

export function frameWorldToLonLat(f: WorldFrame, x: number, z: number): [number, number] {
  return [px2lon(f.ox + x / f.mpp20, FRAME_Z), px2lat(f.oy + z / f.mpp20, FRAME_Z)];
}
