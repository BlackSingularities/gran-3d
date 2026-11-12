// GRAŃ – teren kaflowy: wysokości z kafla DEM (257×257, narożniki pikseli), gradient do cieniowania,
// style (teren, hipsometria, nachylenie, ekspozycja, satelita/ortofoto, mapa, szwajcarska, rycina, nasłonecznienie),
// mapa normalnych, cienie, soczewki (też czas dojścia), mgła.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,          // oko w ramce, w = przewyższenie
    sun_dir: vec4<f32>,      // kierunek do Słońca, w = natężenie (0 nocą)
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    fog_color: vec4<f32>,    // w = gęstość mgły
    style: vec4<f32>,        // styl, cięcie poziomic, poziomice, siatka km
    season: vec4<f32>,       // granica śniegu, śnieg, jesień, szum
    lens: vec4<f32>,         // x, z, wysokość, tryb (0 brak, 1 poziomica, 2 pas, 3 względna, 4 widoczność)
    lens2: vec4<f32>,        // pas ±m, zakres względny, czas dojścia: najdłuższy (h), długość dnia (h, nasłonecznienie)
    cursor: vec4<f32>,       // x, z, wysokość, aktywny
    shadow_rect: vec4<f32>,  // x0, z0, 1/szer, 1/wys (m ramki); w stylu nasłonecznienia – godziny słońca / 24
    vs_rect: vec4<f32>,      // widoczność albo czas dojścia (0..254 – do najdłuższego, 255 – dalej)
    flags: vec4<f32>,        // cienie, widoczność, pokrycie terenu, mapa normalnych
    biome: vec4<f32>,        // las, kosodrzewina, skały, okluzja
    grid_origin: vec4<f32>,  // siatka UTM: punkt odniesienia w ramce (x, z), E i N w nim (mod 10 km)
    land_rect: vec4<f32>,    // maska pokrycia terenu: x0, z0, 1/szer., 1/wys.
    water_rect: vec4<f32>,   // okno poziomów lustra wody: x0, z0, 1/szer., 1/wys.
    water: vec4<f32>,        // czas (s), -, -, -
    grid_lin: vec4<f32>,     // siatka UTM: dE/dx, dE/dz, dN/dx, dN/dz
    grid_q: vec4<f32>,       // E: xx/2, xz, zz/2; N: xx/2
    grid_q2: vec4<f32>,      // N: xz, zz/2; wysokość płaszczyzny siatki względem oka; płaska siatka (1/0)
};

struct Tile {
    rect: vec4<f32>,  // x0, z0, rozmiar (m), głębokość fartucha
    dem: vec4<f32>,   // przesunięcie u, v, skala w kaflu DEM, -
    img: vec4<f32>,   // przesunięcie u, v, skala w kaflu obrazu, obraz jest (1/0)
    info: vec4<f32>,  // poziom, -, -, -
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var shadow_tex: texture_2d<f32>;
@group(0) @binding(3) var vs_tex: texture_2d<f32>;
@group(0) @binding(4) var land_tex: texture_2d<f32>;
@group(0) @binding(5) var water_tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> tile: Tile;
@group(2) @binding(0) var height_tex: texture_2d<f32>;
@group(2) @binding(1) var grad_tex: texture_2d<f32>;
@group(3) @binding(0) var img_tex: texture_2d<f32>;

const EARTH_R: f32 = 6371008.8;

fn height_at(uv: vec2<f32>) -> f32 {
    let p = clamp(uv, vec2(0.0), vec2(1.0)) * 256.0;
    let i = min(vec2<i32>(floor(p)), vec2(255));
    let f = p - vec2<f32>(i);
    let a = textureLoad(height_tex, i, 0).r;
    let b = textureLoad(height_tex, i + vec2(1, 0), 0).r;
    let c = textureLoad(height_tex, i + vec2(0, 1), 0).r;
    let d = textureLoad(height_tex, i + vec2(1, 1), 0).r;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// lustro wody (OSM + wysokość brzegu): x – udział wody (0..1, gładki brzeg z interpolacji 4 komórek),
// y – poziom (m), dla komórek obok jeziora też, żeby brzeg nie miał schodków
fn water_at(wp: vec2<f32>) -> vec2<f32> {
    let uv = (wp - g.water_rect.xy) * g.water_rect.zw;
    if (g.flags.z < 0.5 || any(uv < vec2(0.0)) || any(uv >= vec2(1.0))) {
        return vec2(0.0, -10000.0);
    }
    let dim = vec2<i32>(textureDimensions(water_tex));
    let p = uv * vec2<f32>(dim) - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    var lv = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var o = array<vec2<i32>, 4>(vec2(0, 0), vec2(1, 0), vec2(0, 1), vec2(1, 1));
    var level = -10000.0;
    for (var k = 0; k < 4; k++) {
        lv[k] = textureLoad(water_tex, clamp(i + o[k], vec2(0), dim - 1), 0).r;
        level = max(level, lv[k]);
    }
    var w = array<f32, 4>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    var wet = 0.0;
    for (var k = 0; k < 4; k++) {
        wet += select(0.0, w[k], lv[k] > -1000.0);
    }
    return vec2(wet, level);
}

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) wp: vec2<f32>,
    @location(1) dem_uv: vec2<f32>,
    @location(2) img_uv: vec2<f32>,
    @location(3) dist: f32,
    @location(4) h: f32,
    @location(5) rel_y: f32, // wysokość wierzchołka względem oka (z krzywizną Ziemi)
};

@vertex
fn vs_main(@location(0) uv: vec2<f32>, @location(1) skirt: f32) -> VOut {
    let duv = tile.dem.xy + uv * tile.dem.z;
    var h = height_at(duv);
    let exag = g.eye.w;
    // tafla jeziora płasko na wysokości brzegu (model terenu ma tam dno albo szum); max – brzeg zostaje
    let wa = water_at(vec2(tile.rect.x + uv.x * tile.rect.z, tile.rect.y + uv.y * tile.rect.z));
    if (wa.x > 0.0) {
        h = max(h, wa.y);
    }
    let world = vec3(tile.rect.x + uv.x * tile.rect.z, h * exag - skirt * tile.rect.w, tile.rect.y + uv.y * tile.rect.z);
    var rel = world - g.eye.xyz;
    let hd2 = dot(rel.xz, rel.xz);
    rel.y -= hd2 / (2.0 * EARTH_R) * 0.87;
    var o: VOut;
    o.pos = g.view_proj * vec4(rel, 1.0);
    o.wp = world.xz;
    o.dem_uv = duv;
    o.img_uv = tile.img.xy + uv * tile.img.z;
    o.dist = length(rel);
    o.h = h;
    o.rel_y = rel.y;
    return o;
}

fn srgb(c: vec3<f32>) -> vec3<f32> { return pow(c, vec3(2.2)); }

fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return vnoise(p) * 0.55 + vnoise(p * 2.03 + 7.1) * 0.3 + vnoise(p * 4.1 + 3.3) * 0.15;
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(c.xxx + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return c.z * mix(vec3(1.0), clamp(p - 1.0, vec3(0.0), vec3(1.0)), c.y);
}

fn hypso(e: f32) -> vec3<f32> {
    var stops = array<vec4<f32>, 10>(
        vec4(0.0, 0.40, 0.62, 0.40), vec4(300.0, 0.55, 0.73, 0.45), vec4(600.0, 0.76, 0.82, 0.52),
        vec4(900.0, 0.93, 0.89, 0.60), vec4(1200.0, 0.94, 0.79, 0.52), vec4(1500.0, 0.87, 0.64, 0.41),
        vec4(1800.0, 0.74, 0.49, 0.32), vec4(2100.0, 0.60, 0.40, 0.31), vec4(2400.0, 0.66, 0.60, 0.60),
        vec4(2800.0, 0.97, 0.97, 0.98));
    var c = stops[0].yzw;
    for (var i = 1; i < 10; i++) {
        let a = stops[i - 1];
        let b = stops[i];
        if (e >= a.x) {
            c = mix(a.yzw, b.yzw, clamp((e - a.x) / (b.x - a.x), 0.0, 1.0));
        }
    }
    return srgb(c);
}

// kreski spadku (rycina, skały): pasy wzdłuż linii największego spadku, `cover` – udział tuszu.
// Gdy kresek jest za gęsto na ekranie, zostaje ich średni ton (bez migotania).
fn hachure(wp: vec2<f32>, grad: vec2<f32>, spacing: f32, cover: f32) -> f32 {
    let len = max(length(grad), 1e-5);
    let t = vec2(-grad.y, grad.x) / len;
    let u = dot(wp, t) / spacing;
    let fw = fwidth(u);
    let d = abs(fract(u) - 0.5);
    let hw = cover * 0.5;
    let line = 1.0 - smoothstep(hw - fw, hw + fw, d);
    return mix(line, cover, smoothstep(0.3, 0.7, fw));
}

// nasłonecznienie: udział dnia w słońcu 0..1 → granat, fiolet, czerwień, pomarańcz, żółć
fn insol_ramp(r: f32) -> vec3<f32> {
    var stops = array<vec4<f32>, 5>(
        vec4(0.0, 0.16, 0.19, 0.40), vec4(0.25, 0.40, 0.30, 0.60), vec4(0.5, 0.80, 0.38, 0.42),
        vec4(0.75, 0.97, 0.64, 0.25), vec4(1.0, 1.0, 0.94, 0.55));
    var c = stops[0].yzw;
    for (var i = 1; i < 5; i++) {
        let a = stops[i - 1];
        let b = stops[i];
        if (r >= a.x) {
            c = mix(a.yzw, b.yzw, clamp((r - a.x) / (b.x - a.x), 0.0, 1.0));
        }
    }
    return srgb(c);
}

// czas dojścia: udział najdłuższego czasu 0..1 → zieleń, żółć, pomarańcz, czerwień, fiolet
fn iso_ramp(r: f32) -> vec3<f32> {
    var stops = array<vec4<f32>, 5>(
        vec4(0.0, 0.20, 0.75, 0.45), vec4(0.25, 0.80, 0.88, 0.30), vec4(0.5, 0.98, 0.62, 0.20),
        vec4(0.75, 0.90, 0.25, 0.22), vec4(1.0, 0.55, 0.20, 0.62));
    var c = stops[0].yzw;
    for (var i = 1; i < 5; i++) {
        let a = stops[i - 1];
        let b = stops[i];
        if (r >= a.x) {
            c = mix(a.yzw, b.yzw, clamp((r - a.x) / (b.x - a.x), 0.0, 1.0));
        }
    }
    return srgb(c);
}

fn in_rect(wp: vec2<f32>, r: vec4<f32>) -> vec3<f32> {
    let uv = (wp - r.xy) * r.zw;
    let inside = select(0.0, 1.0, all(uv >= vec2(0.0)) && all(uv <= vec2(1.0)));
    return vec3(uv, inside);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let exag = g.eye.w;
    let style = i32(g.style.x + 0.5);
    let wa = water_at(in.wp);
    let bed = height_at(in.dem_uv);
    // brzeg z wygładzonej maski, ostry na ekranie niezależnie od odległości
    let wedge = max(fwidth(wa.x), 0.02);
    let wet = smoothstep(0.5 - wedge, 0.5 + wedge, wa.x);
    let on_water = wet > 0.5;
    let depth = max(wa.y - bed, 0.0);
    let h = select(bed, max(bed, wa.y), wa.x > 0.0);
    let grad = select(textureSample(grad_tex, samp, in.dem_uv).xy, vec2(0.0), on_water);
    // lekka mapa normalnych z bliska, we wszystkich stylach ta sama: drobny szum w metrach terenu,
    // zmienia tylko normalną (światło), nie kształt ani położenie. Na zdjęciu nie z jasności obrazu –
    // tłoczenie zdjęcia dawało podwójne krawędzie, jakby teren był przesunięty względem obrazu.
    let detail_on = g.flags.w * (1.0 - smoothstep(2000.0, 30000.0, in.dist));
    let photo = style == 4 && tile.img.w > 0.5;
    var detail = vec2(0.0);
    // liczone tylko tam, gdzie widać efekt (włączone i bliżej niż 30 km) – oszczędza kartę graficzną
    if (detail_on > 0.0) {
        let mx = fbm((in.wp + vec2(2.0, 0.0)) / 11.0) - fbm((in.wp - vec2(2.0, 0.0)) / 11.0);
        let mz = fbm((in.wp + vec2(0.0, 2.0)) / 11.0) - fbm((in.wp - vec2(0.0, 2.0)) / 11.0);
        detail = vec2(mx, mz) * 2.2 * detail_on;
    }
    let n = normalize(vec3(-grad.x * exag - detail.x, 1.0, -grad.y * exag - detail.y));
    let slope = length(grad);
    let slope_deg = degrees(atan(slope));
    // ekspozycja: kierunek spadku (0 = N)
    let aspect = (degrees(atan2(-grad.x, grad.y)) + 360.0) % 360.0;
    let north = cos(radians(aspect));
    let wp = in.wp;
    let sun = g.sun_dir.xyz;

    // cień rzucany (mapa z analizy) i światło
    let sr = in_rect(wp, g.shadow_rect);
    let shadow_s = textureSample(shadow_tex, samp, clamp(sr.xy, vec2(0.0), vec2(1.0))).r;
    let vr = in_rect(wp, g.vs_rect);
    let vs_s = textureSample(vs_tex, samp, clamp(vr.xy, vec2(0.0), vec2(1.0))).r;
    // widoczność wygładzona i jej granica (pochodne poza rozgałęzieniami)
    let vs_v = smoothstep(0.0, 0.012, vs_s);
    let vs_fw = max(fwidth(vs_v), 1e-4);
    let img_s = textureSample(img_tex, samp, in.img_uv).rgb;
    let lr = in_rect(wp, g.land_rect);
    let land_s = textureSample(land_tex, samp, clamp(lr.xy, vec2(0.0), vec2(1.0)));
    // pokrycie z OSM: R woda, G las, B skały (1) / zarośla (~0,43)
    let land = select(vec4(0.0), land_s, g.flags.z > 0.5 && lr.z > 0.5 && land_s.a > 0.5);
    let has_land = g.flags.z > 0.5 && lr.z > 0.5 && land_s.a > 0.5;
    var shadow = 1.0;
    // w stylu nasłonecznienia tekstura cieni niesie godziny słońca, nie cień chwili
    if (g.flags.x > 0.5 && sr.z > 0.5 && style != 8) {
        shadow = shadow_s;
    }
    let ndl = max(dot(n, sun), 0.0);
    let sky_amb = mix(0.55, 1.0, n.y * 0.5 + 0.5);
    let light = g.sun_color.rgb * ndl * shadow * g.sun_dir.w + g.sky_color.rgb * sky_amb * 0.42;

    var base = vec3(0.5);
    var lit = true;
    let snowline = g.season.x;
    let autumn = g.season.z;
    let noise_amt = g.season.w;
    var water = 0.0;
    // tusz rysowany na wierzchu po cieniowaniu (kreski skał, ryciny, linie godzin) i jego barwa
    var ink = 0.0;
    var ink_col = vec3(0.0);
    // lodowiec (styl szwajcarski) – niebieskie poziomice
    var glacier = 0.0;
    let paper_style = style == 5 || style == 6 || style == 7;

    if (style == 0 || style == 4) {
        // piętra roślinności zależne od wysokości, nachylenia i ekspozycji
        let jitter = (fbm(wp / 180.0) - 0.5) * 140.0 + north * 60.0;
        let e = h + jitter;
        let meadow_low = mix(srgb(vec3(0.44, 0.52, 0.26)), srgb(vec3(0.58, 0.58, 0.34)), vnoise(wp / 260.0));
        let beech = mix(srgb(vec3(0.20, 0.29, 0.12)), srgb(vec3(0.55, 0.33, 0.12)), autumn * smoothstep(0.35, 0.7, vnoise(wp / 90.0)));
        let spruce = srgb(vec3(0.10, 0.17, 0.10)) * (0.8 + 0.4 * vnoise(wp / 9.0));
        let pine = mix(srgb(vec3(0.17, 0.25, 0.11)), srgb(vec3(0.24, 0.31, 0.14)), vnoise(wp / 12.0));
        var alpine = mix(srgb(vec3(0.46, 0.47, 0.28)), srgb(vec3(0.58, 0.45, 0.26)), autumn);
        alpine = mix(alpine, srgb(vec3(0.52, 0.52, 0.40)), vnoise(wp / 40.0) * 0.5);
        var rock = mix(srgb(vec3(0.44, 0.43, 0.41)), srgb(vec3(0.58, 0.56, 0.52)), fbm(wp / 25.0));
        rock = mix(rock, srgb(vec3(0.36, 0.33, 0.30)), smoothstep(0.9, 1.2, slope) * 0.6);
        let forest_line = g.biome.x;
        let shrub_line = g.biome.y;
        let rock_line = g.biome.z;
        // las: regiel dolny (buk) → górny (świerk)
        let forest_mix = smoothstep(forest_line * 0.62, forest_line * 0.78, e);
        var c = mix(beech, spruce, forest_mix);
        // polany i hale w dolinach płaskich
        let clearing = smoothstep(0.55, 0.75, vnoise(wp / 420.0)) * (1.0 - smoothstep(0.15, 0.35, slope));
        c = mix(c, meadow_low, clearing * (1.0 - smoothstep(forest_line - 80.0, forest_line, e)) * 0.8);
        // niziny poniżej regla: łąki i pola
        c = mix(meadow_low, c, smoothstep(450.0, 650.0, e) * 0.7 + 0.3 * smoothstep(0.1, 0.3, slope));
        // kosodrzewina
        c = mix(c, pine, smoothstep(forest_line - 40.0, forest_line + 40.0, e));
        // hale
        c = mix(c, alpine, smoothstep(shrub_line - 40.0, shrub_line + 60.0, e));
        // turnie i piargi: wysoko albo stromo
        let rock_mask = max(smoothstep(rock_line - 60.0, rock_line + 160.0, e) * smoothstep(0.25, 0.6, slope + 0.2),
                            smoothstep(0.95, 1.4, slope));
        c = mix(c, rock, clamp(rock_mask, 0.0, 1.0));
        if (has_land) {
            // rzeczywiste lasy, kosodrzewina, piargi i wody z OSM zamiast przybliżenia z wysokości
            let below = 1.0 - smoothstep(forest_line - 60.0, forest_line + 60.0, e);
            let forest_c = mix(beech, spruce, forest_mix);
            // kanał G niesie też rodzaj drzew (0,7–1,0): każda z tych wartości to pełny las
            c = mix(c, mix(meadow_low, forest_c, smoothstep(0.3, 0.62, land.g)), below * 0.85);
            let scrub = smoothstep(0.3, 0.5, land.b) * (1.0 - smoothstep(0.7, 0.9, land.b));
            c = mix(c, pine, scrub * 0.9);
            c = mix(c, rock, smoothstep(0.8, 1.0, land.b) * 0.9);
            if (land.r > 0.5 && slope < 0.15) {
                water = 1.0;
            }
        }
        if (on_water) {
            water = 1.0;
        }
        // śnieg: od granicy (dłużej na północnych stokach i w żlebach), nie na ścianach
        if (g.season.y > 0.5) {
            let sline = snowline - north * 180.0 + (fbm(wp / 60.0) - 0.5) * 160.0;
            let snow = smoothstep(sline - 40.0, sline + 60.0, h) * (1.0 - smoothstep(0.9, 1.35, slope));
            c = mix(c, srgb(vec3(0.93, 0.95, 0.98)), snow);
        }
        base = c;
        if (style == 4 && tile.img.w > 0.5) {
            // zdjęcie ma już światło i cienie z chwili wykonania – słabsze cieniowanie
            base = img_s * 1.05;
        }
        // woda: płaskie miejsca na niskich wysokościach (morza, jeziora z modelu)
        if (h < 0.5 && slope < 0.01) {
            water = 1.0;
        }
    } else if (style == 1) {
        base = hypso(h);
    } else if (style == 2) {
        var c = srgb(vec3(0.94, 0.93, 0.89));
        if (slope_deg >= 45.0) { c = srgb(vec3(0.33, 0.18, 0.46)); }
        else if (slope_deg >= 40.0) { c = srgb(vec3(0.63, 0.20, 0.55)); }
        else if (slope_deg >= 35.0) { c = srgb(vec3(0.89, 0.18, 0.15)); }
        else if (slope_deg >= 30.0) { c = srgb(vec3(0.98, 0.56, 0.12)); }
        else if (slope_deg >= 25.0) { c = srgb(vec3(0.99, 0.87, 0.30)); }
        else if (slope_deg >= 15.0) { c = srgb(vec3(0.84, 0.90, 0.72)); }
        base = c;
    } else if (style == 3) {
        let sat = clamp(slope * 2.5, 0.0, 0.85);
        base = srgb(hsv2rgb(vec3(aspect / 360.0, sat, 0.93)));
    } else if (style == 6) {
        // szwajcarska: jasny papier, lasy szarozielone, skały szare z kreskami, lodowce błękitne
        let paper = srgb(vec3(0.96, 0.96, 0.93));
        let wood = srgb(vec3(0.74, 0.83, 0.66));
        var c = mix(wood, paper, smoothstep(g.biome.x - 60.0, g.biome.x + 60.0, h));
        var osm_rock = 0.0;
        if (has_land) {
            c = mix(paper, wood, smoothstep(0.3, 0.62, land.g));
            c = mix(c, srgb(vec3(0.84, 0.88, 0.76)), smoothstep(0.3, 0.5, land.b) * (1.0 - smoothstep(0.7, 0.9, land.b)) * 0.8);
            osm_rock = smoothstep(0.8, 1.0, land.b);
            if (land.r > 0.5 && slope < 0.15) {
                water = 1.0;
            }
        }
        if (on_water) {
            water = 1.0;
        }
        let rk = clamp(smoothstep(0.85, 1.25, slope) + smoothstep(g.biome.z, g.biome.z + 250.0, h) * smoothstep(0.4, 0.8, slope) + osm_rock, 0.0, 1.0);
        c = mix(c, srgb(vec3(0.87, 0.86, 0.84)), rk);
        // lodowce: wysoko ponad granicą skał, na niezbyt stromym terenie (brzeg nieregularny)
        let gl_e = h + (fbm(wp / 300.0) - 0.5) * 220.0;
        glacier = smoothstep(g.biome.z + 700.0, g.biome.z + 820.0, gl_e) * (1.0 - smoothstep(0.75, 1.05, slope));
        c = mix(c, srgb(vec3(0.89, 0.94, 0.99)), glacier);
        base = c;
        // skały: gęste kreski spadku, grubsze na stromiznach
        let strokes = hachure(wp, grad, g.style.y * 0.6, mix(0.25, 0.5, smoothstep(0.8, 1.6, slope)));
        ink = strokes * rk * (1.0 - glacier) * 0.75;
        ink_col = srgb(vec3(0.33, 0.32, 0.33));
    } else if (style == 7) {
        // rycina: biały papier z lekkim ziarnem, lasy ledwie zaznaczone, kreski spadku
        var c = srgb(vec3(0.97, 0.96, 0.92)) * (0.97 + 0.04 * fbm(wp / 70.0));
        if (has_land) {
            c = mix(c, srgb(vec3(0.89, 0.91, 0.84)), smoothstep(0.3, 0.62, land.g) * 0.8);
            if (land.r > 0.5 && slope < 0.15) {
                water = 1.0;
            }
        }
        if (on_water) {
            water = 1.0;
        }
        base = c;
        // kreski spadku (Lehmann): im stromiej, tym grubsze; przerwy na każdej warstwicy jak przy rysowaniu piórem
        let lvl = g.style.y * 0.5;
        let seg = fract(h / lvl);
        let seg_fw = fwidth(h / lvl);
        let gap = smoothstep(0.86 - seg_fw, 0.86 + seg_fw, seg) * (1.0 - smoothstep(0.98 - seg_fw, 0.98 + seg_fw, seg));
        let cover = smoothstep(0.05, 1.2, slope) * 0.6;
        ink = hachure(wp, grad, g.style.y * 0.7, cover) * (1.0 - gap * (1.0 - smoothstep(0.3, 0.7, seg_fw))) * 0.85;
        ink_col = srgb(vec3(0.24, 0.19, 0.14));
    } else if (style == 8) {
        // nasłonecznienie: godziny słońca z tekstury cieni, odniesione do długości dnia
        let hours = shadow_s * 24.0;
        let hfw = max(fwidth(hours), 1e-3);
        let r = clamp(hours / max(g.lens2.w, 0.5), 0.0, 1.0);
        base = select(srgb(vec3(0.62, 0.62, 0.64)), insol_ramp(r), sr.z > 0.5);
        // linie co godzinę (bez linii zera – cały dzień w cieniu to pole, nie kontur)
        let hl = 1.0 - smoothstep(0.0, hfw * 1.3, abs(fract(hours + 0.5) - 0.5));
        ink = hl * 0.45 * sr.z * smoothstep(0.4, 0.8, hours) * (1.0 - smoothstep(0.25, 0.5, hfw));
        ink_col = vec3(1.0);
    } else {
        // mapa turystyczna: papier, lasy i skały delikatnie
        let paper = srgb(vec3(0.97, 0.95, 0.90));
        let wood = srgb(vec3(0.80, 0.89, 0.70));
        var c = mix(wood, paper, smoothstep(g.biome.x - 60.0, g.biome.x + 60.0, h));
        if (has_land) {
            c = mix(paper, wood, smoothstep(0.3, 0.62, land.g));
            if (land.r > 0.5 && slope < 0.15) {
                water = 1.0;
            }
        }
        if (on_water) {
            water = 1.0;
        }
        let rk = smoothstep(0.9, 1.3, slope) + smoothstep(g.biome.z, g.biome.z + 250.0, h) * smoothstep(0.4, 0.8, slope);
        c = mix(c, srgb(vec3(0.80, 0.79, 0.78)), clamp(rk, 0.0, 1.0));
        base = c;
    }

    var col: vec3<f32>;
    if (style == 4 && tile.img.w > 0.5) {
        // zdjęcie: łagodne dodatkowe światło z rzeźby, żeby bryła była czytelna
        let shade = mix(0.7, 1.16, ndl) * mix(0.6, 1.0, shadow);
        col = base * shade * mix(vec3(1.0), g.sun_color.rgb * 0.8 + g.sky_color.rgb * 0.35, 0.35);
    } else if (style == 0) {
        col = base * light;
    } else if (style == 6) {
        // cieniowanie szwajcarskie: światło z północnego zachodu, stoki w cieniu szaroniebieskie, oświetlone ciepłe
        let nc = normalize(vec3(-(grad.x + detail.x) * 1.8, 1.0, -(grad.y + detail.y) * 1.8));
        let l = max(dot(nc, normalize(vec3(-0.6, 0.75, -0.6))), 0.0) / 0.665;
        let shade = mix(srgb(vec3(0.50, 0.56, 0.62)), vec3(1.0), clamp(l, 0.0, 1.0));
        let warm = clamp(l - 1.0, 0.0, 0.5) * srgb(vec3(1.0, 0.96, 0.80)) * 0.35;
        col = base * shade + warm * base;
    } else if (style == 7) {
        // rycina: tylko lekki ton cieniowania pod kreskami
        let nc = normalize(vec3(-grad.x * 1.4, 1.0, -grad.y * 1.4));
        let l = max(dot(nc, normalize(vec3(-0.6, 0.75, -0.6))), 0.0) / 0.665;
        col = base * mix(0.9, 1.03, clamp(l, 0.0, 1.5) / 1.5);
    } else {
        // style analityczne: cieniowanie z wielu kierunków (czytelna rzeźba), bez kolorów pory dnia
        let nc = normalize(vec3(-(grad.x + detail.x) * 1.6, 1.0, -(grad.y + detail.y) * 1.6));
        let h1 = max(dot(nc, normalize(vec3(-0.6, 0.7, -0.6))), 0.0);
        let h2 = max(dot(nc, normalize(vec3(-0.9, 0.9, 0.1))), 0.0);
        let h3 = max(dot(nc, normalize(vec3(0.0, 0.8, -0.8))), 0.0);
        let hs = h1 * 0.55 + h2 * 0.25 + h3 * 0.2;
        col = base * (0.42 + 0.7 * hs) * mix(0.75, 1.0, shadow);
    }
    col = mix(col, ink_col, clamp(ink, 0.0, 1.0));

    if (water > 0.5 && !(wa.x > 0.0)) {
        // woda z samej maski (bez poziomu lustra) – jak dotąd
        var wc = select(srgb(vec3(0.16, 0.56, 0.66)), srgb(vec3(0.62, 0.80, 0.93)), paper_style);
        if (style == 7) {
            wc = srgb(vec3(0.82, 0.88, 0.92));
        }
        col = select(wc * (0.4 + 0.6 * light), wc, style != 0 && style != 4);
    }
    if (wa.x > 0.0 && (style == 0 || style == 4 || paper_style)) {
        var wcol: vec3<f32>;
        if (paper_style) {
            wcol = select(srgb(vec3(0.62, 0.80, 0.93)), srgb(vec3(0.82, 0.88, 0.92)), style == 7);
        } else {
            // fale: dwie warstwy szumu przesuwane w czasie (normalne), słabsze z daleka
            let t = g.water.x;
            let near = 1.0 - smoothstep(800.0, 9000.0, in.dist);
            let q1 = in.wp / 7.0 + vec2(t * 0.35, t * 0.22);
            let q2 = in.wp / 3.1 - vec2(t * 0.5, -t * 0.31);
            let e = 0.15;
            let wx = (vnoise(q1 + vec2(e, 0.0)) - vnoise(q1 - vec2(e, 0.0))) + 0.5 * (vnoise(q2 + vec2(e, 0.0)) - vnoise(q2 - vec2(e, 0.0)));
            let wz = (vnoise(q1 + vec2(0.0, e)) - vnoise(q1 - vec2(0.0, e))) + 0.5 * (vnoise(q2 + vec2(0.0, e)) - vnoise(q2 - vec2(0.0, e)));
            let wn = normalize(vec3(-wx * 0.9 * near, 1.0, -wz * 0.9 * near));
            let p3 = vec3(in.wp.x, h * exag, in.wp.y);
            let v = normalize(g.eye.xyz - p3);
            // Fresnel (Schlick, woda n≈1.33)
            let ndv = clamp(dot(wn, v), 0.0, 1.0);
            let fres = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
            // odbicie nieba: horyzont → zenit
            let r = reflect(-v, wn);
            let sky = mix(g.fog_color.rgb, g.sky_color.rgb * 1.6, smoothstep(0.0, 0.6, r.y));
            // kolor toni: górskie stawy są przejrzyste i lazurowe – płytko jasny turkus, w głębi
            // nasycony błękit (tatrzańskie stawy: Morskie Oko, Czarny Staw, Pięć Stawów)
            let d = 1.0 - exp(-depth / 12.0);
            let deep = srgb(vec3(0.02, 0.26, 0.38));
            let shallow = srgb(vec3(0.20, 0.62, 0.62));
            let lit = clamp(g.sky_color.rgb * 0.75 + g.sun_color.rgb * 0.55 * g.sun_dir.w, vec3(0.25), vec3(1.4));
            var body = mix(shallow, deep, d) * lit;
            var refl = 0.7;
            if (photo) {
                // na zdjęciu: barwa wody z samego zdjęcia zbiornika (każdy staw ma swoją – turkus,
                // granat, zieleń, mętny brąz), lekko nasycona i przyciemniona w głąb
                let tone = img_s * 1.05;
                let l = dot(tone, vec3(0.3, 0.59, 0.11));
                let rich = max(mix(vec3(l), tone, 1.3), vec3(0.0));
                body = rich * mix(1.0, 0.82, d);
                refl = 0.35;
            }
            // odblask słońca
            let hv = normalize(sun + v);
            let spec = pow(max(dot(wn, hv), 0.0), 380.0) * 7.0 * g.sun_dir.w * shadow;
            wcol = mix(body, sky, fres * refl) + g.sun_color.rgb * spec;
            // jasna linia brzegu (płycizna i piana), tylko z bliska
            let shore = (1.0 - smoothstep(0.0, 1.2, depth)) * near * 0.25;
            wcol = mix(wcol, wcol * 1.25 + vec3(0.04), shore);
        }
        // delikatnie przezroczysta: przy brzegu i na płyciźnie prześwituje dno, w głębi prawie pełny kolor
        let clear = 1.0 - exp(-depth / 7.0);
        let opacity = select(mix(0.62, 0.9, clear), 1.0, paper_style);
        col = mix(col, wcol, wet * opacity);
    }

    // soczewka / wysokość względna
    let lens_mode = i32(g.lens.w + 0.5);
    if (lens_mode > 0) {
        let rd = distance(wp, g.lens.xy);
        let dh = h - g.lens.z;
        let px_m = max(fwidth(h), 0.05);
        if (lens_mode == 1 || lens_mode == 2) {
            // ta sama wysokość: pas ± tolerancji albo izolinia
            let tol = max(g.lens2.x, 0.0);
            if (lens_mode == 2 && tol > 0.0) {
                let inside = 1.0 - smoothstep(tol - px_m, tol + px_m, abs(dh));
                let lum = vec3(dot(col, vec3(0.3, 0.59, 0.11)));
                col = mix(col * 0.55 + lum * 0.1, col * vec3(1.18, 1.08, 0.9) + vec3(0.07, 0.045, 0.0), inside);
                let edge = 1.0 - smoothstep(0.0, px_m * 1.8, abs(abs(dh) - tol));
                col = mix(col, vec3(1.0, 0.78, 0.35), edge);
            }
            let line = 1.0 - smoothstep(0.0, px_m * 1.6, abs(dh));
            col = mix(col, vec3(1.0, 0.86, 0.45), line);
        } else if (lens_mode == 3) {
            let t = clamp(dh / max(g.lens2.y, 10.0), -1.0, 1.0);
            var above: vec3<f32>;
            if (t < 0.5) {
                above = mix(srgb(vec3(0.98, 0.94, 0.80)), srgb(vec3(0.97, 0.60, 0.22)), t * 2.0);
            } else {
                above = mix(srgb(vec3(0.97, 0.60, 0.22)), srgb(vec3(0.62, 0.08, 0.10)), (t - 0.5) * 2.0);
            }
            let below = mix(srgb(vec3(0.82, 0.93, 0.95)), srgb(vec3(0.04, 0.22, 0.45)), -t);
            let nc = normalize(vec3(-grad.x * 1.4, 1.0, -grad.y * 1.4));
            let shade = mix(0.55, 1.1, max(dot(nc, normalize(vec3(-0.6, 0.75, -0.5))), 0.0));
            col = select(below, above, t >= 0.0) * shade;
            // izolinia poziomu odniesienia
            let z0 = 1.0 - smoothstep(0.0, px_m * 1.8, abs(dh));
            col = mix(col, vec3(1.0), z0 * 0.95);
            // pierścienie odległości co 1 km
            let ring_w = max(fwidth(rd), 0.5) * 1.2;
            let ring = (1.0 - smoothstep(0.0, ring_w, abs(fract(rd / 1000.0 + 0.5) - 0.5) * 1000.0)) * 0.35;
            col = mix(col, vec3(1.0), ring);
        } else if (lens_mode == 5) {
            // czas dojścia z punktu: barwa – czas marszu, linie co pół godziny (gęściej z bliska)
            let maxh = max(g.lens2.z, 0.25);
            let reach = (1.0 - smoothstep(253.5 / 255.0, 254.5 / 255.0, vs_s)) * vr.z;
            let hours = vs_s * 255.0 / 254.0 * maxh;
            let lum = vec3(dot(col, vec3(0.3, 0.59, 0.11)));
            let dim = mix(col, lum, 0.7) * vec3(0.42, 0.45, 0.55);
            let tint = mix(col, iso_ramp(clamp(hours / maxh, 0.0, 1.0)) * (0.55 + 0.6 * lum), 0.62);
            col = mix(dim, tint, reach);
            let hfw = max(fwidth(hours), 1e-4);
            let l30 = 1.0 - smoothstep(0.0, hfw * 1.4, abs(fract(hours * 2.0 + 0.5) - 0.5) / 2.0);
            let l1 = 1.0 - smoothstep(0.0, hfw * 2.2, abs(fract(hours + 0.5) - 0.5));
            let l15 = (1.0 - smoothstep(0.0, hfw * 1.2, abs(fract(hours * 4.0 + 0.5) - 0.5) / 4.0)) * (1.0 - smoothstep(0.02, 0.05, hfw));
            let iso = max(max(l1 * 0.95, l30 * 0.6), l15 * 0.3) * reach * (1.0 - smoothstep(0.15, 0.35, hfw));
            col = mix(col, vec3(1.0, 0.98, 0.92), iso);
        }
        // znacznik źródła soczewki
        let mk = 1.0 - smoothstep(0.0, 40.0 + in.dist * 0.004, rd);
        col += vec3(1.0, 0.9, 0.7) * mk * 0.6;
    }

    // pole widoczności
    if (g.flags.y > 0.5) {
        let lum = vec3(dot(col, vec3(0.3, 0.59, 0.11)));
        // niewidoczne: wyblakłe i przygaszone w chłodnym tonie; widoczne: naturalne barwy, lekko rozjaśnione
        let hidden = mix(col, lum, 0.7) * vec3(0.40, 0.45, 0.58);
        if (vr.z > 0.5) {
            let seen = col * 1.1 + srgb(vec3(0.03, 0.035, 0.01));
            col = mix(hidden, seen, vs_v);
            // złota linia granicy widoczności
            let edge = 1.0 - smoothstep(0.0, 1.0, abs(vs_v - 0.5) / vs_fw);
            col = mix(col, srgb(vec3(1.0, 0.82, 0.32)), edge * 0.9);
        } else {
            col = hidden;
        }
    }

    // poziomice
    let sketch = style == 7;
    if (g.style.z > 0.5 || sketch) {
        let interval = g.style.y * select(1.0, 0.5, sketch);
        let fw = max(fwidth(h), 0.001);
        let d = abs(fract(h / interval + 0.5) - 0.5) * interval;
        var line = 1.0 - smoothstep(0.0, fw * 1.2, d);
        // co piąta pogrubiona
        let d5 = abs(fract(h / (interval * 5.0) + 0.5) - 0.5) * interval * 5.0;
        let major = 1.0 - smoothstep(0.0, fw * 2.0, d5);
        line = max(line * 0.55, major * 0.9);
        // znikanie gdy zbyt gęsto
        line *= 1.0 - smoothstep(interval * 0.25, interval * 0.6, fw);
        let realistic = style == 0 || style == 4;
        var cc = select(srgb(vec3(0.55, 0.33, 0.16)), vec3(0.96, 0.92, 0.82), realistic);
        if (style == 6) {
            cc = mix(srgb(vec3(0.60, 0.42, 0.24)), srgb(vec3(0.22, 0.48, 0.78)), glacier);
        }
        if (sketch) {
            cc = srgb(vec3(0.24, 0.19, 0.14));
        }
        col = mix(col, cc, line * select(select(0.75, 0.55, realistic), 0.8, sketch));
    }

    // siatka UTM: linie co 100 m (z bliska), 1 km i 10 km (pogrubione), zgodne z mapami topograficznymi
    // Linie są proste: siatka leży na poziomej płaszczyźnie na dnie terenu w okolicy i jest rzutowana
    // promieniem z oka (nie układa się na stokach, więc nie faluje z rzeźbą i pokazuje kierunek).
    // W widoku z ziemi (spacer) płaszczyzna byłaby tuż pod okiem – tam siatka zostaje na terenie.
    if (g.style.w > 0.5) {
        let planar = g.grid_q2.w > 0.5;
        let rxz = wp - g.eye.xz;
        // wysokość z siatki wierzchołków (nie z tekstury wysokości): dokładnie ten punkt, który widać,
        // więc linie nie drgają na drobnej rzeźbie
        let ry = in.rel_y;
        let gp = select(wp, g.eye.xz + rxz * (g.grid_q2.z / min(ry, -0.001)), planar);
        let g_on = select(1.0, 0.0, planar && ry > -0.001);
        let d = gp - g.grid_origin.xy;
        let e = g.grid_origin.z + g.grid_lin.x * d.x + g.grid_lin.y * d.y
            + g.grid_q.x * d.x * d.x + g.grid_q.y * d.x * d.y + g.grid_q.z * d.y * d.y;
        let n = g.grid_origin.w + g.grid_lin.z * d.x + g.grid_lin.w * d.y
            + g.grid_q.w * d.x * d.x + g.grid_q2.x * d.x * d.y + g.grid_q2.y * d.y * d.y;
        let en = vec2(e, n);
        let k01 = en / 100.0;
        let k1 = en / 1000.0;
        let k10 = en / 10000.0;
        let fw01 = fwidth(k01);
        let fw1 = fwidth(k1);
        let fw10 = fwidth(k10);
        let l01 = 1.0 - smoothstep(vec2(0.0), fw01 * 1.2, abs(fract(k01 + 0.5) - 0.5));
        let l1 = 1.0 - smoothstep(vec2(0.0), fw1 * 1.5, abs(fract(k1 + 0.5) - 0.5));
        let l10 = 1.0 - smoothstep(vec2(0.0), fw10 * 2.4, abs(fract(k10 + 0.5) - 0.5));
        // zanikanie, gdy linie są zbyt gęsto
        let f01 = 1.0 - smoothstep(0.06, 0.2, max(fw01.x, fw01.y));
        let f1 = 1.0 - smoothstep(0.08, 0.25, max(fw1.x, fw1.y));
        let f10 = 1.0 - smoothstep(0.08, 0.25, max(fw10.x, fw10.y));
        let gl = max(max(max(l01.x, l01.y) * 0.16 * f01, max(l1.x, l1.y) * 0.42 * f1), max(l10.x, l10.y) * 0.7 * f10);
        col = mix(col, vec3(0.95, 0.9, 0.8), gl * g_on);
    }

    // kursor: mały pierścień na terenie
    if (g.cursor.w > 0.5) {
        let cd = distance(wp, g.cursor.xy);
        let r = 18.0 + in.dist * 0.006;
        let ring = 1.0 - smoothstep(0.0, r * 0.18, abs(cd - r));
        col = mix(col, vec3(1.0, 0.95, 0.85), ring * 0.9);
    }

    // perspektywa powietrzna (mgła zależna od odległości i wysokości)
    let fog = 1.0 - exp(-in.dist * g.fog_color.w * (1.0 + max(0.0, 900.0 - h) / 1800.0));
    col = mix(col, g.fog_color.rgb, clamp(fog, 0.0, 0.92));
    return vec4(col, 1.0);
}
