struct Params {
    center: vec2<f32>,
    padding: vec2<f32>,
    mapping: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var input: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var encoded: texture_storage_2d<rgba8unorm, write>;

fn sample(point: vec2<f32>) -> vec4<f32> {
    let size = textureDimensions(input);
    let p = clamp(point, vec2(0.5), vec2<f32>(size) - vec2(0.5)) - vec2(0.5);
    let base = vec2<u32>(floor(p));
    let f = fract(p);
    var result = vec4(0.0);
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let at = min(base + vec2(x, y), size - vec2(1u));
            let weight = select(1.0 - f.x, f.x, x == 1u) * select(1.0 - f.y, f.y, y == 1u);
            result += textureLoad(input, vec2<i32>(at), 0) * weight;
        }
    }
    return result;
}

@compute @workgroup_size(16, 16)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(input)) { return; }
    let p = vec2<f32>(id.xy) + vec2(0.5) - params.center;
    let a = params.mapping.xy;
    let b = params.mapping.zw;
    let first = params.center + vec2(p.x * a.x - p.y * a.y, p.x * a.y + p.y * a.x);
    let second = params.center + vec2(p.x * b.x - p.y * b.y, p.x * b.y + p.y * b.x);
    textureStore(output, vec2<i32>(id.xy), (sample(first) + sample(second)) * 0.5);
}

@compute @workgroup_size(16, 16)
fn encode(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(input)) { return; }
    let pixel = textureLoad(input, vec2<i32>(id.xy), 0);
    var rgb = vec3(0.0);
    if pixel.a > 0.0 { rgb = pixel.rgb / pixel.a; }
    textureStore(encoded, vec2<i32>(id.xy), vec4(rgb, pixel.a));
}
