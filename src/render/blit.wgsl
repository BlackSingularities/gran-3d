// Przeniesienie obrazu 3D (bufor poza ekranem) do okna egui.

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

// Szkło interfejsu: pod paskami obraz jest rozmyty (jak matowa szyba).
struct Glass {
    rects: array<vec4<f32>, 12>,
    amount: array<vec4<f32>, 3>,
    // liczba prostokątów, 1/szerokość, 1/wysokość, promień (piksele bufora)
    info: vec4<f32>,
};
@group(0) @binding(2) var<uniform> glass: Glass;

struct BOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> BOut {
    let p = vec2(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: BOut;
    o.pos = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2(p.x, 1.0 - p.y);
    return o;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

/// Siła rozmycia w danym punkcie (0 – poza szkłem).
fn glass_amount(uv: vec2<f32>) -> f32 {
    var a = 0.0;
    let n = u32(glass.info.x);
    for (var i = 0u; i < 12u; i++) {
        if (i >= n) {
            break;
        }
        let r = glass.rects[i];
        if (uv.x >= r.x && uv.x <= r.z && uv.y >= r.y && uv.y <= r.w) {
            a = max(a, glass.amount[i / 4u][i % 4u]);
        }
    }
    return a;
}

/// Obraz w punkcie: ostry albo rozmyty (spirala próbek o złotym kącie, wagi gaussowskie).
fn scene_at(uv: vec2<f32>) -> vec3<f32> {
    let sharp = textureSampleLevel(src_tex, src_samp, uv, 0.0).rgb;
    let a = glass_amount(uv);
    if (a <= 0.001) {
        return sharp;
    }
    let texel = glass.info.yz;
    let radius = glass.info.w * a;
    var sum = sharp;
    var wsum = 1.0;
    let golden = 2.39996323;
    for (var k = 1; k < 40; k++) {
        let t = sqrt(f32(k) / 40.0);
        let ang = f32(k) * golden;
        let off = vec2(cos(ang), sin(ang)) * t * radius * texel;
        let w = exp(-2.0 * t * t);
        sum += textureSampleLevel(src_tex, src_samp, uv + off, 0.0).rgb * w;
        wsum += w;
    }
    let blur = sum / wsum;
    // lekkie przyciemnienie: tekst na szkle czytelny także nad jasnym śniegiem
    return mix(sharp, blur * 0.82, a);
}

@fragment
fn fs_blit_linear(in: BOut) -> @location(0) vec4<f32> {
    return vec4(scene_at(in.uv), 1.0);
}

@fragment
fn fs_blit_gamma(in: BOut) -> @location(0) vec4<f32> {
    return vec4(to_srgb(scene_at(in.uv)), 1.0);
}
