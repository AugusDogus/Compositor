struct Parameters {
    color_density: vec4<f32>,
    preserve: u32,
    count: u32,
    padding: vec2<u32>,
}
@group(0) @binding(0) var<uniform> settings: Parameters;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= settings.count { return; }
    let packed = source[id.x];
    if (packed >> 24u) == 0u {
        output[id.x] = packed;
        return;
    }
    let input = vec3<f32>(f32(packed & 255u), f32((packed >> 8u) & 255u), f32((packed >> 16u) & 255u)) / 255.0;
    let density = settings.color_density.w;
    var rgb = input * (1.0 - density) + input * settings.color_density.xyz * density;
    if settings.preserve != 0u {
        let weights = vec3<f32>(0.299, 0.587, 0.114);
        let before = dot(input, weights);
        let after = dot(rgb, weights);
        if after > 0.000001 { rgb *= before / after; }
    }
    let encoded = vec3<u32>(floor(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + 0.5));
    output[id.x] = encoded.x | (encoded.y << 8u) | (encoded.z << 16u) | (packed & 0xff000000u);
}
