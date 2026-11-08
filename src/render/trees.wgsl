// Drzewa z pokrycia terenu: proste bryły rysowane instancjami.
// Blisko: dwie kondygnacje korony (świerk – stożki, buk – „romb”) i pień (66 wierzchołków);
// dalej: czworościenna piramidka (12 wierzchołków). Dalej niż kilka km drzew nie ma – las widać na teksturze.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    fog_color: vec4<f32>,
    style: vec4<f32>,
    season: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct Inst {
    @location(0) pos: vec3<f32>,   // x, wysokość terenu (m), z – w ramce
    @location(1) p: vec4<f32>,     // wysokość drzewa (m), promień korony (m), rodzaj (0 iglaste, 1 liściaste, 2 kosodrzewina), ziarno
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) col: vec3<f32>,
    @location(2) ao: f32,
    @location(3) dist: f32,
};

const EARTH_R: f32 = 6371008.8;
const PI: f32 = 3.14159265;
const S: u32 = 7u; // boki korony

fn srgb(c: vec3<f32>) -> vec3<f32> { return pow(c, vec3(2.2)); }

// Wierzchołek stożka: podstawa (y0, r0) → wierzchołek (y1, r1); `k` – numer trójkąta, `c` – narożnik.
fn frustum_vert(k: u32, c: u32, y0: f32, r0: f32, y1: f32, r1: f32, rot: f32, out_n: ptr<function, vec3<f32>>) -> vec3<f32> {
    let a0 = (f32(k) / f32(S)) * 2.0 * PI + rot;
    let a1 = (f32(k + 1u) / f32(S)) * 2.0 * PI + rot;
    let am = (a0 + a1) * 0.5;
    // normalna ściany (płaskie cieniowanie: ładnie widać bryłę)
    let slope = (r0 - r1) / max(y1 - y0, 0.01);
    *out_n = normalize(vec3(cos(am), slope, sin(am)));
    var a = a0;
    var y = y0;
    var r = r0;
    if (c == 1u) { a = a1; }
    if (c == 2u) { y = y1; r = r1; a = am; }
    return vec3(cos(a) * r, y, sin(a) * r);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    let h = inst.p.x;
    let r = inst.p.y;
    let kind = u32(inst.p.z + 0.5);
    let seed = inst.p.w;
    let rot = seed * 6.28;
    var n = vec3(0.0, 1.0, 0.0);
    var lp = vec3(0.0);
    var is_trunk = false;
    let c = vi % 3u;
    if (vi < 21u) {
        // dolna część korony
        let k = vi / 3u;
        if (kind == 1u) {
            lp = frustum_vert(k, c, 0.62 * h, r, 0.28 * h, r * 0.15, rot, &n);
            n = vec3(n.x, -n.y, n.z);
        } else {
            lp = frustum_vert(k, c, 0.18 * h, r, 0.78 * h, 0.0, rot, &n);
        }
    } else if (vi < 42u) {
        // górna część korony
        let k = (vi - 21u) / 3u;
        if (kind == 1u) {
            lp = frustum_vert(k, c, 0.62 * h, r, h, r * 0.12, rot, &n);
        } else {
            lp = frustum_vert(k, c, 0.5 * h, r * 0.68, h, 0.0, rot + 0.45, &n);
        }
    } else if (vi < 66u) {
        // pień: 4 ściany po 2 trójkąty
        is_trunk = true;
        let q = (vi - 42u) / 6u;
        let j = (vi - 42u) % 6u;
        let tr = max(r * 0.09, 0.12);
        let top = select(0.3, 0.32, kind == 1u) * h;
        let a0 = f32(q) * PI * 0.5 + rot;
        let a1 = a0 + PI * 0.5;
        var a = a0;
        var y = 0.0;
        if (j == 1u || j == 4u || j == 5u) { a = a1; }
        if (j == 2u || j == 3u || j == 5u) { y = top; }
        n = vec3(cos(a0 + PI * 0.25), 0.0, sin(a0 + PI * 0.25));
        lp = vec3(cos(a) * tr, y - 0.5, sin(a) * tr);
    } else {
        // daleko: piramidka z 4 ścian
        let k = (vi - 66u) / 3u;
        let a0 = f32(k) * PI * 0.5 + rot;
        let a1 = a0 + PI * 0.5;
        let am = a0 + PI * 0.25;
        let base = select(0.15, 0.3, kind == 1u) * h;
        n = normalize(vec3(cos(am), r / max(h - base, 0.01), sin(am)));
        if (c == 0u) { lp = vec3(cos(a0) * r, base, sin(a0) * r); }
        if (c == 1u) { lp = vec3(cos(a1) * r, base, sin(a1) * r); }
        if (c == 2u) { lp = vec3(0.0, h, 0.0); }
    }

    let world = vec3(inst.pos.x + lp.x, inst.pos.y * g.eye.w + lp.y, inst.pos.z + lp.z);
    var rel = world - g.eye.xyz;
    let dist = length(rel);
    rel.y -= dot(rel.xz, rel.xz) / (2.0 * EARTH_R) * 0.87;

    // barwa: świerk ciemny, buk jaśniejszy (jesienią rudy), kosodrzewina oliwkowa; drobne różnice między drzewami
    var base_c = srgb(vec3(0.11, 0.20, 0.11));
    if (kind == 1u) {
        base_c = mix(srgb(vec3(0.22, 0.33, 0.12)), srgb(vec3(0.55, 0.33, 0.10)), g.season.z);
    } else if (kind == 2u) {
        base_c = srgb(vec3(0.17, 0.25, 0.12));
    }
    base_c *= 0.82 + 0.36 * fract(seed * 7.13);
    if (is_trunk) {
        base_c = srgb(vec3(0.24, 0.18, 0.13));
    }

    var o: VOut;
    o.pos = g.view_proj * vec4(rel, 1.0);
    o.n = n;
    o.col = base_c;
    o.ao = clamp(lp.y / max(h, 0.1), 0.0, 1.0);
    o.dist = dist;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sun = normalize(g.sun_dir.xyz);
    let n = normalize(in.n);
    let ndl = max(dot(n, sun), 0.0);
    let light = g.sun_color.rgb * ndl * g.sun_dir.w * 0.9 + g.sky_color.rgb * (0.38 + 0.2 * n.y);
    // u dołu korony ciemniej (zacienienie wnętrza lasu)
    var col = in.col * light * mix(0.55, 1.05, in.ao);
    let fog = 1.0 - exp(-in.dist * g.fog_color.w);
    col = mix(col, g.fog_color.rgb, clamp(fog, 0.0, 0.92));
    return vec4(col, 1.0);
}
