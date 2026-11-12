// Drzewa z pokrycia terenu: proste bryły rysowane instancjami, kształt w całości z numeru wierzchołka.
// Blisko: korona z trzech pierścieni o sześciu bokach (kształt zależy od gatunku) i pień (132 wierzchołki);
// dalej: czworościenna piramidka (12 wierzchołków). Dalej niż kilka km drzew nie ma – las widać na teksturze.
//
// Gatunki (Inst.p.z): 0 świerk, 1 buk (liściaste o kulistej koronie), 2 kosodrzewina, 3 sosna,
// 4 modrzew, 5 brzoza, 6 drzewo parasolowate (tropiki).

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
    @location(1) p: vec4<f32>,     // wysokość drzewa (m), promień korony (m), gatunek, ziarno
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) col: vec3<f32>,
    @location(2) ao: f32,
    @location(3) dist: f32,
    @location(4) snow: f32,
};

const EARTH_R: f32 = 6371008.8;
const PI: f32 = 3.14159265;
const S: u32 = 6u;             // boki korony
const SEG_VERTS: u32 = 36u;    // pierścień: 6 boków po 2 trójkąty
const CROWN_VERTS: u32 = 108u; // 3 pierścienie
const NEAR_VERTS: u32 = 132u;  // korona + pień (4 ściany po 2 trójkąty)
const KINDS: u32 = 7u;

fn srgb(c: vec3<f32>) -> vec3<f32> { return pow(c, vec3(2.2)); }

fn hash1(x: f32) -> f32 { return fract(sin(x * 127.1) * 43758.5453); }

// Pierścień korony `s` gatunku `kind`: dół (y0, r0) i góra (y1, r1) jako ułamki wysokości drzewa
// i promienia korony.
fn profile(kind: u32, s: u32) -> vec4<f32> {
    var p = array<vec4<f32>, 21>(
        // świerk: trzy zachodzące na siebie piętra gałęzi
        vec4(0.14, 1.00, 0.50, 0.42), vec4(0.40, 0.74, 0.76, 0.26), vec4(0.66, 0.46, 1.00, 0.00),
        // buk: kulista korona na pniu
        vec4(0.30, 0.34, 0.50, 1.00), vec4(0.50, 1.00, 0.82, 0.76), vec4(0.82, 0.76, 1.00, 0.00),
        // kosodrzewina: niska kopuła przy ziemi
        vec4(0.00, 1.00, 0.45, 0.92), vec4(0.45, 0.92, 0.85, 0.52), vec4(0.85, 0.52, 1.00, 0.00),
        // sosna: wysoki goły pień, spłaszczona korona u góry
        vec4(0.56, 0.30, 0.68, 1.00), vec4(0.68, 1.00, 0.88, 0.72), vec4(0.88, 0.72, 1.00, 0.00),
        // modrzew: smukły, luźniejsze piętra
        vec4(0.16, 0.82, 0.50, 0.40), vec4(0.42, 0.60, 0.78, 0.22), vec4(0.70, 0.36, 1.00, 0.00),
        // brzoza: wąska, jajowata korona
        vec4(0.28, 0.30, 0.52, 0.82), vec4(0.52, 0.82, 0.84, 0.58), vec4(0.84, 0.58, 1.00, 0.00),
        // parasolowate: płaska, szeroka korona
        vec4(0.62, 0.22, 0.78, 1.00), vec4(0.78, 1.00, 0.91, 0.84), vec4(0.91, 0.84, 1.00, 0.00),
    );
    return p[min(kind, KINDS - 1u) * 3u + min(s, 2u)];
}

// Pień gatunku: do jakiej części wysokości sięga i jaką częścią promienia korony jest jego promień.
fn trunk(kind: u32) -> vec2<f32> {
    var t = array<vec2<f32>, 7>(
        vec2(0.22, 0.070), vec2(0.38, 0.070), vec2(0.00, 0.000), vec2(0.64, 0.075),
        vec2(0.24, 0.070), vec2(0.36, 0.050), vec2(0.70, 0.060),
    );
    return t[min(kind, KINDS - 1u)];
}

// Barwa listowia: latem i jesienią (iglaste zimozielone nie zmieniają barwy).
fn foliage(kind: u32, autumn: f32) -> vec3<f32> {
    var summer = array<vec3<f32>, 7>(
        vec3(0.10, 0.19, 0.11), vec3(0.21, 0.33, 0.12), vec3(0.16, 0.25, 0.12), vec3(0.17, 0.27, 0.14),
        vec3(0.27, 0.38, 0.14), vec3(0.30, 0.42, 0.15), vec3(0.20, 0.32, 0.11),
    );
    var fall = array<vec3<f32>, 7>(
        vec3(0.10, 0.19, 0.11), vec3(0.58, 0.32, 0.09), vec3(0.16, 0.25, 0.12), vec3(0.17, 0.27, 0.14),
        vec3(0.72, 0.52, 0.12), vec3(0.80, 0.66, 0.14), vec3(0.20, 0.32, 0.11),
    );
    let k = min(kind, KINDS - 1u);
    return mix(srgb(summer[k]), srgb(fall[k]), autumn);
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
    var tier = 1.0;
    if (vi < CROWN_VERTS) {
        // korona: pierścień `s`, bok `k`, narożnik `j` czworokąta (dwa trójkąty)
        let s = vi / SEG_VERTS;
        let k = (vi % SEG_VERTS) / 6u;
        let j = vi % 6u;
        let pr = profile(kind, s);
        var ai = k;
        var top = false;
        if (j == 1u || j == 2u || j == 4u) { ai = k + 1u; }
        if (j == 2u || j == 4u || j == 5u) { top = true; }
        let side = ai % S;
        let a = f32(side) / f32(S) * 2.0 * PI + rot;
        // nieregularny obrys: ten sam dla wszystkich pierścieni, więc stykające się pierścienie pasują
        let jit = 0.84 + 0.32 * hash1(seed * 31.7 + f32(side) * 1.37);
        let y = select(pr.x, pr.z, top) * h;
        let rr = select(pr.y, pr.w, top) * r * jit;
        lp = vec3(cos(a) * rr, y, sin(a) * rr);
        // gładkie normalne: korona wygląda na zaokrągloną mimo sześciu boków
        let slope = (pr.y - pr.w) * r / max((pr.z - pr.x) * h, 0.01);
        n = normalize(vec3(cos(a), slope, sin(a)));
        // niższe piętra ciemniejsze (cień wyższych gałęzi)
        tier = 0.84 + 0.08 * f32(s);
    } else if (vi < NEAR_VERTS) {
        // pień: 4 ściany po 2 trójkąty
        is_trunk = true;
        let q = (vi - CROWN_VERTS) / 6u;
        let j = (vi - CROWN_VERTS) % 6u;
        let t = trunk(kind);
        let tr = select(max(r * t.y, 0.10), 0.0, t.x <= 0.0);
        let a0 = f32(q) * PI * 0.5 + rot;
        let a1 = a0 + PI * 0.5;
        var a = a0;
        var y = 0.0;
        var w = 1.0;
        if (j == 1u || j == 4u || j == 5u) { a = a1; }
        // u góry pień cieńszy
        if (j == 2u || j == 3u || j == 5u) { y = t.x * h; w = 0.6; }
        n = vec3(cos(a0 + PI * 0.25), 0.0, sin(a0 + PI * 0.25));
        lp = vec3(cos(a) * tr * w, y - 0.5, sin(a) * tr * w);
    } else {
        // daleko: piramidka z 4 ścian od dołu korony do wierzchołka
        let c = (vi - NEAR_VERTS) % 3u;
        let k = (vi - NEAR_VERTS) / 3u;
        let a0 = f32(k) * PI * 0.5 + rot;
        let a1 = a0 + PI * 0.5;
        let am = a0 + PI * 0.25;
        let pr = profile(kind, 0u);
        // korony na wysokim pniu (sosna, parasolowate) zaczynają się wyżej
        let base = min(pr.x, 0.6) * h;
        n = normalize(vec3(cos(am), r / max(h - base, 0.01), sin(am)));
        if (c == 0u) { lp = vec3(cos(a0) * r, base, sin(a0) * r); }
        if (c == 1u) { lp = vec3(cos(a1) * r, base, sin(a1) * r); }
        if (c == 2u) { lp = vec3(0.0, h, 0.0); }
    }

    // drzewo tuż przy kamerze maleje do zera: w spacerze pień ani korona nie zasłaniają całego widoku
    let near_eye = smoothstep(2.5, 10.0, length(inst.pos.xz - g.eye.xz));
    lp *= near_eye;

    let world = vec3(inst.pos.x + lp.x, inst.pos.y * g.eye.w + lp.y, inst.pos.z + lp.z);
    var rel = world - g.eye.xyz;
    let dist = length(rel);
    rel.y -= dot(rel.xz, rel.xz) / (2.0 * EARTH_R) * 0.87;

    // barwa gatunku z drobnymi różnicami jasności i odcienia między drzewami
    var base_c = foliage(kind, g.season.z);
    base_c *= (0.80 + 0.40 * fract(seed * 7.13)) * tier;
    base_c = mix(base_c, base_c * vec3(1.12, 1.06, 0.80), fract(seed * 3.71) * 0.5);
    if (is_trunk) {
        base_c = srgb(vec3(0.24, 0.18, 0.13));
        if (kind == 3u) { base_c = srgb(vec3(0.44, 0.25, 0.15)); }
        if (kind == 5u) { base_c = srgb(vec3(0.84, 0.84, 0.80)); }
    }

    // śnieg na gałęziach powyżej granicy śniegu (gdy śnieg jest włączony); pień bez śniegu
    var snow = 0.0;
    if (g.season.y > 0.5 && !is_trunk) {
        snow = smoothstep(g.season.x - 60.0, g.season.x + 120.0, inst.pos.y);
    }

    var o: VOut;
    o.pos = g.view_proj * vec4(rel, 1.0);
    o.n = n;
    o.col = base_c;
    o.ao = clamp(lp.y / max(h, 0.1), 0.0, 1.0);
    o.dist = dist;
    o.snow = snow;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sun = normalize(g.sun_dir.xyz);
    let n = normalize(in.n);
    // miękkie światło listowia: strona odwrócona od słońca nie jest czarna
    let wrap = clamp(dot(n, sun) * 0.6 + 0.4, 0.0, 1.0);
    let light = g.sun_color.rgb * wrap * wrap * g.sun_dir.w * 0.95 + g.sky_color.rgb * (0.36 + 0.22 * n.y);
    var albedo = in.col;
    // śnieg leży na powierzchniach zwróconych ku górze
    albedo = mix(albedo, vec3(0.86, 0.89, 0.93), in.snow * smoothstep(0.15, 0.75, n.y) * 0.8);
    // u dołu korony ciemniej (zacienienie wnętrza lasu)
    var col = albedo * light * mix(0.50, 1.08, in.ao);
    let fog = 1.0 - exp(-in.dist * g.fog_color.w);
    col = mix(col, g.fog_color.rgb, clamp(fog, 0.0, 0.92));
    return vec4(col, 1.0);
}
