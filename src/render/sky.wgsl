// Niebo (gradient z poświatą Słońca) i przeniesienie obrazu 3D do okna egui.

struct Sky {
    inv_view_proj: mat4x4<f32>,
    sun_dir: vec4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    sun_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> sky: Sky;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VOut {
    let p = vec2(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
    var o: VOut;
    o.pos = vec4(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs_sky(in: VOut) -> @location(0) vec4<f32> {
    let a = sky.inv_view_proj * vec4(in.ndc, 1.0, 1.0);
    let b = sky.inv_view_proj * vec4(in.ndc, 0.01, 1.0);
    let dir = normalize(b.xyz / b.w - a.xyz / a.w);
    let up = clamp(dir.y, -0.2, 1.0);
    var col = mix(sky.horizon.rgb, sky.zenith.rgb, pow(clamp(up, 0.0, 1.0), 0.55));
    col = mix(col, sky.horizon.rgb * 0.8, smoothstep(0.0, -0.2, up));
    let s = max(dot(dir, sky.sun_dir.xyz), 0.0);
    col += sky.sun_color.rgb * (pow(s, 900.0) * 6.0 + pow(s, 12.0) * 0.25) * sky.sun_dir.w;
    return vec4(col, 1.0);
}

