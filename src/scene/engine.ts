import * as THREE from 'three';
import { MapControls } from 'three/examples/jsm/controls/MapControls.js';
import type { Dem } from '../core/dem';
import { DEG, clamp } from '../core/geo';

export interface View {
  x: number;
  z: number;
  distance: number;
  heading: number; // stopnie, 0 = patrzymy na północ
  pitch: number; // stopnie nad horyzontem (kąt patrzenia w dół)
}

const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

export class Engine {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera: THREE.PerspectiveCamera;
  readonly controls: MapControls;
  readonly world = new THREE.Group();
  dem: Dem | null = null;
  /** dokładniejsze źródło wysokości (kafle LiDAR), gdy dostępne */
  fineHeight: ((x: number, z: number) => number | null) | null = null;
  exag = 1.4;
  private frameCbs: ((dt: number) => void)[] = [];
  private anim: ((t: number) => boolean) | null = null;
  private last = performance.now();
  private moving = 0;
  dirty = true;
  panorama = false;
  private savedView: View | null = null;
  // adaptacyjna rozdzielczość: przy wolnych klatkach obniżamy gęstość pikseli
  /** docelowa gęstość pikseli (ustawienie jakości) */
  private maxPR = Math.min(window.devicePixelRatio, 2);
  private pr = this.maxPR;
  adaptive = true;
  fpsCap = 0;
  /** średni czas klatki [s] – do wyświetlania wydajności */
  get frameTime() { return this.ema; }
  get pixelRatio() { return this.pr; }
  private ema = 1 / 60;
  private lastRender = 0;
  private lastAdjust = 0;

  constructor(readonly canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: true,
      logarithmicDepthBuffer: true,
      powerPreference: 'high-performance',
      preserveDrawingBuffer: false,
    });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.toneMapping = THREE.AgXToneMapping;
    this.renderer.toneMappingExposure = 1.25;
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.setClearColor(0x000000, 0);

    this.camera = new THREE.PerspectiveCamera(38, 1, 5, 400000);
    this.scene.add(this.world);

    this.controls = new MapControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.09;
    this.controls.screenSpacePanning = false;
    this.controls.zoomToCursor = true;
    this.controls.maxPolarAngle = 86 * DEG;
    this.controls.minDistance = 120;
    this.controls.maxDistance = 2_500_000;
    this.controls.rotateSpeed = 0.55;
    this.controls.zoomSpeed = 1.1;
    this.controls.keys = { LEFT: 'ArrowLeft', UP: 'ArrowUp', RIGHT: 'ArrowRight', BOTTOM: 'ArrowDown' };
    this.controls.listenToKeyEvents(window);
    this.controls.addEventListener('change', () => (this.dirty = true));
    this.controls.addEventListener('start', () => (this.anim = null));
    // prawy przycisk: własny obrót wokół punktu terenu pod kursorem (zamiast wokół środka ekranu)
    this.controls.mouseButtons.RIGHT = -1 as unknown as THREE.MOUSE;
    this.bindPivotOrbit();

    const ro = new ResizeObserver(() => this.resize());
    ro.observe(canvas.parentElement!);
    this.resize();
    requestAnimationFrame(this.loop);
  }

  /** czy ostatnie przeciągnięcie prawym przyciskiem było obrotem (wtedy bez menu kontekstowego) */
  rightDragged = false;
  private orbit: { pivot: THREE.Vector3; x: number; y: number; moved: boolean } | null = null;

  private bindPivotOrbit() {
    const c = this.canvas;
    c.addEventListener('pointerdown', (e) => {
      if (e.button !== 2 || this.panorama) return;
      const hit = this.pick(e.clientX, e.clientY);
      const pivot = hit ? new THREE.Vector3(hit.x, hit.y * this.exag, hit.z) : this.controls.target.clone();
      this.orbit = { pivot, x: e.clientX, y: e.clientY, moved: false };
      this.rightDragged = false;
      this.anim = null;
      c.setPointerCapture(e.pointerId);
    });
    c.addEventListener('pointermove', (e) => {
      const o = this.orbit;
      if (!o) return;
      const dx = e.clientX - o.x, dy = e.clientY - o.y;
      if (!o.moved && Math.hypot(dx, dy) < 4) return;
      o.moved = true;
      this.rightDragged = true;
      o.x = e.clientX;
      o.y = e.clientY;
      this.rotateAround(o.pivot, -dx * 0.0055, -dy * 0.0045);
    });
    const end = () => {
      this.orbit = null;
    };
    c.addEventListener('pointerup', end);
    c.addEventListener('pointercancel', end);
  }

  private tmpA = new THREE.Vector3();
  private tmpB = new THREE.Vector3();
  /** Obrót kamery i celu wokół punktu: odchylenie (wokół pionu) i nachylenie (wokół osi poziomej kamery). */
  private rotateAround(pivot: THREE.Vector3, yaw: number, pitch: number) {
    const cam = this.camera.position, t = this.controls.target;
    const up = new THREE.Vector3(0, 1, 0);
    const qYaw = new THREE.Quaternion().setFromAxisAngle(up, yaw);
    // oś pozioma: prawy wektor kamery rzutowany na płaszczyznę
    const fwd = this.tmpA.copy(t).sub(cam);
    const right = new THREE.Vector3().crossVectors(fwd, up).normalize();
    const qPitch = new THREE.Quaternion().setFromAxisAngle(right, pitch);
    const q = qYaw.clone().multiply(qPitch);
    const nc = this.tmpB.copy(cam).sub(pivot).applyQuaternion(q).add(pivot);
    const nt = t.clone().sub(pivot).applyQuaternion(q).add(pivot);
    // ograniczenie nachylenia: kamera nad celem, nie „przez zenit”
    const d = nc.clone().sub(nt);
    const elev = Math.asin(clamp(d.y / d.length(), -1, 1)) / DEG;
    if (elev < 3 || elev > 89) {
      // tylko odchylenie
      nc.copy(cam).sub(pivot).applyQuaternion(qYaw).add(pivot);
      nt.copy(t).sub(pivot).applyQuaternion(qYaw).add(pivot);
    }
    const g = this.groundY(nc.x, nc.z) + 30 * this.exag;
    if (nc.y < g) return;
    cam.copy(nc);
    t.copy(nt);
    this.camera.lookAt(t);
    this.dirty = true;
  }

  resize() {
    const el = this.canvas.parentElement!;
    const w = el.clientWidth, h = el.clientHeight;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  setDem(dem: Dem) {
    this.dem = dem;
    const r = Math.max(dem.widthM, dem.heightM);
    this.controls.maxDistance = Math.max(r * 1.6, 2_500_000);
    this.dirty = true;
  }

  setExag(e: number) {
    const old = this.exag;
    this.exag = e;
    this.world.scale.y = e;
    // zachowaj położenie kamery względem terenu
    const k = e / old;
    this.controls.target.y *= k;
    this.camera.position.y = this.camera.position.y * k;
    this.dirty = true;
  }

  onFrame(cb: (dt: number) => void) {
    this.frameCbs.push(cb);
  }

  /** Wysokość terenu (m n.p.m.) – z kafli LiDAR, a gdy ich brak – z modelu analitycznego. */
  heightAt(x: number, z: number) {
    const f = this.fineHeight?.(x, z);
    if (f != null) return f;
    return this.dem ? this.dem.sampleWorld(x, z) : 0;
  }

  groundY(x: number, z: number) {
    return this.heightAt(x, z) * this.exag;
  }

  private loop = () => {
    requestAnimationFrame(this.loop);
    const now = performance.now();
    const dt = Math.min(0.1, (now - this.last) / 1000);
    this.last = now;
    if (this.anim) {
      const keep = this.anim(now);
      if (!keep) this.anim = null;
      this.dirty = true;
    }
    const before = this.camera.position.clone();
    this.controls.update(dt);
    if (this.dem && !this.panorama) this.keepAboveGround();
    if (!before.equals(this.camera.position)) this.dirty = true;
    for (const cb of this.frameCbs) cb(dt);
    if (this.dirty && this.fpsCap && now - this.lastRender < 1000 / this.fpsCap - 2) return;
    if (this.dirty) {
      this.updateClipping();
      if (this.moving > 0 && this.lastRender) {
        this.ema = this.ema * 0.9 + ((now - this.lastRender) / 1000) * 0.1;
        const target = this.fpsCap ? 1 / (this.fpsCap - 3) : 1 / 32;
        if (this.adaptive && now - this.lastAdjust > 1200) {
          const floor = Math.max(0.25, this.maxPR * 0.5);
          if (this.ema > target && this.pr > floor + 0.01) this.setPR(Math.max(floor, this.pr - 0.15));
          else if (this.ema < target * 0.6 && this.pr < this.maxPR) this.setPR(Math.min(this.maxPR, this.pr + 0.1));
        }
      }
      this.lastRender = now;
      this.renderer.render(this.scene, this.camera);
      this.dirty = false;
      this.moving = 3;
    } else if (this.moving > 0) this.moving--;
  };

  /** Skala rozdzielczości renderu (0,25–1 gęstości pikseli ekranu). */
  setRenderScale(scale: number, adaptive: boolean) {
    this.maxPR = Math.max(0.2, Math.min(window.devicePixelRatio, 2) * scale);
    this.adaptive = adaptive;
    this.setPR(this.maxPR);
  }

  private setPR(pr: number) {
    this.pr = pr;
    this.lastAdjust = performance.now();
    this.renderer.setPixelRatio(pr);
    this.resize();
  }

  get isMoving() {
    return this.moving > 0 || this.anim !== null;
  }

  private keepAboveGround() {
    const t = this.controls.target;
    const dem = this.dem!;
    // cel kamery przyklejony do terenu (płynnie)
    void dem;
    const gy = this.groundY(t.x, t.z);
    if (!this.anim) {
      const dy = (gy - t.y) * 0.15;
      if (Math.abs(dy) > 0.01) {
        t.y += dy;
        this.camera.position.y += dy;
      }
    }
    const cg = this.groundY(this.camera.position.x, this.camera.position.z);
    const minClear = 40 * this.exag;
    if (this.camera.position.y < cg + minClear) this.camera.position.y = cg + minClear;
  }

  private updateClipping() {
    const cam = this.camera;
    const above = Math.max(10, cam.position.y - this.groundY(cam.position.x, cam.position.z));
    cam.near = this.panorama ? 0.5 : clamp(above * 0.05, 2, 400);
    cam.far = 20_000_000;
    cam.updateProjectionMatrix();
  }

  get view(): View {
    const t = this.controls.target, c = this.camera.position;
    const d = c.clone().sub(t);
    const distance = d.length();
    const heading = ((Math.atan2(-d.x, d.z) / DEG) + 360) % 360;
    const pitch = Math.asin(clamp(d.y / distance, -1, 1)) / DEG;
    return { x: t.x, z: t.z, distance, heading, pitch };
  }

  private placeCamera(v: View, ty?: number) {
    const t = this.controls.target;
    t.set(v.x, ty ?? this.groundY(v.x, v.z), v.z);
    const h = v.heading * DEG, p = v.pitch * DEG;
    this.camera.position.set(
      t.x - Math.sin(h) * Math.cos(p) * v.distance,
      t.y + Math.sin(p) * v.distance,
      t.z + Math.cos(h) * Math.cos(p) * v.distance
    );
    this.camera.lookAt(t);
    this.dirty = true;
  }

  setView(v: View) {
    this.anim = null;
    this.placeCamera(v);
    this.controls.update();
  }

  flyTo(target: Partial<View>, ms = 1600) {
    const from = this.view;
    const fromY = this.controls.target.y;
    const to: View = { ...from, ...target };
    const dh = ((to.heading - from.heading + 540) % 360) - 180;
    const dist = Math.hypot(to.x - from.x, to.z - from.z);
    // przy dłuższym przelocie – łuk: oddalenie w połowie drogi
    const lift = Math.min(dist * 0.6, 30000);
    const t0 = performance.now();
    const toY = this.groundY(to.x, to.z);
    this.anim = (now) => {
      const k = Math.min(1, (now - t0) / ms);
      const e = ease(k);
      const arc = Math.sin(Math.PI * e) * Math.max(0, lift - Math.max(from.distance, to.distance) * 0.4);
      this.placeCamera(
        {
          x: from.x + (to.x - from.x) * e,
          z: from.z + (to.z - from.z) * e,
          distance: from.distance + (to.distance - from.distance) * e + arc,
          heading: from.heading + dh * e,
          pitch: from.pitch + (to.pitch - from.pitch) * e,
        },
        fromY + (toY - fromY) * e
      );
      return k < 1;
    };
  }

  /** Własna animacja kamery (np. przelot nad trasą); zwraca false po zakończeniu. */
  animate(fn: (now: number) => boolean) {
    this.anim = fn;
  }
  stopAnimation() {
    this.anim = null;
  }

  lookFrom(pos: THREE.Vector3, target: THREE.Vector3) {
    this.camera.position.copy(pos);
    this.controls.target.copy(target);
    this.camera.lookAt(target);
    this.dirty = true;
  }

  /** Tryb panoramy: kamera na wysokości oczu w danym punkcie, obrót wokół własnej osi. */
  enterPanorama(x: number, z: number, heading = this.view.heading) {
    if (!this.panorama) this.savedView = this.view;
    this.panorama = true;
    this.anim = null;
    // oko nad najwyższym z sąsiednich węzłów siatki – trójkąty mogą leżeć wyżej niż interpolacja dwuliniowa
    const dem = this.dem!;
    const gx = Math.floor(dem.xToGx(x)), gy = Math.floor(dem.zToGy(z));
    let top = this.heightAt(x, z);
    if (this.fineHeight) {
      for (let j = -1; j <= 1; j++) for (let i = -1; i <= 1; i++) top = Math.max(top, this.heightAt(x + i * 2, z + j * 2));
    } else {
      for (let j = 0; j <= 1; j++) for (let i = 0; i <= 1; i++) top = Math.max(top, dem.at(gx + i, gy + j));
    }
    const eye = (top + 2) * this.exag;
    const h = heading * DEG;
    const pos = new THREE.Vector3(x, eye, z);
    const dir = new THREE.Vector3(Math.sin(h), 0.02, -Math.cos(h)).normalize();
    this.camera.position.copy(pos);
    this.controls.target.copy(pos.clone().add(dir.multiplyScalar(1)));
    this.controls.minDistance = this.controls.maxDistance = 1;
    this.controls.enablePan = false;
    this.controls.enableZoom = false;
    this.controls.maxPolarAngle = 179 * DEG;
    this.controls.rotateSpeed = 0.35;
    this.camera.fov = 55;
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  exitPanorama() {
    if (!this.panorama) return;
    this.panorama = false;
    this.controls.minDistance = 120;
    this.controls.maxDistance = 2_500_000;
    this.controls.enablePan = true;
    this.controls.enableZoom = true;
    this.controls.maxPolarAngle = 86 * DEG;
    this.controls.rotateSpeed = 0.55;
    this.camera.fov = 38;
    this.camera.updateProjectionMatrix();
    if (this.savedView) this.setView(this.savedView);
  }

  zoomFov(delta: number) {
    this.camera.fov = clamp(this.camera.fov * (delta > 0 ? 1.08 : 0.93), 8, 80);
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  /** Punkt terenu pod pikselem ekranu. */
  pick(clientX: number, clientY: number) {
    if (!this.dem) return null;
    const r = this.canvas.getBoundingClientRect();
    const ndc = new THREE.Vector2(((clientX - r.left) / r.width) * 2 - 1, -((clientY - r.top) / r.height) * 2 + 1);
    const ray = new THREE.Raycaster();
    ray.setFromCamera(ndc, this.camera);
    const o = ray.ray.origin, d = ray.ray.direction;
    return this.dem.raycast(o.x, o.y, o.z, d.x, d.y, d.z, this.exag, this.fineHeight ? (x, z) => this.heightAt(x, z) : undefined);
  }

  private v = new THREE.Vector3();
  /** Rzut punktu świata (wysokość rzeczywista) na ekran; null gdy za kamerą. */
  project(x: number, elev: number, z: number) {
    this.v.set(x, elev * this.exag, z).project(this.camera);
    if (this.v.z > 1 || this.v.z < -1) return null;
    const w = this.canvas.clientWidth, h = this.canvas.clientHeight;
    return { x: (this.v.x * 0.5 + 0.5) * w, y: (-this.v.y * 0.5 + 0.5) * h, depth: this.v.z };
  }

  /** Czy punkt jest zasłonięty przez teren z perspektywy kamery. */
  occluded(x: number, elev: number, z: number, samples = 32) {
    if (!this.dem) return false;
    const c = this.camera.position;
    const ty = elev * this.exag + 8 * this.exag;
    for (let i = 1; i < samples; i++) {
      const f = i / samples;
      const px = c.x + (x - c.x) * f, pz = c.z + (z - c.z) * f;
      const py = c.y + (ty - c.y) * f;
      if (this.groundY(px, pz) > py + 2) return true;
    }
    return false;
  }

  /** Metry na piksel w środku ekranu (do podziałki). */
  metersPerPixel() {
    const w = this.canvas.clientWidth, h = this.canvas.clientHeight;
    const r = this.canvas.getBoundingClientRect();
    const a = this.pick(r.left + w / 2, r.top + h / 2);
    const b = this.pick(r.left + w / 2 + 100, r.top + h / 2);
    if (!a || !b) return null;
    return Math.hypot(a.x - b.x, a.z - b.z) / 100;
  }

  screenshot(): string {
    this.renderer.render(this.scene, this.camera);
    return this.canvas.toDataURL('image/png');
  }
}
