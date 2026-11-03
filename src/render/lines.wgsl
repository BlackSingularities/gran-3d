// Linie na terenie (szlaki, trasy, pomiary): odcinki rozciągane w przestrzeni ekranu do zadanej grubości.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    fog_color: vec4<f32>,
    style: vec4<f32>,
    season: vec4<f32>,
    lens: vec4<f32>,
    lens2: vec4<f32>,
    cursor: vec4<f32>,
    shadow_rect: vec4<f32>,
    vs_rect: vec4<f32>,
    flags: vec4<f32>,
    biome: vec4<f32>,
    grid_origin: vec4<f32>,
    land_rect: vec4<f32>,    // maska pokrycia terenu: x0, z0, 1/szer., 1/wys.
};

struct LineParams {
    viewport: vec4<f32>, // szerokość, wysokość (px), uniesienie (m), zanikanie (m)
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> lp: LineParams;

struct Inst {
    @location(0) p0: vec3<f32>,
    @location(1) p1: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) wo: vec2<f32>, // grubość (px), przesunięcie w bok (px)
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) across: f32,
    @location(2) dist: f32,
};

const EARTH_R: f32 = 6371008.8;

fn clip_of(p: vec3<f32>) -> vec4<f32> {
    let world = vec3(p.x, p.y * g.eye.w + lp.viewport.z, p.z);
    var rel = world - g.eye.xyz;
    rel.y -= dot(rel.xz, rel.xz) / (2.0 * EARTH_R) * 0.87;
    return g.view_proj * vec4(rel, 1.0);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    var c0 = clip_of(inst.p0);
    var c1 = clip_of(inst.p1);
    // odcinek za kamerą: przycięcie do płaszczyzny tuż przed okiem
    let w_min = 0.05;
    if (c0.w < w_min && c1.w < w_min) {
        var o: VOut;
        o.pos = vec4(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    if (c0.w < w_min) { c0 = mix(c0, c1, (w_min - c0.w) / (c1.w - c0.w)); }
    if (c1.w < w_min) { c1 = mix(c1, c0, (w_min - c1.w) / (c0.w - c1.w)); }
    let vp = lp.viewport.xy;
    let s0 = c0.xy / c0.w * vp * 0.5;
    let s1 = c1.xy / c1.w * vp * 0.5;
    var dir = s1 - s0;
    let len = length(dir);
    dir = select(vec2(1.0, 0.0), dir / len, len > 1e-4);
    let nrm = vec2(-dir.y, dir.x);
    let corner = array<vec2<f32>, 6>(vec2(0.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(0.0, -1.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let k = corner[vi];
    var c = select(c0, c1, k.x > 0.5);
    let half_w = inst.wo.x * 0.5;
    // przedłużenie końców o pół grubości (bez szczelin na zakrętach)
    let ext = select(-dir, dir, k.x > 0.5) * half_w;
    let off = (nrm * (k.y * half_w + inst.wo.y) + ext) / (vp * 0.5) * c.w;
    c = vec4(c.xy + off, c.z, c.w);
    // linia przed terenem: ułamek odległości bliżej kamery (odwrócona głębia)
    c.z = c.z * 1.004;
    var o: VOut;
    o.pos = c;
    o.color = inst.color;
    o.across = k.y;
    o.dist = c.w;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let a = 1.0 - smoothstep(0.75, 1.0, abs(in.across));
    let fog = 1.0 - exp(-in.dist * g.fog_color.w * 0.8);
    let col = mix(in.color.rgb, g.fog_color.rgb, clamp(fog, 0.0, 0.85));
    // szlaki (cienkie) zanikają z odległością, trasy (grube) zostają
    let fade = select(1.0, 1.0 - smoothstep(lp.viewport.w * 0.5, lp.viewport.w, in.dist), in.color.a < 0.99);
    return vec4(col, min(in.color.a * 1.02, 1.0) * a * fade);
}
