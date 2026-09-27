import type { Route, Snap } from './graph';

export type Tool = 'explore' | 'route' | 'relative' | 'viewshed' | 'measure';
export type Style = 'terrain' | 'hypso' | 'slope' | 'aspect' | 'satellite' | 'paper';

export interface Waypoint {
  id: string;
  lon: number;
  lat: number;
  x: number;
  z: number;
  e: number;
  snap: Snap | null;
  label?: string;
}

export interface MeasurePoint {
  x: number;
  z: number;
  e: number;
  lon: number;
  lat: number;
}

export interface State {
  regionId: string;
  tool: Tool;
  style: Style;
  contours: boolean;
  trails: boolean;
  labels: boolean;
  shadows: boolean;
  grid: boolean;
  snow: boolean;
  cursorIso: boolean;
  exag: number;
  day: string;
  hour: number;
  routeMode: 'trails' | 'terrain';
  maxSlope: number;
  timeKind: 'pttk' | 'din';
  pace: number;
  waypoints: Waypoint[];
  routes: Route[];
  activeRoute: number;
  routing: boolean;
  ref: MeasurePoint | null;
  relRange: number;
  viewshed: { point: MeasurePoint; eye: number; areaKm2: number; count: number; peaks: { id: number; dist: number }[] } | null;
  vsEye: number;
  measure: MeasurePoint[];
  gpx: Route | null;
  profileOpen: boolean;
  panelOpen: boolean;
  panorama: boolean;
  flying: boolean;
}

type Listener = (s: State, changed: Set<keyof State>) => void;

export class Store {
  state: State;
  private listeners: Listener[] = [];
  private pending = new Set<keyof State>();
  private scheduled = false;

  constructor(init: State) {
    this.state = init;
  }

  get<K extends keyof State>(k: K) {
    return this.state[k];
  }

  set(patch: Partial<State>) {
    for (const k of Object.keys(patch) as (keyof State)[]) {
      if (this.state[k] !== patch[k]) {
        (this.state as unknown as Record<string, unknown>)[k] = patch[k];
        this.pending.add(k);
      }
    }
    if (!this.scheduled && this.pending.size) {
      this.scheduled = true;
      queueMicrotask(() => {
        this.scheduled = false;
        const ch = this.pending;
        this.pending = new Set();
        for (const l of this.listeners) l(this.state, ch);
      });
    }
  }

  on(l: Listener) {
    this.listeners.push(l);
    return () => (this.listeners = this.listeners.filter((x) => x !== l));
  }
}

export function todayWarsaw() {
  const f = new Intl.DateTimeFormat('sv-SE', { timeZone: 'Europe/Warsaw' });
  return f.format(new Date());
}
