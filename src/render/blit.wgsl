// Przeniesienie obrazu 3D (bufor poza ekranem) do okna egui.

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

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

@fragment
fn fs_blit_linear(in: BOut) -> @location(0) vec4<f32> {
    return vec4(textureSample(src_tex, src_samp, in.uv).rgb, 1.0);
}

@fragment
fn fs_blit_gamma(in: BOut) -> @location(0) vec4<f32> {
    return vec4(to_srgb(textureSample(src_tex, src_samp, in.uv).rgb), 1.0);
}
