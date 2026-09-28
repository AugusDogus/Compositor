struct Parameters { width: u32, height: u32, radius: u32, vertical: u32 }
@group(0) @binding(0) var<uniform> p: Parameters;
@group(0) @binding(1) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> weights: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;
@compute @workgroup_size(16, 16)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.width || id.y >= p.height { return; }
    var value = vec4<f32>(0.0);
    for (var i = 0u; i <= 2u * p.radius; i++) {
        let offset = i32(i) - i32(p.radius);
        var xy = vec2<i32>(id.xy);
        if p.vertical == 0u { xy.x += offset; } else { xy.y += offset; }
        xy = clamp(xy, vec2<i32>(0), vec2<i32>(i32(p.width)-1, i32(p.height)-1));
        value += input[u32(xy.y) * p.width + u32(xy.x)] * weights[i];
    }
    output[id.y * p.width + id.x] = value;
}
