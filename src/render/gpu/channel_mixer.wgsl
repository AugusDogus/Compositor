struct Parameters {
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    extent: vec4<u32>,
}
@group(0) @binding(0) var<uniform> settings: Parameters;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= settings.extent.x { return; }
    let packed = source[id.x];
    if (packed >> 24u) == 0u { output[id.x] = packed; return; }
    let input = vec4<f32>(f32(packed & 255u) / 255.0, f32((packed >> 8u) & 255u) / 255.0, f32((packed >> 16u) & 255u) / 255.0, 1.0);
    let rgb = vec3<f32>(dot(input, settings.red), dot(input, settings.green), dot(input, settings.blue));
    let encoded = vec3<u32>(floor(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + 0.5));
    output[id.x] = encoded.x | (encoded.y << 8u) | (encoded.z << 16u) | (packed & 0xff000000u);
}
