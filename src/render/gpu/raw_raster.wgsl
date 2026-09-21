// Adapted from Xuan, copyright (c) 2026 Wonder Assembly LLC and Silver Ling.
// Distributed under the MIT license; see licenses/Xuan-MIT.txt.
@compute @workgroup_size(8, 8)
fn gaussian(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let radius = i32(config[1].x);
    let direction = vec2<i32>(config[1].yz);
    var sum = vec4(0.0);
    for (var k = -radius; k <= radius; k++) {
        let p = vec2<u32>(clamp(vec2<i32>(id.xy) + direction * k, vec2(0), vec2<i32>(size) - 1));
        sum += load_float(p.y * size.x + p.x) * config[u32(k + radius) + 2u].x;
    }
    store_float(id.y * size.x + id.x, sum);
}
