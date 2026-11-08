// Odczyt głębi obrazu 3D w jednym pikselu (pod kursorem) – do dokładnego wskazywania punktu terenu.

@group(0) @binding(0) var depth: texture_depth_multisampled_2d;
@group(0) @binding(1) var<storage, read_write> out_depth: array<f32, 4>;
@group(0) @binding(2) var<uniform> px: vec4<u32>;

@compute @workgroup_size(1)
fn main() {
    let dims = textureDimensions(depth);
    let p = vec2<i32>(min(px.xy, dims - vec2(1u)));
    // najbliższa z próbek wygładzania (krawędzie grani)
    var d = 0.0;
    for (var s = 0; s < 4; s++) {
        d = max(d, textureLoad(depth, p, s));
    }
    out_depth[0] = d;
}
