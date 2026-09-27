@group(0) @binding(0) var<uniform> parameters: vec4<u32>;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= parameters.z { return; }
    let packed = source[id.x];
    let amounts = bitcast<vec2<f32>>(parameters.xy);
    if (packed >> 24u) == 0u || all(amounts == vec2(0.0)) { output[id.x] = packed; return; }
    let input = vec3<f32>(f32(packed & 255u), f32((packed >> 8u) & 255u), f32((packed >> 16u) & 255u)) / 255.0;
    let rgb = vibrance_rgb(input, amounts.x, amounts.y);
    let encoded = vec3<u32>(floor(rgb * 255.0 + 0.5));
    output[id.x] = encoded.x | (encoded.y << 8u) | (encoded.z << 16u) | (packed & 0xff000000u);
}
