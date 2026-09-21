// Union one-point guides before applying opacity, including antialiased crossings.
fn guide_line(point: vec2<f32>, low: vec2<f32>, high: vec2<f32>, scale: f32) -> f32 {
    let coverage = clamp(vec2<f32>(0.5) + min(point - low, high - point) * scale,
                         vec2<f32>(0.0), vec2<f32>(1.0));
    return coverage.x * coverage.y;
}

fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    let point = input.uv * input.size;
    let origin = input.params[0].xy;
    let size = input.params[0].zw;
    let scale = input.params[1].x;
    let divisions = u32(input.params[1].y);
    let inset = u32(input.params[1].z);
    let opacity = input.params[1].w;
    var uncovered = 1.0;
    for (var i = inset; i <= divisions - inset; i += 1u) {
        let guide = origin + size * f32(i) / f32(divisions);
        let vertical = guide_line(point, vec2<f32>(guide.x - 0.5, origin.y),
                                 vec2<f32>(guide.x + 0.5, origin.y + size.y), scale);
        let horizontal = guide_line(point, vec2<f32>(origin.x, guide.y - 0.5),
                                   vec2<f32>(origin.x + size.x, guide.y + 0.5), scale);
        uncovered *= (1.0 - vertical) * (1.0 - horizontal);
    }
    return vec4<f32>(1.0, 1.0, 1.0, opacity * (1.0 - uncovered));
}
